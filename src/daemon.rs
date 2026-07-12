use crate::clipboard::{detect_local_backend, read_image, sha256_hex};
use crate::command::{shell_quote, ssh};
use crate::config::{AppConfig, DestinationConfig, load_config};
use crate::error::Result;
use crate::history::{append_transfer, read_history};
use crate::remote::{read_clipboard_command, resolve_remote_mode, sync_remote_image_command};
use crate::state::{remove_pid, write_daemon_ready, write_pid};
use crate::validation::{validate_config, validate_destination};
use std::collections::BTreeMap;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub fn run_daemon() -> Result<()> {
    write_pid()?;
    let _cleanup = RemovePid;
    let backend = detect_local_backend()?;
    let mut initial_config = Some(load_config()?);
    write_daemon_ready()?;
    let mut last_success = BTreeMap::<String, String>::new();
    let mut retry_after = BTreeMap::<String, (String, Instant)>::new();
    let mut last_cleanup = Instant::now() - Duration::from_secs(3600);

    eprintln!(
        "pasteforward daemon running: local clipboard backend={}",
        backend.as_str()
    );

    loop {
        let config = if let Some(config) = initial_config.take() {
            config
        } else {
            load_config()?
        };
        let interval = Duration::from_millis(config.daemon.interval_millis.max(250));

        if let Some(image) = read_image(&backend)? {
            sync_pending(
                &config,
                &image.bytes,
                &image.sha256,
                &mut last_success,
                &mut retry_after,
            )?;
        }

        if last_cleanup.elapsed() >= Duration::from_secs(60) {
            cleanup_expired(&config, None)?;
            last_cleanup = Instant::now();
        }

        thread::sleep(interval);
    }

    #[allow(unreachable_code)]
    drop(_cleanup);
}

fn sync_pending(
    config: &AppConfig,
    bytes: &[u8],
    sha256: &str,
    last_success: &mut BTreeMap<String, String>,
    retry_after: &mut BTreeMap<String, (String, Instant)>,
) -> Result<()> {
    validate_config(config)?;
    let now = Instant::now();
    for (name, dest) in config.destinations.iter().filter(|(_, dest)| dest.enabled) {
        if last_success.get(name).is_some_and(|hash| hash == sha256) {
            continue;
        }
        if retry_after
            .get(name)
            .is_some_and(|(hash, after)| hash == sha256 && now < *after)
        {
            continue;
        }
        match sync_one(config, name, dest, bytes, sha256) {
            Ok(path) => {
                eprintln!("synced {name} -> {path}");
                last_success.insert(name.clone(), sha256.to_string());
                retry_after.remove(name);
            }
            Err(error) => {
                eprintln!("sync failed for {name}: {error}");
                retry_after.insert(
                    name.clone(),
                    (sha256.to_string(), now + Duration::from_secs(5)),
                );
            }
        }
    }
    last_success.retain(|name, _| config.destinations.contains_key(name));
    retry_after.retain(|name, _| config.destinations.contains_key(name));
    Ok(())
}

pub fn sync_all(config: &AppConfig, bytes: &[u8], sha256: &str) -> Result<()> {
    validate_config(config)?;
    let mut first_error = None;
    for (name, dest) in config.destinations.iter().filter(|(_, d)| d.enabled) {
        match sync_one(config, name, dest, bytes, sha256) {
            Ok(path) => eprintln!("synced {name} -> {path}"),
            Err(err) => {
                eprintln!("sync failed for {name}: {err}");
                if first_error.is_none() {
                    first_error = Some(err);
                }
            }
        }
    }
    first_error.map_or(Ok(()), Err)
}

pub fn sync_one(
    config: &AppConfig,
    name: &str,
    dest: &DestinationConfig,
    bytes: &[u8],
    sha256: &str,
) -> Result<String> {
    validate_config(config)?;
    validate_destination(name, dest)?;
    let (remote_path, mode) = sync_one_without_history(config, name, dest, bytes, sha256)?;
    append_transfer(config, name, &dest.host, sha256, bytes, &remote_path, &mode)?;
    Ok(remote_path)
}

pub fn sync_one_without_history(
    config: &AppConfig,
    name: &str,
    dest: &DestinationConfig,
    bytes: &[u8],
    sha256: &str,
) -> Result<(String, crate::config::RemoteMode)> {
    validate_config(config)?;
    validate_destination(name, dest)?;
    let actual_sha = sha256_hex(bytes);
    if actual_sha != sha256 {
        return Err(crate::error::Error::InvalidDestination(
            "image SHA-256 does not match the provided bytes".to_string(),
        ));
    }
    let mode = resolve_remote_mode(dest)?;
    let remote_path = remote_image_path(config, dest, name, sha256);
    let command = sync_remote_image_command(config, dest, &mode, &remote_path)?;
    let write_output = match ssh(&dest.host, &command, Some(bytes)) {
        Ok(output) => output,
        Err(error) => {
            remove_remote_file(dest, &remote_path);
            return Err(error);
        }
    };
    let readback = match mode {
        crate::config::RemoteMode::LinuxWayland | crate::config::RemoteMode::LinuxX11 => {
            write_output.stdout
        }
        crate::config::RemoteMode::MacosPasteboard => {
            let read_command = read_clipboard_command(dest, &mode)?;
            match ssh(&dest.host, &read_command, None) {
                Ok(output) => output.stdout,
                Err(error) => {
                    remove_remote_file(dest, &remote_path);
                    return Err(error);
                }
            }
        }
        crate::config::RemoteMode::Auto => {
            remove_remote_file(dest, &remote_path);
            return Err(crate::error::Error::DoctorFailed(
                "remote mode remained unresolved during sync".to_string(),
            ));
        }
    };
    let remote_sha = sha256_hex(&readback);
    if remote_sha != sha256 {
        remove_remote_file(dest, &remote_path);
        return Err(crate::error::Error::DoctorFailed(format!(
            "remote clipboard verification failed for {name}"
        )));
    }
    Ok((remote_path, mode))
}

fn remove_remote_file(dest: &DestinationConfig, remote_path: &str) {
    let command = format!("rm -f {}", shell_quote(remote_path));
    let _ = ssh(&dest.host, &command, None);
}

pub fn cleanup_expired(config: &AppConfig, destination: Option<&str>) -> Result<()> {
    validate_config(config)?;
    let ttl_ms = (config.retention.ttl_seconds as u128) * 1000;
    let now = unix_ms();
    for event in read_history(destination, 10_000)? {
        if now.saturating_sub(event.unix_ms) < ttl_ms {
            continue;
        }
        let Some(dest) = config.destinations.get(&event.destination) else {
            continue;
        };
        let remote_dir = config.destination_remote_dir(dest);
        if !event
            .remote_path
            .starts_with(&(remote_dir.trim_end_matches('/').to_string() + "/"))
        {
            continue;
        }
        let command = format!("rm -f {}", shell_quote(&event.remote_path));
        let _ = ssh(&dest.host, &command, None);
    }
    Ok(())
}

fn remote_image_path(
    config: &AppConfig,
    dest: &DestinationConfig,
    name: &str,
    sha256: &str,
) -> String {
    let dir = config.destination_remote_dir(dest);
    let prefix = sha256.chars().take(12).collect::<String>();
    format!(
        "{}/{}-{}-{}.png",
        dir.trim_end_matches('/'),
        sanitize_name(name),
        unix_ms(),
        prefix
    )
}

fn sanitize_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

fn unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

struct RemovePid;

impl Drop for RemovePid {
    fn drop(&mut self) {
        let _ = remove_pid();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AppConfig, DestinationConfig, RemoteMode};
    use std::collections::BTreeMap;

    #[test]
    fn remote_paths_stay_under_remote_dir() {
        let config = AppConfig::empty();
        let dest = DestinationConfig {
            host: "host".to_string(),
            enabled: true,
            remote_mode: RemoteMode::MacosPasteboard,
            remote_env: BTreeMap::new(),
            remote_dir: None,
        };
        let path = remote_image_path(&config, &dest, "mac mini", "abcdef0123456789");
        assert!(path.starts_with("/tmp/pasteforward/mac-mini-"));
        assert!(path.ends_with("-abcdef012345.png"));
    }

    #[test]
    fn execution_boundary_rejects_injected_destination_before_ssh() {
        let config = AppConfig::empty();
        let mut dest = DestinationConfig {
            host: "host".to_string(),
            enabled: true,
            remote_mode: RemoteMode::LinuxX11,
            remote_env: BTreeMap::new(),
            remote_dir: None,
        };
        dest.remote_env
            .insert("DISPLAY;touch /tmp/pwned".to_string(), ":0".to_string());
        let result = sync_one_without_history(&config, "test", &dest, b"png", &sha256_hex(b"png"));
        assert!(matches!(
            result,
            Err(crate::error::Error::InvalidDestination(_))
        ));
    }

    #[test]
    fn failed_destination_is_not_marked_successful() {
        let mut config = AppConfig::empty();
        config.destinations.insert(
            "bad".to_string(),
            DestinationConfig {
                host: "-invalid".to_string(),
                enabled: true,
                remote_mode: RemoteMode::LinuxX11,
                remote_env: BTreeMap::new(),
                remote_dir: None,
            },
        );
        let mut success = BTreeMap::new();
        let mut retries = BTreeMap::new();
        assert!(
            sync_pending(
                &config,
                b"png",
                &sha256_hex(b"png"),
                &mut success,
                &mut retries
            )
            .is_err()
        );
        assert!(success.is_empty());
    }
}
