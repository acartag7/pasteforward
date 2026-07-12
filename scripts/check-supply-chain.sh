#!/usr/bin/env sh
set -eu

if git ls-files --cached --others --exclude-standard -z \
  | xargs -0 grep -IInE 'curl .*\| *(sh|bash)|wget .*\| *(sh|bash)' 2>/dev/null \
  | grep -v '^scripts/check-supply-chain.sh:'; then
  echo "pipe-to-shell pattern found" >&2
  exit 1
fi

action_refs() {
  sed -nE 's/^.*uses:[[:space:]]+[^@[:space:]]+@([^[:space:]#]+).*$/\1/p'
}

test "$(printf '%s\n' '  - uses: owner/action@v4 # mutable' | action_refs)" = "v4"
test "$(printf '%s\n' '  - uses: owner/action@0123456789abcdef0123456789abcdef01234567 # pinned' | action_refs)" = "0123456789abcdef0123456789abcdef01234567"

if grep -RInE 'uses:[[:space:]]+[^[:space:]]+@[^[:space:]#]+' .github/workflows 2>/dev/null \
  | action_refs \
  | grep -Ev '^[0-9a-f]{40,}$'; then
  echo "unpinned GitHub Action found" >&2
  exit 1
fi

test -f Cargo.lock
grep -q '^channel = "1.85.1"$' rust-toolchain.toml
grep -q 'musl-tools=1.2.4-2' .github/workflows/release.yml
grep -q 'rpm=4.18.2+dfsg-2.1build2' .github/workflows/release.yml
grep -q 'cpio=2.15+dfsg-1ubuntu2' .github/workflows/release.yml
grep -q 'fedora:42@sha256:e78cd1a688cd079c23864f289a89a49a3f4ad66d817864e325e1d058310ee95c' .github/workflows/release.yml
cargo metadata --locked --format-version 1 >/dev/null
