use crate::clipboard::{detect_local_backend, read_image};
use crate::command::{shell_quote, ssh};
use crate::config::{AppConfig, DestinationConfig, load_config};
use crate::doctor::{resolve_remote_mode, sync_remote_image_command};
use crate::error::Result;
use crate::history::{append_transfer, read_history};
use crate::state::{remove_pid, write_pid};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub fn run_daemon() -> Result<()> {
    write_pid()?;
    let _cleanup = RemovePid;
    let backend = detect_local_backend()?;
    let mut last_hash: Option<String> = None;
    let mut last_cleanup = Instant::now() - Duration::from_secs(3600);

    eprintln!(
        "pasteforward daemon running: local clipboard backend={}",
        backend.as_str()
    );

    loop {
        let config = load_config()?;
        let interval = Duration::from_millis(config.daemon.interval_millis.max(250));

        if let Some(image) = read_image(&backend)? {
            if last_hash.as_deref() != Some(image.sha256.as_str()) {
                sync_all(&config, &image.bytes, &image.sha256)?;
                last_hash = Some(image.sha256);
            }
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

pub fn sync_all(config: &AppConfig, bytes: &[u8], sha256: &str) -> Result<()> {
    for (name, dest) in config.destinations.iter().filter(|(_, d)| d.enabled) {
        match sync_one(config, name, dest, bytes, sha256) {
            Ok(path) => eprintln!("synced {name} -> {path}"),
            Err(err) => eprintln!("sync failed for {name}: {err}"),
        }
    }
    Ok(())
}

pub fn sync_one(
    config: &AppConfig,
    name: &str,
    dest: &DestinationConfig,
    bytes: &[u8],
    sha256: &str,
) -> Result<String> {
    let mode = resolve_remote_mode(dest)?;
    let remote_path = remote_image_path(config, dest, name, sha256);
    let command = sync_remote_image_command(config, dest, &mode, &remote_path)?;
    ssh(&dest.host, &command, Some(bytes))?;
    append_transfer(config, name, &dest.host, sha256, bytes, &remote_path, &mode)?;
    Ok(remote_path)
}

pub fn cleanup_expired(config: &AppConfig, destination: Option<&str>) -> Result<()> {
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

pub fn remote_image_path(
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
}
