#!/usr/bin/env sh
set -eu

ROOT="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
REAL_HOME="${HOME:?}"
VM_NAME="${PASTEFORWARD_LIMA_VM:-pasteforward-linux}"
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
png="$tmp/pf-ttl-smoke.png"
remote_dir="/tmp/pasteforward-ttl-$$"
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
exec /usr/bin/ssh -F "$REAL_HOME/.lima/$VM_NAME/ssh.config" "\$@"
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

PATH="$test_bin:$PATH" PASTEFORWARD_CONFIG_HOME="$config_home" PASTEFORWARD_STATE_HOME="$state_home" \
  "$BIN" init ttlvm \
  --host "lima-$VM_NAME" \
  --remote-mode linux-x11 \
  --remote-env "DISPLAY=$DISPLAY_NAME" \
  --remote-dir "$remote_dir" \
  --no-install-service
PATH="$test_bin:$PATH" PASTEFORWARD_CONFIG_HOME="$config_home" PASTEFORWARD_STATE_HOME="$state_home" \
  "$BIN" doctor ttlvm

PATH="$test_bin:$PATH" PASTEFORWARD_CONFIG_HOME="$config_home" PASTEFORWARD_STATE_HOME="$state_home" \
  "$BIN" daemon >"$tmp/daemon.out" 2>"$tmp/daemon.err" &
daemon_pid=$!
sleep 1

osascript -e "set the clipboard to (read POSIX file \"$png\" as «class PNGf»)"

line=""
for _ in 1 2 3 4 5 6 7 8 9 10; do
  line="$(PATH="$test_bin:$PATH" PASTEFORWARD_CONFIG_HOME="$config_home" PASTEFORWARD_STATE_HOME="$state_home" "$BIN" history ttlvm | tail -n 1 || true)"
  [ -n "$line" ] && break
  sleep 1
done
if [ -z "$line" ]; then
  echo "history missing" >&2
  cat "$tmp/daemon.err" >&2 || true
  exit 1
fi

remote_path="$(printf '%s\n' "$line" | awk '{ print $6 }')"
PATH="$test_bin:$PATH" ssh "lima-$VM_NAME" "test -s '$remote_path'"

perl -0pi -e 's/"ttl_seconds":\s*\d+/"ttl_seconds": 0/' "$config_home/config.json"
PATH="$test_bin:$PATH" PASTEFORWARD_CONFIG_HOME="$config_home" PASTEFORWARD_STATE_HOME="$state_home" \
  "$BIN" cleanup ttlvm

if PATH="$test_bin:$PATH" ssh "lima-$VM_NAME" "test -e '$remote_path'"; then
  echo "remote file still exists after cleanup: $remote_path" >&2
  exit 1
fi

printf 'ttl_cleanup_removed=%s\n' "$remote_path"
