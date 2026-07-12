#!/usr/bin/env sh
set -eu

ROOT="$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)"
cd "$ROOT"

VERSION="${1:-$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)}"
case "$VERSION" in
  *[!0-9.]* | .* | *. | *..*) echo "invalid version: $VERSION" >&2; exit 1 ;;
esac
major="${VERSION%%.*}"
rest="${VERSION#*.}"
minor="${rest%%.*}"
patch="${rest#*.}"
case "$patch" in *.*) echo "version must be MAJOR.MINOR.PATCH" >&2; exit 1 ;; esac
test "$VERSION" = "$major.$minor.$patch" || { echo "version must be MAJOR.MINOR.PATCH" >&2; exit 1; }
TARGET="${PASTEFORWARD_TARGET:?PASTEFORWARD_TARGET is required}"
case "$TARGET" in
  x86_64-unknown-linux-musl) DEB_ARCH=amd64 ;;
  aarch64-unknown-linux-musl) DEB_ARCH=arm64 ;;
  *) echo "unsupported deb target: $TARGET" >&2; exit 1 ;;
esac

BIN="${PASTEFORWARD_BIN:-target/$TARGET/release/pasteforward}"
test -x "$BIN"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/control" "$tmp/data/usr/bin" "$tmp/data/usr/share/doc/pasteforward"
install -m 0755 "$BIN" "$tmp/data/usr/bin/pasteforward"
install -m 0644 LICENSE README.md "$tmp/data/usr/share/doc/pasteforward/"
cat >"$tmp/control/control" <<EOF
Package: pasteforward
Version: $VERSION
Architecture: $DEB_ARCH
Maintainer: PasteForward maintainers
Section: utils
Priority: optional
Description: Make image paste work in Claude Code and Codex over SSH.
EOF
printf '2.0\n' >"$tmp/debian-binary"
tar -C "$tmp/control" -czf "$tmp/control.tar.gz" .
tar -C "$tmp/data" -czf "$tmp/data.tar.gz" .
mkdir -p dist
OUTPUT="$ROOT/dist/pasteforward_${VERSION}_${DEB_ARCH}.deb"
rm -f "$OUTPUT"
(cd "$tmp" && ar rcs "$OUTPUT" debian-binary control.tar.gz data.tar.gz)
echo "$OUTPUT"
