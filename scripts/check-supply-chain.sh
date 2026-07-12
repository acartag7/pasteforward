#!/usr/bin/env sh
set -eu

if git ls-files --cached --others --exclude-standard -z \
  | xargs -0 grep -IInE 'curl .*\| *(sh|bash)|wget .*\| *(sh|bash)' 2>/dev/null \
  | grep -v '^scripts/check-supply-chain.sh:'; then
  echo "pipe-to-shell pattern found" >&2
  exit 1
fi

action_targets() {
  sed -nE \
    -e '/^[[:space:]]*#/d' \
    -e 's/^[[:space:]]*(-[[:space:]]+)?uses[[:space:]]*:[[:space:]]*([^#[:space:]]+).*$/\2/p'
}

action_target_is_pinned() {
  target=$1
  case "$target" in
    ./*) return 0 ;;
    *@*) ref=${target##*@} ;;
    *) return 1 ;;
  esac
  [ "${#ref}" -eq 40 ] && ! printf '%s' "$ref" | grep -q '[^0-9a-f]'
}

test "$(printf '%s\n' '  - uses: owner/action@v4 # uses: decoy/action@0123456789abcdef0123456789abcdef01234567' | action_targets)" = "owner/action@v4"
test -z "$(printf '%s\n' '  # - uses: owner/action@v4' | action_targets)"
test "$(printf '%s\n' '    uses: owner/repo/.github/workflows/check.yml@0123456789abcdef0123456789abcdef01234567 # reusable' | action_targets)" = "owner/repo/.github/workflows/check.yml@0123456789abcdef0123456789abcdef01234567"
if action_target_is_pinned 'owner/action@v4'; then
  echo "action pinning self-test failed" >&2
  exit 1
fi
action_target_is_pinned 'owner/action@0123456789abcdef0123456789abcdef01234567'

grep -RhE 'uses[[:space:]]*:[[:space:]]+' .github/workflows 2>/dev/null \
  | action_targets \
  | while IFS= read -r target; do
      if ! action_target_is_pinned "$target"; then
        echo "unpinned GitHub Action found: $target" >&2
        exit 1
      fi
    done

test -f Cargo.lock
grep -q '^channel = "1.85.1"$' rust-toolchain.toml
grep -q 'musl-tools=1.2.4-2' .github/workflows/release.yml
grep -q 'rpm=4.18.2+dfsg-2.1build2' .github/workflows/release.yml
grep -q 'cpio=2.15+dfsg-1ubuntu2' .github/workflows/release.yml
grep -q 'fedora:42@sha256:e78cd1a688cd079c23864f289a89a49a3f4ad66d817864e325e1d058310ee95c' .github/workflows/release.yml
cargo metadata --locked --format-version 1 >/dev/null
