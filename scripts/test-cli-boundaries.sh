#!/usr/bin/env sh
set -eu

ROOT="$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)"
BIN="${PASTEFORWARD_BIN:-$ROOT/target/release/pasteforward}"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

expect_failure() {
  if "$@" >"$tmp/stdout" 2>"$tmp/stderr"; then
    echo "expected command to fail: $*" >&2
    exit 1
  fi
}

config="$tmp/config"
state="$tmp/state"

expect_failure env PASTEFORWARD_CONFIG_HOME="$config" PASTEFORWARD_STATE_HOME="$state" \
  "$BIN" init badhost --host -V --no-install-service
test ! -e "$config/config.json"
test ! -e "$state"

expect_failure env PASTEFORWARD_CONFIG_HOME="$config" PASTEFORWARD_STATE_HOME="$state" \
  "$BIN" init badenv --host example.test --remote-env 'PATH=/tmp' --no-install-service
test ! -e "$config/config.json"
test ! -e "$state"

mkdir -p "$config"
cat >"$config/config.json" <<'EOF'
{
  "version": 2,
  "remote_dir": "/tmp/pasteforward",
  "retention": { "ttl_seconds": 3600 },
  "history": { "metadata": true, "image": false },
  "daemon": { "interval_millis": 1000 },
  "destinations": {}
}
EOF
expect_failure env PASTEFORWARD_CONFIG_HOME="$config" PASTEFORWARD_STATE_HOME="$state" \
  "$BIN" status

sed 's/"version": 2/"version": 1/' "$config/config.json" >"$config/root-path.json"
mv "$config/root-path.json" "$config/config.json"
env PASTEFORWARD_CONFIG_HOME="$config" PASTEFORWARD_STATE_HOME="$state" \
  "$BIN" status >"$tmp/stdout" 2>"$tmp/stderr"
test ! -e "$state"
for remote_dir in / // /// /tmp//pasteforward /tmp/./pasteforward /tmp/../pasteforward /tmp/pasteforward/; do
  sed "s#\"remote_dir\": \"/tmp/pasteforward\"#\"remote_dir\": \"$remote_dir\"#" \
    "$config/config.json" >"$config/root-path.json"
  mv "$config/root-path.json" "$config/config.json"
  expect_failure env PASTEFORWARD_CONFIG_HOME="$config" PASTEFORWARD_STATE_HOME="$state" \
    "$BIN" status
  grep -q 'remote directory must be' "$tmp/stderr"
  sed "s#\"remote_dir\": \"$remote_dir\"#\"remote_dir\": \"/tmp/pasteforward\"#" \
    "$config/config.json" >"$config/root-path.json"
  mv "$config/root-path.json" "$config/config.json"
done

cat >"$config/config.json" <<'EOF'
{
  "version": 1,
  "remote_dir": "/tmp/pasteforward",
  "retention": { "ttl_seconds": 3600 },
  "history": { "metadata": true, "image": false },
  "daemon": { "interval_millis": 1000 },
  "destinations": {
    "broken": {
      "host": "127.0.0.1",
      "enabled": true,
      "remote_mode": "auto"
    }
  }
}
EOF
expect_failure env PASTEFORWARD_CONFIG_HOME="$config" PASTEFORWARD_STATE_HOME="$state" \
  "$BIN" doctor

expect_failure "$ROOT/scripts/package-release.sh" '../../escape'
expect_failure env PASTEFORWARD_TARGET='../../escape' "$ROOT/scripts/package-release.sh"

echo "cli boundary tests ok"
