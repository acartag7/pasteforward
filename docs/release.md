# Release

V0 distribution targets:

- GitHub Release binary tarballs
- Homebrew tap after the release artifact flow is stable

## Local Build

```sh
make verify
make build
```

The binary is:

```text
target/release/pasteforward
```

## Local Tarball

```sh
scripts/package-release.sh
scripts/test-release-tarball.sh
```

The script builds with `cargo --locked`, writes a tarball under `dist/`, and
updates `dist/SHA256SUMS`.

## CI Release

Pushing a version tag creates a GitHub Release draft using the hand-written
`.github/release-notes/v<version>.md`, uploads Linux and macOS tarballs for
x86_64 and arm64, uploads Linux `.deb` and `.rpm` packages, publishes SHA-256
files and build-provenance attestations, installs the native packages, tests the
formula on both macOS architectures, and waits at the protected owner-acceptance
gate. After the public release exists, automation opens a tap PR:

```sh
git tag v0.2.0
git push origin v0.2.0
```

The tag must match the Cargo package version as `v<version>`.

Repository setup required before the first release:

- `HOMEBREW_TAP_TOKEN`: fine-grained token limited to pull-request branches in
  `acartag7/homebrew-tap`
- protected `release` environment with a required human reviewer
- `PASTEFORWARD_RELEASE_ACCEPTED_SHA` repository variable, set to the exact tag
  commit only after the owner-run commands below and real Claude/Codex paste
  checks pass

Automation never merges the tap PR and never replaces a working tap formula
with one whose assets are private. Review and merge the PR only after the public
release job passes. If tap publication fails, retry only that job; the prior tap
formula remains intact and the public release remains installable through its
native packages and tarballs.

## Release Rules

- Release from a clean git tree.
- Run `make verify`.
- Run Linux X11 and Wayland integration tests with Lima.
- Run fan-out, TTL cleanup, service lifecycle, and release tarball smoke tests.
- Verify real image paste in a normal SSH session with `claude`.
- Verify real image paste in a normal SSH session with `codex`.
- Build platform tarballs locally or in CI.
- Publish SHA-256 checksums with every tarball.
- Publish build-provenance attestations for every release artifact.
- Publish hand-written release notes from `.github/release-notes/`.
- Homebrew formula must pin the GitHub Release tarball checksum.
- Install, audit, and test the formula against the exact built tarball on Intel
  and Apple Silicon before opening the tap PR.
- Install/run/remove `.deb` and `.rpm` artifacts in clean distro environments.
- Never publish until the owner-acceptance variable equals the exact tag commit.
- Do not add auto-update behavior to the binary.

## Validation Commands

```sh
make verify
scripts/test-lima-x11.sh
scripts/test-lima-wayland.sh
scripts/test-lima-negative.sh
scripts/test-ttl-cleanup.sh
scripts/test-fanout.sh
scripts/test-release-tarball.sh
PASTEFORWARD_SERVICE_TEST_HOST=user@host \
  PASTEFORWARD_SERVICE_RESTORE_BIN="$HOME/.local/bin/pasteforward" \
  scripts/test-service-lifecycle.sh
```

## Suggested Artifact Names

```text
pasteforward-v0.2.0-aarch64-apple-darwin.tar.gz
pasteforward-v0.2.0-x86_64-apple-darwin.tar.gz
pasteforward-v0.2.0-x86_64-unknown-linux-musl.tar.gz
pasteforward-v0.2.0-aarch64-unknown-linux-musl.tar.gz
pasteforward_0.2.0_amd64.deb
pasteforward_0.2.0_arm64.deb
pasteforward-0.2.0-1.x86_64.rpm
pasteforward-0.2.0-1.aarch64.rpm
```
