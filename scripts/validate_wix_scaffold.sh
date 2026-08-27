#!/usr/bin/env bash
# Validate WiX installer scaffold contains VCam registration hooks (Linux-hostable).
# REQ-PICOO-VCAM-004 — full MSI build still requires windows-latest.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WXS="$ROOT/installers/windows/picoo-camera.wxs"
fail=0
need() {
  if ! grep -qF "$1" "$WXS"; then
    echo "MISSING: $1"
    fail=1
  else
    echo "ok: $1"
  fi
}
need 'Name="Picoo Camera"'
need 'PicooVirtualCameraSource.dll'
need 'regsvr32 /s'
need 'regsvr32 /u /s'
need 'picoo-desktop.exe'
need 'RegisterVcamDll'
need 'UnregisterVcamDll'
need 'Return="check"'
if [[ "$fail" -ne 0 ]]; then
  echo "WiX scaffold validation failed"
  exit 1
fi
echo "ok picoo-camera.wxs scaffold"
