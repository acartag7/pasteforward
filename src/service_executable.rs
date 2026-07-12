use crate::error::{Error, Result};
use std::path::{Path, PathBuf};

pub fn stable_executable_path() -> Result<PathBuf> {
    let invoked = std::env::args_os()
        .next()
        .ok_or_else(|| Error::UnsupportedPlatform("executable argument is missing".to_string()))?;
    let invoked = PathBuf::from(invoked);
    let cwd = std::env::current_dir()?;
    resolve_invoked_path(&invoked, std::env::var_os("PATH").as_deref(), &cwd)
}

fn resolve_invoked_path(
    invoked: &Path,
    path: Option<&std::ffi::OsStr>,
    cwd: &Path,
) -> Result<PathBuf> {
    let candidate = if invoked.components().count() > 1 {
        if invoked.is_absolute() {
            invoked.to_path_buf()
        } else {
            cwd.join(invoked)
        }
    } else {
        let path = path.ok_or_else(|| {
            Error::UnsupportedPlatform("PATH is not set; cannot install service".to_string())
        })?;
        std::env::split_paths(path)
            .map(|dir| dir.join(invoked))
            .find(|entry| is_executable_file(entry))
            .ok_or_else(|| {
                Error::UnsupportedPlatform(format!(
                    "cannot resolve executable on PATH: {}",
                    invoked.display()
                ))
            })?
    };
    if !candidate.is_absolute() || !is_executable_file(&candidate) {
        return Err(Error::UnsupportedPlatform(format!(
            "service executable is not an absolute executable file: {}",
            candidate.display()
        )));
    }
    Ok(candidate)
}

#[cfg(unix)]
fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    path.metadata()
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}

pub fn systemd_quote(path: &Path) -> String {
    format!(
        "\"{}\"",
        path.to_string_lossy()
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_systemd_executable_paths() {
        assert_eq!(
            systemd_quote(Path::new("/tmp/Paste Forward/bin")),
            "\"/tmp/Paste Forward/bin\""
        );
        assert_eq!(systemd_quote(Path::new("/tmp/a\"b")), "\"/tmp/a\\\"b\"");
    }

    #[test]
    fn preserves_path_entry_instead_of_canonicalizing_it() {
        let dir = std::env::temp_dir().join(format!("pasteforward-path-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let executable = dir.join("pasteforward");
        std::fs::write(&executable, b"test").unwrap();
        make_executable(&executable);
        let resolved = resolve_invoked_path(
            Path::new("pasteforward"),
            Some(dir.as_os_str()),
            Path::new("/unused"),
        )
        .unwrap();
        assert_eq!(resolved, executable);
        std::fs::remove_file(executable).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn skips_non_executable_path_shadows() {
        let root =
            std::env::temp_dir().join(format!("pasteforward-path-shadow-{}", std::process::id()));
        let shadow_dir = root.join("shadow");
        let executable_dir = root.join("executable");
        std::fs::create_dir_all(&shadow_dir).unwrap();
        std::fs::create_dir_all(&executable_dir).unwrap();
        std::fs::write(shadow_dir.join("pasteforward"), b"not executable").unwrap();
        let executable = executable_dir.join("pasteforward");
        std::fs::write(&executable, b"executable").unwrap();
        make_executable(&executable);
        let path = std::env::join_paths([shadow_dir, executable_dir]).unwrap();

        let resolved = resolve_invoked_path(
            Path::new("pasteforward"),
            Some(path.as_os_str()),
            Path::new("/unused"),
        )
        .unwrap();

        assert_eq!(resolved, executable);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    fn make_executable(path: &Path) {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = std::fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions).unwrap();
    }

    #[cfg(not(unix))]
    fn make_executable(_path: &Path) {}
}
