#!/usr/bin/env bash
# Launch the release build with WebView2's remote debugger on, so the renderer can
# be driven over CDP instead of by clicking.
#
#   ./tools/run-app.sh                 # default device
#   MIO_DEVICE=E2x2 ./tools/run-app.sh
#   ./tools/run-app.sh --cdp 9223
#
# Why a script: `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS` has to be set before the
# webview is created, and it is the only way to inspect or screenshot the real
# renderer. Without it, verifying the UI means asking a human to look.
set -euo pipefail

cd "$(dirname "$0")/.."

PORT=9223
if [ "${1:-}" = "--cdp" ]; then
  PORT="${2:-9223}"
fi

if [ ! -f ui/dist/index.html ]; then
  echo "ui/dist is missing; run: (cd ui && npm run build)" >&2
  exit 1
fi

if [ ! -f target/release/mio-sound-manager.exe ]; then
  echo "target/release/mio-sound-manager.exe is missing; run: cargo build -p mio-app --release" >&2
  exit 1
fi

export WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=${PORT}"
echo "webview debugger: http://127.0.0.1:${PORT}"
exec ./target/release/mio-sound-manager.exe
