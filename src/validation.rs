use crate::config::AppConfig;
use crate::error::{Error, Result};

pub const ALLOWED_REMOTE_ENV: [&str; 3] = ["DISPLAY", "WAYLAND_DISPLAY", "XDG_RUNTIME_DIR"];
const MAX_DESTINATIONS: usize = 64;
const MAX_NAME_BYTES: usize = 64;
const MAX_HOST_BYTES: usize = 512;
const MAX_PATH_BYTES: usize = 4096;
const MAX_ENV_VALUE_BYTES: usize = 4096;

pub fn validate_destination_name(name: &str) -> Result<()> {
    if name.is_empty() || name.len() > MAX_NAME_BYTES {
        return Err(Error::InvalidDestination(format!(
            "destination name must be 1 to {MAX_NAME_BYTES} bytes"
        )));
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

pub fn validate_host(host: &str) -> Result<()> {
    if host.is_empty()
        || host.len() > MAX_HOST_BYTES
        || host.starts_with('-')
        || host.chars().any(char::is_control)
    {
        return Err(Error::InvalidDestination(
            "SSH host must be 1 to 512 bytes, must not begin with '-', and must not contain control characters"
                .to_string(),
        ));
    }
    Ok(())
}

pub fn validate_remote_env(key: &str, value: &str) -> Result<()> {
    if !ALLOWED_REMOTE_ENV.contains(&key) {
        return Err(Error::InvalidDestination(format!(
            "remote environment key must be DISPLAY, WAYLAND_DISPLAY, or XDG_RUNTIME_DIR; got {key}"
        )));
    }
    if value.is_empty() || value.len() > MAX_ENV_VALUE_BYTES || value.chars().any(char::is_control)
    {
        return Err(Error::InvalidDestination(format!(
            "remote environment value for {key} must be non-empty and contain no control characters"
        )));
    }
    Ok(())
}

pub fn validate_config(config: &AppConfig) -> Result<()> {
    if config.version != 1 {
        return Err(Error::InvalidDestination(format!(
            "unsupported config version: {}",
            config.version
        )));
    }
    if config.destinations.len() > MAX_DESTINATIONS {
        return Err(Error::InvalidDestination(format!(
            "config has more than {MAX_DESTINATIONS} destinations"
        )));
    }
    validate_remote_dir(&config.remote_dir)?;
    if config.retention.ttl_seconds == 0 {
        return Err(Error::InvalidDestination(
            "ttl_seconds must be greater than zero".to_string(),
        ));
    }
    if config.daemon.interval_millis < 250 {
        return Err(Error::InvalidDestination(
            "interval_millis must be at least 250".to_string(),
        ));
    }
    for (name, dest) in &config.destinations {
        validate_destination(name, dest)?;
    }
    Ok(())
}

pub fn validate_destination(name: &str, dest: &crate::config::DestinationConfig) -> Result<()> {
    validate_destination_name(name)?;
    validate_host(&dest.host)?;
    if let Some(remote_dir) = &dest.remote_dir {
        validate_remote_dir(remote_dir)?;
    }
    for (key, value) in &dest.remote_env {
        validate_remote_env(key, value)?;
    }
    Ok(())
}

pub fn validate_remote_dir(value: &str) -> Result<()> {
    let segments = value.strip_prefix('/').map(|rest| rest.split('/'));
    let invalid = value.is_empty()
        || value.len() > MAX_PATH_BYTES
        || value.chars().any(char::is_control)
        || value.starts_with("//")
        || segments.is_none()
        || segments.is_some_and(|mut parts| {
            let mut count = 0;
            let invalid = parts.any(|part| {
                count += 1;
                part.is_empty() || part == "." || part == ".."
            });
            invalid || count == 0
        });
    if invalid {
        return Err(Error::InvalidDestination(format!(
            "remote directory must be a non-root absolute path up to 4096 bytes without control characters, '.' or '..': {value}"
        )));
    }
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
    fn rejects_option_shaped_hosts_and_unknown_remote_env() {
        assert!(validate_host("-V").is_err());
        assert!(validate_host("host\nname").is_err());
        assert!(validate_remote_env("PATH", "/tmp").is_err());
        assert!(validate_remote_env("DISPLAY", ":0").is_ok());
    }

    #[test]
    fn rejects_invalid_complete_configs() {
        let mut config = AppConfig::empty();
        config.version = 2;
        assert!(validate_config(&config).is_err());
        config.version = 1;
        config.remote_dir = "relative".to_string();
        assert!(validate_config(&config).is_err());
        config.remote_dir = "/".to_string();
        assert!(validate_config(&config).is_err());
        for path in [
            "//",
            "///",
            "/tmp//pasteforward",
            "/tmp/./pf",
            "/tmp/../pf",
            "/tmp/pf/",
        ] {
            config.remote_dir = path.to_string();
            assert!(validate_config(&config).is_err(), "accepted {path}");
        }
        config.remote_dir = "/tmp/pasteforward".to_string();
        for index in 0..=MAX_DESTINATIONS {
            config.destinations.insert(
                format!("d{index}"),
                crate::config::DestinationConfig {
                    host: "example.test".to_string(),
                    enabled: true,
                    remote_mode: crate::config::RemoteMode::Auto,
                    remote_env: Default::default(),
                    remote_dir: None,
                },
            );
        }
        assert!(validate_config(&config).is_err());
    }

    #[test]
    fn direct_destination_validation_rejects_injected_environment_keys() {
        let mut dest = crate::config::DestinationConfig {
            host: "example.test".to_string(),
            enabled: true,
            remote_mode: crate::config::RemoteMode::LinuxX11,
            remote_env: Default::default(),
            remote_dir: None,
        };
        dest.remote_env
            .insert("DISPLAY;touch /tmp/pwned".to_string(), ":0".to_string());
        assert!(validate_destination("test", &dest).is_err());
    }
}
