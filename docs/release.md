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

Pushing a version tag creates a GitHub Release draft, uploads Linux and macOS
tarballs for x86_64 and arm64, uploads per-tarball SHA-256 files, then
publishes the release after all uploads pass:

```sh
git tag v0.1.0
git push origin v0.1.0
```

The tag must match the Cargo package version as `v<version>`.

## Release Rules

- Release from a clean git tree.
- Run `make verify`.
- Run Linux X11 and Wayland integration tests with Lima.
- Run fan-out, TTL cleanup, service lifecycle, and release tarball smoke tests.
- Verify real image paste in a normal SSH session with `claude`.
- Verify real image paste in a normal SSH session with `codex`.
- Smoke-test the optional `pasteforward ssh <dest> -- <command>` wrapper.
- Build platform tarballs locally or in CI.
- Publish SHA-256 checksums with every tarball.
- Homebrew formula must pin the GitHub Release tarball checksum.
- Do not add auto-update behavior to the binary.

## Validation Commands

```sh
make verify
scripts/test-lima-x11.sh
scripts/test-lima-wayland.sh
scripts/test-ttl-cleanup.sh
scripts/test-fanout.sh
scripts/test-release-tarball.sh
PASTEFORWARD_SERVICE_TEST_HOST=user@host scripts/test-service-lifecycle.sh
```

## Suggested Artifact Names

```text
pasteforward-v0.1.0-aarch64-apple-darwin.tar.gz
pasteforward-v0.1.0-x86_64-apple-darwin.tar.gz
pasteforward-v0.1.0-x86_64-unknown-linux-gnu.tar.gz
pasteforward-v0.1.0-aarch64-unknown-linux-gnu.tar.gz
pasteforward-v0.1.0-aarch64-apple-darwin.tar.gz.sha256
pasteforward-v0.1.0-x86_64-apple-darwin.tar.gz.sha256
pasteforward-v0.1.0-x86_64-unknown-linux-gnu.tar.gz.sha256
pasteforward-v0.1.0-aarch64-unknown-linux-gnu.tar.gz.sha256
```
