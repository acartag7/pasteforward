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
- Packages install the binary only. Package installation never creates config,
  installs a user service, or probes SSH destinations.

## Dependency Policy

Runtime dependencies are intentionally small:

- `libc 0.2.186`, published 2026-04-23; used for portable no-follow
  file-descriptor operations on macOS and Linux
- `serde 1.0.228`, published 2025-09-27
- `serde_json 1.0.150`, published 2026-05-21
- `sha2 0.10.9`, published more than 7 days before 2026-06-09

The Remotion demo pins the transitive development-only dependency `ws 8.21.0`,
published 2026-05-22, to exclude vulnerable earlier 8.x releases. It is not
linked into the PasteForward binary.

The CLI is hand-rolled to avoid an argument-parser dependency in v0.

`Cargo.lock` is committed and verification uses locked dependencies.

Release build inputs are pinned:

- Rust `1.85.1`, published 2025-03-18
- Ubuntu 24.04 `musl-tools 1.2.4-2`
- Ubuntu 24.04 `rpm 4.18.2+dfsg-2.1build2`
- Ubuntu 24.04 `cpio 2.15+dfsg-1ubuntu2`
- Fedora 42 release-test container manifest
  `sha256:e78cd1a688cd079c23864f289a89a49a3f4ad66d817864e325e1d058310ee95c`

## Input And Execution Boundaries

- Config is parsed and validated completely before any side effect.
- SSH destinations beginning with `-`, blank destinations, and ASCII control
  characters are rejected.
- Remote environment keys are allowlisted to `DISPLAY`, `WAYLAND_DISPLAY`, and
  `XDG_RUNTIME_DIR`; blank values are rejected.
- The same validation runs for CLI input and config loaded from disk.
- Remote cache paths must be canonical non-root absolute paths. Repeated
  separators, trailing separators, and `.` or `..` segments are rejected.
- Exported sync functions repeat complete config and destination validation.
- Child processes have fixed wall-clock timeouts and captured-output caps.
- Clipboard images and SSH clipboard readback have fixed byte caps.
- Setup and doctor failures return non-zero. Failure is never reported as a
  successful partial initialization.
- Config reads use `O_NOFOLLOW`; config and service files use owner-only atomic
  replacement; newly created directory components are mode `0700`.

## Remote Command Allowlist

PasteForward-generated remote commands are limited to:

Shell control flow is limited to fixed PasteForward-generated conditionals,
variable assignments, exports, case statements, and redirections. Executed
programs are limited to:

- `uname`
- `command -v`
- `test`
- `mkdir`
- `chmod`
- `cat`
- `printf`
- `sleep`
- shell built-ins `if`, `kill -0`, `wait`, and variable assignment for verified
  Linux clipboard ownership
- `timeout`
- `osascript`
- `pbcopy`
- `wl-copy`
- `wl-paste`
- `xclip`
- `rm -f` only for paths under the configured remote cache

The macOS pasteboard writer runs `osascript -l JavaScript` and uses AppKit to
publish a pasteboard item with `public.file-url`, `public.png`, and
`public.tiff` representations for the remote temp image.

`doctor` may suggest package manager commands to the user, but PasteForward does
not run them.

`test` may read image bytes back with `osascript`, `wl-paste`, or `xclip`. The
readback is bounded before hashing and is never recorded in history.

## Release Rules

Release binaries should be distributed as GitHub Release tarballs with SHA-256
checksums and build-provenance attestations. Linux release binaries are static
musl builds for x86_64 and aarch64.

The supported install surfaces are:

- a Homebrew formula for macOS and Linuxbrew
- `.deb` packages for Debian and Ubuntu
- `.rpm` packages for Fedora and RHEL-family systems
- release tarballs as the manual fallback

Homebrew formulae and native packages pin or embed the exact release artifact.
Package maintainer scripts must not install, start, restart, or remove the user
service. Native package archives record root ownership for installed binaries
and documentation, independent of the build user's UID and GID.

GitHub Actions and reusable workflows must be pinned by commit SHA. Verification
parses workflow YAML semantically and fails closed on aliases, malformed input,
unbounded structure, symlinks, and dynamic or non-SHA remote `uses` values.

The release remains draft until native packages and the Homebrew formula have
been installed and run from the exact built artifacts. Publishing is protected
by the `release` environment and an exact-commit owner-acceptance repository
variable. Only after assets are public does automation open a Homebrew tap PR;
it never merges the PR or replaces the prior working formula automatically.
