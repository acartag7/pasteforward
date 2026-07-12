use crate::command::run;
use crate::error::{Error, Result};
use crate::service_install::{install_launch_agent, install_systemd_user};
use crate::state::{pid_path, process_alive, process_is_pasteforward_daemon, read_pid};
use std::fs;
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

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
        install_launch_agent(&launch_agent_path()?, MAC_LABEL, unsafe { libc_getuid() })
    } else if cfg!(target_os = "linux") {
        install_systemd_user(&systemd_unit_path()?, LINUX_UNIT)
    } else {
        Err(Error::UnsupportedPlatform(
            "services are supported on macOS launchd and Linux systemd user services".to_string(),
        ))
    }
}

pub fn uninstall_service() -> Result<()> {
    if cfg!(target_os = "macos") {
        let plist = launch_agent_path()?;
        if service_running() {
            run(
                "launchctl",
                &[
                    "bootout".to_string(),
                    format!("gui/{}/{}", unsafe { libc_getuid() }, MAC_LABEL),
                ],
                None,
            )?;
        }
        stop_recorded_daemon()?;
        if plist.exists() {
            fs::remove_file(plist)?;
        }
        Ok(())
    } else if cfg!(target_os = "linux") {
        let unit = systemd_unit_path()?;
        if unit.exists() {
            run(
                "systemctl",
                &[
                    "--user".to_string(),
                    "disable".to_string(),
                    "--now".to_string(),
                    LINUX_UNIT.to_string(),
                ],
                None,
            )?;
            stop_recorded_daemon()?;
            fs::remove_file(unit)?;
            run(
                "systemctl",
                &["--user".to_string(), "daemon-reload".to_string()],
                None,
            )?;
        }
        Ok(())
    } else {
        Ok(())
    }
}

pub fn restart_service_if_installed() -> Result<()> {
    match service_status()? {
        ServiceStatus::Installed => {
            if cfg!(target_os = "macos") {
                install_launch_agent(&launch_agent_path()?, MAC_LABEL, unsafe { libc_getuid() })?;
            } else if cfg!(target_os = "linux") {
                run(
                    "systemctl",
                    &[
                        "--user".to_string(),
                        "restart".to_string(),
                        LINUX_UNIT.to_string(),
                    ],
                    None,
                )?;
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

pub fn service_running() -> bool {
    if cfg!(target_os = "macos") {
        run(
            "launchctl",
            &[
                "print".to_string(),
                format!("gui/{}/{}", unsafe { libc_getuid() }, MAC_LABEL),
            ],
            None,
        )
        .is_ok()
    } else if cfg!(target_os = "linux") {
        run(
            "systemctl",
            &[
                "--user".to_string(),
                "is-active".to_string(),
                "--quiet".to_string(),
                LINUX_UNIT.to_string(),
            ],
            None,
        )
        .is_ok()
    } else {
        false
    }
}

pub(crate) fn stop_recorded_daemon() -> Result<()> {
    let Some(pid) = read_pid()? else {
        return Ok(());
    };

    if process_alive(pid) && !process_is_pasteforward_daemon(pid) {
        let path = pid_path()?;
        if path.exists() {
            fs::remove_file(path)?;
        }
        return Ok(());
    }

    if process_alive(pid) {
        terminate_process(pid)?;
        for _ in 0..50 {
            if !process_alive(pid) {
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
    }

    if process_alive(pid) {
        return Err(Error::DoctorFailed(format!(
            "pasteforward daemon did not stop after SIGTERM: pid {pid}"
        )));
    }

    let path = pid_path()?;
    if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

pub(crate) fn recorded_daemon_running() -> Result<bool> {
    let Some(pid) = read_pid()? else {
        return Ok(false);
    };
    Ok(process_alive(pid) && process_is_pasteforward_daemon(pid))
}

#[cfg(unix)]
pub(crate) fn start_manual_daemon(executable: &Path) -> Result<()> {
    use std::os::unix::process::CommandExt;

    let mut command = Command::new(executable);
    command
        .arg("daemon")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    let mut child = command.spawn()?;
    for _ in 0..50 {
        if recorded_daemon_running()? {
            return Ok(());
        }
        if child.try_wait()?.is_some() {
            return Err(Error::DoctorFailed(
                "manual daemon exited while service rollback was restoring it".to_string(),
            ));
        }
        thread::sleep(Duration::from_millis(100));
    }
    Err(Error::DoctorFailed(
        "manual daemon did not become ready during service rollback".to_string(),
    ))
}

#[cfg(not(unix))]
pub(crate) fn start_manual_daemon(_executable: &Path) -> Result<()> {
    Err(Error::UnsupportedPlatform(
        "manual daemon restoration is supported only on Unix".to_string(),
    ))
}

#[cfg(unix)]
fn terminate_process(pid: u32) -> Result<()> {
    unsafe extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }

    let rc = unsafe { kill(pid as i32, 15) };
    if rc == 0 {
        return Ok(());
    }

    let err = std::io::Error::last_os_error();
    if err.raw_os_error() == Some(3) {
        Ok(())
    } else {
        Err(err.into())
    }
}

#[cfg(not(unix))]
fn terminate_process(_pid: u32) -> Result<()> {
    Err(Error::UnsupportedPlatform(
        "service process termination is only supported on Unix".to_string(),
    ))
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
