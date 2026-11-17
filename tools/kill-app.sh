#!/usr/bin/env bash
# Stop the running Mio Sound Manager window before rebuilding: Windows refuses to replace an
# exe that is still mapped, and `taskkill //F` under git-bash mangles the flags.
powershell -NoProfile -Command "Stop-Process -Name mio-sound-manager -Force -ErrorAction SilentlyContinue"
sleep 1
