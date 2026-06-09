#!/usr/bin/env sh
set -eu

ROOT="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
REAL_HOME="${HOME:?}"
VM_NAME="${PASTEFORWARD_LIMA_VM:-pasteforward-linux}"
BIN="${PASTEFORWARD_BIN:-$ROOT/target/release/pasteforward}"

if ! command -v limactl >/dev/null 2>&1; then
  echo "limactl is required" >&2
  exit 1
fi

if [ ! -x "$BIN" ]; then
  (cd "$ROOT" && cargo build --locked --release)
fi

status="$(limactl list 2>/dev/null | awk -v name="$VM_NAME" '$1 == name { print $2 }')"
if [ -z "$status" ]; then
  limactl start \
    --name="$VM_NAME" \
    --cpus=2 \
    --memory=2 \
    --disk=10 \
    --mount-none \
    --tty=false \
    --timeout=20m \
    template:ubuntu
elif [ "$status" != "Running" ]; then
  limactl start --tty=false "$VM_NAME"
fi

limactl shell "$VM_NAME" sudo apt-get update
limactl shell "$VM_NAME" sudo DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends sway wl-clipboard

uid="$(limactl shell "$VM_NAME" id -u | tr -d '\r')"
runtime_dir="/run/user/$uid"
wayland_display="$(limactl shell "$VM_NAME" -- sh -lc "
  set -eu
  export XDG_RUNTIME_DIR='$runtime_dir'
  mkdir -p \"\$XDG_RUNTIME_DIR\"
  chmod 700 \"\$XDG_RUNTIME_DIR\"
  if ! pgrep -x sway >/dev/null 2>&1; then
    cat >/tmp/pasteforward-sway.conf <<'EOF'
output * resolution 800x600
EOF
    WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 nohup sway -c /tmp/pasteforward-sway.conf >/tmp/pasteforward-sway.log 2>&1 &
    sleep 5
  fi
  ls \"\$XDG_RUNTIME_DIR\" | grep -E '^wayland-[0-9]+$' | tail -n 1
" | tr -d '\r')"

if [ -z "$wayland_display" ]; then
  echo "failed to start a headless Wayland compositor" >&2
  limactl shell "$VM_NAME" -- sh -lc 'cat /tmp/pasteforward-sway.log 2>/dev/null || true' >&2
  exit 1
fi

tmp="$(mktemp -d)"
config_home="$tmp/config"
state_home="$tmp/state"
test_bin="$tmp/bin"
png="$tmp/pf-lima-wayland-smoke.png"
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
local_sha="$(shasum -a 256 "$png" | awk '{ print $1 }')"

PATH="$test_bin:$PATH" PASTEFORWARD_CONFIG_HOME="$config_home" PASTEFORWARD_STATE_HOME="$state_home" \
  "$BIN" init limawayland \
  --host "lima-$VM_NAME" \
  --remote-mode linux-wayland \
  --remote-env "WAYLAND_DISPLAY=$wayland_display" \
  --remote-env "XDG_RUNTIME_DIR=$runtime_dir" \
  --no-install-service

PATH="$test_bin:$PATH" PASTEFORWARD_CONFIG_HOME="$config_home" PASTEFORWARD_STATE_HOME="$state_home" \
  "$BIN" doctor limawayland

PATH="$test_bin:$PATH" PASTEFORWARD_CONFIG_HOME="$config_home" PASTEFORWARD_STATE_HOME="$state_home" \
  "$BIN" daemon >"$tmp/daemon.out" 2>"$tmp/daemon.err" &
daemon_pid=$!
sleep 1

osascript -e "set the clipboard to (read POSIX file \"$png\" as «class PNGf»)"

line=""
for _ in 1 2 3 4 5 6 7 8 9 10; do
  line="$(PATH="$test_bin:$PATH" PASTEFORWARD_CONFIG_HOME="$config_home" PASTEFORWARD_STATE_HOME="$state_home" "$BIN" history limawayland | tail -n 1 || true)"
  [ -n "$line" ] && break
  sleep 1
done
if [ -z "$line" ]; then
  echo "history missing" >&2
  cat "$tmp/daemon.err" >&2 || true
  exit 1
fi

history_sha="$(printf '%s\n' "$line" | awk '{ print $5 }')"
remote_path="$(printf '%s\n' "$line" | awk '{ print $6 }')"
remote_file_sha="$(PATH="$test_bin:$PATH" ssh "lima-$VM_NAME" "sha256sum '$remote_path'" | awk '{ print $1 }')"
remote_clip_sha="$(PATH="$test_bin:$PATH" ssh "lima-$VM_NAME" "XDG_RUNTIME_DIR='$runtime_dir' WAYLAND_DISPLAY='$wayland_display' timeout 5 wl-paste --type image/png 2>/dev/null | sha256sum" | awk '{ print $1 }')"

printf 'wayland_display=%s\n' "$wayland_display"
printf 'local_sha=%s\n' "$local_sha"
printf 'history_sha=%s\n' "$history_sha"
printf 'remote_path=%s\n' "$remote_path"
printf 'remote_file_sha=%s\n' "$remote_file_sha"
printf 'remote_clip_sha=%s\n' "$remote_clip_sha"
cat "$tmp/daemon.err"

test "$history_sha" = "$local_sha"
test "$remote_file_sha" = "$local_sha"
test "$remote_clip_sha" = "$local_sha"
