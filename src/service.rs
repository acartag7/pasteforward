use crate::command::run;
use crate::error::{Error, Result};
use crate::service_install::{install_launch_agent, install_systemd_user};
use crate::state::{
    clear_daemon_ready, clear_daemon_ready_for, clear_stopped_daemon_state, daemon_ready,
    process_alive, process_is_pasteforward_daemon, read_pid, read_pid_for_stop,
};
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
    install_service_with_rollback_precondition(|| Ok(()))
}

pub fn install_service_with_rollback_precondition(
    before_rollback: impl FnOnce() -> Result<()>,
) -> Result<()> {
    if cfg!(target_os = "macos") {
        install_launch_agent(
            &launch_agent_path()?,
            MAC_LABEL,
            unsafe { libc_getuid() },
            before_rollback,
        )
    } else if cfg!(target_os = "linux") {
        install_systemd_user(&systemd_unit_path()?, LINUX_UNIT, before_rollback)
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
                install_launch_agent(
                    &launch_agent_path()?,
                    MAC_LABEL,
                    unsafe { libc_getuid() },
                    || Ok(()),
                )?;
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
    let Some(pid) = read_pid_for_stop()? else {
        return Ok(());
    };

    if process_alive(pid) && !process_is_pasteforward_daemon(pid) {
        clear_stopped_daemon_state(pid)?;
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

    clear_stopped_daemon_state(pid)?;
    Ok(())
}

pub(crate) fn recorded_daemon_pid() -> Result<Option<u32>> {
    let Some(pid) = read_pid_for_stop()? else {
        return Ok(None);
    };
    Ok(recorded_daemon_pid_running(pid).then_some(pid))
}

pub(crate) fn recorded_daemon_pid_is_running(expected_pid: u32) -> Result<bool> {
    Ok(read_pid_for_stop()? == Some(expected_pid) && recorded_daemon_pid_running(expected_pid))
}

fn recorded_daemon_pid_running(expected_pid: u32) -> bool {
    process_alive(expected_pid) && process_is_pasteforward_daemon(expected_pid)
}

pub(crate) fn wait_for_recorded_daemon_ready() -> Result<()> {
    let mut stable_ready_checks = 0;
    let mut stable_pid = None;
    for _ in 0..50 {
        let ready_pid = if let Some(pid) = read_pid()? {
            (daemon_ready(pid)? && process_alive(pid) && process_is_pasteforward_daemon(pid))
                .then_some(pid)
        } else {
            None
        };
        if record_stable_ready_pid(&mut stable_pid, &mut stable_ready_checks, ready_pid) {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }
    Err(Error::DoctorFailed(
        "managed daemon did not become ready after service activation".to_string(),
    ))
}

fn record_stable_ready_pid(
    stable_pid: &mut Option<u32>,
    stable_ready_checks: &mut usize,
    ready_pid: Option<u32>,
) -> bool {
    match ready_pid {
        Some(pid) if *stable_pid == Some(pid) => *stable_ready_checks += 1,
        Some(pid) => {
            *stable_pid = Some(pid);
            *stable_ready_checks = 1;
        }
        None => {
            *stable_pid = None;
            *stable_ready_checks = 0;
        }
    }
    *stable_ready_checks >= 5
}

#[cfg(unix)]
pub(crate) fn start_manual_daemon(executable: &Path) -> Result<()> {
    use std::os::unix::process::CommandExt;

    clear_daemon_ready()?;
    let mut command = Command::new(executable);
    command
        .arg("daemon")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    let mut child = command.spawn()?;
    let result = wait_for_manual_daemon_start(
        &mut child,
        50,
        |child_pid| Ok(daemon_ready(child_pid)? && recorded_daemon_pid_is_running(child_pid)?),
        || thread::sleep(Duration::from_millis(100)),
    );
    if let Err(original) = result {
        return match clear_daemon_ready_for(Some(child.id())) {
            Ok(()) => Err(original),
            Err(cleanup) => Err(Error::DoctorFailed(format!(
                "manual daemon restoration failed ({original}) and its readiness marker could not be removed ({cleanup})"
            ))),
        };
    }
    Ok(())
}

#[cfg(unix)]
fn wait_for_manual_daemon_start(
    child: &mut std::process::Child,
    attempts: usize,
    mut is_ready: impl FnMut(u32) -> Result<bool>,
    mut pause: impl FnMut(),
) -> Result<()> {
    let child_pid = child.id();
    let mut stable_ready_checks = 0;
    for _ in 0..attempts {
        let observation = (|| -> Result<bool> {
            if child.try_wait()?.is_some() {
                return Err(Error::DoctorFailed(
                    "manual daemon exited while service rollback was restoring it".to_string(),
                ));
            }
            is_ready(child_pid)
        })();
        match observation {
            Ok(true) => {
                stable_ready_checks += 1;
                if stable_ready_checks >= 5 {
                    return Ok(());
                }
            }
            Ok(false) => stable_ready_checks = 0,
            Err(error) => return fail_manual_daemon_start(child, error),
        }
        pause();
    }
    fail_manual_daemon_start(
        child,
        Error::DoctorFailed(
            "manual daemon did not become ready during service rollback".to_string(),
        ),
    )
}

#[cfg(unix)]
fn fail_manual_daemon_start(child: &mut std::process::Child, original: Error) -> Result<()> {
    let cleanup = match child.try_wait() {
        Ok(Some(_)) => child.wait().map(|_| ()).map_err(Error::Io),
        Ok(None) => child
            .kill()
            .and_then(|()| child.wait().map(|_| ()))
            .map_err(Error::Io),
        Err(probe) => match child.kill() {
            Ok(()) => child.wait().map(|_| ()).map_err(Error::Io),
            Err(kill) => Err(Error::DoctorFailed(format!(
                "could not inspect spawned daemon ({probe}) or kill it ({kill})"
            ))),
        },
    };
    match cleanup {
        Ok(()) => Err(original),
        Err(cleanup) => Err(Error::DoctorFailed(format!(
            "manual daemon restoration failed ({original}) and spawned-process cleanup failed ({cleanup})"
        ))),
    }
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

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn managed_readiness_stability_resets_when_pid_changes() {
        let mut stable_pid = None;
        let mut checks = 0;
        for _ in 0..4 {
            assert!(!record_stable_ready_pid(
                &mut stable_pid,
                &mut checks,
                Some(101)
            ));
        }
        assert!(!record_stable_ready_pid(
            &mut stable_pid,
            &mut checks,
            Some(202)
        ));
        for _ in 0..3 {
            assert!(!record_stable_ready_pid(
                &mut stable_pid,
                &mut checks,
                Some(202)
            ));
        }
        assert!(record_stable_ready_pid(
            &mut stable_pid,
            &mut checks,
            Some(202)
        ));
    }

    #[test]
    fn manual_daemon_supervisor_requires_stable_readiness() {
        let mut child = Command::new("/bin/sleep").arg("30").spawn().unwrap();
        let checks = std::cell::Cell::new(0);
        wait_for_manual_daemon_start(
            &mut child,
            5,
            |_| {
                checks.set(checks.get() + 1);
                Ok(true)
            },
            || {},
        )
        .unwrap();
        assert_eq!(checks.get(), 5);
        child.kill().unwrap();
        child.wait().unwrap();
    }

    #[test]
    fn manual_daemon_supervisor_reaps_immediate_and_late_exits() {
        let mut immediate = Command::new("/usr/bin/false").spawn().unwrap();
        thread::sleep(Duration::from_millis(20));
        assert!(wait_for_manual_daemon_start(&mut immediate, 5, |_| Ok(false), || {}).is_err());
        assert!(immediate.try_wait().unwrap().is_some());

        let mut late = Command::new("/bin/sleep").arg("1").spawn().unwrap();
        assert!(
            wait_for_manual_daemon_start(
                &mut late,
                10,
                |_| Ok(true),
                || thread::sleep(Duration::from_millis(300)),
            )
            .is_err()
        );
        assert!(late.try_wait().unwrap().is_some());
    }

    #[test]
    fn manual_daemon_supervisor_cleans_up_timeout_and_probe_error() {
        let mut timed_out = Command::new("/bin/sleep").arg("30").spawn().unwrap();
        assert!(wait_for_manual_daemon_start(&mut timed_out, 2, |_| Ok(false), || {}).is_err());
        assert!(timed_out.try_wait().unwrap().is_some());

        let mut probe_error = Command::new("/bin/sleep").arg("30").spawn().unwrap();
        assert!(
            wait_for_manual_daemon_start(
                &mut probe_error,
                2,
                |_| Err(Error::DoctorFailed(
                    "injected readiness failure".to_string()
                )),
                || {},
            )
            .is_err()
        );
        assert!(probe_error.try_wait().unwrap().is_some());
    }
}
