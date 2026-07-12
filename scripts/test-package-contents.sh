#!/usr/bin/env sh
set -eu

ROOT="$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)"
cd "$ROOT"
found=0

for deb in dist/pasteforward_*.deb; do
  [ -e "$deb" ] || continue
  found=1
  members="$(ar t "$deb")"
  test "$members" = "debian-binary
control.tar.gz
data.tar.gz"
  listing="$(ar p "$deb" data.tar.gz | tar -tzf - | sort)"
  expected="$(printf '%s\n' \
    ./ \
    ./usr/ \
    ./usr/bin/ \
    ./usr/bin/pasteforward \
    ./usr/share/ \
    ./usr/share/doc/ \
    ./usr/share/doc/pasteforward/ \
    ./usr/share/doc/pasteforward/LICENSE \
    ./usr/share/doc/pasteforward/README.md | sort)"
  test "$listing" = "$expected" || { echo "deb file manifest is not allowlisted" >&2; exit 1; }
done

for rpm in dist/pasteforward-*.rpm; do
  [ -e "$rpm" ] || continue
  found=1
  command -v rpm >/dev/null 2>&1 || { echo "rpm is required to inspect $rpm" >&2; exit 1; }
  listing="$(rpm -qlp "$rpm" | sort)"
  expected="$(printf '%s\n' \
    /usr/bin/pasteforward \
    /usr/share/doc/pasteforward/LICENSE \
    /usr/share/doc/pasteforward/README.md | sort)"
  test "$listing" = "$expected" || { echo "rpm file manifest is not allowlisted" >&2; exit 1; }
  scripts="$(rpm -qp --scripts "$rpm")"
  test -z "$scripts"
done

if [ "${PASTEFORWARD_EXPECT_PACKAGES:-0}" = 1 ] && [ "$found" -eq 0 ]; then
  echo "no deb or rpm packages found" >&2
  exit 1
fi

echo "package contents ok"
