#!/usr/bin/env bash
# Stop every running Mio Sound Manager window before rebuilding.
#
# Windows refuses to replace an exe that is still mapped, and `taskkill //F` under
# git-bash mangles the flags, so this goes through PowerShell.
#
# Two names, because a build produces two different process names:
#
#   mio-sound-manager      `cargo build` output and the installed copy
#   Mio Sound Manager      the portable zip's exe, which is named for the product
#
# Matching only the first left a portable copy running and made the next build fail
# with `failed to remove file … 拒绝被访问`, which reads like a permissions problem.
for name in 'mio-sound-manager' 'Mio Sound Manager'; do
  powershell -NoProfile -Command \
    "Stop-Process -Name '$name' -Force -ErrorAction SilentlyContinue"
done
sleep 1
