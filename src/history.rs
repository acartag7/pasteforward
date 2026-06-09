use crate::config::{AppConfig, RemoteMode, history_path, image_history_dir};
use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEvent {
    pub schema_version: u32,
    pub unix_ms: u128,
    pub destination: String,
    pub host: String,
    pub sha256: String,
    pub bytes: usize,
    pub remote_path: String,
    pub remote_mode: RemoteMode,
    pub image_history_path: Option<String>,
}

pub fn append_transfer(
    config: &AppConfig,
    destination: &str,
    host: &str,
    sha256: &str,
    bytes: &[u8],
    remote_path: &str,
    remote_mode: &RemoteMode,
) -> Result<()> {
    if !config.history.metadata {
        return Ok(());
    }

    fs::create_dir_all(crate::config::state_dir()?)?;

    let image_history_path = if config.history.image {
        let dir = image_history_dir()?.join(destination);
        fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{sha256}.png"));
        if !path.exists() {
            fs::write(&path, bytes)?;
        }
        Some(path.to_string_lossy().to_string())
    } else {
        None
    };

    let event = HistoryEvent {
        schema_version: 1,
        unix_ms: unix_ms(),
        destination: destination.to_string(),
        host: host.to_string(),
        sha256: sha256.to_string(),
        bytes: bytes.len(),
        remote_path: remote_path.to_string(),
        remote_mode: remote_mode.clone(),
        image_history_path,
    };

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(history_path()?)?;
    let line = serde_json::to_vec(&event)?;
    file.write_all(&line)?;
    file.write_all(b"\n")?;
    Ok(())
}

pub fn read_history(destination: Option<&str>, limit: usize) -> Result<Vec<HistoryEvent>> {
    let path = history_path()?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = fs::read_to_string(path)?;
    let mut events = Vec::new();
    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let event: HistoryEvent = serde_json::from_str(line)?;
        if destination.is_none_or(|dest| dest == event.destination) {
            events.push(event);
        }
    }
    if events.len() > limit {
        Ok(events.split_off(events.len() - limit))
    } else {
        Ok(events)
    }
}

pub fn purge_all_history() -> Result<()> {
    let path = history_path()?;
    if path.exists() {
        fs::remove_file(path)?;
    }
    let dir = image_history_dir()?;
    if dir.exists() {
        fs::remove_dir_all(dir)?;
    }
    Ok(())
}

pub fn purge_destination_history(destination: &str) -> Result<()> {
    let path = history_path()?;
    if path.exists() {
        let content = fs::read_to_string(&path)?;
        let mut kept = Vec::new();
        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let event: HistoryEvent = serde_json::from_str(line)?;
            if event.destination != destination {
                kept.push(line.to_string());
            }
        }
        fs::write(
            &path,
            kept.join("\n") + if kept.is_empty() { "" } else { "\n" },
        )?;
    }
    let dir = image_history_dir()?.join(destination);
    if dir.exists() {
        fs::remove_dir_all(dir)?;
    }
    Ok(())
}

fn unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
