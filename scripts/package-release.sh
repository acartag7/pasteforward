#!/usr/bin/env sh
set -eu

ROOT="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
cd "$ROOT"

VERSION="${1:-$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)}"
if [ -z "$VERSION" ]; then
  echo "failed to determine version" >&2
  exit 1
fi

OS="$(uname -s)"
ARCH="$(uname -m)"
case "$OS:$ARCH" in
  Darwin:arm64) TARGET="aarch64-apple-darwin" ;;
  Darwin:x86_64) TARGET="x86_64-apple-darwin" ;;
  Linux:x86_64) TARGET="x86_64-unknown-linux-gnu" ;;
  Linux:aarch64 | Linux:arm64) TARGET="aarch64-unknown-linux-gnu" ;;
  *)
    echo "unsupported release platform: $OS $ARCH" >&2
    exit 1
    ;;
esac

cargo build --locked --release

NAME="pasteforward-v${VERSION}-${TARGET}"
DIST="$ROOT/dist"
STAGE="$DIST/$NAME"
rm -rf "$STAGE"
mkdir -p "$STAGE/docs"

cp target/release/pasteforward "$STAGE/pasteforward"
cp README.md LICENSE "$STAGE/"
cp docs/*.md "$STAGE/docs/"
if [ -d docs/assets ]; then
  mkdir -p "$STAGE/docs/assets"
  cp docs/assets/* "$STAGE/docs/assets/"
fi

mkdir -p "$DIST"
tar -C "$DIST" -czf "$DIST/$NAME.tar.gz" "$NAME"
rm -rf "$STAGE"

(
  cd "$DIST"
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$NAME.tar.gz"
  else
    sha256sum "$NAME.tar.gz"
  fi
) > "$DIST/SHA256SUMS"

echo "$DIST/$NAME.tar.gz"
echo "$DIST/SHA256SUMS"
