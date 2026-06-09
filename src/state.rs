use crate::config::state_dir;
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
    fs::create_dir_all(state_dir()?)?;
    let current_pid = std::process::id();
    if let Some(pid) = read_pid()? {
        if pid != current_pid && process_alive(pid) {
            return Err(Error::DoctorFailed(format!(
                "pasteforward daemon is already running with pid {pid}"
            )));
        }
    }
    fs::write(pid_path()?, current_pid.to_string())?;
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
}
