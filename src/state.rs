use crate::config::{create_owner_only_dir, state_dir, write_owner_only_atomic};
use crate::error::{Error, Result};
use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

const MAX_PID_MARKER_BYTES: u64 = 32;
const STATE_LOCK_ATTEMPTS: usize = 100;
const STATE_LOCK_RETRY: Duration = Duration::from_millis(10);

struct StateLock {
    _file: File,
}

fn lock_state() -> Result<StateLock> {
    prepare_state_directory()?;
    lock_file(&state_dir()?.join("daemon.lock"), true)
}

fn prepare_state_directory() -> Result<()> {
    validate_state_directory(&state_dir()?, false)?;
    create_owner_only_dir(&state_dir()?)
}

fn lock_state_existing() -> Result<Option<StateLock>> {
    let directory = state_dir()?;
    match fs::symlink_metadata(&directory) {
        Ok(_) => validate_state_directory(&directory, true)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    }
    let path = directory.join("daemon.lock");
    match lock_file(&path, false) {
        Ok(lock) => Ok(Some(lock)),
        Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn lock_file(path: &Path, create: bool) -> Result<StateLock> {
    lock_file_with_policy(path, create, STATE_LOCK_ATTEMPTS, STATE_LOCK_RETRY)
}

fn lock_file_with_policy(
    path: &Path,
    create: bool,
    attempts: usize,
    retry: Duration,
) -> Result<StateLock> {
    #[cfg(unix)]
    let file = {
        use std::os::fd::AsRawFd;
        use std::os::unix::fs::OpenOptionsExt;

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(create)
            .mode(0o600)
            .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() {
            return Err(Error::DoctorFailed(
                "daemon state lock is not a regular file".to_string(),
            ));
        }
        validate_owned_metadata(&metadata, !create, "daemon state lock")?;
        let mut acquired = false;
        for attempt in 0..attempts {
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
                acquired = true;
                break;
            }
            let error = std::io::Error::last_os_error();
            let would_block = error
                .raw_os_error()
                .is_some_and(|code| code == libc::EAGAIN || code == libc::EWOULDBLOCK);
            if !would_block {
                return Err(error.into());
            }
            if attempt + 1 < attempts {
                thread::sleep(retry);
            }
        }
        if !acquired {
            return Err(Error::StateLockTimedOut {
                milliseconds: retry.as_millis() as u64 * attempts as u64,
            });
        }
        if create && unsafe { libc::fchmod(file.as_raw_fd(), 0o600) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        file
    };
    #[cfg(not(unix))]
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(create)
        .open(path)?;
    Ok(StateLock { _file: file })
}

#[cfg(unix)]
fn validate_state_directory(path: &Path, require_owner_only: bool) -> Result<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && !require_owner_only => {
            return Ok(());
        }
        Err(error) => return Err(error.into()),
    };
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(Error::DoctorFailed(
            "daemon state directory is not a trusted directory".to_string(),
        ));
    }
    validate_owned_metadata(&metadata, require_owner_only, "daemon state directory")
}

#[cfg(unix)]
fn validate_owned_metadata(
    metadata: &fs::Metadata,
    require_owner_only: bool,
    label: &str,
) -> Result<()> {
    use std::os::unix::fs::MetadataExt;

    let expected_uid = unsafe { libc::geteuid() };
    if metadata.uid() != expected_uid {
        return Err(Error::DoctorFailed(format!(
            "{label} is not owned by the current user"
        )));
    }
    if require_owner_only && !owner_mode_is_trusted(metadata.uid(), metadata.mode(), expected_uid) {
        return Err(Error::DoctorFailed(format!("{label} is not owner-only")));
    }
    Ok(())
}

#[cfg(unix)]
fn owner_mode_is_trusted(uid: u32, mode: u32, expected_uid: u32) -> bool {
    uid == expected_uid && mode & 0o077 == 0
}

#[cfg(not(unix))]
fn validate_state_directory(_path: &Path, _require_owner_only: bool) -> Result<()> {
    Ok(())
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
    prepare_state_directory()?;
    write_pid_at(
        &pid_path()?,
        &ready_path()?,
        &state_dir()?.join("daemon.lock"),
        std::process::id(),
    )
}

fn write_pid_at(path: &Path, ready_path: &Path, lock_path: &Path, current_pid: u32) -> Result<()> {
    let _lock = lock_file(lock_path, true)?;
    if let Some(pid) = read_pid_marker_for_mutation(path)? {
        if pid != current_pid && process_alive(pid) && process_is_pasteforward_daemon(pid) {
            return Err(Error::DoctorFailed(format!(
                "pasteforward daemon is already running with pid {pid}"
            )));
        }
    }
    clear_marker_unlocked(ready_path)?;
    write_owner_only_atomic(path, current_pid.to_string().as_bytes())?;
    Ok(())
}

pub fn read_pid() -> Result<Option<u32>> {
    let _lock = lock_state_existing()?;
    read_pid_marker(&pid_path()?)
}

pub fn remove_pid() -> Result<()> {
    let _lock = lock_state()?;
    let current_pid = std::process::id();
    let pid_cleanup = (|| -> Result<()> {
        let path = pid_path()?;
        if let Some(recorded_pid) = read_pid_marker_for_mutation(&path)? {
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
    prepare_state_directory()?;
    read_pid_for_stop_at(
        &pid_path()?,
        &ready_path()?,
        &state_dir()?.join("daemon.lock"),
        || {},
    )
}

fn read_pid_for_stop_at(
    pid_path: &Path,
    ready_path: &Path,
    lock_path: &Path,
    before_cleanup: impl FnOnce(),
) -> Result<Option<u32>> {
    let _lock = lock_file(lock_path, true)?;
    let pid = read_pid_marker_for_mutation(pid_path)?;
    before_cleanup();
    if pid.is_none() {
        clear_marker_unlocked(ready_path)?;
    }
    Ok(pid)
}

pub fn clear_stopped_daemon_state(expected_pid: u32) -> Result<()> {
    prepare_state_directory()?;
    clear_stopped_daemon_state_at(
        &pid_path()?,
        &ready_path()?,
        &state_dir()?.join("daemon.lock"),
        expected_pid,
        || {},
    )
}

fn clear_stopped_daemon_state_at(
    pid_path: &Path,
    ready_path: &Path,
    lock_path: &Path,
    expected_pid: u32,
    before_ready_unlink: impl FnOnce(),
) -> Result<()> {
    let _lock = lock_file(lock_path, true)?;
    let pid_cleanup = (|| -> Result<()> {
        if read_pid_marker_for_mutation(pid_path)? == Some(expected_pid) {
            fs::remove_file(pid_path)?;
        }
        Ok(())
    })();
    let ready_cleanup =
        clear_marker_for_pid_unlocked(ready_path, Some(expected_pid), before_ready_unlink);
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
    prepare_state_directory()?;
    write_daemon_ready_at(
        &ready_path()?,
        &state_dir()?.join("daemon.lock"),
        std::process::id(),
    )
}

fn write_daemon_ready_at(path: &Path, lock_path: &Path, pid: u32) -> Result<()> {
    let _lock = lock_file(lock_path, true)?;
    write_owner_only_atomic(path, pid.to_string().as_bytes())
}

pub fn daemon_ready(pid: u32) -> Result<bool> {
    let _lock = lock_state()?;
    Ok(read_pid_marker_for_mutation(&ready_path()?)? == Some(pid))
}

pub fn clear_daemon_ready() -> Result<()> {
    let _lock = lock_state()?;
    clear_daemon_ready_unlocked()
}

fn clear_daemon_ready_unlocked() -> Result<()> {
    clear_marker_unlocked(&ready_path()?)
}

fn clear_marker_unlocked(path: &Path) -> Result<()> {
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
    clear_marker_for_pid_unlocked(&ready_path()?, expected_pid, || {})
}

fn clear_marker_for_pid_unlocked(
    path: &Path,
    expected_pid: Option<u32>,
    before_unlink: impl FnOnce(),
) -> Result<()> {
    if expected_pid.is_none() {
        return clear_marker_unlocked(path);
    }
    let ready_pid = read_pid_marker_for_mutation(path)?;
    if ready_pid.is_some() && (expected_pid.is_none() || ready_pid == expected_pid) {
        before_unlink();
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
    if !metadata.is_file() {
        return Err(Error::DoctorFailed(
            "daemon state marker is not a regular file".to_string(),
        ));
    }
    if metadata.len() > MAX_PID_MARKER_BYTES {
        return Err(Error::MalformedDaemonMarker);
    }
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(MAX_PID_MARKER_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_PID_MARKER_BYTES {
        return Err(Error::MalformedDaemonMarker);
    }
    let value = std::str::from_utf8(&bytes)
        .ok()
        .and_then(|value| value.trim().parse::<u32>().ok())
        .filter(|pid| *pid != 0)
        .ok_or(Error::MalformedDaemonMarker)?;
    Ok(Some(value))
}

fn read_pid_marker_for_mutation(path: &Path) -> Result<Option<u32>> {
    match read_pid_marker(path) {
        Err(Error::MalformedDaemonMarker) => {
            clear_marker_unlocked(path)?;
            Ok(None)
        }
        result => result,
    }
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
    fn mutation_paths_recover_malformed_markers_as_stale() {
        let root = std::env::temp_dir().join(format!(
            "pasteforward-malformed-marker-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let pid_path = root.join("daemon.pid");
        let ready_path = root.join("daemon.ready");
        let lock_path = root.join("daemon.lock");
        std::fs::write(&pid_path, b"truncated").unwrap();
        write_pid_at(&pid_path, &ready_path, &lock_path, 123).unwrap();
        assert_eq!(read_pid_marker(&pid_path).unwrap(), Some(123));

        std::fs::write(&pid_path, vec![b'1'; MAX_PID_MARKER_BYTES as usize + 1]).unwrap();
        assert!(read_pid_marker(&pid_path).is_err());
        write_pid_at(&pid_path, &ready_path, &lock_path, 456).unwrap();
        assert_eq!(read_pid_marker(&pid_path).unwrap(), Some(456));

        std::fs::write(&ready_path, b"not-a-pid").unwrap();
        assert_eq!(read_pid_marker_for_mutation(&ready_path).unwrap(), None);
        assert!(!ready_path.exists());

        std::fs::write(&ready_path, vec![b'2'; MAX_PID_MARKER_BYTES as usize + 1]).unwrap();
        assert!(read_pid_marker(&ready_path).is_err());
        assert_eq!(read_pid_marker_for_mutation(&ready_path).unwrap(), None);
        assert!(!ready_path.exists());

        std::fs::write(&pid_path, b"truncated").unwrap();
        std::fs::write(&ready_path, b"123").unwrap();
        assert_eq!(
            read_pid_for_stop_at(&pid_path, &ready_path, &lock_path, || {}).unwrap(),
            None
        );
        assert!(!pid_path.exists());
        assert!(!ready_path.exists());

        std::fs::write(&pid_path, vec![b'3'; MAX_PID_MARKER_BYTES as usize + 1]).unwrap();
        std::fs::write(&ready_path, b"123").unwrap();
        assert_eq!(
            read_pid_for_stop_at(&pid_path, &ready_path, &lock_path, || {}).unwrap(),
            None
        );
        assert!(!pid_path.exists());
        assert!(!ready_path.exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn write_pid_replaces_a_live_pid_that_is_not_a_pasteforward_daemon() {
        let root = std::env::temp_dir().join(format!(
            "pasteforward-reused-pid-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let pid_path = root.join("daemon.pid");
        let ready_path = root.join("daemon.ready");
        let lock_path = root.join("daemon.lock");
        let live_non_daemon_pid = std::process::id();
        let replacement_pid = live_non_daemon_pid.checked_add(1).unwrap();
        assert!(process_alive(live_non_daemon_pid));
        assert!(!process_is_pasteforward_daemon(live_non_daemon_pid));

        std::fs::write(&pid_path, live_non_daemon_pid.to_string()).unwrap();
        std::fs::write(&ready_path, live_non_daemon_pid.to_string()).unwrap();
        write_pid_at(&pid_path, &ready_path, &lock_path, replacement_pid).unwrap();

        assert_eq!(read_pid_marker(&pid_path).unwrap(), Some(replacement_pid));
        assert!(!ready_path.exists());

        std::fs::write(&ready_path, replacement_pid.to_string()).unwrap();
        write_pid_at(&pid_path, &ready_path, &lock_path, replacement_pid).unwrap();
        assert!(!ready_path.exists());
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
        let first = lock_file(&lock_path, true).unwrap();
        assert!(matches!(
            lock_file_with_policy(&lock_path, false, 2, Duration::from_millis(5)),
            Err(Error::StateLockTimedOut { .. })
        ));
        let (sender, receiver) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            let _second = lock_file(&lock_path, false).unwrap();
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

    #[cfg(unix)]
    #[test]
    fn observational_lock_rejects_untrusted_modes_owners_and_file_types() {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::fs::PermissionsExt;

        let root = std::env::temp_dir().join(format!(
            "pasteforward-observational-lock-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let mut root_permissions = std::fs::metadata(&root).unwrap().permissions();
        root_permissions.set_mode(0o700);
        std::fs::set_permissions(&root, root_permissions).unwrap();
        let lock_path = root.join("daemon.lock");

        std::fs::write(&lock_path, b"").unwrap();
        let mut lock_permissions = std::fs::metadata(&lock_path).unwrap().permissions();
        lock_permissions.set_mode(0o666);
        std::fs::set_permissions(&lock_path, lock_permissions).unwrap();
        assert!(lock_file(&lock_path, false).is_err());

        let metadata = std::fs::metadata(&lock_path).unwrap();
        use std::os::unix::fs::MetadataExt;
        assert!(!owner_mode_is_trusted(
            metadata.uid(),
            0o600,
            metadata.uid().wrapping_add(1)
        ));
        std::fs::remove_file(&lock_path).unwrap();

        let target = root.join("target");
        std::fs::write(&target, b"").unwrap();
        std::os::unix::fs::symlink(&target, &lock_path).unwrap();
        assert!(lock_file(&lock_path, false).is_err());
        std::fs::remove_file(&lock_path).unwrap();

        let fifo_path = CString::new(lock_path.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(fifo_path.as_ptr(), 0o600) }, 0);
        assert!(lock_file(&lock_path, false).is_err());
        std::fs::remove_file(&lock_path).unwrap();

        let mut root_permissions = std::fs::metadata(&root).unwrap().permissions();
        root_permissions.set_mode(0o777);
        std::fs::set_permissions(&root, root_permissions).unwrap();
        assert!(validate_state_directory(&root, true).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn marker_protocol_preserves_publication_across_cleanup_interleavings() {
        let root = std::env::temp_dir().join(format!(
            "pasteforward-state-protocol-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let pid_path = root.join("daemon.pid");
        let ready_path = root.join("daemon.ready");
        let lock_path = root.join("daemon.lock");

        write_daemon_ready_at(&ready_path, &lock_path, 111).unwrap();
        let (entered_sender, entered_receiver) = std::sync::mpsc::channel();
        let (release_sender, release_receiver) = std::sync::mpsc::channel();
        let cleanup_pid = pid_path.clone();
        let cleanup_ready = ready_path.clone();
        let cleanup_lock = lock_path.clone();
        let cleanup = std::thread::spawn(move || {
            read_pid_for_stop_at(&cleanup_pid, &cleanup_ready, &cleanup_lock, || {
                entered_sender.send(()).unwrap();
                release_receiver.recv().unwrap();
            })
            .unwrap()
        });
        entered_receiver.recv().unwrap();
        let publish_pid = pid_path.clone();
        let publish_ready = ready_path.clone();
        let publish_lock = lock_path.clone();
        let (published_sender, published_receiver) = std::sync::mpsc::channel();
        let publisher = std::thread::spawn(move || {
            write_pid_at(&publish_pid, &publish_ready, &publish_lock, 222).unwrap();
            write_daemon_ready_at(&publish_ready, &publish_lock, 222).unwrap();
            published_sender.send(()).unwrap();
        });
        assert!(
            published_receiver
                .recv_timeout(Duration::from_millis(50))
                .is_err()
        );
        release_sender.send(()).unwrap();
        assert_eq!(cleanup.join().unwrap(), None);
        publisher.join().unwrap();
        assert_eq!(read_pid_marker(&pid_path).unwrap(), Some(222));
        assert_eq!(read_pid_marker(&ready_path).unwrap(), Some(222));

        std::fs::remove_file(&pid_path).unwrap();
        write_pid_at(&pid_path, &ready_path, &lock_path, 333).unwrap();
        write_daemon_ready_at(&ready_path, &lock_path, 333).unwrap();
        let (entered_sender, entered_receiver) = std::sync::mpsc::channel();
        let (release_sender, release_receiver) = std::sync::mpsc::channel();
        let cleanup_pid = pid_path.clone();
        let cleanup_ready = ready_path.clone();
        let cleanup_lock = lock_path.clone();
        let cleanup = std::thread::spawn(move || {
            clear_stopped_daemon_state_at(&cleanup_pid, &cleanup_ready, &cleanup_lock, 333, || {
                entered_sender.send(()).unwrap();
                release_receiver.recv().unwrap();
            })
            .unwrap();
        });
        entered_receiver.recv().unwrap();
        let publish_ready = ready_path.clone();
        let publish_lock = lock_path.clone();
        let (published_sender, published_receiver) = std::sync::mpsc::channel();
        let publisher = std::thread::spawn(move || {
            write_daemon_ready_at(&publish_ready, &publish_lock, 444).unwrap();
            published_sender.send(()).unwrap();
        });
        assert!(
            published_receiver
                .recv_timeout(Duration::from_millis(50))
                .is_err()
        );
        release_sender.send(()).unwrap();
        cleanup.join().unwrap();
        publisher.join().unwrap();
        assert_eq!(read_pid_marker(&ready_path).unwrap(), Some(444));

        std::fs::remove_dir_all(root).unwrap();
    }
}
