use crate::command::{run, run_ok};
use crate::config::state_dir;
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
        if run_ok("wl-paste", &["--version".to_string()], None) {
            return Ok(LocalClipboardBackend::LinuxWayland);
        }
        if run_ok("xclip", &["-version".to_string()], None) {
            return Ok(LocalClipboardBackend::LinuxX11);
        }
        return Err(Error::UnsupportedPlatform(
            "Linux clipboard support requires wl-paste or xclip".to_string(),
        ));
    }

    Err(Error::UnsupportedPlatform(
        "pasteforward v0 supports local macOS and Linux only".to_string(),
    ))
}

pub fn read_image(backend: &LocalClipboardBackend) -> Result<Option<ClipboardImage>> {
    let bytes = match backend {
        LocalClipboardBackend::MacosPasteboard => read_macos_image()?,
        LocalClipboardBackend::LinuxWayland => read_wayland_image()?,
        LocalClipboardBackend::LinuxX11 => read_x11_image()?,
    };

    Ok(bytes.map(|data| ClipboardImage {
        sha256: sha256_hex(&data),
        bytes: data,
    }))
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

fn read_macos_image() -> Result<Option<Vec<u8>>> {
    let tmp_dir = state_dir()?.join("tmp");
    fs::create_dir_all(&tmp_dir)?;
    let path = tmp_dir.join(format!("clipboard-{}.png", std::process::id()));
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

    if run("osascript", &args, None).is_err() {
        let _ = fs::remove_file(&path);
        return Ok(None);
    }

    let data = fs::read(&path)?;
    let _ = fs::remove_file(&path);
    if data.is_empty() {
        Ok(None)
    } else {
        Ok(Some(data))
    }
}

fn read_wayland_image() -> Result<Option<Vec<u8>>> {
    let args = vec!["--type".to_string(), "image/png".to_string()];
    match run("wl-paste", &args, None) {
        Ok(output) if !output.stdout.is_empty() => Ok(Some(output.stdout)),
        _ => Ok(None),
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
    match run("xclip", &args, None) {
        Ok(output) if !output.stdout.is_empty() => Ok(Some(output.stdout)),
        _ => Ok(None),
    }
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
}
