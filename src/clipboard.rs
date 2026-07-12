use crate::command::{MAX_COMMAND_INPUT_BYTES, run, run_ok};
use crate::config::{create_owner_only_dir, state_dir};
use crate::error::{Error, Result};
use sha2::{Digest, Sha256};
use std::fs;

#[derive(Debug, Clone)]
pub struct ClipboardImage {
    pub bytes: Vec<u8>,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalClipboardBackend {
    MacosPasteboard,
    LinuxWayland,
    LinuxX11,
}

impl LocalClipboardBackend {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::MacosPasteboard => "macos-pasteboard",
            Self::LinuxWayland => "linux-wayland",
            Self::LinuxX11 => "linux-x11",
        }
    }
}

pub fn detect_local_backend() -> Result<LocalClipboardBackend> {
    if cfg!(target_os = "macos") {
        let args = vec!["-e".to_string(), "return 1".to_string()];
        if run_ok("osascript", &args, None) {
            return Ok(LocalClipboardBackend::MacosPasteboard);
        }
        return Err(Error::UnsupportedPlatform(
            "macOS clipboard support requires /usr/bin/osascript".to_string(),
        ));
    }

    if cfg!(target_os = "linux") {
        if local_wayland_socket_reachable() && run_ok("wl-paste", &["--version".to_string()], None)
        {
            return Ok(LocalClipboardBackend::LinuxWayland);
        }
        if local_x11_socket_reachable() && run_ok("xclip", &["-version".to_string()], None) {
            return Ok(LocalClipboardBackend::LinuxX11);
        }
        return Err(Error::UnsupportedPlatform(
            "Linux clipboard support requires a reachable Wayland session with wl-paste or an X11 session with xclip"
                .to_string(),
        ));
    }

    Err(Error::UnsupportedPlatform(
        "pasteforward v0 supports local macOS and Linux only".to_string(),
    ))
}

fn local_wayland_socket_reachable() -> bool {
    let (Some(runtime), Some(display)) = (
        std::env::var_os("XDG_RUNTIME_DIR"),
        std::env::var_os("WAYLAND_DISPLAY"),
    ) else {
        return false;
    };
    let socket = std::path::PathBuf::from(runtime).join(display);
    is_unix_socket(&socket)
}

fn local_x11_socket_reachable() -> bool {
    let Some(display) = std::env::var_os("DISPLAY") else {
        return false;
    };
    let Some(socket) = x11_socket_path(&display.to_string_lossy()) else {
        return false;
    };
    is_unix_socket(&socket)
}

fn x11_socket_path(display: &str) -> Option<std::path::PathBuf> {
    let value = display
        .strip_prefix("unix:")
        .or_else(|| display.strip_prefix(':'))?;
    let number = value.split('.').next()?;
    if number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some(std::path::PathBuf::from(format!(
        "/tmp/.X11-unix/X{number}"
    )))
}

#[cfg(unix)]
fn is_unix_socket(path: &std::path::Path) -> bool {
    use std::os::unix::fs::{FileTypeExt, PermissionsExt};
    path.metadata().is_ok_and(|metadata| {
        let mode = metadata.permissions().mode();
        metadata.file_type().is_socket() && mode & 0o444 != 0 && mode & 0o222 != 0
    })
}

#[cfg(not(unix))]
fn is_unix_socket(_path: &std::path::Path) -> bool {
    false
}

pub fn read_image(backend: &LocalClipboardBackend) -> Result<Option<ClipboardImage>> {
    let bytes = match backend {
        LocalClipboardBackend::MacosPasteboard => read_macos_image()?,
        LocalClipboardBackend::LinuxWayland => read_wayland_image()?,
        LocalClipboardBackend::LinuxX11 => read_x11_image()?,
    };

    if bytes
        .as_ref()
        .is_some_and(|data| data.len() > MAX_COMMAND_INPUT_BYTES)
    {
        return Err(Error::LimitExceeded(format!(
            "clipboard image exceeds {} byte limit",
            MAX_COMMAND_INPUT_BYTES
        )));
    }

    Ok(bytes.map(|data| ClipboardImage {
        sha256: sha256_hex(&data),
        bytes: data,
    }))
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

fn read_macos_image() -> Result<Option<Vec<u8>>> {
    let tmp_dir = state_dir()?.join("tmp");
    create_owner_only_dir(&tmp_dir)?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let path = tmp_dir.join(format!("clipboard-{}-{nonce}.png", std::process::id()));
    let _file = fs::File::options()
        .write(true)
        .create_new(true)
        .open(&path)?;
    let cleanup = RemoveFile(path.clone());
    let path_str = path.to_string_lossy().to_string();
    let args = vec![
        "-e".to_string(),
        "set png_data to (the clipboard as «class PNGf»)".to_string(),
        "-e".to_string(),
        format!(
            "set fp to open for access POSIX file \"{}\" with write permission",
            path_str.replace('"', "\\\"")
        ),
        "-e".to_string(),
        "set eof fp to 0".to_string(),
        "-e".to_string(),
        "write png_data to fp".to_string(),
        "-e".to_string(),
        "close access fp".to_string(),
    ];

    if let Err(error) = run("osascript", &args, None) {
        return match error {
            Error::LimitExceeded(_) => Err(error),
            _ => Ok(None),
        };
    }

    let size = fs::metadata(&path)?.len();
    if size > MAX_COMMAND_INPUT_BYTES as u64 {
        return Err(Error::LimitExceeded(format!(
            "clipboard image exceeds {} byte limit",
            MAX_COMMAND_INPUT_BYTES
        )));
    }
    let data = fs::read(&path)?;
    drop(cleanup);
    if data.is_empty() {
        Ok(None)
    } else {
        Ok(Some(data))
    }
}

struct RemoveFile(std::path::PathBuf);

impl Drop for RemoveFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn read_wayland_image() -> Result<Option<Vec<u8>>> {
    let args = vec!["--type".to_string(), "image/png".to_string()];
    linux_clipboard_result(run("wl-paste", &args, None))
}

fn linux_clipboard_result(
    result: Result<crate::command::CommandOutput>,
) -> Result<Option<Vec<u8>>> {
    match result {
        Ok(output) if !output.stdout.is_empty() => Ok(Some(output.stdout)),
        Ok(_) => Ok(None),
        Err(error @ Error::LimitExceeded(_)) => Err(error),
        Err(_) => Ok(None),
    }
}

fn read_x11_image() -> Result<Option<Vec<u8>>> {
    let args = vec![
        "-selection".to_string(),
        "clipboard".to_string(),
        "-t".to_string(),
        "image/png".to_string(),
        "-o".to_string(),
    ];
    linux_clipboard_result(run("xclip", &args, None))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_are_stable() {
        assert_eq!(
            sha256_hex(b"pasteforward"),
            "78f5e7afb3df1001af7b63e844cdbd6a2b0aba819ba09269514790bcf8b70544"
        );
    }

    #[test]
    fn x11_display_parser_accepts_only_local_socket_forms() {
        assert_eq!(
            x11_socket_path(":99.0").unwrap(),
            std::path::PathBuf::from("/tmp/.X11-unix/X99")
        );
        assert_eq!(
            x11_socket_path("unix:1").unwrap(),
            std::path::PathBuf::from("/tmp/.X11-unix/X1")
        );
        assert!(x11_socket_path("example.test:0").is_none());
        assert!(x11_socket_path(":bad").is_none());
    }

    #[test]
    fn linux_clipboard_propagates_output_limit_errors() {
        let result = linux_clipboard_result(Err(Error::LimitExceeded("oversized".to_string())));
        assert!(matches!(result, Err(Error::LimitExceeded(_))));
        let result = linux_clipboard_result(Err(Error::CommandFailed {
            program: "xclip".to_string(),
            args: Vec::new(),
            code: Some(1),
            stderr: String::new(),
        }));
        assert!(matches!(result, Ok(None)));
    }
}
