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
  for archive in control.tar.gz data.tar.gz; do
    owners="$(ar p "$deb" "$archive" | tar --numeric-owner -tvzf - | awk '{print $2}' | sort -u)"
    test "$owners" = "0/0" || { echo "deb archive ownership is not root:root" >&2; exit 1; }
  done
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
  owners="$(rpm -qp --qf '[%{FILEUSERNAME}:%{FILEGROUPNAME}\n]' "$rpm" | sort -u)"
  test "$owners" = "root:root" || { echo "rpm archive ownership is not root:root" >&2; exit 1; }
done

if [ "${PASTEFORWARD_EXPECT_PACKAGES:-0}" = 1 ] && [ "$found" -eq 0 ]; then
  echo "no deb or rpm packages found" >&2
  exit 1
fi

echo "package contents ok"
