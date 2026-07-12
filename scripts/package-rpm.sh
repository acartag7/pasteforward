#!/usr/bin/env sh
set -eu

ROOT="$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)"
cd "$ROOT"
command -v rpmbuild >/dev/null 2>&1 || { echo "rpmbuild is required" >&2; exit 1; }

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
  x86_64-unknown-linux-musl) RPM_ARCH=x86_64 ;;
  aarch64-unknown-linux-musl) RPM_ARCH=aarch64 ;;
  *) echo "unsupported rpm target: $TARGET" >&2; exit 1 ;;
esac

BIN="${PASTEFORWARD_BIN:-$ROOT/target/$TARGET/release/pasteforward}"
test -x "$BIN"
top="$(mktemp -d)"
trap 'rm -rf "$top"' EXIT
mkdir -p "$top/BUILD" "$top/BUILDROOT" "$top/RPMS" "$top/SOURCES" "$top/SPECS" "$top/SRPMS"
cat >"$top/SPECS/pasteforward.spec" <<EOF
Name: pasteforward
Version: $VERSION
Release: 1
Summary: Make image paste work in Claude Code and Codex over SSH
License: MIT
BuildArch: $RPM_ARCH

%description
PasteForward forwards local image clipboard changes to remote GUI clipboards over SSH.

%install
mkdir -p %{buildroot}/usr/bin %{buildroot}/usr/share/doc/pasteforward
install -m 0755 $BIN %{buildroot}/usr/bin/pasteforward
install -m 0644 $ROOT/LICENSE $ROOT/README.md %{buildroot}/usr/share/doc/pasteforward/

%files
/usr/bin/pasteforward
%doc /usr/share/doc/pasteforward/LICENSE
%doc /usr/share/doc/pasteforward/README.md
EOF
rpmbuild --define "_topdir $top" -bb "$top/SPECS/pasteforward.spec" >/dev/null
mkdir -p dist
OUTPUT="$ROOT/dist/pasteforward-${VERSION}-1.${RPM_ARCH}.rpm"
rm -f "$OUTPUT"
cp "$top/RPMS/$RPM_ARCH/"*.rpm "$OUTPUT"
echo "$OUTPUT"
