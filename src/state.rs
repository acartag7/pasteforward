use crate::config::{create_owner_only_dir, state_dir, write_owner_only_atomic};
use crate::error::{Error, Result};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

const MAX_PID_MARKER_BYTES: u64 = 32;

pub fn pid_path() -> Result<PathBuf> {
    Ok(state_dir()?.join("daemon.pid"))
}

pub fn status_path() -> Result<PathBuf> {
    Ok(state_dir()?.join("status.json"))
}

fn ready_path() -> Result<PathBuf> {
    Ok(state_dir()?.join("daemon.ready"))
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
    read_pid_marker(&pid_path()?)
}

pub fn remove_pid() -> Result<()> {
    let path = pid_path()?;
    if let Some(recorded_pid) = read_pid_marker(&path)? {
        let current_pid = std::process::id();
        if recorded_pid == current_pid {
            fs::remove_file(path)?;
        }
    }
    clear_daemon_ready_for(Some(std::process::id()))?;
    Ok(())
}

pub fn write_daemon_ready() -> Result<()> {
    write_owner_only_atomic(&ready_path()?, std::process::id().to_string().as_bytes())
}

pub fn daemon_ready(pid: u32) -> Result<bool> {
    Ok(read_pid_marker(&ready_path()?)? == Some(pid))
}

pub fn clear_daemon_ready() -> Result<()> {
    let path = ready_path()?;
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

pub fn clear_daemon_ready_for(expected_pid: Option<u32>) -> Result<()> {
    if expected_pid.is_none() {
        return clear_daemon_ready();
    }
    let path = ready_path()?;
    let ready_pid = read_pid_marker(&path)?;
    if ready_pid.is_some() && (expected_pid.is_none() || ready_pid == expected_pid) {
        fs::remove_file(path)?;
    }
    Ok(())
}

fn read_pid_marker(path: &Path) -> Result<Option<u32>> {
    let mut file = match crate::secure_fs::open_read(path) {
        Ok(file) => file,
        Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > MAX_PID_MARKER_BYTES {
        return Err(Error::DoctorFailed(
            "daemon state marker is not a bounded regular file".to_string(),
        ));
    }
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(MAX_PID_MARKER_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_PID_MARKER_BYTES {
        return Err(Error::DoctorFailed(
            "daemon state marker exceeds its size limit".to_string(),
        ));
    }
    let value = std::str::from_utf8(&bytes)
        .ok()
        .and_then(|value| value.trim().parse::<u32>().ok())
        .filter(|pid| *pid != 0)
        .ok_or_else(|| Error::DoctorFailed("daemon state marker is malformed".to_string()))?;
    Ok(Some(value))
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
    #[cfg(unix)]
    use std::time::{SystemTime, UNIX_EPOCH};

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

    #[cfg(unix)]
    #[test]
    fn pid_markers_reject_symlinks_fifos_and_oversize_files() {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;

        let root = std::env::temp_dir().join(format!(
            "pasteforward-state-marker-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();

        let valid = root.join("valid");
        std::fs::write(&valid, b"123").unwrap();
        assert_eq!(read_pid_marker(&valid).unwrap(), Some(123));

        let oversized = root.join("oversized");
        std::fs::write(&oversized, vec![b'1'; MAX_PID_MARKER_BYTES as usize + 1]).unwrap();
        assert!(read_pid_marker(&oversized).is_err());

        let symlink = root.join("symlink");
        std::os::unix::fs::symlink(&valid, &symlink).unwrap();
        assert!(read_pid_marker(&symlink).is_err());

        let fifo = root.join("fifo");
        let fifo_path = CString::new(fifo.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(fifo_path.as_ptr(), 0o600) }, 0);
        assert!(read_pid_marker(&fifo).is_err());

        std::fs::remove_dir_all(root).unwrap();
    }
}
