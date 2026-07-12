use crate::command::{javascript_string, shell_quote, ssh};
use crate::config::{AppConfig, DestinationConfig, RemoteMode};
use crate::error::{Error, Result};
use crate::validation::{validate_destination, validate_remote_env};

pub fn resolve_remote_mode(dest: &DestinationConfig) -> Result<RemoteMode> {
    validate_destination("remote", dest)?;
    if dest.remote_mode != RemoteMode::Auto {
        return Ok(dest.remote_mode.clone());
    }
    let command = format!(
        "{}{}",
        remote_env_exports(dest)?,
        remote_mode_probe_script()
    );
    let output = ssh(&dest.host, &command, None)?;
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    RemoteMode::parse(&value).map_err(|_| {
        Error::DoctorFailed(format!(
            "remote clipboard backend not found on {}; install wl-clipboard or xclip for Linux GUI remotes",
            dest.host
        ))
    })
}

fn remote_mode_probe_script() -> &'static str {
    r#"uname_s="$(uname -s 2>/dev/null || true)"
wayland_socket=""
if test -n "${WAYLAND_DISPLAY:-}"; then
  case "$WAYLAND_DISPLAY" in
    /*) wayland_socket="$WAYLAND_DISPLAY" ;;
    *) if test -n "${XDG_RUNTIME_DIR:-}"; then wayland_socket="${XDG_RUNTIME_DIR}/${WAYLAND_DISPLAY}"; fi ;;
  esac
fi
if [ "$uname_s" = "Darwin" ]; then
  printf macos-pasteboard
elif command -v wl-copy >/dev/null 2>&1 && command -v wl-paste >/dev/null 2>&1 && test -n "$wayland_socket" && test -S "$wayland_socket" && test -r "$wayland_socket" && test -w "$wayland_socket"; then
  printf linux-wayland
elif command -v xclip >/dev/null 2>&1 && test -n "${DISPLAY:-}"; then
  display_number="${DISPLAY#*:}"
  display_number="${display_number%%.*}"
  case "${DISPLAY}" in
    :*|unix:*) if test -n "$display_number" && test -S "/tmp/.X11-unix/X${display_number}" && test -r "/tmp/.X11-unix/X${display_number}" && test -w "/tmp/.X11-unix/X${display_number}"; then printf linux-x11; else printf unsupported; fi ;;
    *) printf unsupported ;;
  esac
else
  printf unsupported
fi"#
}

pub fn sync_remote_image_command(
    config: &AppConfig,
    dest: &DestinationConfig,
    remote_mode: &RemoteMode,
    remote_path: &str,
) -> Result<String> {
    crate::validation::validate_config(config)?;
    validate_destination("remote", dest)?;
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
    let env_prefix = remote_env_prefix(dest)?;
    match remote_mode {
        RemoteMode::MacosPasteboard => macos_set_clipboard_command(remote_path),
        RemoteMode::LinuxWayland => Ok(format!(
            concat!(
                "({env}wl-copy --foreground --type image/png < {path} >/dev/null 2>&1 & owner=$!; ",
                "sleep 0.2; owner_status=0; ",
                "if ! kill -0 \"$owner\" 2>/dev/null; then wait \"$owner\" || owner_status=$?; fi; ",
                "test \"$owner_status\" -eq 0 && {env}wl-paste --type image/png)"
            ),
            env = env_prefix,
            path = shell_quote(remote_path)
        )),
        RemoteMode::LinuxX11 => Ok(format!(
            concat!(
                "({env}xclip -quiet -selection clipboard -t image/png -i {path} >/dev/null 2>&1 & owner=$!; ",
                "sleep 0.2; owner_status=0; ",
                "if ! kill -0 \"$owner\" 2>/dev/null; then wait \"$owner\" || owner_status=$?; fi; ",
                "test \"$owner_status\" -eq 0 && {env}xclip -selection clipboard -t image/png -o)"
            ),
            env = env_prefix,
            path = shell_quote(remote_path)
        )),
        RemoteMode::Auto => Err(Error::DoctorFailed(
            "remote mode must be resolved before sync".to_string(),
        )),
    }
}

fn macos_set_clipboard_command(remote_path: &str) -> Result<String> {
    let script = format!(
        concat!(
            "ObjC.import(\"AppKit\");ObjC.import(\"Foundation\");",
            "const path = {};const png = $.NSData.dataWithContentsOfFile(path);",
            "if (!png) throw new Error(\"failed to read png\");",
            "const image = $.NSImage.alloc.initWithData(png);",
            "if (!image) throw new Error(\"failed to load image\");",
            "const item = $.NSPasteboardItem.alloc.init;",
            "const url = $.NSURL.fileURLWithPath(path);",
            "item.setStringForType(url.absoluteString, \"public.file-url\");",
            "item.setDataForType(png, \"public.png\");",
            "const tiff = image.TIFFRepresentation;",
            "if (tiff && tiff.length > 0) item.setDataForType(tiff, \"public.tiff\");",
            "const objects = $.NSMutableArray.arrayWithCapacity(1);objects.addObject(item);",
            "const pasteboard = $.NSPasteboard.generalPasteboard;pasteboard.clearContents;",
            "if (!pasteboard.writeObjects(objects)) throw new Error(\"failed to write pasteboard\");"
        ),
        javascript_string(remote_path)
    );
    Ok(format!(
        "/usr/bin/osascript -l JavaScript -e {}",
        shell_quote(&script)
    ))
}

pub fn clear_clipboard_command(
    dest: &DestinationConfig,
    remote_mode: &RemoteMode,
) -> Result<String> {
    let env_prefix = remote_env_prefix(dest)?;
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

pub fn read_clipboard_command(
    dest: &DestinationConfig,
    remote_mode: &RemoteMode,
) -> Result<String> {
    let env_prefix = remote_env_prefix(dest)?;
    match remote_mode {
        RemoteMode::MacosPasteboard => Ok(concat!(
            "/usr/bin/osascript -l JavaScript -e '",
            "ObjC.import(\"AppKit\");ObjC.import(\"Foundation\");",
            "const data=$.NSPasteboard.generalPasteboard.dataForType(\"public.png\");",
            "if(!data) throw new Error(\"public.png missing\");",
            "$.NSFileHandle.fileHandleWithStandardOutput.writeData(data);'"
        )
        .to_string()),
        RemoteMode::LinuxWayland => {
            Ok(format!("{}timeout 5 wl-paste --type image/png", env_prefix))
        }
        RemoteMode::LinuxX11 => Ok(format!(
            "{}timeout 5 xclip -selection clipboard -t image/png -o",
            env_prefix
        )),
        RemoteMode::Auto => Err(Error::DoctorFailed(
            "remote mode must be resolved before clipboard readback".to_string(),
        )),
    }
}

pub(crate) fn remote_env_prefix(dest: &DestinationConfig) -> Result<String> {
    if dest.remote_env.is_empty() {
        return Ok(String::new());
    }
    let mut assignments = Vec::new();
    for (key, value) in &dest.remote_env {
        validate_remote_env(key, value)?;
        assignments.push(format!("{key}={}", shell_quote(value)));
    }
    Ok(format!("{} ", assignments.join(" ")))
}

fn remote_env_exports(dest: &DestinationConfig) -> Result<String> {
    let mut exports = String::new();
    for (key, value) in &dest.remote_env {
        validate_remote_env(key, value)?;
        exports.push_str(&format!("export {key}={}; ", shell_quote(value)));
    }
    Ok(exports)
}

pub(crate) fn remote_clipboard_probe_command(
    dest: &DestinationConfig,
    remote_mode: &RemoteMode,
) -> Result<String> {
    validate_destination("remote", dest)?;
    let env = remote_env_exports(dest)?;
    let probe = match remote_mode {
        RemoteMode::MacosPasteboard => concat!(
            "command -v /usr/bin/osascript >/dev/null",
            " && command -v /usr/bin/pbcopy >/dev/null",
            " && /usr/bin/osascript -l JavaScript -e ",
            "'ObjC.import(\"AppKit\");ObjC.import(\"Foundation\");$.NSPasteboard.generalPasteboard;'",
            " >/dev/null"
        )
        .to_string(),
        RemoteMode::LinuxWayland => concat!(
            "command -v timeout >/dev/null && command -v wl-copy >/dev/null && command -v wl-paste >/dev/null",
            " && test -n \"${WAYLAND_DISPLAY:-}\"",
            " && wayland_socket= && case \"$WAYLAND_DISPLAY\" in /*) wayland_socket=\"$WAYLAND_DISPLAY\" ;; *) test -n \"${XDG_RUNTIME_DIR:-}\" && wayland_socket=\"${XDG_RUNTIME_DIR}/${WAYLAND_DISPLAY}\" ;; esac",
            " && test -S \"$wayland_socket\"",
            " && test -r \"$wayland_socket\"",
            " && test -w \"$wayland_socket\"",
            " && probe_status=0 && probe_output=\"$(timeout 2 wl-paste --list-types 2>&1 >/dev/null)\" || probe_status=$?",
            " && if test \"$probe_status\" -ne 0; then case \"$probe_output\" in \"No selection\"|\"Nothing is copied\") true ;; *) false ;; esac; fi"
        )
        .to_string(),
        RemoteMode::LinuxX11 => concat!(
            "command -v timeout >/dev/null && command -v xclip >/dev/null && test -n \"${DISPLAY:-}\"",
            " && display_number=\"${DISPLAY#*:}\" && display_number=\"${display_number%%.*}\"",
            " && case \"${DISPLAY}\" in :*|unix:*) test -n \"$display_number\"",
            " && test -S \"/tmp/.X11-unix/X${display_number}\"",
            " && test -r \"/tmp/.X11-unix/X${display_number}\"",
            " && test -w \"/tmp/.X11-unix/X${display_number}\" ;; *) false ;; esac",
            " && probe_status=0 && probe_output=\"$(timeout 2 xclip -selection clipboard -t TARGETS -o 2>&1 >/dev/null)\" || probe_status=$?",
            " && if test \"$probe_status\" -ne 0; then case \"$probe_output\" in *\"target TARGETS not available\"*) true ;; *) false ;; esac; fi"
        )
        .to_string(),
        RemoteMode::Auto => {
            return Err(Error::DoctorFailed(
                "remote mode must be resolved before clipboard probe".to_string(),
            ));
        }
    };
    Ok(format!("{env}{probe}"))
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
        assert!(command.contains("public.file-url"));
        assert!(command.contains("public.png"));
        assert!(command.contains("public.tiff"));
    }

    #[test]
    fn builds_linux_env_prefix() {
        let mut d = dest();
        d.remote_env.insert("DISPLAY".to_string(), ":0".to_string());
        let command = set_clipboard_command(&d, &RemoteMode::LinuxX11, "/tmp/a.png").unwrap();
        assert!(command.starts_with("(DISPLAY=':0' xclip -quiet -selection"));
    }

    #[test]
    fn builds_clipboard_readback_commands() {
        let mac = read_clipboard_command(&dest(), &RemoteMode::MacosPasteboard).unwrap();
        assert!(mac.contains("public.png"));
        assert!(mac.contains("fileHandleWithStandardOutput"));
    }

    #[test]
    fn wayland_probe_allows_only_known_empty_clipboard_results() {
        let command = remote_clipboard_probe_command(&dest(), &RemoteMode::LinuxWayland).unwrap();
        assert!(command.contains("\"No selection\"|\"Nothing is copied\""));
        assert!(command.contains("*) false"));
    }

    #[test]
    fn auto_detection_requires_a_reachable_session_and_falls_back_to_x11() {
        let script = include_str!("remote.rs");
        assert!(script.contains("/*) wayland_socket=\"$WAYLAND_DISPLAY\""));
        assert!(script.contains("test -S \"$wayland_socket\""));
        assert!(script.contains("elif command -v xclip"));
        assert!(script.contains("/tmp/.X11-unix/X${display_number}"));
    }

    #[test]
    fn wayland_probe_supports_absolute_display_sockets() {
        let command = remote_clipboard_probe_command(&dest(), &RemoteMode::LinuxWayland).unwrap();
        assert!(
            command
                .contains("case \"$WAYLAND_DISPLAY\" in /*) wayland_socket=\"$WAYLAND_DISPLAY\"")
        );
        assert!(command.contains("test -S \"$wayland_socket\""));
    }

    #[cfg(unix)]
    #[test]
    fn wayland_socket_resolution_executes_for_absolute_relative_and_x11_fallback() {
        use std::os::unix::fs::PermissionsExt;
        use std::os::unix::net::UnixListener;
        use std::process::Command;

        let root =
            std::path::PathBuf::from(format!("/tmp/pf-remote-socket-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let bin = root.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        for (name, content) in [
            ("uname", "#!/bin/sh\nprintf '%s\\n' Linux\n"),
            ("wl-copy", "#!/bin/sh\nexit 0\n"),
            ("wl-paste", "#!/bin/sh\nexit 0\n"),
            ("xclip", "#!/bin/sh\nexit 0\n"),
            ("timeout", "#!/bin/sh\nshift\nexec \"$@\"\n"),
        ] {
            let path = bin.join(name);
            std::fs::write(&path, content).unwrap();
            let mut permissions = std::fs::metadata(&path).unwrap().permissions();
            permissions.set_mode(0o700);
            std::fs::set_permissions(path, permissions).unwrap();
        }

        let run = |script: &str,
                   wayland_display: Option<&std::path::Path>,
                   runtime_dir: Option<&std::path::Path>,
                   display: Option<&str>| {
            let mut command = Command::new("sh");
            command
                .arg("-c")
                .arg(script)
                .env(
                    "PATH",
                    format!("{}:{}", bin.display(), std::env::var("PATH").unwrap()),
                )
                .env_remove("WAYLAND_DISPLAY")
                .env_remove("XDG_RUNTIME_DIR")
                .env_remove("DISPLAY");
            if let Some(path) = wayland_display {
                command.env("WAYLAND_DISPLAY", path);
            }
            if let Some(path) = runtime_dir {
                command.env("XDG_RUNTIME_DIR", path);
            }
            if let Some(value) = display {
                command.env("DISPLAY", value);
            }
            command.output().unwrap()
        };

        let absolute = root.join("absolute socket; $(not-run)");
        let absolute_listener = UnixListener::bind(&absolute).unwrap();
        let auto = remote_mode_probe_script();
        assert_eq!(
            String::from_utf8(run(auto, Some(&absolute), None, None).stdout).unwrap(),
            "linux-wayland"
        );
        let probe = remote_clipboard_probe_command(&dest(), &RemoteMode::LinuxWayland).unwrap();
        assert!(run(&probe, Some(&absolute), None, None).status.success());

        let runtime = root.join("runtime");
        std::fs::create_dir(&runtime).unwrap();
        let relative = runtime.join("wayland-0");
        let relative_listener = UnixListener::bind(&relative).unwrap();
        assert_eq!(
            String::from_utf8(
                run(
                    auto,
                    Some(std::path::Path::new("wayland-0")),
                    Some(&runtime),
                    None,
                )
                .stdout,
            )
            .unwrap(),
            "linux-wayland"
        );
        assert_eq!(
            String::from_utf8(
                run(auto, Some(std::path::Path::new("wayland-0")), None, None,).stdout,
            )
            .unwrap(),
            "unsupported"
        );
        assert!(
            !run(&probe, Some(std::path::Path::new("wayland-0")), None, None,)
                .status
                .success()
        );

        let x11_dir = root.join("x11");
        std::fs::create_dir(&x11_dir).unwrap();
        let display_number = "42117";
        let x11_listener = UnixListener::bind(x11_dir.join(format!("X{display_number}"))).unwrap();
        let x11_script = auto.replace("/tmp/.X11-unix", &x11_dir.to_string_lossy());
        assert_eq!(
            String::from_utf8(
                run(
                    &x11_script,
                    Some(std::path::Path::new("/missing/wayland-0")),
                    None,
                    Some(&format!(":{display_number}")),
                )
                .stdout,
            )
            .unwrap(),
            "linux-x11"
        );

        drop(x11_listener);
        drop(relative_listener);
        drop(absolute_listener);
        std::fs::remove_dir_all(root).unwrap();
    }
}
