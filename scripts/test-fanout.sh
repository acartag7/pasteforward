#!/usr/bin/env sh
set -eu

ROOT="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
REAL_HOME="${HOME:?}"
VM_NAME="${PASTEFORWARD_LIMA_VM:-pasteforward-linux}"
MAC_HOST="${PASTEFORWARD_MAC_HOST:-}"
BIN="${PASTEFORWARD_BIN:-$ROOT/target/release/pasteforward}"
DISPLAY_NAME="${PASTEFORWARD_LIMA_DISPLAY:-:99}"
DISPLAY_NUM="${DISPLAY_NAME#:}"

if ! command -v limactl >/dev/null 2>&1; then
  echo "limactl is required" >&2
  exit 1
fi

if [ ! -x "$BIN" ]; then
  (cd "$ROOT" && cargo build --locked --release)
fi

status="$(limactl list 2>/dev/null | awk -v name="$VM_NAME" '$1 == name { print $2 }')"
if [ -z "$status" ]; then
  limactl start --name="$VM_NAME" --cpus=2 --memory=2 --disk=10 --mount-none --tty=false --timeout=20m template:ubuntu
elif [ "$status" != "Running" ]; then
  limactl start --tty=false "$VM_NAME"
fi

limactl shell "$VM_NAME" sudo apt-get update
limactl shell "$VM_NAME" sudo DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends xvfb xclip
limactl shell "$VM_NAME" -- sh -lc "
  rm -f /tmp/.X${DISPLAY_NUM}-lock
  if [ ! -S /tmp/.X11-unix/X${DISPLAY_NUM} ]; then
    nohup Xvfb '$DISPLAY_NAME' -ac -screen 0 1280x720x24 >/tmp/pasteforward-xvfb.log 2>&1 &
    sleep 2
  fi
  DISPLAY='$DISPLAY_NAME' xclip -version >/dev/null
"

tmp="$(mktemp -d)"
config_home="$tmp/config"
state_home="$tmp/state"
test_bin="$tmp/bin"
png="$tmp/pf-fanout-smoke.png"
prev_img="$tmp/prev.png"
prev_text="$tmp/prev.txt"
daemon_pid=""
restore_mode=none

cleanup() {
  if [ -n "$daemon_pid" ]; then
    kill "$daemon_pid" 2>/dev/null || true
    wait "$daemon_pid" 2>/dev/null || true
  fi
  if [ "$restore_mode" = image ]; then
    osascript -e "set the clipboard to (read POSIX file \"$prev_img\" as «class PNGf»)" >/dev/null 2>&1 || true
  elif [ "$restore_mode" = text ]; then
    pbcopy < "$prev_text" || true
  fi
}
trap cleanup EXIT

mkdir -p "$test_bin"
cat > "$test_bin/ssh" <<EOF
#!/usr/bin/env sh
if [ "\$1" = "lima-$VM_NAME" ]; then
  shift
  exec /usr/bin/ssh -F "$REAL_HOME/.lima/$VM_NAME/ssh.config" "lima-$VM_NAME" "\$@"
fi
exec /usr/bin/ssh "\$@"
EOF
chmod 700 "$test_bin"
chmod 700 "$test_bin/ssh"

pbpaste > "$prev_text" 2>/dev/null || true
if osascript \
  -e 'set png_data to (the clipboard as «class PNGf»)' \
  -e "set fp to open for access POSIX file \"$prev_img\" with write permission" \
  -e 'set eof fp to 0' \
  -e 'write png_data to fp' \
  -e 'close access fp' >/dev/null 2>&1; then
  restore_mode=image
elif [ -s "$prev_text" ]; then
  restore_mode=text
fi

if base64 -D </dev/null >/dev/null 2>&1; then
  printf '%s' 'iVBORw0KGgoAAAANSUhEUgAAABAAAAAQCAIAAACQkWg2AAABlUlEQVR4nA3LQQEAIQgAQStYgQpWsAIVqEAFK1jB9/6oYAUrUIG7+U9rjd6QxmjMhjas4Y3V2I3TiMZtvEY2qtFap3ekMzqzox3reGd1dud0onM7r5Od6n8QuiDCEKagggkuLGELRwjhCk9IoeQPgz6QwRjMgQ5s4IM12IMziMEdvEEOavxh0icyGZM50YlNfLIme3ImMbmTN8lJzT8oXRFlKFNRxRRXlrKVo4RylaekUvoHoxtiDGMaapjhxjK2cYwwrvGMNMr+4HRHnOFMRx1z3FnOdo4TznWek075HxZ9IYuxmAtd2MIXa7EXZxGLu3iLXNT6w6ZvZDM2c6Mb2/hmbfbmbGJzN2+Tm9p/OPSDHMZhHvRgBz+swz6cQxzu4R3yUOcPQQ8kGMEMNLDAgxXs4AQR3OAFGVT84dIvchmXedGLXfyyLvtyLnG5l3fJS90/PPpDHuMxH/qwhz/WYz/OIx738R75qPeHpCeSjGQmmljiyUp2cpJIbvKSTCr/UPRCilHMQgsrvFjFLk4RxS1ekUUVH6hAsxCYFr8vAAAAAElFTkSuQmCC' | base64 -D > "$png"
else
  printf '%s' 'iVBORw0KGgoAAAANSUhEUgAAABAAAAAQCAIAAACQkWg2AAABlUlEQVR4nA3LQQEAIQgAQStYgQpWsAIVqEAFK1jB9/6oYAUrUIG7+U9rjd6QxmjMhjas4Y3V2I3TiMZtvEY2qtFap3ekMzqzox3reGd1dud0onM7r5Od6n8QuiDCEKagggkuLGELRwjhCk9IoeQPgz6QwRjMgQ5s4IM12IMziMEdvEEOavxh0icyGZM50YlNfLIme3ImMbmTN8lJzT8oXRFlKFNRxRRXlrKVo4RylaekUvoHoxtiDGMaapjhxjK2cYwwrvGMNMr+4HRHnOFMRx1z3FnOdo4TznWek075HxZ9IYuxmAtd2MIXa7EXZxGLu3iLXNT6w6ZvZDM2c6Mb2/hmbfbmbGJzN2+Tm9p/OPSDHMZhHvRgBz+swz6cQxzu4R3yUOcPQQ8kGMEMNLDAgxXs4AQR3OAFGVT84dIvchmXedGLXfyyLvtyLnG5l3fJS90/PPpDHuMxH/qwhz/WYz/OIx738R75qPeHpCeSjGQmmljiyUp2cpJIbvKSTCr/UPRCilHMQgsrvFjFLk4RxS1ekUUVH6hAsxCYFr8vAAAAAElFTkSuQmCC' | base64 -d > "$png"
fi
local_sha="$(shasum -a 256 "$png" | awk '{ print $1 }')"

PATH="$test_bin:$PATH" PASTEFORWARD_CONFIG_HOME="$config_home" PASTEFORWARD_STATE_HOME="$state_home" \
  "$BIN" init limaone --host "lima-$VM_NAME" --remote-mode linux-x11 --remote-env "DISPLAY=$DISPLAY_NAME" --no-install-service
PATH="$test_bin:$PATH" PASTEFORWARD_CONFIG_HOME="$config_home" PASTEFORWARD_STATE_HOME="$state_home" \
  "$BIN" init limatwo --host "lima-$VM_NAME" --remote-mode linux-x11 --remote-env "DISPLAY=$DISPLAY_NAME" --no-install-service
if [ -n "$MAC_HOST" ]; then
  PATH="$test_bin:$PATH" PASTEFORWARD_CONFIG_HOME="$config_home" PASTEFORWARD_STATE_HOME="$state_home" \
    "$BIN" init macfanout --host "$MAC_HOST" --remote-mode macos-pasteboard --no-install-service
fi

PATH="$test_bin:$PATH" PASTEFORWARD_CONFIG_HOME="$config_home" PASTEFORWARD_STATE_HOME="$state_home" \
  "$BIN" daemon >"$tmp/daemon.out" 2>"$tmp/daemon.err" &
daemon_pid=$!
sleep 1

osascript -e "set the clipboard to (read POSIX file \"$png\" as «class PNGf»)"

lima_one_line=""
lima_two_line=""
mac_line=""
for _ in 1 2 3 4 5 6 7 8 9 10; do
  history="$(PATH="$test_bin:$PATH" PASTEFORWARD_CONFIG_HOME="$config_home" PASTEFORWARD_STATE_HOME="$state_home" "$BIN" history || true)"
  lima_one_line="$(printf '%s\n' "$history" | awk '$2 == "limaone" { line=$0 } END { print line }')"
  lima_two_line="$(printf '%s\n' "$history" | awk '$2 == "limatwo" { line=$0 } END { print line }')"
  if [ -n "$MAC_HOST" ]; then
    mac_line="$(printf '%s\n' "$history" | awk '$2 == "macfanout" { line=$0 } END { print line }')"
  fi
  if [ -n "$lima_one_line" ] && [ -n "$lima_two_line" ] && { [ -z "$MAC_HOST" ] || [ -n "$mac_line" ]; }; then
    break
  fi
  sleep 1
done
if [ -z "$lima_one_line" ] || [ -z "$lima_two_line" ] || { [ -n "$MAC_HOST" ] && [ -z "$mac_line" ]; }; then
  echo "missing fan-out history" >&2
  cat "$tmp/daemon.err" >&2 || true
  exit 1
fi

lima_one_path="$(printf '%s\n' "$lima_one_line" | awk '{ print $6 }')"
lima_two_path="$(printf '%s\n' "$lima_two_line" | awk '{ print $6 }')"
lima_one_file_sha="$(PATH="$test_bin:$PATH" ssh "lima-$VM_NAME" "sha256sum '$lima_one_path'" | awk '{ print $1 }')"
lima_two_file_sha="$(PATH="$test_bin:$PATH" ssh "lima-$VM_NAME" "sha256sum '$lima_two_path'" | awk '{ print $1 }')"
lima_clip_sha="$(PATH="$test_bin:$PATH" ssh "lima-$VM_NAME" "DISPLAY='$DISPLAY_NAME' timeout 5 xclip -selection clipboard -t image/png -o 2>/dev/null | sha256sum" | awk '{ print $1 }')"
if [ -n "$MAC_HOST" ]; then
  mac_path="$(printf '%s\n' "$mac_line" | awk '{ print $6 }')"
  mac_file_sha="$(PATH="$test_bin:$PATH" ssh "$MAC_HOST" "shasum -a 256 '$mac_path'" | awk '{ print $1 }')"
  mac_clip_sha="$(PATH="$test_bin:$PATH" ssh "$MAC_HOST" "tmp=\"/tmp/pasteforward-clip-\$\$.png\"; /usr/bin/osascript -e 'set png_data to (the clipboard as «class PNGf»)' -e \"set fp to open for access POSIX file \\\"\$tmp\\\" with write permission\" -e 'set eof fp to 0' -e 'write png_data to fp' -e 'close access fp' >/dev/null && shasum -a 256 \"\$tmp\"; rm -f \"\$tmp\"" | awk '{ print $1 }')"
fi

printf 'local_sha=%s\n' "$local_sha"
printf 'lima_one_path=%s\n' "$lima_one_path"
printf 'lima_two_path=%s\n' "$lima_two_path"
if [ -n "$MAC_HOST" ]; then
  printf 'mac_path=%s\n' "$mac_path"
fi
printf 'lima_one_file_sha=%s\n' "$lima_one_file_sha"
printf 'lima_two_file_sha=%s\n' "$lima_two_file_sha"
printf 'lima_clip_sha=%s\n' "$lima_clip_sha"
if [ -n "$MAC_HOST" ]; then
  printf 'mac_file_sha=%s\n' "$mac_file_sha"
  printf 'mac_clip_sha=%s\n' "$mac_clip_sha"
fi
cat "$tmp/daemon.err"

test "$lima_one_file_sha" = "$local_sha"
test "$lima_two_file_sha" = "$local_sha"
test "$lima_clip_sha" = "$local_sha"
if [ -n "$MAC_HOST" ]; then
  test "$mac_file_sha" = "$local_sha"
  test "$mac_clip_sha" = "$local_sha"
fi
