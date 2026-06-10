#!/usr/bin/env sh
set -eu

BIN="${PASTEFORWARD_BIN:-$HOME/.local/bin/pasteforward}"
TEST_HOST="${PASTEFORWARD_SERVICE_TEST_HOST:?set PASTEFORWARD_SERVICE_TEST_HOST to an SSH host to run this test}"
RESTORE_DEST="${PASTEFORWARD_SERVICE_RESTORE_DEST:-macmini}"
RESTORE_HOST="${PASTEFORWARD_SERVICE_RESTORE_HOST:-$TEST_HOST}"
RESTORE_BIN="${PASTEFORWARD_SERVICE_RESTORE_BIN:-}"
CONFIG_PATH="${PASTEFORWARD_SERVICE_CONFIG_PATH:-$HOME/.config/pasteforward/config.json}"
CONFIG_DIR="$(dirname "$CONFIG_PATH")"
LAUNCHD_PLIST="$HOME/Library/LaunchAgents/io.github.acartag7.pasteforward.plist"

if [ ! -x "$BIN" ]; then
  echo "pasteforward binary not executable: $BIN" >&2
  exit 1
fi

tmp="$(mktemp -d)"
backup="$tmp/config.json.backup"
status_out="$tmp/status.out"
had_config=0
if [ -f "$CONFIG_PATH" ]; then
  cp "$CONFIG_PATH" "$backup"
  had_config=1
fi

service_was_installed=0
restore_bin="$BIN"
status_has() {
  "$BIN" status > "$status_out"
  grep -q "$1" "$status_out"
}

if status_has '^service: installed$'; then
  service_was_installed=1
  if [ -n "$RESTORE_BIN" ]; then
    restore_bin="$RESTORE_BIN"
  elif [ -f "$LAUNCHD_PLIST" ]; then
    detected_bin="$(/usr/libexec/PlistBuddy -c 'Print :ProgramArguments:0' "$LAUNCHD_PLIST" 2>/dev/null || true)"
    if [ -n "$detected_bin" ] && [ -x "$detected_bin" ]; then
      restore_bin="$detected_bin"
    fi
  fi
fi

restore() {
  mkdir -p "$CONFIG_DIR"
  if [ "$had_config" -eq 1 ]; then
    cp "$backup" "$CONFIG_PATH"
  else
    rm -f "$CONFIG_PATH"
  fi

  if [ "$service_was_installed" -eq 1 ]; then
    "$restore_bin" init "$RESTORE_DEST" --host "$RESTORE_HOST" --yes >/dev/null || true
  else
    "$BIN" uninstall-service "$RESTORE_DEST" >/dev/null 2>&1 || true
  fi
}
trap restore EXIT

"$BIN" init "$RESTORE_DEST" --host "$RESTORE_HOST" --yes >/dev/null
"$BIN" init pfsvc_keep --host "$TEST_HOST" --yes >/dev/null
status_has '^service: installed$'
status_has '^pfsvc_keep '
"$BIN" delete pfsvc_keep >/dev/null
status_has '^service: installed$'
if status_has '^pfsvc_keep '; then
  echo "pfsvc_keep still present after delete" >&2
  exit 1
fi

rm -f "$CONFIG_PATH"
"$BIN" init pfsvc_last --host "$TEST_HOST" --yes >/dev/null
status_has '^service: installed$'
"$BIN" delete pfsvc_last >/dev/null
status_has '^service: not installed$'

restore
trap - EXIT
status_has "^$RESTORE_DEST "
if [ "$service_was_installed" -eq 1 ]; then
  status_has '^service: installed$'
fi

echo "service lifecycle ok"
