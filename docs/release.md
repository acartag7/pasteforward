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
```

The script builds with `cargo --locked`, writes a tarball under `dist/`, and
updates `dist/SHA256SUMS`.

## Release Rules

- Release from a clean git tree.
- Run `make verify`.
- Build platform tarballs locally or in CI.
- Publish SHA-256 checksums with every tarball.
- Homebrew formula must pin the GitHub Release tarball checksum.
- Do not add auto-update behavior to the binary.

## Suggested Artifact Names

```text
pasteforward-v0.1.0-aarch64-apple-darwin.tar.gz
pasteforward-v0.1.0-x86_64-apple-darwin.tar.gz
pasteforward-v0.1.0-x86_64-unknown-linux-gnu.tar.gz
pasteforward-v0.1.0-aarch64-unknown-linux-gnu.tar.gz
SHA256SUMS
```
