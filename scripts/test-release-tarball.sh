#!/usr/bin/env sh
set -eu

ROOT="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
cd "$ROOT"

artifact_path="$(scripts/package-release.sh | sed -n 's#^\(.*/pasteforward-v.*\.tar\.gz\)$#\1#p' | tail -n 1)"
if [ -z "$artifact_path" ]; then
  echo "package script did not report a tarball path" >&2
  exit 1
fi

tmp="$(mktemp -d)"
tar -C "$tmp" -xzf "$artifact_path"
dir="$(find "$tmp" -mindepth 1 -maxdepth 1 -type d | head -n 1)"
bin="$dir/pasteforward"

test -x "$bin"
test -f "$dir/README.md"
test -f "$dir/LICENSE"
test -f "$dir/docs/security.md"

"$bin" --version
"$bin" help >/dev/null
PASTEFORWARD_CONFIG_HOME="$tmp/config" PASTEFORWARD_STATE_HOME="$tmp/state" "$bin" status
PASTEFORWARD_CONFIG_HOME="$tmp/config" PASTEFORWARD_STATE_HOME="$tmp/state" "$bin" doctor

(
  cd "$(dirname "$artifact_path")"
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 -c SHA256SUMS
  else
    sha256sum -c SHA256SUMS
  fi
)
