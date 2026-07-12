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
  ls-remote)
    if [ "${TAP_BRANCH_LOOKUP:-ok}" = error ]; then
      exit 1
    fi
    if [ "${TAP_BRANCH_EXISTS:-true}" = true ]; then
      printf '%s\n' '0123456789012345678901234567890123456789	refs/heads/chore/pasteforward-v0.2.0'
    fi
    ;;
  diff) [ "${FORMULA_MATCHES:-true}" = true ] ;;
  fetch|checkout|config|add|commit|push) ;;
  *) echo "unexpected git invocation: $*" >&2; exit 1 ;;
esac
EOF
  chmod 700 "$bin/gh" "$bin/git"

  PATH="$bin:$PATH" CALL_LOG="$log" PR_STATE="$state" PR_LOOKUP=ok TAP_BRANCH_EXISTS=true GH_TOKEN=test-token \
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

expect_branch_lookup_failure() {
  bin="$tmp/branch-failure-bin"
  log="$tmp/branch-failure.log"
  mkdir -p "$bin"
  cp "$tmp/OPEN-bin/gh" "$bin/gh"
  cp "$tmp/OPEN-bin/git" "$bin/git"
  if PATH="$bin:$PATH" CALL_LOG="$log" PR_STATE='' PR_LOOKUP=ok TAP_BRANCH_LOOKUP=error GH_TOKEN=test-token \
    "$ROOT/scripts/open-homebrew-tap-pr.sh" v0.2.0 "$formula"; then
    echo "expected tap branch lookup failure" >&2
    exit 1
  fi
  grep -q '^ls-remote ' "$log"
  if grep -Eq '^(pr |fetch |checkout |push )' "$log"; then
    echo "tap publication must not continue after branch lookup failure" >&2
    exit 1
  fi
}

expect_main_already_updated_without_release_branch() {
  bin="$tmp/up-to-date-bin"
  log="$tmp/up-to-date.log"
  mkdir -p "$bin"
  cp "$tmp/OPEN-bin/gh" "$bin/gh"
  cp "$tmp/OPEN-bin/git" "$bin/git"
  PATH="$bin:$PATH" CALL_LOG="$log" PR_STATE='' PR_LOOKUP=ok TAP_BRANCH_EXISTS=false GH_TOKEN=test-token \
    "$ROOT/scripts/open-homebrew-tap-pr.sh" v0.2.0 "$formula"
  grep -Fqx 'ls-remote --heads origin chore/pasteforward-v0.2.0' "$log"
  if grep -Eq '^pr (list|create|reopen) ' "$log"; then
    echo "up-to-date tap main must not create or inspect a release branch PR" >&2
    exit 1
  fi
  if grep -q '^push ' "$log"; then
    echo "up-to-date tap main must not push a no-op release branch" >&2
    exit 1
  fi
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

expect_changed_formula_branch_absent_creates_pr() {
  bin="$tmp/changed-absent-bin"
  log="$tmp/changed-absent.log"
  mkdir -p "$bin"
  cp "$tmp/OPEN-bin/gh" "$bin/gh"
  cp "$tmp/OPEN-bin/git" "$bin/git"
  PATH="$bin:$PATH" CALL_LOG="$log" PR_STATE='' PR_LOOKUP=ok TAP_BRANCH_EXISTS=false FORMULA_MATCHES=false GH_TOKEN=test-token \
    "$ROOT/scripts/open-homebrew-tap-pr.sh" v0.2.0 "$formula"
  grep -Fqx 'checkout -b chore/pasteforward-v0.2.0' "$log"
  grep -Fqx 'commit -m chore: update PasteForward to v0.2.0' "$log"
  grep -Fqx 'push --set-upstream origin chore/pasteforward-v0.2.0' "$log"
  grep -Fqx 'pr create --repo acartag7/homebrew-tap --base main --head chore/pasteforward-v0.2.0 --title chore: update PasteForward to v0.2.0 --body Update the checksum-pinned PasteForward formula for v0.2.0. The source release remains draft until this PR is reviewed and merged.' "$log"
}

expect_changed_formula_existing_branch_reopens_pr() {
  bin="$tmp/changed-existing-bin"
  log="$tmp/changed-existing.log"
  mkdir -p "$bin"
  cp "$tmp/OPEN-bin/gh" "$bin/gh"
  cp "$tmp/OPEN-bin/git" "$bin/git"
  PATH="$bin:$PATH" CALL_LOG="$log" PR_STATE=CLOSED PR_LOOKUP=ok TAP_BRANCH_EXISTS=true FORMULA_MATCHES=false GH_TOKEN=test-token \
    "$ROOT/scripts/open-homebrew-tap-pr.sh" v0.2.0 "$formula"
  grep -Fqx 'checkout -B chore/pasteforward-v0.2.0 FETCH_HEAD' "$log"
  grep -Fqx 'commit -m chore: update PasteForward to v0.2.0' "$log"
  grep -Fqx 'push --set-upstream origin chore/pasteforward-v0.2.0' "$log"
  grep -Fqx 'pr reopen chore/pasteforward-v0.2.0 --repo acartag7/homebrew-tap' "$log"
}

run_case OPEN 'pr list --repo acartag7/homebrew-tap --head chore/pasteforward-v0.2.0 --state all --json state --jq .[0].state // ""'
run_case CLOSED 'pr reopen chore/pasteforward-v0.2.0 --repo acartag7/homebrew-tap'
run_case MERGED 'pr list --repo acartag7/homebrew-tap --head chore/pasteforward-v0.2.0 --state all --json state --jq .[0].state // ""'
run_case '' 'pr create --repo acartag7/homebrew-tap --base main --head chore/pasteforward-v0.2.0 --title chore: update PasteForward to v0.2.0 --body Update the checksum-pinned PasteForward formula for v0.2.0. The source release remains draft until this PR is reviewed and merged.'
expect_lookup_failure
expect_branch_lookup_failure
expect_main_already_updated_without_release_branch
expect_changed_formula_branch_absent_creates_pr
expect_changed_formula_existing_branch_reopens_pr

echo "homebrew tap PR tests ok"
