use crate::clipboard::detect_local_backend;
use crate::command::{applescript_string, shell_quote, ssh};
use crate::config::{AppConfig, DestinationConfig, RemoteMode};
use crate::error::{Error, Result};

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
        }
        Err(err) => result.problems.push(err.to_string()),
    }

    let remote_dir = config.destination_remote_dir(dest);
    let dir_cmd = format!(
        "umask 077 && mkdir -p {} && chmod 700 {} && test -w {}",
        shell_quote(&remote_dir),
        shell_quote(&remote_dir),
        shell_quote(&remote_dir)
    );
    match ssh(&dest.host, &dir_cmd, None) {
        Ok(_) => result.remote_dir_ok = true,
        Err(err) => result.problems.push(format!("remote dir failed: {err}")),
    }

    result
}

pub fn resolve_remote_mode(dest: &DestinationConfig) -> Result<RemoteMode> {
    if dest.remote_mode != RemoteMode::Auto {
        return Ok(dest.remote_mode.clone());
    }

    let script = r#"uname_s="$(uname -s 2>/dev/null || true)"
if [ "$uname_s" = "Darwin" ]; then
  printf macos-pasteboard
elif command -v wl-copy >/dev/null 2>&1; then
  printf linux-wayland
elif command -v xclip >/dev/null 2>&1; then
  printf linux-x11
else
  printf unsupported
fi"#;

    let output = ssh(&dest.host, script, None)?;
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    RemoteMode::parse(&value).map_err(|_| {
        Error::DoctorFailed(format!(
            "remote clipboard backend not found on {}; install wl-clipboard or xclip for Linux GUI remotes",
            dest.host
        ))
    })
}

pub fn sync_remote_image_command(
    config: &AppConfig,
    dest: &DestinationConfig,
    remote_mode: &RemoteMode,
    remote_path: &str,
) -> Result<String> {
    let remote_dir = config.destination_remote_dir(dest);
    Ok([
        "umask 077".to_string(),
        format!("mkdir -p {}", shell_quote(&remote_dir)),
        format!("chmod 700 {}", shell_quote(&remote_dir)),
        format!("cat > {}", shell_quote(remote_path)),
        set_clipboard_command(dest, remote_mode, remote_path)?,
    ]
    .join(" && "))
}

pub fn set_clipboard_command(
    dest: &DestinationConfig,
    remote_mode: &RemoteMode,
    remote_path: &str,
) -> Result<String> {
    let env_prefix = remote_env_prefix(dest);
    match remote_mode {
        RemoteMode::MacosPasteboard => {
            let script = format!(
                "set the clipboard to (read POSIX file {} as «class PNGf»)",
                applescript_string(remote_path)
            );
            Ok(format!("/usr/bin/osascript -e {}", shell_quote(&script)))
        }
        RemoteMode::LinuxWayland => Ok(format!(
            "{}wl-copy --type image/png < {}",
            env_prefix,
            shell_quote(remote_path)
        )),
        RemoteMode::LinuxX11 => Ok(format!(
            "({}xclip -selection clipboard -t image/png -i {} >/dev/null 2>&1 & sleep 0.2)",
            env_prefix,
            shell_quote(remote_path)
        )),
        RemoteMode::Auto => Err(Error::DoctorFailed(
            "remote mode must be resolved before sync".to_string(),
        )),
    }
}

pub fn clear_clipboard_command(
    dest: &DestinationConfig,
    remote_mode: &RemoteMode,
) -> Result<String> {
    let env_prefix = remote_env_prefix(dest);
    match remote_mode {
        RemoteMode::MacosPasteboard => Ok("printf '' | /usr/bin/pbcopy".to_string()),
        RemoteMode::LinuxWayland => Ok(format!("printf '' | {}wl-copy", env_prefix)),
        RemoteMode::LinuxX11 => Ok(format!(
            "printf '' | {}xclip -selection clipboard",
            env_prefix
        )),
        RemoteMode::Auto => Err(Error::DoctorFailed(
            "remote mode must be resolved before clear".to_string(),
        )),
    }
}

pub fn local_doctor_problem() -> Option<String> {
    detect_local_backend().err().map(|err| err.to_string())
}

fn check_remote_clipboard(dest: &DestinationConfig, remote_mode: &RemoteMode) -> bool {
    let command = match remote_mode {
        RemoteMode::MacosPasteboard => {
            "command -v /usr/bin/osascript >/dev/null && command -v /usr/bin/pbcopy >/dev/null"
                .to_string()
        }
        RemoteMode::LinuxWayland => {
            format!("{}command -v wl-copy >/dev/null", remote_env_prefix(dest))
        }
        RemoteMode::LinuxX11 => {
            let env_prefix = remote_env_prefix(dest);
            [
                "command -v timeout >/dev/null".to_string(),
                "command -v xclip >/dev/null".to_string(),
                "payload=pasteforward-doctor-$$".to_string(),
                format!(
                    "(printf \"$payload\" | {}xclip -selection clipboard -loops 1 -i >/dev/null 2>&1 &)",
                    env_prefix
                ),
                "sleep 0.2".to_string(),
                format!(
                    "test \"$({}timeout 5 xclip -selection clipboard -o 2>/dev/null)\" = \"$payload\"",
                    env_prefix
                ),
            ]
            .join(" && ")
        }
        RemoteMode::Auto => return false,
    };
    ssh(&dest.host, &command, None).is_ok()
}

fn remote_env_prefix(dest: &DestinationConfig) -> String {
    if dest.remote_env.is_empty() {
        return String::new();
    }
    let assignments = dest
        .remote_env
        .iter()
        .map(|(key, value)| format!("{key}={}", shell_quote(value)))
        .collect::<Vec<_>>()
        .join(" ");
    format!("{assignments} ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn dest() -> DestinationConfig {
        DestinationConfig {
            host: "user@example.test".to_string(),
            enabled: true,
            remote_mode: RemoteMode::MacosPasteboard,
            remote_env: BTreeMap::new(),
            remote_dir: None,
        }
    }

    #[test]
    fn builds_macos_clipboard_command() {
        let command =
            set_clipboard_command(&dest(), &RemoteMode::MacosPasteboard, "/tmp/a b.png").unwrap();
        assert!(command.contains("/usr/bin/osascript"));
        assert!(command.contains("/tmp/a b.png"));
    }

    #[test]
    fn builds_linux_env_prefix() {
        let mut d = dest();
        d.remote_env.insert("DISPLAY".to_string(), ":0".to_string());
        let command = set_clipboard_command(&d, &RemoteMode::LinuxX11, "/tmp/a.png").unwrap();
        assert!(command.contains("DISPLAY=':0' xclip"));
        assert!(command.contains(">/dev/null 2>&1 & sleep 0.2"));
    }
}
