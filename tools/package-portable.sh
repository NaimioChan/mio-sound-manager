#!/usr/bin/env bash
# Package the portable build as a zip.
#
#   ./tools/package-portable.sh                 # after ./tools/build-app.sh
#   ./tools/package-portable.sh --out DIR       # put the zip somewhere else
#
# The portable edition is a zip rather than a single exe on purpose. A "single exe"
# is a lie about what runs: it is the raw cargo output with a nicer name, and the
# moment anything has to sit next to it (licence text, notices, a future plugin
# directory) the rename becomes a trap — the file people downloaded stops being the
# thing that works. A zip says what is inside.
#
# What goes in:
#
#   Mio Sound Manager.exe   the release binary, WebView2 bundle embedded
#   LICENSE                 AGPL requires it to travel with the binary
#   THIRD-PARTY-NOTICES.md  the ASIO SDK's GPLv3 branch and iX's MIT attribution
#
# No DLLs: `tauri` links WebView2Loader statically and the webview assets are
# compiled into the exe via the `custom-protocol` feature, so the directory really
# is self-contained. The script verifies that by refusing to ship if any .dll
# appears next to the binary.
set -euo pipefail

cd "$(dirname "$0")/.."

OUT_DIR="dist"
while [ $# -gt 0 ]; do
  case "$1" in
    --out) OUT_DIR="${2:?--out needs a directory}"; shift 2 ;;
    *) echo "unknown flag $1; expected --out DIR" >&2; exit 1 ;;
  esac
done

EXE="target/release/mio-sound-manager.exe"
for required in "$EXE" LICENSE THIRD-PARTY-NOTICES.md; do
  if [ ! -f "$required" ]; then
    echo "missing $required" >&2
    if [ "$required" = "$EXE" ]; then
      echo "  build it first: ./tools/build-app.sh" >&2
    fi
    exit 1
  fi
done

# A .dll next to the binary would mean the zip is incomplete, and a zip that is
# missing a dependency fails on the user's machine rather than here.
stray=$(find target/release -maxdepth 1 -name '*.dll' -printf '%f\n' 2>/dev/null || true)
if [ -n "$stray" ]; then
  echo "these DLLs sit next to the binary and would have to ship too:" >&2
  echo "$stray" >&2
  echo "add them to the staging directory below before releasing." >&2
  exit 1
fi

# Version comes from the one place that decides it, so the zip name cannot drift
# from the binary it contains.
VERSION=$(grep -m1 '^version' Cargo.toml | sed 's/.*"\(.*\)".*/\1/')
STAGE="$OUT_DIR/Mio-Sound-Manager_${VERSION}_x64-portable"
ZIP="$STAGE.zip"

rm -rf "$STAGE" "$ZIP"
mkdir -p "$STAGE"
cp "$EXE" "$STAGE/Mio Sound Manager.exe"
cp LICENSE THIRD-PARTY-NOTICES.md "$STAGE/"

# `-mx=9` costs a second on a 17 MB binary and saves a few MB of download; the
# archive is mostly already-compressed code, so do not expect a ratio.
(cd "$OUT_DIR" && 7z a -tzip -mx=9 -bso0 -bsp0 "$(basename "$ZIP")" "$(basename "$STAGE")" >/dev/null)

rm -rf "$STAGE"
echo "wrote $ZIP ($(stat -c%s "$ZIP" 2>/dev/null || wc -c < "$ZIP") bytes)"
echo "  contains: Mio Sound Manager.exe, LICENSE, THIRD-PARTY-NOTICES.md"
