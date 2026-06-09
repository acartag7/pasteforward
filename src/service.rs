use crate::command::run;
use crate::config::{config_dir, state_dir};
use crate::error::{Error, Result};
use std::fs;
use std::path::PathBuf;

const MAC_LABEL: &str = "io.github.acartag7.pasteforward";
const LINUX_UNIT: &str = "pasteforward.service";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceStatus {
    Installed,
    NotInstalled,
    Unknown(String),
}

pub fn install_service() -> Result<()> {
    if cfg!(target_os = "macos") {
        install_launch_agent()
    } else if cfg!(target_os = "linux") {
        install_systemd_user()
    } else {
        Err(Error::UnsupportedPlatform(
            "services are supported on macOS launchd and Linux systemd user services".to_string(),
        ))
    }
}

pub fn uninstall_service() -> Result<()> {
    if cfg!(target_os = "macos") {
        let plist = launch_agent_path()?;
        let _ = run(
            "launchctl",
            &[
                "bootout".to_string(),
                format!("gui/{}/{}", unsafe { libc_getuid() }, MAC_LABEL),
            ],
            None,
        );
        if plist.exists() {
            fs::remove_file(plist)?;
        }
        Ok(())
    } else if cfg!(target_os = "linux") {
        let _ = run(
            "systemctl",
            &[
                "--user".to_string(),
                "disable".to_string(),
                "--now".to_string(),
                LINUX_UNIT.to_string(),
            ],
            None,
        );
        let unit = systemd_unit_path()?;
        if unit.exists() {
            fs::remove_file(unit)?;
        }
        let _ = run(
            "systemctl",
            &["--user".to_string(), "daemon-reload".to_string()],
            None,
        );
        Ok(())
    } else {
        Ok(())
    }
}

pub fn restart_service_if_installed() -> Result<()> {
    match service_status()? {
        ServiceStatus::Installed => {
            if cfg!(target_os = "macos") {
                let plist = launch_agent_path()?;
                let _ = run(
                    "launchctl",
                    &[
                        "kickstart".to_string(),
                        "-k".to_string(),
                        format!("gui/{}/{}", unsafe { libc_getuid() }, MAC_LABEL),
                    ],
                    None,
                );
                if !plist.exists() {
                    install_launch_agent()?;
                }
            } else if cfg!(target_os = "linux") {
                let _ = run(
                    "systemctl",
                    &[
                        "--user".to_string(),
                        "restart".to_string(),
                        LINUX_UNIT.to_string(),
                    ],
                    None,
                );
            }
        }
        ServiceStatus::NotInstalled | ServiceStatus::Unknown(_) => {}
    }
    Ok(())
}

pub fn service_status() -> Result<ServiceStatus> {
    if cfg!(target_os = "macos") {
        Ok(if launch_agent_path()?.exists() {
            ServiceStatus::Installed
        } else {
            ServiceStatus::NotInstalled
        })
    } else if cfg!(target_os = "linux") {
        Ok(if systemd_unit_path()?.exists() {
            ServiceStatus::Installed
        } else {
            ServiceStatus::NotInstalled
        })
    } else {
        Ok(ServiceStatus::Unknown("unsupported platform".to_string()))
    }
}

fn install_launch_agent() -> Result<()> {
    let plist = launch_agent_path()?;
    if let Some(parent) = plist.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::create_dir_all(config_dir()?)?;
    fs::create_dir_all(state_dir()?)?;
    let exe = std::env::current_exe()?;
    let stdout = state_dir()?.join("daemon.out.log");
    let stderr = state_dir()?.join("daemon.err.log");
    let content = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "https://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{MAC_LABEL}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{}</string>
    <string>daemon</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>StandardOutPath</key>
  <string>{}</string>
  <key>StandardErrorPath</key>
  <string>{}</string>
</dict>
</plist>
"#,
        xml_escape(&exe.to_string_lossy()),
        xml_escape(&stdout.to_string_lossy()),
        xml_escape(&stderr.to_string_lossy())
    );
    fs::write(&plist, content)?;
    let _ = run(
        "launchctl",
        &[
            "bootout".to_string(),
            format!("gui/{}/{}", unsafe { libc_getuid() }, MAC_LABEL),
        ],
        None,
    );
    run(
        "launchctl",
        &[
            "bootstrap".to_string(),
            format!("gui/{}", unsafe { libc_getuid() }),
            plist.to_string_lossy().to_string(),
        ],
        None,
    )?;
    run(
        "launchctl",
        &[
            "kickstart".to_string(),
            "-k".to_string(),
            format!("gui/{}/{}", unsafe { libc_getuid() }, MAC_LABEL),
        ],
        None,
    )?;
    Ok(())
}

fn install_systemd_user() -> Result<()> {
    let unit = systemd_unit_path()?;
    if let Some(parent) = unit.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::create_dir_all(config_dir()?)?;
    fs::create_dir_all(state_dir()?)?;
    let exe = std::env::current_exe()?;
    let content = format!(
        r#"[Unit]
Description=PasteForward SSH image paste bridge

[Service]
Type=simple
ExecStart={} daemon
Restart=always
RestartSec=2

[Install]
WantedBy=default.target
"#,
        exe.to_string_lossy()
    );
    fs::write(&unit, content)?;
    run(
        "systemctl",
        &["--user".to_string(), "daemon-reload".to_string()],
        None,
    )?;
    run(
        "systemctl",
        &[
            "--user".to_string(),
            "enable".to_string(),
            "--now".to_string(),
            LINUX_UNIT.to_string(),
        ],
        None,
    )?;
    Ok(())
}

fn launch_agent_path() -> Result<PathBuf> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| Error::UnsupportedPlatform("HOME is not set".to_string()))?;
    Ok(home
        .join("Library")
        .join("LaunchAgents")
        .join(format!("{MAC_LABEL}.plist")))
}

fn systemd_unit_path() -> Result<PathBuf> {
    let config_home = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .ok_or_else(|| Error::UnsupportedPlatform("HOME is not set".to_string()))?;
    Ok(config_home.join("systemd").join("user").join(LINUX_UNIT))
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(unix)]
unsafe fn libc_getuid() -> u32 {
    unsafe extern "C" {
        fn getuid() -> u32;
    }
    unsafe { getuid() }
}

#[cfg(not(unix))]
unsafe fn libc_getuid() -> u32 {
    0
}
