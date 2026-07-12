#!/usr/bin/env sh
set -eu

ROOT="$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
formula="$tmp/pasteforward.rb"
printf '%s\n' 'class Pasteforward < Formula; end' >"$formula"

run_case() {
  state="$1"
  expected="$2"
  bin="$tmp/$state-bin"
  log="$tmp/$state.log"
  mkdir -p "$bin"

  cat >"$bin/gh" <<'EOF'
#!/usr/bin/env sh
set -eu
printf '%s\n' "$*" >>"$CALL_LOG"
case "$1 $2" in
  'repo clone') mkdir -p "$4" ;;
  'pr list')
    if [ "${PR_LOOKUP:-ok}" = error ]; then
      exit 1
    fi
    if [ -n "${PR_STATE:-}" ]; then
      printf '%s\n' "$PR_STATE"
    fi
    ;;
  'pr reopen'|'pr create') ;;
  *) echo "unexpected gh invocation: $*" >&2; exit 1 ;;
esac
EOF
  cat >"$bin/git" <<'EOF'
#!/usr/bin/env sh
set -eu
printf '%s\n' "$*" >>"$CALL_LOG"
case "$1" in
  ls-remote|fetch|checkout|diff) ;;
  *) echo "unexpected git invocation: $*" >&2; exit 1 ;;
esac
EOF
  chmod 700 "$bin/gh" "$bin/git"

  PATH="$bin:$PATH" CALL_LOG="$log" PR_STATE="$state" PR_LOOKUP=ok GH_TOKEN=test-token \
    "$ROOT/scripts/open-homebrew-tap-pr.sh" v0.2.0 "$formula"
  grep -Fqx "$expected" "$log"
  case "$state" in
    OPEN|MERGED)
      if grep -Eq '^pr (create|reopen) ' "$log"; then
        echo "existing tap pull request must not be changed" >&2
        exit 1
      fi
      ;;
    CLOSED)
      if grep -q '^pr create ' "$log"; then
        echo "closed tap pull request must be reopened, not recreated" >&2
        exit 1
      fi
      ;;
    '')
      if grep -q '^pr reopen ' "$log"; then
        echo "missing tap pull request must be created, not reopened" >&2
        exit 1
      fi
      ;;
  esac
}

expect_lookup_failure() {
  bin="$tmp/failure-bin"
  log="$tmp/failure.log"
  mkdir -p "$bin"
  cp "$tmp/OPEN-bin/gh" "$bin/gh"
  cp "$tmp/OPEN-bin/git" "$bin/git"
  if PATH="$bin:$PATH" CALL_LOG="$log" PR_STATE='' PR_LOOKUP=error GH_TOKEN=test-token \
    "$ROOT/scripts/open-homebrew-tap-pr.sh" v0.2.0 "$formula"; then
    echo "expected tap PR lookup failure" >&2
    exit 1
  fi
  grep -q '^pr list ' "$log"
  if grep -Eq '^pr (create|reopen) ' "$log"; then
    echo "tap PR mutation must not run after lookup failure" >&2
    exit 1
  fi
}

run_case OPEN 'pr list --repo acartag7/homebrew-tap --head chore/pasteforward-v0.2.0 --state all --json state --jq .[0].state // ""'
run_case CLOSED 'pr reopen chore/pasteforward-v0.2.0 --repo acartag7/homebrew-tap'
run_case MERGED 'pr list --repo acartag7/homebrew-tap --head chore/pasteforward-v0.2.0 --state all --json state --jq .[0].state // ""'
run_case '' 'pr create --repo acartag7/homebrew-tap --base main --head chore/pasteforward-v0.2.0 --title chore: update PasteForward to v0.2.0 --body Update the checksum-pinned PasteForward formula for v0.2.0. The source release remains draft until this PR is reviewed and merged.'
expect_lookup_failure

echo "homebrew tap PR tests ok"
