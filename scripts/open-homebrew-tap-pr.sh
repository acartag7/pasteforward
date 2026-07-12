#!/usr/bin/env sh
set -eu

TAG="${1:?usage: open-homebrew-tap-pr.sh <vMAJOR.MINOR.PATCH> <formula>}"
FORMULA="${2:?usage: open-homebrew-tap-pr.sh <vMAJOR.MINOR.PATCH> <formula>}"
case "$TAG" in v[0-9]*.[0-9]*.[0-9]*) ;; *) echo "invalid release tag: $TAG" >&2; exit 1 ;; esac
test -f "$FORMULA"
test -n "${GH_TOKEN:?GH_TOKEN must be a fine-grained token for the Homebrew tap}"
FORMULA="$(CDPATH='' cd -- "$(dirname -- "$FORMULA")" && pwd)/$(basename -- "$FORMULA")"

TAP_REPO="acartag7/homebrew-tap"
BRANCH="chore/pasteforward-${TAG}"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

ensure_tap_pr() {
  state="$(gh pr list \
    --repo "$TAP_REPO" \
    --head "$BRANCH" \
    --state all \
    --json state \
    --jq '.[0].state // ""')"
  case "$state" in
    OPEN)
      echo "tap pull request already exists for $TAG"
      ;;
    CLOSED)
      gh pr reopen "$BRANCH" --repo "$TAP_REPO"
      ;;
    MERGED)
      echo "tap pull request for $TAG is already merged"
      ;;
    '')
      gh pr create \
        --repo "$TAP_REPO" \
        --base main \
        --head "$BRANCH" \
        --title "chore: update PasteForward to $TAG" \
        --body "Update the checksum-pinned PasteForward formula for $TAG. The source release remains draft until this PR is reviewed and merged."
      ;;
    *)
      echo "unexpected tap pull request state: $state" >&2
      exit 1
      ;;
  esac
}

gh repo clone "$TAP_REPO" "$tmp/tap" -- --depth=1
cd "$tmp/tap"
if git ls-remote --exit-code --heads origin "$BRANCH" >/dev/null 2>&1; then
  git fetch origin "$BRANCH"
  git checkout -B "$BRANCH" FETCH_HEAD
else
  git checkout -b "$BRANCH"
fi
mkdir -p Formula
install -m 0644 "$FORMULA" Formula/pasteforward.rb

if git diff --quiet -- Formula/pasteforward.rb; then
  echo "tap formula already matches $TAG"
  ensure_tap_pr
  exit 0
fi

git config user.name "PasteForward release automation"
git config user.email "release-automation@users.noreply.github.com"
git add Formula/pasteforward.rb
git commit -m "chore: update PasteForward to $TAG"
git push --set-upstream origin "$BRANCH"
ensure_tap_pr
