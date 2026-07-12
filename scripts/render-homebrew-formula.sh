#!/usr/bin/env sh
set -eu

ROOT="$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)"
cd "$ROOT"
VERSION="${1:-$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)}"
BASE_URL="${PASTEFORWARD_RELEASE_BASE_URL:-https://github.com/acartag7/pasteforward/releases/download/v$VERSION}"
case "$VERSION" in
  *[!0-9.]* | .* | *. | *..*) echo "invalid version: $VERSION" >&2; exit 1 ;;
esac
major="${VERSION%%.*}"
rest="${VERSION#*.}"
minor="${rest%%.*}"
patch="${rest#*.}"
case "$patch" in *.*) echo "version must be MAJOR.MINOR.PATCH" >&2; exit 1 ;; esac
test "$VERSION" = "$major.$minor.$patch" || { echo "version must be MAJOR.MINOR.PATCH" >&2; exit 1; }
case "$BASE_URL" in
  *\"* | *\\*) echo "invalid release base URL" >&2; exit 1 ;;
esac

checksum() {
  file="dist/$1.sha256"
  test -f "$file" || { echo "missing checksum: $file" >&2; exit 1; }
  awk 'NR == 1 { print $1 }' "$file"
}

mac_arm="pasteforward-v${VERSION}-aarch64-apple-darwin.tar.gz"
mac_intel="pasteforward-v${VERSION}-x86_64-apple-darwin.tar.gz"
linux_arm="pasteforward-v${VERSION}-aarch64-unknown-linux-musl.tar.gz"
linux_intel="pasteforward-v${VERSION}-x86_64-unknown-linux-musl.tar.gz"
output="dist/pasteforward.rb"

cat >"$output" <<EOF
class Pasteforward < Formula
  desc "Make image paste work in Claude Code and Codex over SSH"
  homepage "https://github.com/acartag7/pasteforward"
  version "$VERSION"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "$BASE_URL/$mac_arm"
      sha256 "$(checksum "$mac_arm")"
    else
      url "$BASE_URL/$mac_intel"
      sha256 "$(checksum "$mac_intel")"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "$BASE_URL/$linux_arm"
      sha256 "$(checksum "$linux_arm")"
    else
      url "$BASE_URL/$linux_intel"
      sha256 "$(checksum "$linux_intel")"
    end
  end

  def install
    bin.install "pasteforward"
  end

  test do
    ENV["HOME"] = testpath/"home"
    ENV["PASTEFORWARD_CONFIG_HOME"] = testpath/"config"
    ENV["PASTEFORWARD_STATE_HOME"] = testpath/"state"
    assert_match "pasteforward #{version}", shell_output("#{bin}/pasteforward --version")
    system bin/"pasteforward", "status"
  end
end
EOF

echo "$ROOT/$output"
