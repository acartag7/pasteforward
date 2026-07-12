use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

pub const DEFAULT_REMOTE_DIR: &str = "/tmp/pasteforward";
pub const DEFAULT_TTL_SECONDS: u64 = 3600;
pub const MAX_CONFIG_BYTES: u64 = 1024 * 1024;

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
#[serde(deny_unknown_fields)]
pub struct AppConfig {
    pub version: u32,
    pub remote_dir: String,
    pub retention: RetentionConfig,
    pub history: HistoryConfig,
    pub daemon: DaemonConfig,
    pub destinations: BTreeMap<String, DestinationConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetentionConfig {
    pub ttl_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryConfig {
    pub metadata: bool,
    pub image: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DaemonConfig {
    pub interval_millis: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
    if fs::symlink_metadata(&path)?.file_type().is_symlink() {
        return Err(Error::InvalidDestination(format!(
            "refusing to read config through symlink: {}",
            path.display()
        )));
    }
    let mut file = crate::secure_fs::open_read(&path)?;
    if file.metadata()?.len() > MAX_CONFIG_BYTES {
        return Err(Error::LimitExceeded(format!(
            "config exceeds {MAX_CONFIG_BYTES} byte limit"
        )));
    }
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(MAX_CONFIG_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_CONFIG_BYTES {
        return Err(Error::LimitExceeded(format!(
            "config exceeds {MAX_CONFIG_BYTES} byte limit"
        )));
    }
    let config = serde_json::from_slice(&bytes)?;
    crate::validation::validate_config(&config)?;
    Ok(config)
}

pub fn remove_config() -> Result<()> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(());
    }
    if fs::symlink_metadata(&path)?.file_type().is_symlink() {
        return Err(Error::InvalidDestination(format!(
            "refusing to remove config symlink: {}",
            path.display()
        )));
    }
    fs::remove_file(path)?;
    Ok(())
}

pub fn save_config(config: &AppConfig) -> Result<()> {
    crate::validation::validate_config(config)?;
    let path = config_path()?;
    if let Some(parent) = path.parent() {
        create_owner_only_dir(parent)?;
    }
    let data = serde_json::to_vec_pretty(config)?;
    let mut content = data;
    content.push(b'\n');
    write_owner_only_atomic(&path, &content)?;
    Ok(())
}

pub fn write_owner_only_atomic(path: &Path, content: &[u8]) -> Result<()> {
    crate::secure_fs::atomic_write(path, content)
}

pub fn ensure_state_dirs(config: &AppConfig) -> Result<()> {
    create_owner_only_dir(&state_dir()?)?;
    if config.history.image {
        create_owner_only_dir(&image_history_dir()?)?;
    }
    Ok(())
}

pub fn create_owner_only_dir(path: &Path) -> Result<()> {
    crate::secure_fs::create_owner_only_dir(path)
}

fn home_dir() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| Error::UnsupportedPlatform("HOME is not set".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn remote_mode_round_trips() {
        assert_eq!(
            RemoteMode::parse("linux-wayland").unwrap(),
            RemoteMode::LinuxWayland
        );
        assert_eq!(RemoteMode::LinuxX11.as_str(), "linux-x11");
    }

    #[test]
    fn rejects_unknown_config_fields() {
        let json = serde_json::to_string(&AppConfig::empty()).unwrap();
        let json = json.replacen('{', "{\"unknown\":true,", 1);
        assert!(serde_json::from_str::<AppConfig>(&json).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn no_follow_open_rejects_config_symlinks() {
        use std::os::unix::fs::symlink;
        let root = test_dir("config-symlink");
        create_owner_only_dir(&root).unwrap();
        let target = root.join("target");
        fs::write(&target, b"{}").unwrap();
        let link = root.join("config.json");
        symlink(&target, &link).unwrap();
        assert!(crate::secure_fs::open_read(&link).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn newly_created_directory_chain_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let root = test_dir("directory-modes");
        let nested = root.join("one").join("two");
        create_owner_only_dir(&nested).unwrap();
        for path in [&root, &root.join("one"), &nested] {
            assert_eq!(
                fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o700
            );
        }
        fs::remove_dir_all(root).unwrap();
    }

    fn test_dir(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "pasteforward-{label}-{}-{nonce}",
            std::process::id()
        ))
    }
}
