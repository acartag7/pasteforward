# Security And Supply Chain

PasteForward is intentionally conservative because it sits between your local
clipboard and remote developer machines.

## V0 Rules

- No pipe-to-shell installers.
- No remote scripts fetched from the internet.
- No package-manager auto-install.
- No auto-update.
- No telemetry.
- No network except SSH to configured hosts.
- No clipboard text sync.
- Metadata history is default-on.
- Image history is opt-in.
- Daemon installation requires explicit interactive confirmation or a flag.
- Non-interactive service installation requires `--yes` or `--install-service`.

## Dependency Policy

Runtime dependencies are intentionally small:

- `serde 1.0.228`, published 2025-09-27
- `serde_json 1.0.150`, published 2026-05-21
- `sha2 0.10.9`, published more than 7 days before 2026-06-09

The CLI is hand-rolled to avoid an argument-parser dependency in v0.

`Cargo.lock` is committed and verification uses locked dependencies.

## Remote Command Allowlist

PasteForward-generated remote commands are limited to:

- `uname`
- `command -v`
- `mkdir`
- `chmod`
- `cat`
- `printf`
- `sleep`
- `timeout`
- `osascript`
- `pbcopy`
- `wl-copy`
- `xclip`
- `rm -f` only for paths under the configured remote cache

`doctor` may suggest package manager commands to the user, but PasteForward does
not run them.

`pasteforward ssh <dest> -- <command...>` intentionally runs the user-provided
command through SSH on the configured destination. The clipboard-command
allowlist above applies to PasteForward-generated clipboard and cleanup
commands, not to commands the operator explicitly passes after `--`.

## Release Rules

Release binaries should be distributed as GitHub Release tarballs with SHA-256
checksums. Homebrew formulae should pin tarball checksums.

GitHub Actions must be pinned by commit SHA.
