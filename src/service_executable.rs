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
            .find(|entry| entry.is_file())
            .ok_or_else(|| {
                Error::UnsupportedPlatform(format!(
                    "cannot resolve executable on PATH: {}",
                    invoked.display()
                ))
            })?
    };
    if !candidate.is_absolute() || !candidate.is_file() {
        return Err(Error::UnsupportedPlatform(format!(
            "service executable is not an absolute file path: {}",
            candidate.display()
        )));
    }
    Ok(candidate)
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
}
