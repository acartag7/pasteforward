use crate::error::{Error, Result};
use std::fs::File;
use std::path::{Component, Path, PathBuf};

#[cfg(unix)]
mod unix {
    use super::*;
    use std::ffi::{CString, OsStr};
    use std::io::Write;
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;

    pub fn open_read(path: &Path) -> Result<File> {
        let (parent, name) = open_parent(path, false)?;
        let fd = open_at(
            parent.as_raw_fd(),
            &name,
            libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            0,
        )?;
        Ok(unsafe { File::from_raw_fd(fd) })
    }

    pub fn atomic_write(path: &Path, content: &[u8]) -> Result<()> {
        let (parent, name) = open_parent(path, true)?;
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let tmp_name =
            OsStr::new(&format!(".pasteforward-{}-{nonce}.tmp", std::process::id())).to_os_string();
        let fd = open_at(
            parent.as_raw_fd(),
            &tmp_name,
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            0o600,
        )?;
        let mut tmp = unsafe { File::from_raw_fd(fd) };
        let result = (|| -> Result<()> {
            tmp.write_all(content)?;
            tmp.sync_all()?;
            call_fchmod(tmp.as_raw_fd(), 0o600)?;
            rename_at(parent.as_raw_fd(), &tmp_name, &name)?;
            parent.sync_all()?;
            Ok(())
        })();
        if result.is_err() {
            let _ = unlink_at(parent.as_raw_fd(), &tmp_name);
        }
        result
    }

    pub fn create_owner_only_dir(path: &Path) -> Result<()> {
        if std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
            return Err(Error::InvalidDestination(format!(
                "refusing directory symlink: {}",
                path.display()
            )));
        }
        let trusted = trusted_target(path)?;
        let components = normal_components(&trusted)?;
        let mut current = File::open("/")?;
        for (index, component) in components.iter().enumerate() {
            let last = index + 1 == components.len();
            let next = match open_directory_at(current.as_raw_fd(), component) {
                Ok(directory) => directory,
                Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                    mkdir_at(current.as_raw_fd(), component, 0o700)?;
                    open_directory_at(current.as_raw_fd(), component)?
                }
                Err(error) => return Err(error),
            };
            if last {
                call_fchmod(next.as_raw_fd(), 0o700)?;
            }
            current = next;
        }
        Ok(())
    }

    fn open_parent(path: &Path, create: bool) -> Result<(File, std::ffi::OsString)> {
        let name = path
            .file_name()
            .ok_or_else(|| {
                Error::InvalidDestination(format!("path has no file name: {}", path.display()))
            })?
            .to_os_string();
        let parent = path.parent().ok_or_else(|| {
            Error::InvalidDestination(format!("path has no parent: {}", path.display()))
        })?;
        if create {
            create_owner_only_dir(parent)?;
        }
        let trusted = trusted_target(parent)?;
        let mut current = File::open("/")?;
        for component in normal_components(&trusted)? {
            current = open_directory_at(current.as_raw_fd(), &component)?;
        }
        Ok((current, name))
    }

    fn trusted_target(path: &Path) -> Result<PathBuf> {
        if !path.is_absolute() {
            return Err(Error::InvalidDestination(format!(
                "local config and state paths must be absolute: {}",
                path.display()
            )));
        }
        let mut missing = Vec::new();
        let mut ancestor = path;
        while !ancestor.exists() {
            missing.push(
                ancestor
                    .file_name()
                    .ok_or_else(|| Error::InvalidDestination("invalid absolute path".to_string()))?
                    .to_os_string(),
            );
            ancestor = ancestor.parent().ok_or_else(|| {
                Error::InvalidDestination("absolute path has no parent".to_string())
            })?;
        }
        let mut trusted = ancestor.canonicalize()?;
        for component in missing.iter().rev() {
            trusted.push(component);
        }
        Ok(trusted)
    }

    fn normal_components(path: &Path) -> Result<Vec<std::ffi::OsString>> {
        path.components()
            .filter_map(|component| match component {
                Component::RootDir => None,
                Component::Normal(value) => Some(Ok(value.to_os_string())),
                _ => Some(Err(Error::InvalidDestination(format!(
                    "local path is not canonical: {}",
                    path.display()
                )))),
            })
            .collect()
    }

    fn open_directory_at(parent: i32, name: &OsStr) -> Result<File> {
        let fd = open_at(
            parent,
            name,
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            0,
        )?;
        Ok(unsafe { File::from_raw_fd(fd) })
    }

    fn open_at(parent: i32, name: &OsStr, flags: i32, mode: u32) -> Result<i32> {
        let name = c_string(name)?;
        let fd = unsafe { libc::openat(parent, name.as_ptr(), flags, mode as libc::c_uint) };
        if fd < 0 {
            Err(std::io::Error::last_os_error().into())
        } else {
            Ok(fd)
        }
    }

    fn mkdir_at(parent: i32, name: &OsStr, mode: u32) -> Result<()> {
        let name = c_string(name)?;
        if unsafe { libc::mkdirat(parent, name.as_ptr(), mode as libc::mode_t) } == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error().into())
        }
    }

    fn rename_at(parent: i32, old: &OsStr, new: &OsStr) -> Result<()> {
        let old = c_string(old)?;
        let new = c_string(new)?;
        if unsafe { libc::renameat(parent, old.as_ptr(), parent, new.as_ptr()) } == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error().into())
        }
    }

    fn unlink_at(parent: i32, name: &OsStr) -> Result<()> {
        let name = c_string(name)?;
        if unsafe { libc::unlinkat(parent, name.as_ptr(), 0) } == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error().into())
        }
    }

    fn call_fchmod(fd: i32, mode: u32) -> Result<()> {
        if unsafe { libc::fchmod(fd, mode as libc::mode_t) } == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error().into())
        }
    }

    fn c_string(value: &OsStr) -> Result<CString> {
        CString::new(value.as_bytes())
            .map_err(|_| Error::InvalidDestination("local path contains a NUL byte".to_string()))
    }
}

#[cfg(unix)]
pub use unix::{atomic_write, create_owner_only_dir, open_read};

#[cfg(not(unix))]
pub fn open_read(path: &Path) -> Result<File> {
    Ok(File::open(path)?)
}

#[cfg(not(unix))]
pub fn atomic_write(path: &Path, content: &[u8]) -> Result<()> {
    std::fs::write(path, content)?;
    Ok(())
}

#[cfg(not(unix))]
pub fn create_owner_only_dir(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path)?;
    Ok(())
}
