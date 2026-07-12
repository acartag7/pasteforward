#!/usr/bin/env sh
set -eu

ROOT="$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)"
cd "$ROOT"

VERSION="${1:-$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)}"
if [ -z "$VERSION" ]; then
  echo "failed to determine version" >&2
  exit 1
fi
case "$VERSION" in
  *[!0-9.]* | .* | *. | *..*) echo "invalid version: $VERSION" >&2; exit 1 ;;
esac
major="${VERSION%%.*}"
rest="${VERSION#*.}"
minor="${rest%%.*}"
patch="${rest#*.}"
case "$patch" in *.*) echo "version must be MAJOR.MINOR.PATCH" >&2; exit 1 ;; esac
test "$VERSION" = "$major.$minor.$patch" || { echo "version must be MAJOR.MINOR.PATCH" >&2; exit 1; }

TARGET="${PASTEFORWARD_TARGET:-}"
if [ -z "$TARGET" ]; then
  OS="$(uname -s)"
  ARCH="$(uname -m)"
  case "$OS:$ARCH" in
    Darwin:arm64) TARGET="aarch64-apple-darwin" ;;
    Darwin:x86_64) TARGET="x86_64-apple-darwin" ;;
    Linux:x86_64) TARGET="x86_64-unknown-linux-musl" ;;
    Linux:aarch64 | Linux:arm64) TARGET="aarch64-unknown-linux-musl" ;;
    *)
      echo "unsupported release platform: $OS $ARCH" >&2
      exit 1
      ;;
  esac
fi
case "$TARGET" in
  aarch64-apple-darwin | x86_64-apple-darwin | x86_64-unknown-linux-musl | aarch64-unknown-linux-musl) ;;
  *) echo "unsupported release target: $TARGET" >&2; exit 1 ;;
esac

cargo build --locked --release --target "$TARGET"

NAME="pasteforward-v${VERSION}-${TARGET}"
DIST="$ROOT/dist"
STAGE="$DIST/$NAME"
rm -rf "$STAGE"
mkdir -p "$STAGE/docs"

cp "target/$TARGET/release/pasteforward" "$STAGE/pasteforward"
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
