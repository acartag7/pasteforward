use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;

pub const DEFAULT_REMOTE_DIR: &str = "/tmp/pasteforward";
pub const DEFAULT_TTL_SECONDS: u64 = 3600;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RemoteMode {
    Auto,
    MacosPasteboard,
    LinuxWayland,
    LinuxX11,
}

impl RemoteMode {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "auto" => Ok(Self::Auto),
            "macos-pasteboard" => Ok(Self::MacosPasteboard),
            "linux-wayland" => Ok(Self::LinuxWayland),
            "linux-x11" => Ok(Self::LinuxX11),
            _ => Err(Error::Usage(format!(
                "remote mode must be auto, macos-pasteboard, linux-wayland, or linux-x11; got {value}"
            ))),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::MacosPasteboard => "macos-pasteboard",
            Self::LinuxWayland => "linux-wayland",
            Self::LinuxX11 => "linux-x11",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub version: u32,
    pub remote_dir: String,
    pub retention: RetentionConfig,
    pub history: HistoryConfig,
    pub daemon: DaemonConfig,
    pub destinations: BTreeMap<String, DestinationConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetentionConfig {
    pub ttl_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryConfig {
    pub metadata: bool,
    pub image: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonConfig {
    pub interval_millis: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DestinationConfig {
    pub host: String,
    pub enabled: bool,
    pub remote_mode: RemoteMode,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub remote_env: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_dir: Option<String>,
}

impl AppConfig {
    pub fn empty() -> Self {
        Self {
            version: 1,
            remote_dir: DEFAULT_REMOTE_DIR.to_string(),
            retention: RetentionConfig {
                ttl_seconds: DEFAULT_TTL_SECONDS,
            },
            history: HistoryConfig {
                metadata: true,
                image: false,
            },
            daemon: DaemonConfig {
                interval_millis: 1000,
            },
            destinations: BTreeMap::new(),
        }
    }

    pub fn destination_remote_dir(&self, dest: &DestinationConfig) -> String {
        dest.remote_dir
            .clone()
            .unwrap_or_else(|| self.remote_dir.clone())
    }
}

pub fn validate_destination_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(Error::InvalidDestination(
            "destination name cannot be empty".to_string(),
        ));
    }
    let ok = name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !ok {
        return Err(Error::InvalidDestination(format!(
            "destination name may only contain ASCII letters, digits, '-' and '_': {name}"
        )));
    }
    Ok(())
}

pub fn config_dir() -> Result<PathBuf> {
    if let Ok(value) = std::env::var("PASTEFORWARD_CONFIG_HOME") {
        return Ok(PathBuf::from(value));
    }
    if let Ok(value) = std::env::var("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(value).join("pasteforward"));
    }
    Ok(home_dir()?.join(".config").join("pasteforward"))
}

pub fn state_dir() -> Result<PathBuf> {
    if let Ok(value) = std::env::var("PASTEFORWARD_STATE_HOME") {
        return Ok(PathBuf::from(value));
    }
    if let Ok(value) = std::env::var("XDG_STATE_HOME") {
        return Ok(PathBuf::from(value).join("pasteforward"));
    }
    Ok(home_dir()?
        .join(".local")
        .join("state")
        .join("pasteforward"))
}

pub fn config_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("config.json"))
}

pub fn history_path() -> Result<PathBuf> {
    Ok(state_dir()?.join("history.jsonl"))
}

pub fn image_history_dir() -> Result<PathBuf> {
    Ok(state_dir()?.join("images"))
}

pub fn load_config() -> Result<AppConfig> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(AppConfig::empty());
    }
    let bytes = fs::read(path)?;
    Ok(serde_json::from_slice(&bytes)?)
}

pub fn save_config(config: &AppConfig) -> Result<()> {
    let path = config_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    let data = serde_json::to_vec_pretty(config)?;
    {
        let mut file = File::create(&tmp)?;
        file.write_all(&data)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
    }
    set_owner_only_file(&tmp)?;
    fs::rename(tmp, path)?;
    Ok(())
}

pub fn ensure_state_dirs(config: &AppConfig) -> Result<()> {
    fs::create_dir_all(state_dir()?)?;
    if config.history.image {
        fs::create_dir_all(image_history_dir()?)?;
    }
    Ok(())
}

fn home_dir() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| Error::UnsupportedPlatform("HOME is not set".to_string()))
}

#[cfg(unix)]
fn set_owner_only_file(path: &PathBuf) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o600);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(not(unix))]
fn set_owner_only_file(_path: &PathBuf) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_destination_names() {
        assert!(validate_destination_name("macmini-1").is_ok());
        assert!(validate_destination_name("bad/name").is_err());
    }

    #[test]
    fn remote_mode_round_trips() {
        assert_eq!(
            RemoteMode::parse("linux-wayland").unwrap(),
            RemoteMode::LinuxWayland
        );
        assert_eq!(RemoteMode::LinuxX11.as_str(), "linux-x11");
    }
}
