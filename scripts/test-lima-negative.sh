#!/usr/bin/env sh
set -eu

ROOT="$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)"
REAL_HOME="${HOME:?}"
BIN="${PASTEFORWARD_BIN:-$ROOT/target/release/pasteforward}"
X11_VM="${PASTEFORWARD_X11_VM:-pf-test-x11}"
WAYLAND_VM="${PASTEFORWARD_WAYLAND_VM:-pf-test-wayland}"
tmp="$(mktemp -d)"
test_bin="$tmp/bin"
mkdir -p "$test_bin"

cleanup() {
  PATH="$test_bin:$PATH" ssh "lima-$X11_VM" 'sudo rm -f /usr/local/bin/xclip' >/dev/null 2>&1 || true
  PATH="$test_bin:$PATH" ssh "lima-$WAYLAND_VM" 'if test -f /tmp/pf-denied/pid; then kill "$(cat /tmp/pf-denied/pid)" 2>/dev/null || true; fi; rm -rf /tmp/pf-denied' >/dev/null 2>&1 || true
  rm -rf "$tmp"
}
trap cleanup EXIT

cat >"$test_bin/ssh" <<EOF
#!/usr/bin/env sh
case "\$1" in
  lima-$X11_VM) exec /usr/bin/ssh -F "$REAL_HOME/.lima/$X11_VM/ssh.config" "\$@" ;;
  lima-$WAYLAND_VM) exec /usr/bin/ssh -F "$REAL_HOME/.lima/$WAYLAND_VM/ssh.config" "\$@" ;;
  *) exec /usr/bin/ssh "\$@" ;;
esac
EOF
chmod 700 "$test_bin/ssh"

expect_failure() {
  if "$@" >"$tmp/stdout" 2>"$tmp/stderr"; then
    echo "expected command to fail: $*" >&2
    exit 1
  fi
}

limactl shell "$X11_VM" sudo apt-get update >/dev/null
limactl shell "$X11_VM" sudo DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends wl-clipboard >/dev/null

config="$tmp/invalid-config"
state="$tmp/invalid-state"
expect_failure env PATH="$test_bin:$PATH" PASTEFORWARD_CONFIG_HOME="$config" PASTEFORWARD_STATE_HOME="$state" \
  "$BIN" init invalid --host "lima-$X11_VM" --remote-mode linux-x11 --remote-env DISPLAY=:404 --no-install-service
test ! -e "$config/config.json"

config="$tmp/fallback-config"
state="$tmp/fallback-state"
if ! PATH="$test_bin:$PATH" PASTEFORWARD_CONFIG_HOME="$config" PASTEFORWARD_STATE_HOME="$state" \
  "$BIN" init fallback --host "lima-$X11_VM" --remote-mode auto --remote-env DISPLAY=:99 --no-install-service >"$tmp/fallback-init"; then
  cat "$tmp/fallback-init" >&2
  exit 1
fi
PATH="$test_bin:$PATH" PASTEFORWARD_CONFIG_HOME="$config" PASTEFORWARD_STATE_HOME="$state" \
  "$BIN" doctor fallback >"$tmp/fallback-doctor"
grep -q 'remote mode: linux-x11' "$tmp/fallback-doctor"

PATH="$test_bin:$PATH" ssh "lima-$WAYLAND_VM" \
  'mkdir -p /tmp/pf-denied; chmod 700 /tmp/pf-denied; nohup python3 -c '\''import os,socket,time; p="/tmp/pf-denied/wayland-0"; s=socket.socket(socket.AF_UNIX); s.bind(p); os.chmod(p,0); open("/tmp/pf-denied/pid","w").write(str(os.getpid())); time.sleep(60)'\'' >/tmp/pf-denied/server.log 2>&1 &'
for _ in 1 2 3 4 5; do
  PATH="$test_bin:$PATH" ssh "lima-$WAYLAND_VM" 'test -S /tmp/pf-denied/wayland-0' && break
  sleep 1
done
config="$tmp/denied-config"
state="$tmp/denied-state"
expect_failure env PATH="$test_bin:$PATH" PASTEFORWARD_CONFIG_HOME="$config" PASTEFORWARD_STATE_HOME="$state" \
  "$BIN" init denied --host "lima-$WAYLAND_VM" --remote-mode linux-wayland \
  --remote-env XDG_RUNTIME_DIR=/tmp/pf-denied --remote-env WAYLAND_DISPLAY=wayland-0 --no-install-service
test ! -e "$config/config.json"

cat >"$tmp/xclip" <<'EOF'
#!/usr/bin/env sh
exit 7
EOF
PATH="$test_bin:$PATH" scp -F "$REAL_HOME/.lima/$X11_VM/ssh.config" "$tmp/xclip" "lima-$X11_VM:/tmp/pf-failing-xclip" >/dev/null
PATH="$test_bin:$PATH" ssh "lima-$X11_VM" 'sudo install -m 0755 /tmp/pf-failing-xclip /usr/local/bin/xclip'
config="$tmp/backend-config"
state="$tmp/backend-state"
PATH="$test_bin:$PATH" PASTEFORWARD_CONFIG_HOME="$config" PASTEFORWARD_STATE_HOME="$state" \
  "$BIN" init backend --host "lima-$X11_VM" --remote-mode linux-x11 --remote-env DISPLAY=:99 --no-install-service >/dev/null
expect_failure env PATH="$test_bin:$PATH" PASTEFORWARD_CONFIG_HOME="$config" PASTEFORWARD_STATE_HOME="$state" \
  "$BIN" test backend
test ! -e "$state/history.jsonl"

echo "linux negative clipboard tests ok"
