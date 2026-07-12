use crate::config::{create_owner_only_dir, state_dir, write_owner_only_atomic};
use crate::error::{Error, Result};
use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::path::{Path, PathBuf};

const MAX_PID_MARKER_BYTES: u64 = 32;

struct StateLock {
    _file: File,
}

fn lock_state() -> Result<StateLock> {
    create_owner_only_dir(&state_dir()?)?;
    lock_file(&state_dir()?.join("daemon.lock"))
}

fn lock_file(path: &Path) -> Result<StateLock> {
    #[cfg(unix)]
    let file = {
        use std::os::fd::AsRawFd;
        use std::os::unix::fs::OpenOptionsExt;

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .mode(0o600)
            .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)?;
        if !file.metadata()?.is_file() {
            return Err(Error::DoctorFailed(
                "daemon state lock is not a regular file".to_string(),
            ));
        }
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        if unsafe { libc::fchmod(file.as_raw_fd(), 0o600) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        file
    };
    #[cfg(not(unix))]
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(path)?;
    Ok(StateLock { _file: file })
}

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
    let _lock = lock_state()?;
    let current_pid = std::process::id();
    if let Some(pid) = read_pid_marker(&pid_path()?)? {
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
    let _lock = lock_state()?;
    read_pid_marker(&pid_path()?)
}

pub fn remove_pid() -> Result<()> {
    let _lock = lock_state()?;
    let current_pid = std::process::id();
    let pid_cleanup = (|| -> Result<()> {
        let path = pid_path()?;
        if let Some(recorded_pid) = read_pid_marker(&path)? {
            if recorded_pid == current_pid {
                fs::remove_file(path)?;
            }
        }
        Ok(())
    })();
    let ready_cleanup = clear_daemon_ready_for_unlocked(Some(current_pid));
    combine_state_cleanup(pid_cleanup, ready_cleanup)
}

pub fn read_pid_for_stop() -> Result<Option<u32>> {
    let _lock = lock_state()?;
    let pid = read_pid_marker(&pid_path()?)?;
    if pid.is_none() {
        clear_daemon_ready_unlocked()?;
    }
    Ok(pid)
}

pub fn clear_stopped_daemon_state(expected_pid: u32) -> Result<()> {
    let _lock = lock_state()?;
    let pid_cleanup = (|| -> Result<()> {
        let path = pid_path()?;
        if read_pid_marker(&path)? == Some(expected_pid) {
            fs::remove_file(path)?;
        }
        Ok(())
    })();
    let ready_cleanup = clear_daemon_ready_for_unlocked(Some(expected_pid));
    combine_state_cleanup(pid_cleanup, ready_cleanup)
}

fn combine_state_cleanup(first: Result<()>, second: Result<()>) -> Result<()> {
    match (first, second) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
        (Err(first), Err(second)) => Err(Error::DoctorFailed(format!(
            "daemon state cleanup failed for both markers ({first}; {second})"
        ))),
    }
}

pub fn write_daemon_ready() -> Result<()> {
    let _lock = lock_state()?;
    write_owner_only_atomic(&ready_path()?, std::process::id().to_string().as_bytes())
}

pub fn daemon_ready(pid: u32) -> Result<bool> {
    let _lock = lock_state()?;
    Ok(read_pid_marker(&ready_path()?)? == Some(pid))
}

pub fn clear_daemon_ready() -> Result<()> {
    let _lock = lock_state()?;
    clear_daemon_ready_unlocked()
}

fn clear_daemon_ready_unlocked() -> Result<()> {
    let path = ready_path()?;
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

pub fn clear_daemon_ready_for(expected_pid: Option<u32>) -> Result<()> {
    let _lock = lock_state()?;
    clear_daemon_ready_for_unlocked(expected_pid)
}

fn clear_daemon_ready_for_unlocked(expected_pid: Option<u32>) -> Result<()> {
    if expected_pid.is_none() {
        return clear_daemon_ready_unlocked();
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

    #[cfg(unix)]
    #[test]
    fn state_lock_serializes_marker_publication_and_cleanup() {
        let root = std::env::temp_dir().join(format!(
            "pasteforward-state-lock-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let lock_path = root.join("daemon.lock");
        let first = lock_file(&lock_path).unwrap();
        let (sender, receiver) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            let _second = lock_file(&lock_path).unwrap();
            sender.send(()).unwrap();
        });
        assert!(
            receiver
                .recv_timeout(std::time::Duration::from_millis(100))
                .is_err()
        );
        drop(first);
        receiver
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        thread.join().unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
}
