use crate::command::run;
use crate::config::{config_dir, create_owner_only_dir, state_dir, write_owner_only_atomic};
use crate::error::{Error, Result};
use crate::service::{
    launch_agent_is_loaded, recorded_daemon_pid, recorded_daemon_pid_is_running,
    start_manual_daemon, stop_recorded_daemon, wait_for_recorded_daemon_ready,
};
use crate::service_executable::{stable_executable_path, systemd_quote};
use std::fs;
use std::io::Read;
use std::path::Path;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SystemdEnablement {
    Persistent,
    Runtime,
    Disabled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SystemdActivity {
    Active,
    Inactive,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SystemdState {
    enablement: SystemdEnablement,
    activity: SystemdActivity,
}

#[derive(Debug)]
struct LaunchActivationFailure {
    error: Error,
    candidate_may_be_loaded: bool,
}

pub fn install_launch_agent(
    plist: &Path,
    label: &str,
    uid: u32,
    before_rollback: impl FnOnce() -> Result<()>,
) -> Result<()> {
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
    let was_running = launch_agent_is_loaded(label, uid)?;
    let manual_daemon_pid = if was_running {
        None
    } else {
        recorded_daemon_pid()?
    };
    write_owner_only_atomic(plist, content.as_bytes())?;
    let result = activate_launch_agent(
        was_running,
        || bootout_launch_agent(label, uid),
        stop_recorded_daemon,
        || bootstrap_launch_agent(plist, uid),
        wait_for_recorded_daemon_ready,
    );
    if let Err(failure) = result {
        let activation_error = failure.error.to_string();
        let rollback = rollback_after_config_restore(
            &activation_error,
            || {
                cleanup_launch_candidate_if_needed(failure.candidate_may_be_loaded, || {
                    cleanup_launch_agent_candidate(label, uid)
                })
            },
            before_rollback,
            || restore_service_file(plist, previous.as_deref()),
            || {
                let original = failure.error;
                let state_restore = (|| {
                    if was_running && previous.is_some() {
                        bootstrap_launch_agent(plist, uid)?;
                        wait_for_recorded_daemon_ready()
                    } else {
                        Ok(())
                    }
                })();
                match state_restore {
                    Ok(()) => Err(original),
                    Err(restore) => Err(Error::DoctorFailed(format!(
                        "service activation failed ({original}) and service state could not be restored ({restore})"
                    ))),
                }
            },
        )?;
        return complete_service_rollback(
            rollback,
            manual_daemon_pid,
            recorded_daemon_pid_is_running,
            || start_manual_daemon(&exe),
        );
    }
    Ok(())
}

fn activate_launch_agent(
    was_running: bool,
    bootout_previous: impl FnOnce() -> Result<()>,
    stop_daemon: impl FnOnce() -> Result<()>,
    bootstrap_candidate: impl FnOnce() -> Result<()>,
    wait_until_ready: impl FnOnce() -> Result<()>,
) -> std::result::Result<(), LaunchActivationFailure> {
    if was_running {
        bootout_previous().map_err(|error| LaunchActivationFailure {
            error,
            candidate_may_be_loaded: false,
        })?;
    }
    stop_daemon().map_err(|error| LaunchActivationFailure {
        error,
        candidate_may_be_loaded: false,
    })?;
    bootstrap_candidate().map_err(|error| LaunchActivationFailure {
        error,
        candidate_may_be_loaded: true,
    })?;
    wait_until_ready().map_err(|error| LaunchActivationFailure {
        error,
        candidate_may_be_loaded: true,
    })
}

fn bootout_launch_agent(label: &str, uid: u32) -> Result<()> {
    run(
        "launchctl",
        &["bootout".to_string(), format!("gui/{uid}/{label}")],
        None,
    )?;
    Ok(())
}

fn cleanup_launch_agent_candidate(label: &str, uid: u32) -> Result<()> {
    unload_launch_agent_if_present(label, uid)
}

pub fn unload_launch_agent_if_present(label: &str, uid: u32) -> Result<()> {
    match run(
        "launchctl",
        &["bootout".to_string(), format!("gui/{uid}/{label}")],
        None,
    ) {
        Ok(_) => Ok(()),
        Err(error) if launch_agent_is_absent(&error) => Ok(()),
        Err(error) => Err(error),
    }
}

fn cleanup_launch_candidate_if_needed(
    candidate_may_be_loaded: bool,
    cleanup: impl FnOnce() -> Result<()>,
) -> Result<()> {
    if candidate_may_be_loaded {
        cleanup()
    } else {
        Ok(())
    }
}

fn launch_agent_is_absent(error: &Error) -> bool {
    matches!(
        error,
        Error::CommandFailed {
            code: Some(3),
            stderr,
            ..
        } if stderr.contains("Boot-out failed: 3:") && stderr.contains("No such process")
    )
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

fn rollback_after_config_restore(
    activation_error: &str,
    cleanup_candidate: impl FnOnce() -> Result<()>,
    restore_config: impl FnOnce() -> Result<()>,
    restore_previous_file: impl FnOnce() -> Result<()>,
    restore_previous_service: impl FnOnce() -> Result<()>,
) -> Result<Result<()>> {
    let candidate_cleanup = cleanup_candidate();
    let config_restore = restore_config();
    let file_restore = restore_previous_file();
    if candidate_cleanup.is_ok() && config_restore.is_ok() && file_restore.is_ok() {
        return Ok(restore_previous_service());
    }
    let mut errors = vec![format!("service activation failed ({activation_error})")];
    if let Err(cleanup) = candidate_cleanup {
        errors.push(format!("candidate cleanup failed ({cleanup})"));
    }
    if let Err(config) = config_restore {
        errors.push(format!("configuration restoration failed ({config})"));
    }
    if let Err(file) = file_restore {
        errors.push(format!("service definition restoration failed ({file})"));
    }
    Err(Error::DoctorFailed(errors.join(" and ")))
}

pub fn install_systemd_user(
    unit: &Path,
    unit_name: &str,
    before_rollback: impl FnOnce() -> Result<()>,
) -> Result<()> {
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
    let previous_state = previous
        .as_ref()
        .map(|_| systemd_unit_state(unit_name))
        .transpose()?;
    let manual_daemon_pid = independent_manual_daemon_pid(recorded_daemon_pid()?, previous_state);
    write_owner_only_atomic(unit, content.as_bytes())?;
    if let Err(error) = activate_systemd(
        unit_name,
        systemctl,
        stop_recorded_daemon,
        wait_for_recorded_daemon_ready,
    ) {
        let activation_error = error.to_string();
        let rollback = rollback_after_config_restore(
            &activation_error,
            || cleanup_systemd_candidate(unit_name, systemctl),
            before_rollback,
            || restore_service_file(unit, previous.as_deref()),
            || {
                let state_restore = restore_systemd_runtime(unit_name, previous_state, systemctl);
                match state_restore {
                    Ok(()) => Err(error),
                    Err(restore) => Err(Error::DoctorFailed(format!(
                        "service activation failed ({error}) and service state could not be restored ({restore})"
                    ))),
                }
            },
        )?;
        return complete_service_rollback(
            rollback,
            manual_daemon_pid,
            recorded_daemon_pid_is_running,
            || start_manual_daemon(&exe),
        );
    }
    Ok(())
}

fn cleanup_systemd_candidate(
    unit_name: &str,
    mut invoke_systemctl: impl FnMut(&[&str]) -> Result<()>,
) -> Result<()> {
    invoke_systemctl(&["disable", "--now", unit_name])
}

fn independent_manual_daemon_pid(
    recorded_daemon_pid: Option<u32>,
    previous_state: Option<SystemdState>,
) -> Option<u32> {
    recorded_daemon_pid
        .filter(|_| previous_state.is_none_or(|state| state.activity != SystemdActivity::Active))
}

fn complete_service_rollback(
    rollback: Result<()>,
    manual_daemon_pid: Option<u32>,
    check_manual_daemon: impl FnOnce(u32) -> Result<bool>,
    restart_manual_daemon: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let manual_restore = if let Some(manual_daemon_pid) = manual_daemon_pid {
        match check_manual_daemon(manual_daemon_pid) {
            Ok(true) => Ok(()),
            Ok(false) => restart_manual_daemon(),
            Err(error) => Err(error),
        }
    } else {
        Ok(())
    };
    match (rollback, manual_restore) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(rollback), Ok(())) => Err(rollback),
        (Ok(()), Err(manual)) => Err(manual),
        (Err(rollback), Err(manual)) => Err(Error::DoctorFailed(format!(
            "service rollback failed ({rollback}) and the previous manual daemon could not be restored ({manual})"
        ))),
    }
}

fn activate_systemd(
    unit_name: &str,
    mut invoke_systemctl: impl FnMut(&[&str]) -> Result<()>,
    stop_daemon: impl FnOnce() -> Result<()>,
    wait_until_ready: impl FnOnce() -> Result<()>,
) -> Result<()> {
    invoke_systemctl(&["daemon-reload"])?;
    invoke_systemctl(&["stop", unit_name])?;
    stop_daemon()?;
    invoke_systemctl(&["enable", unit_name])?;
    invoke_systemctl(&["start", unit_name])?;
    wait_until_ready()
}

fn systemctl(args: &[&str]) -> Result<()> {
    let args = std::iter::once("--user".to_string())
        .chain(args.iter().map(|arg| (*arg).to_string()))
        .collect::<Vec<_>>();
    run("systemctl", &args, None)?;
    Ok(())
}

fn systemd_unit_state(unit_name: &str) -> Result<SystemdState> {
    let args = vec![
        "--user".to_string(),
        "show".to_string(),
        "--property=UnitFileState".to_string(),
        "--property=ActiveState".to_string(),
        unit_name.to_string(),
    ];
    let output = run("systemctl", &args, None)?;
    parse_systemd_state(&String::from_utf8_lossy(&output.stdout))
}

fn parse_systemd_state(output: &str) -> Result<SystemdState> {
    let mut enablement = None;
    let mut activity = None;
    for line in output.lines() {
        let Some((property, value)) = line.split_once('=') else {
            return Err(Error::DoctorFailed(
                "systemd returned a malformed unit state".to_string(),
            ));
        };
        match (property, value) {
            ("UnitFileState", "enabled") if enablement.is_none() => {
                enablement = Some(SystemdEnablement::Persistent)
            }
            ("UnitFileState", "enabled-runtime") if enablement.is_none() => {
                enablement = Some(SystemdEnablement::Runtime)
            }
            ("UnitFileState", "disabled") if enablement.is_none() => {
                enablement = Some(SystemdEnablement::Disabled)
            }
            ("ActiveState", "active") if activity.is_none() => {
                activity = Some(SystemdActivity::Active)
            }
            ("ActiveState", "inactive") if activity.is_none() => {
                activity = Some(SystemdActivity::Inactive)
            }
            ("ActiveState", "failed") if activity.is_none() => {
                activity = Some(SystemdActivity::Inactive)
            }
            _ => {
                return Err(Error::DoctorFailed(
                    "systemd returned an unsupported unit state".to_string(),
                ));
            }
        }
    }
    Ok(SystemdState {
        enablement: enablement.ok_or_else(|| {
            Error::DoctorFailed("systemd omitted the unit enablement state".to_string())
        })?,
        activity: activity.ok_or_else(|| {
            Error::DoctorFailed("systemd omitted the unit activity state".to_string())
        })?,
    })
}

#[cfg(test)]
fn rollback_systemd_install(
    unit: &Path,
    previous: Option<&[u8]>,
    original: Error,
    unit_name: &str,
    previous_state: Option<SystemdState>,
    mut invoke_systemctl: impl FnMut(&[&str]) -> Result<()>,
) -> Result<()> {
    rollback_service_file(unit, previous, original, || {
        restore_systemd_runtime(unit_name, previous_state, &mut invoke_systemctl)
    })
}

fn restore_systemd_runtime(
    unit_name: &str,
    previous_state: Option<SystemdState>,
    mut invoke_systemctl: impl FnMut(&[&str]) -> Result<()>,
) -> Result<()> {
    match previous_state {
        Some(state) => restore_systemd_state(unit_name, state, &mut invoke_systemctl),
        None => invoke_systemctl(&["daemon-reload"]),
    }
}

fn restore_systemd_state(
    unit_name: &str,
    previous_state: SystemdState,
    invoke_systemctl: &mut impl FnMut(&[&str]) -> Result<()>,
) -> Result<()> {
    invoke_systemctl(&["daemon-reload"])?;
    match previous_state.enablement {
        SystemdEnablement::Persistent => invoke_systemctl(&["enable", unit_name])?,
        SystemdEnablement::Runtime => {
            invoke_systemctl(&["disable", unit_name])?;
            invoke_systemctl(&["enable", "--runtime", unit_name])?;
        }
        SystemdEnablement::Disabled => invoke_systemctl(&["disable", unit_name])?,
    }
    invoke_systemctl(&[
        match previous_state.activity {
            SystemdActivity::Active => "restart",
            SystemdActivity::Inactive => "stop",
        },
        unit_name,
    ])
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

#[cfg(test)]
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
            "service install failed ({original}); the previous service file was restored but its service state could not be restored ({reactivate})"
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

    type SystemdRestoreCase = (&'static str, Option<SystemdState>, Vec<Vec<String>>);

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
    fn rollback_removes_new_file_and_surfaces_state_restore_failure() {
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
            matches!(result, Err(Error::DoctorFailed(message)) if message.contains("service state could not be restored"))
        );
        assert!(!path.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn parses_only_restorable_systemd_states() {
        assert_eq!(
            parse_systemd_state("ActiveState=active\nUnitFileState=enabled\n").unwrap(),
            SystemdState {
                enablement: SystemdEnablement::Persistent,
                activity: SystemdActivity::Active,
            }
        );
        assert_eq!(
            parse_systemd_state("UnitFileState=enabled-runtime\nActiveState=inactive\n").unwrap(),
            SystemdState {
                enablement: SystemdEnablement::Runtime,
                activity: SystemdActivity::Inactive,
            }
        );
        assert!(parse_systemd_state("UnitFileState=static\nActiveState=active\n").is_err());
        assert_eq!(
            parse_systemd_state("UnitFileState=enabled\nActiveState=failed\n").unwrap(),
            SystemdState {
                enablement: SystemdEnablement::Persistent,
                activity: SystemdActivity::Inactive,
            }
        );
        assert!(parse_systemd_state("UnitFileState=enabled\nActiveState=activating\n").is_err());
        assert!(parse_systemd_state("UnitFileState=disabled\n").is_err());
        assert!(parse_systemd_state("").is_err());
    }

    #[test]
    fn systemd_activation_stops_recorded_daemon_before_start() {
        use std::cell::RefCell;

        let trace = RefCell::new(Vec::new());
        activate_systemd(
            "pasteforward.service",
            |args| {
                trace.borrow_mut().push(args.join(" "));
                Ok(())
            },
            || {
                trace.borrow_mut().push("stop-recorded-daemon".to_string());
                Ok(())
            },
            || {
                trace.borrow_mut().push("wait-until-ready".to_string());
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(
            trace.into_inner(),
            vec![
                "daemon-reload",
                "stop pasteforward.service",
                "stop-recorded-daemon",
                "enable pasteforward.service",
                "start pasteforward.service",
                "wait-until-ready",
            ]
        );
    }

    #[test]
    fn launch_agent_bootstrap_errors_require_typed_candidate_cleanup() {
        use std::cell::Cell;

        let failure = activate_launch_agent(
            false,
            || Ok(()),
            || Ok(()),
            || {
                Err(Error::CommandTimedOut {
                    program: "launchctl".to_string(),
                    seconds: 30,
                })
            },
            || Ok(()),
        )
        .unwrap_err();
        assert!(failure.candidate_may_be_loaded);
        let cleanup_invoked = Cell::new(false);
        cleanup_launch_candidate_if_needed(failure.candidate_may_be_loaded, || {
            cleanup_invoked.set(true);
            Ok(())
        })
        .unwrap();
        assert!(cleanup_invoked.get());

        assert!(launch_agent_is_absent(&Error::CommandFailed {
            program: "launchctl".to_string(),
            args: Vec::new(),
            code: Some(3),
            stderr: "Boot-out failed: 3: No such process".to_string(),
        }));
        assert!(!launch_agent_is_absent(&Error::CommandFailed {
            program: "launchctl".to_string(),
            args: Vec::new(),
            code: Some(1),
            stderr: "permission denied".to_string(),
        }));
    }

    #[test]
    fn launch_agent_activation_waits_for_readiness_after_daemon_handoff() {
        use std::cell::RefCell;

        let trace = RefCell::new(Vec::new());
        let result = activate_launch_agent(
            true,
            || {
                trace.borrow_mut().push("bootout-previous");
                Ok(())
            },
            || {
                trace.borrow_mut().push("stop-recorded-daemon");
                Ok(())
            },
            || {
                trace.borrow_mut().push("bootstrap-candidate");
                Ok(())
            },
            || {
                trace.borrow_mut().push("wait-until-ready");
                Err(Error::DoctorFailed(
                    "injected readiness failure".to_string(),
                ))
            },
        );
        let failure = result.unwrap_err();
        assert!(failure.candidate_may_be_loaded);
        assert_eq!(
            trace.into_inner(),
            vec![
                "bootout-previous",
                "stop-recorded-daemon",
                "bootstrap-candidate",
                "wait-until-ready"
            ]
        );
    }

    #[test]
    fn launch_agent_cleanup_failure_blocks_prior_service_reactivation() {
        use std::cell::Cell;

        let prior_file_restore_attempted = Cell::new(false);
        let prior_agent_restore_attempted = Cell::new(false);
        let result = rollback_after_config_restore(
            "activation failed",
            || Err(Error::DoctorFailed("candidate bootout failed".to_string())),
            || Ok(()),
            || {
                prior_file_restore_attempted.set(true);
                Ok(())
            },
            || {
                prior_agent_restore_attempted.set(true);
                Ok(())
            },
        );

        assert!(prior_file_restore_attempted.get());
        assert!(!prior_agent_restore_attempted.get());
        assert!(
            matches!(result, Err(Error::DoctorFailed(message)) if message.contains("activation failed") && message.contains("candidate bootout failed"))
        );
    }

    #[test]
    fn systemd_activation_propagates_every_handoff_failure() {
        use std::cell::{Cell, RefCell};

        let expected = [
            vec!["daemon-reload"],
            vec!["daemon-reload", "stop pasteforward.service"],
            vec![
                "daemon-reload",
                "stop pasteforward.service",
                "stop-recorded-daemon",
                "enable pasteforward.service",
            ],
            vec![
                "daemon-reload",
                "stop pasteforward.service",
                "stop-recorded-daemon",
                "enable pasteforward.service",
                "start pasteforward.service",
            ],
        ];
        for (fail_at, expected_trace) in expected.into_iter().enumerate() {
            let trace = RefCell::new(Vec::new());
            let systemctl_index = Cell::new(0);
            let result = activate_systemd(
                "pasteforward.service",
                |args| {
                    trace.borrow_mut().push(args.join(" "));
                    let current = systemctl_index.get();
                    systemctl_index.set(current + 1);
                    if current == fail_at {
                        Err(Error::DoctorFailed(
                            "injected systemctl failure".to_string(),
                        ))
                    } else {
                        Ok(())
                    }
                },
                || {
                    trace.borrow_mut().push("stop-recorded-daemon".to_string());
                    Ok(())
                },
                || {
                    trace.borrow_mut().push("wait-until-ready".to_string());
                    Ok(())
                },
            );
            assert!(result.is_err());
            assert_eq!(trace.into_inner(), expected_trace);
        }

        let trace = RefCell::new(Vec::new());
        let result = activate_systemd(
            "pasteforward.service",
            |args| {
                trace.borrow_mut().push(args.join(" "));
                Ok(())
            },
            || {
                trace.borrow_mut().push("stop-recorded-daemon".to_string());
                Err(Error::DoctorFailed(
                    "injected daemon-stop failure".to_string(),
                ))
            },
            || {
                trace.borrow_mut().push("wait-until-ready".to_string());
                Ok(())
            },
        );
        assert!(result.is_err());
        assert_eq!(
            trace.into_inner(),
            vec![
                "daemon-reload",
                "stop pasteforward.service",
                "stop-recorded-daemon"
            ]
        );

        let trace = RefCell::new(Vec::new());
        let result = activate_systemd(
            "pasteforward.service",
            |args| {
                trace.borrow_mut().push(args.join(" "));
                Ok(())
            },
            || {
                trace.borrow_mut().push("stop-recorded-daemon".to_string());
                Ok(())
            },
            || {
                trace.borrow_mut().push("wait-until-ready".to_string());
                Err(Error::DoctorFailed(
                    "injected readiness failure".to_string(),
                ))
            },
        );
        assert!(result.is_err());
        assert_eq!(
            trace.into_inner(),
            vec![
                "daemon-reload",
                "stop pasteforward.service",
                "stop-recorded-daemon",
                "enable pasteforward.service",
                "start pasteforward.service",
                "wait-until-ready",
            ]
        );
    }

    #[test]
    fn manual_daemon_state_is_restored_after_service_rollback() {
        use std::cell::Cell;

        let inactive = SystemdState {
            enablement: SystemdEnablement::Persistent,
            activity: SystemdActivity::Inactive,
        };
        let active = SystemdState {
            enablement: SystemdEnablement::Persistent,
            activity: SystemdActivity::Active,
        };
        assert_eq!(independent_manual_daemon_pid(Some(123), None), Some(123));
        assert_eq!(
            independent_manual_daemon_pid(Some(123), Some(inactive)),
            Some(123)
        );
        assert_eq!(independent_manual_daemon_pid(Some(123), Some(active)), None);
        assert_eq!(independent_manual_daemon_pid(None, None), None);

        let original_manual_pid = 123;
        let surviving_candidate_pid = 456;
        let checked_pid = Cell::new(None);
        let restarted = Cell::new(false);
        let result = complete_service_rollback(
            Err(Error::DoctorFailed("activation failed".to_string())),
            Some(original_manual_pid),
            |expected_pid| {
                checked_pid.set(Some(expected_pid));
                Ok(expected_pid == surviving_candidate_pid)
            },
            || {
                restarted.set(true);
                Ok(())
            },
        );
        assert_eq!(checked_pid.get(), Some(original_manual_pid));
        assert!(restarted.get());
        assert!(
            matches!(result, Err(Error::DoctorFailed(message)) if message == "activation failed")
        );

        let result = complete_service_rollback(
            Err(Error::DoctorFailed("activation failed".to_string())),
            Some(123),
            |_| Ok(false),
            || Err(Error::DoctorFailed("restart failed".to_string())),
        );
        assert!(
            matches!(result, Err(Error::DoctorFailed(message)) if message.contains("previous manual daemon could not be restored"))
        );
    }

    #[test]
    fn systemd_rollback_restores_every_enablement_and_running_state() {
        for (label, state, expected) in systemd_restore_cases() {
            let root = test_dir(label);
            create_owner_only_dir(&root).unwrap();
            let path = root.join("service");
            let previous = state.map(|_| b"previous".as_slice());
            write_owner_only_atomic(&path, b"candidate").unwrap();
            let mut trace = Vec::new();
            let result = rollback_systemd_install(
                &path,
                previous,
                Error::DoctorFailed("activate failed".to_string()),
                "pasteforward.service",
                state,
                |args| {
                    trace.push(
                        args.iter()
                            .map(|arg| (*arg).to_string())
                            .collect::<Vec<_>>(),
                    );
                    Ok(())
                },
            );
            assert!(
                matches!(result, Err(Error::DoctorFailed(message)) if message == "activate failed")
            );
            assert_eq!(trace, expected, "state case {label}");
            if previous.is_some() {
                assert_eq!(fs::read(&path).unwrap(), b"previous");
            } else {
                assert!(!path.exists());
            }
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn systemd_rollback_propagates_every_command_failure_after_file_restore() {
        for (label, state, expected) in systemd_restore_cases() {
            for fail_at in 0..expected.len() {
                let root = test_dir(&format!("{label}-{fail_at}"));
                create_owner_only_dir(&root).unwrap();
                let path = root.join("service");
                let previous = state.map(|_| b"previous".as_slice());
                write_owner_only_atomic(&path, b"candidate").unwrap();
                let mut trace = Vec::new();
                let result = rollback_systemd_install(
                    &path,
                    previous,
                    Error::DoctorFailed("activate failed".to_string()),
                    "pasteforward.service",
                    state,
                    |args| {
                        trace.push(
                            args.iter()
                                .map(|arg| (*arg).to_string())
                                .collect::<Vec<_>>(),
                        );
                        if trace.len() - 1 == fail_at {
                            Err(Error::DoctorFailed("systemctl failed".to_string()))
                        } else {
                            Ok(())
                        }
                    },
                );
                assert!(
                    matches!(result, Err(Error::DoctorFailed(message)) if message.contains("service state could not be restored"))
                );
                let expected_trace = if state.is_none() && fail_at == 0 {
                    expected.as_slice()
                } else {
                    &expected[..=fail_at]
                };
                assert_eq!(
                    trace, expected_trace,
                    "state case {label}, failure {fail_at}"
                );
                if previous.is_some() {
                    assert_eq!(fs::read(&path).unwrap(), b"previous");
                } else {
                    assert!(!path.exists());
                }
                fs::remove_dir_all(root).unwrap();
            }
        }
    }

    #[test]
    fn systemd_candidate_cleanup_is_attempted_before_config_restoration() {
        let mut trace = Vec::new();
        let result = cleanup_systemd_candidate("pasteforward.service", |args| {
            trace.push(
                args.iter()
                    .map(|arg| (*arg).to_string())
                    .collect::<Vec<_>>(),
            );
            Err(Error::DoctorFailed("candidate cleanup failed".to_string()))
        });
        assert!(
            matches!(result, Err(Error::DoctorFailed(message)) if message == "candidate cleanup failed")
        );
        assert_eq!(
            trace,
            vec![vec!["disable", "--now", "pasteforward.service"]]
        );
    }

    #[test]
    fn config_restoration_precedes_prior_service_reactivation_and_blocks_it_on_failure() {
        use std::cell::{Cell, RefCell};

        let activation = Error::DoctorFailed("activation failed".to_string()).to_string();
        let trace = RefCell::new(Vec::new());
        let rollback = rollback_after_config_restore(
            &activation,
            || {
                trace.borrow_mut().push("candidate-cleanup");
                Ok(())
            },
            || {
                trace.borrow_mut().push("config-restore");
                Ok(())
            },
            || {
                trace.borrow_mut().push("prior-file-restore");
                Ok(())
            },
            || {
                trace.borrow_mut().push("prior-service-restore");
                Ok(())
            },
        )
        .unwrap();
        assert!(rollback.is_ok());
        assert_eq!(
            trace.into_inner(),
            vec![
                "candidate-cleanup",
                "config-restore",
                "prior-file-restore",
                "prior-service-restore"
            ]
        );

        let prior_file_restored = Cell::new(false);
        let prior_service_restored = Cell::new(false);
        let result = rollback_after_config_restore(
            &activation,
            || Err(Error::DoctorFailed("candidate cleanup failed".to_string())),
            || Err(Error::DoctorFailed("config restore failed".to_string())),
            || {
                prior_file_restored.set(true);
                Ok(())
            },
            || {
                prior_service_restored.set(true);
                Ok(())
            },
        );
        assert!(prior_file_restored.get());
        assert!(!prior_service_restored.get());
        assert!(
            matches!(result, Err(Error::DoctorFailed(message)) if message.contains("activation failed") && message.contains("candidate cleanup failed") && message.contains("config restore failed"))
        );
    }

    #[test]
    fn new_systemd_rollback_reports_reload_failure_after_candidate_cleanup() {
        let root = test_dir("new-reload-failure");
        create_owner_only_dir(&root).unwrap();
        let path = root.join("service");
        write_owner_only_atomic(&path, b"candidate").unwrap();
        let mut trace = Vec::new();
        let result = rollback_systemd_install(
            &path,
            None,
            Error::DoctorFailed("activate failed".to_string()),
            "pasteforward.service",
            None,
            |args| {
                trace.push(
                    args.iter()
                        .map(|arg| (*arg).to_string())
                        .collect::<Vec<_>>(),
                );
                Err(Error::DoctorFailed("injected failure".to_string()))
            },
        );
        assert!(
            matches!(result, Err(Error::DoctorFailed(message)) if message.contains("service state could not be restored"))
        );
        assert_eq!(trace, vec![vec!["daemon-reload"]]);
        assert!(!path.exists());
        fs::remove_dir_all(root).unwrap();
    }

    fn systemd_restore_cases() -> Vec<SystemdRestoreCase> {
        let commands = |rows: &[&[&str]]| {
            rows.iter()
                .map(|row| row.iter().map(|value| (*value).to_string()).collect())
                .collect()
        };
        vec![
            ("new", None, commands(&[&["daemon-reload"]])),
            (
                "persistent-running",
                Some(SystemdState {
                    enablement: SystemdEnablement::Persistent,
                    activity: SystemdActivity::Active,
                }),
                commands(&[
                    &["daemon-reload"],
                    &["enable", "pasteforward.service"],
                    &["restart", "pasteforward.service"],
                ]),
            ),
            (
                "persistent-stopped",
                Some(SystemdState {
                    enablement: SystemdEnablement::Persistent,
                    activity: SystemdActivity::Inactive,
                }),
                commands(&[
                    &["daemon-reload"],
                    &["enable", "pasteforward.service"],
                    &["stop", "pasteforward.service"],
                ]),
            ),
            (
                "runtime-running",
                Some(SystemdState {
                    enablement: SystemdEnablement::Runtime,
                    activity: SystemdActivity::Active,
                }),
                commands(&[
                    &["daemon-reload"],
                    &["disable", "pasteforward.service"],
                    &["enable", "--runtime", "pasteforward.service"],
                    &["restart", "pasteforward.service"],
                ]),
            ),
            (
                "runtime-stopped",
                Some(SystemdState {
                    enablement: SystemdEnablement::Runtime,
                    activity: SystemdActivity::Inactive,
                }),
                commands(&[
                    &["daemon-reload"],
                    &["disable", "pasteforward.service"],
                    &["enable", "--runtime", "pasteforward.service"],
                    &["stop", "pasteforward.service"],
                ]),
            ),
            (
                "disabled-running",
                Some(SystemdState {
                    enablement: SystemdEnablement::Disabled,
                    activity: SystemdActivity::Active,
                }),
                commands(&[
                    &["daemon-reload"],
                    &["disable", "pasteforward.service"],
                    &["restart", "pasteforward.service"],
                ]),
            ),
            (
                "disabled-stopped",
                Some(SystemdState {
                    enablement: SystemdEnablement::Disabled,
                    activity: SystemdActivity::Inactive,
                }),
                commands(&[
                    &["daemon-reload"],
                    &["disable", "pasteforward.service"],
                    &["stop", "pasteforward.service"],
                ]),
            ),
        ]
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
