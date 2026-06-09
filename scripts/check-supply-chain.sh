#!/usr/bin/env sh
set -eu

if grep -RInE 'curl .*\| *(sh|bash)|wget .*\| *(sh|bash)' . \
  --exclude=check-supply-chain.sh --exclude-dir=.git --exclude-dir=target; then
  echo "pipe-to-shell pattern found" >&2
  exit 1
fi

if grep -RInE 'uses: [^ ]+@([A-Za-z0-9_.-]+)$' .github/workflows 2>/dev/null \
  | grep -Ev '@[0-9a-f]{40,}$'; then
  echo "unpinned GitHub Action found" >&2
  exit 1
fi

test -f Cargo.lock
cargo metadata --locked --format-version 1 >/dev/null
