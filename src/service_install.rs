use crate::command::run;
use crate::config::{config_dir, create_owner_only_dir, state_dir, write_owner_only_atomic};
use crate::error::{Error, Result};
use crate::service::{service_running, stop_recorded_daemon};
use crate::service_executable::{stable_executable_path, systemd_quote};
use std::fs;
use std::io::Read;
use std::path::Path;

pub fn install_launch_agent(plist: &Path, label: &str, uid: u32) -> Result<()> {
    if let Some(parent) = plist.parent() {
        create_owner_only_dir(parent)?;
    }
    create_owner_only_dir(&config_dir()?)?;
    create_owner_only_dir(&state_dir()?)?;
    let exe = stable_executable_path()?;
    let stdout = state_dir()?.join("daemon.out.log");
    let stderr = state_dir()?.join("daemon.err.log");
    let content = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "https://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>Label</key><string>{label}</string>
  <key>ProgramArguments</key><array><string>{}</string><string>daemon</string></array>
  <key>RunAtLoad</key><true/><key>KeepAlive</key><true/>
  <key>StandardOutPath</key><string>{}</string>
  <key>StandardErrorPath</key><string>{}</string>
</dict></plist>
"#,
        xml_escape(&exe.to_string_lossy()),
        xml_escape(&stdout.to_string_lossy()),
        xml_escape(&stderr.to_string_lossy())
    );
    let previous = read_existing_service_file(plist)?;
    let was_running = service_running();
    write_owner_only_atomic(plist, content.as_bytes())?;
    let result = (|| -> Result<()> {
        if was_running {
            run(
                "launchctl",
                &["bootout".to_string(), format!("gui/{uid}/{label}")],
                None,
            )?;
        }
        stop_recorded_daemon()?;
        bootstrap_launch_agent(plist, uid)
    })();
    if let Err(error) = result {
        return rollback_service_file(plist, previous.as_deref(), error, || {
            if was_running && previous.is_some() {
                bootstrap_launch_agent(plist, uid)
            } else {
                Ok(())
            }
        });
    }
    Ok(())
}

fn bootstrap_launch_agent(plist: &Path, uid: u32) -> Result<()> {
    run(
        "launchctl",
        &[
            "bootstrap".to_string(),
            format!("gui/{uid}"),
            plist.to_string_lossy().to_string(),
        ],
        None,
    )?;
    Ok(())
}

pub fn install_systemd_user(unit: &Path, unit_name: &str) -> Result<()> {
    if let Some(parent) = unit.parent() {
        create_owner_only_dir(parent)?;
    }
    create_owner_only_dir(&config_dir()?)?;
    create_owner_only_dir(&state_dir()?)?;
    let exe = stable_executable_path()?;
    let content = format!(
        "[Unit]\nDescription=PasteForward SSH image paste bridge\n\n[Service]\nType=simple\nExecStart={} daemon\nRestart=always\nRestartSec=2\n\n[Install]\nWantedBy=default.target\n",
        systemd_quote(&exe)
    );
    let previous = read_existing_service_file(unit)?;
    let was_running = service_running();
    write_owner_only_atomic(unit, content.as_bytes())?;
    if let Err(error) = reload_and_enable_systemd(unit_name) {
        let _ = systemctl(&["disable", "--now", unit_name]);
        return rollback_service_file(unit, previous.as_deref(), error, || {
            systemctl(&["daemon-reload"])?;
            if was_running && previous.is_some() {
                reload_and_enable_systemd(unit_name)
            } else {
                Ok(())
            }
        });
    }
    Ok(())
}

fn reload_and_enable_systemd(unit_name: &str) -> Result<()> {
    systemctl(&["daemon-reload"])?;
    systemctl(&["enable", "--now", unit_name])
}

fn systemctl(args: &[&str]) -> Result<()> {
    let args = std::iter::once("--user".to_string())
        .chain(args.iter().map(|arg| (*arg).to_string()))
        .collect::<Vec<_>>();
    run("systemctl", &args, None)?;
    Ok(())
}

fn restore_service_file(path: &Path, previous: Option<&[u8]>) -> Result<()> {
    if let Some(content) = previous {
        write_owner_only_atomic(path, content)
    } else if path.exists() {
        fs::remove_file(path)?;
        Ok(())
    } else {
        Ok(())
    }
}

fn read_existing_service_file(path: &Path) -> Result<Option<Vec<u8>>> {
    if fs::symlink_metadata(path).is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound) {
        return Ok(None);
    }
    let mut file = crate::secure_fs::open_read(path)?;
    if file.metadata()?.len() > 1024 * 1024 {
        return Err(Error::LimitExceeded(format!(
            "service definition exceeds 1 MiB: {}",
            path.display()
        )));
    }
    let mut content = Vec::new();
    Read::by_ref(&mut file)
        .take(1024 * 1024 + 1)
        .read_to_end(&mut content)?;
    if content.len() > 1024 * 1024 {
        return Err(Error::LimitExceeded(format!(
            "service definition exceeds 1 MiB: {}",
            path.display()
        )));
    }
    Ok(Some(content))
}

fn rollback_service_file(
    path: &Path,
    previous: Option<&[u8]>,
    original: Error,
    reactivate_previous: impl FnOnce() -> Result<()>,
) -> Result<()> {
    if let Err(restore) = restore_service_file(path, previous) {
        return Err(Error::DoctorFailed(format!(
            "service install failed ({original}) and the previous service file could not be restored ({restore})"
        )));
    }
    if let Err(reactivate) = reactivate_previous() {
        return Err(Error::DoctorFailed(format!(
            "service install failed ({original}); the previous service file was restored but could not be restarted ({reactivate})"
        )));
    }
    Err(original)
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn rollback_restores_previous_service_file() {
        let root = test_dir("restore");
        create_owner_only_dir(&root).unwrap();
        let path = root.join("service");
        write_owner_only_atomic(&path, b"previous").unwrap();
        write_owner_only_atomic(&path, b"candidate").unwrap();
        let result = rollback_service_file(
            &path,
            Some(b"previous"),
            Error::DoctorFailed("activate failed".to_string()),
            || Ok(()),
        );
        assert!(result.is_err());
        assert_eq!(fs::read(&path).unwrap(), b"previous");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rollback_removes_new_file_and_surfaces_restart_failure() {
        let root = test_dir("remove");
        create_owner_only_dir(&root).unwrap();
        let path = root.join("service");
        write_owner_only_atomic(&path, b"candidate").unwrap();
        let result = rollback_service_file(
            &path,
            None,
            Error::DoctorFailed("activate failed".to_string()),
            || Err(Error::DoctorFailed("restart failed".to_string())),
        );
        assert!(
            matches!(result, Err(Error::DoctorFailed(message)) if message.contains("could not be restarted"))
        );
        assert!(!path.exists());
        fs::remove_dir_all(root).unwrap();
    }

    fn test_dir(label: &str) -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "pasteforward-service-{label}-{}-{nonce}",
            std::process::id()
        ))
    }
}
