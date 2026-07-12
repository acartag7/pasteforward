use crate::clipboard::detect_local_backend;
use crate::command::{shell_quote, ssh};
use crate::config::{AppConfig, DestinationConfig, RemoteMode};
use crate::error::Result;
use crate::remote::{remote_clipboard_probe_command, resolve_remote_mode};
use crate::validation::{validate_config, validate_destination};

#[derive(Debug, Clone)]
pub struct DestinationDoctor {
    pub name: String,
    pub host: String,
    pub enabled: bool,
    pub ssh_ok: bool,
    pub remote_mode: Option<RemoteMode>,
    pub remote_clipboard_ok: bool,
    pub remote_dir_ok: bool,
    pub problems: Vec<String>,
}

impl DestinationDoctor {
    pub fn ok(&self) -> bool {
        self.enabled
            && self.ssh_ok
            && self.remote_mode.is_some()
            && self.remote_clipboard_ok
            && self.remote_dir_ok
            && self.problems.is_empty()
    }
}

pub fn doctor_destination(
    config: &AppConfig,
    name: &str,
    dest: &DestinationConfig,
) -> DestinationDoctor {
    let mut result = DestinationDoctor {
        name: name.to_string(),
        host: dest.host.clone(),
        enabled: dest.enabled,
        ssh_ok: false,
        remote_mode: None,
        remote_clipboard_ok: false,
        remote_dir_ok: false,
        problems: Vec::new(),
    };

    if let Err(err) = validate_config(config).and_then(|()| validate_destination(name, dest)) {
        result.problems.push(format!("invalid config: {err}"));
        return result;
    }

    if !dest.enabled {
        result.problems.push("destination is disabled".to_string());
    }

    match ssh(&dest.host, "true", None) {
        Ok(_) => result.ssh_ok = true,
        Err(err) => {
            result.problems.push(format!("ssh failed: {err}"));
            return result;
        }
    }

    match resolve_remote_mode(dest) {
        Ok(mode) => {
            result.remote_mode = Some(mode.clone());
            result.remote_clipboard_ok = check_remote_clipboard(dest, &mode);
            if !result.remote_clipboard_ok {
                let detail = match mode {
                    RemoteMode::LinuxWayland => {
                        "remote Wayland clipboard is unavailable; set WAYLAND_DISPLAY and XDG_RUNTIME_DIR and ensure wl-clipboard is installed"
                    }
                    RemoteMode::LinuxX11 => {
                        "remote X11 clipboard is unavailable; set DISPLAY and ensure xclip is installed"
                    }
                    RemoteMode::MacosPasteboard => {
                        "remote macOS pasteboard is unavailable in this SSH login session"
                    }
                    RemoteMode::Auto => "remote clipboard is unavailable",
                };
                result.problems.push(detail.to_string());
            }
        }
        Err(err) => result.problems.push(err.to_string()),
    }

    let remote_dir = config.destination_remote_dir(dest);
    let parent = remote_dir_parent(&remote_dir);
    let dir_cmd = format!(
        "if test -d {dir}; then test -w {dir}; else test -d {parent} && test -w {parent}; fi",
        dir = shell_quote(&remote_dir),
        parent = shell_quote(parent)
    );
    match ssh(&dest.host, &dir_cmd, None) {
        Ok(_) => result.remote_dir_ok = true,
        Err(err) => result.problems.push(format!("remote dir failed: {err}")),
    }

    result
}

pub fn prepare_remote_directory(config: &AppConfig, dest: &DestinationConfig) -> Result<()> {
    validate_config(config)?;
    validate_destination("remote", dest)?;
    let remote_dir = config.destination_remote_dir(dest);
    let command = format!(
        "umask 077 && mkdir -p {} && chmod 700 {} && test -w {}",
        shell_quote(&remote_dir),
        shell_quote(&remote_dir),
        shell_quote(&remote_dir)
    );
    ssh(&dest.host, &command, None)?;
    Ok(())
}

fn remote_dir_parent(remote_dir: &str) -> &str {
    remote_dir
        .trim_end_matches('/')
        .rsplit_once('/')
        .map_or(
            "/",
            |(parent, _)| if parent.is_empty() { "/" } else { parent },
        )
}

pub fn local_doctor_problem() -> Option<String> {
    detect_local_backend().err().map(|err| err.to_string())
}

fn check_remote_clipboard(dest: &DestinationConfig, remote_mode: &RemoteMode) -> bool {
    remote_clipboard_probe_command(dest, remote_mode)
        .and_then(|command| ssh(&dest.host, &command, None))
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn remote_directory_probe_is_read_only() {
        assert_eq!(remote_dir_parent("/tmp/pasteforward"), "/tmp");
        assert_eq!(remote_dir_parent("/cache"), "/");
    }

    #[test]
    fn configured_display_probe_does_not_expand_unset_shell_state() {
        let mut remote_env = BTreeMap::new();
        remote_env.insert("DISPLAY".to_string(), ":99".to_string());
        let dest = DestinationConfig {
            host: "example.test".to_string(),
            enabled: true,
            remote_mode: RemoteMode::LinuxX11,
            remote_env,
            remote_dir: None,
        };
        let command = remote_clipboard_probe_command(&dest, &RemoteMode::LinuxX11).unwrap();
        assert!(command.contains("export DISPLAY=':99'"));
        assert!(command.contains("${DISPLAY"));
        assert!(command.contains("/tmp/.X11-unix/X${display_number}"));
    }

    #[test]
    fn prepare_boundary_rejects_root_alias_before_ssh() {
        let mut config = AppConfig::empty();
        config.remote_dir = "//".to_string();
        let dest = DestinationConfig {
            host: "example.test".to_string(),
            enabled: true,
            remote_mode: RemoteMode::LinuxX11,
            remote_env: BTreeMap::new(),
            remote_dir: None,
        };
        assert!(prepare_remote_directory(&config, &dest).is_err());
    }
}
