use crate::config::{create_owner_only_dir, state_dir, write_owner_only_atomic};
use crate::error::{Error, Result};
use std::fs;
use std::path::PathBuf;

pub fn pid_path() -> Result<PathBuf> {
    Ok(state_dir()?.join("daemon.pid"))
}

pub fn status_path() -> Result<PathBuf> {
    Ok(state_dir()?.join("status.json"))
}

pub fn write_pid() -> Result<()> {
    create_owner_only_dir(&state_dir()?)?;
    let current_pid = std::process::id();
    if let Some(pid) = read_pid()? {
        if pid != current_pid && process_alive(pid) {
            return Err(Error::DoctorFailed(format!(
                "pasteforward daemon is already running with pid {pid}"
            )));
        }
    }
    write_owner_only_atomic(&pid_path()?, current_pid.to_string().as_bytes())?;
    Ok(())
}

pub fn read_pid() -> Result<Option<u32>> {
    let path = pid_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let value = fs::read_to_string(path)?;
    Ok(value.trim().parse::<u32>().ok())
}

pub fn remove_pid() -> Result<()> {
    let path = pid_path()?;
    if path.exists() {
        let current_pid = std::process::id();
        let recorded_pid = fs::read_to_string(&path)
            .ok()
            .and_then(|value| value.trim().parse::<u32>().ok());
        if recorded_pid.is_none_or(|pid| pid == current_pid) {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}

pub fn process_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    process_alive_impl(pid)
}

pub fn process_is_pasteforward_daemon(pid: u32) -> bool {
    if !process_alive(pid) {
        return false;
    }
    process_is_pasteforward_daemon_impl(pid)
}

#[cfg(target_os = "linux")]
fn process_is_pasteforward_daemon_impl(pid: u32) -> bool {
    let Ok(bytes) = fs::read(format!("/proc/{pid}/cmdline")) else {
        return false;
    };
    let args = bytes
        .split(|byte| *byte == 0)
        .filter(|arg| !arg.is_empty())
        .collect::<Vec<_>>();
    args.len() >= 2
        && args.last().is_some_and(|arg| *arg == b"daemon")
        && args.first().is_some_and(|arg| {
            std::path::Path::new(std::ffi::OsStr::from_bytes(arg))
                .file_name()
                .is_some_and(|name| name == "pasteforward")
        })
}

#[cfg(target_os = "linux")]
use std::os::unix::ffi::OsStrExt;

#[cfg(target_os = "macos")]
fn process_is_pasteforward_daemon_impl(pid: u32) -> bool {
    let Ok(output) = std::process::Command::new("/bin/ps")
        .args(["-p", &pid.to_string(), "-o", "command="])
        .output()
    else {
        return false;
    };
    if !output.status.success() || output.stdout.len() > 4096 {
        return false;
    }
    let command = String::from_utf8_lossy(&output.stdout);
    command.split_whitespace().last() == Some("daemon") && command.contains("pasteforward")
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn process_is_pasteforward_daemon_impl(_pid: u32) -> bool {
    false
}

#[cfg(unix)]
fn process_alive_impl(pid: u32) -> bool {
    unsafe extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    unsafe { kill(pid as i32, 0) == 0 }
}

#[cfg(not(unix))]
fn process_alive_impl(_pid: u32) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_process_is_alive() {
        assert!(process_alive(std::process::id()));
    }

    #[test]
    fn zero_pid_is_not_alive() {
        assert!(!process_alive(0));
    }

    #[test]
    fn current_test_process_is_not_the_daemon() {
        assert!(!process_is_pasteforward_daemon(std::process::id()));
    }
}
