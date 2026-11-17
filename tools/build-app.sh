#!/usr/bin/env bash
# Build the desktop shell. ASIO is part of the product build, so it is on by default.
#
#   ./tools/build-app.sh                 # WASAPI + ASIO (the shipping build)
#   ./tools/build-app.sh --no-asio       # WASAPI only, for a machine without the SDK
#
# Two steps either way: bundle the webview, then build the exe that embeds it.
#
# The ASIO build needs two things the WASAPI build does not, because the
# Steinberg SDK is proprietary and cannot be vendored here:
#
#   1. The ASIO SDK sources, extracted to `%LOCALAPPDATA%\mio\asio_sdk` (the
#      layout the SDK zip has: `asio/common`, `asio/host`, ...). It lives under
#      `LOCALAPPDATA` rather than `%TEMP%` because Windows cleans the temp
#      directory, and a release build that breaks because the SDK was garbage
#      collected is not a reproducible build. `CPAL_ASIO_DIR` overrides it.
#   2. A `libclang.dll` for bindgen, located through `LIBCLANG_PATH`. The pip
#      `libclang` wheel ships one; so does a full LLVM install.
#
# Both are checked below so a missing one fails with a sentence instead of a
# bindgen panic halfway through a C++ compile.
set -euo pipefail

cd "$(dirname "$0")/.."

WITH_ASIO=1
for arg in "$@"; do
  case "$arg" in
    --asio) WITH_ASIO=1 ;;
    --no-asio) WITH_ASIO=0 ;;
    *) echo "unknown flag $arg; expected --asio or --no-asio" >&2; exit 1 ;;
  esac
done

if [ "$WITH_ASIO" = "1" ]; then
  sdk="${CPAL_ASIO_DIR:-$LOCALAPPDATA/mio/asio_sdk}"
  if [ ! -f "$sdk/common/asio.h" ]; then
    echo "ASIO SDK not found at $sdk" >&2
    echo "  download it from https://www.steinberg.net/asiosdk and extract so that" >&2
    echo "  $sdk/common/asio.h exists, or set CPAL_ASIO_DIR to where it is." >&2
    echo "  (./tools/build-app.sh --no-asio builds without it.)" >&2
    exit 1
  fi
  export CPAL_ASIO_DIR="$sdk"

  if [ -z "${LIBCLANG_PATH:-}" ]; then
    for candidate in \
      "$LOCALAPPDATA/hermes/hermes-agent/venv/Lib/site-packages/clang/native" \
      "/c/Program Files/LLVM/bin" \
      "/c/Program Files (x86)/LLVM/bin"
    do
      if [ -f "$candidate/libclang.dll" ]; then
        export LIBCLANG_PATH="$candidate"
        break
      fi
    done
  fi
  if [ -z "${LIBCLANG_PATH:-}" ]; then
    echo "libclang.dll not found; set LIBCLANG_PATH to a folder containing it" >&2
    echo "  (the pip 'libclang' wheel provides one)" >&2
    exit 1
  fi
  echo "ASIO: sdk=$CPAL_ASIO_DIR libclang=$LIBCLANG_PATH"
fi

(cd ui && npm run build)

if [ "$WITH_ASIO" = "1" ]; then
  cargo build -p mio-app --release --features asio
else
  cargo build -p mio-app --release
fi

echo "built target/release/mio-sound-manager.exe"
