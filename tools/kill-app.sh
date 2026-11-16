#!/usr/bin/env bash
# Stop the running ReSlip window before rebuilding: Windows refuses to replace an
# exe that is still mapped, and `taskkill //F` under git-bash mangles the flags.
powershell -NoProfile -Command "Stop-Process -Name reslip -Force -ErrorAction SilentlyContinue"
sleep 1
