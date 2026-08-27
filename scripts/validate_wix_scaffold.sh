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
need_re() {
  if ! grep -qE "$1" "$WXS"; then
    echo "MISSING_RE: $1"
    fail=1
  else
    echo "ok_re: $1"
  fi
}
need 'Name="Picoo Camera"'
need 'Manufacturer="Picoo"'
need 'UpgradeCode="A7C4E2F1-8B3D-4C6A-9E5F-1D2C3B4A5E70"'
need 'PicooVirtualCameraSource.dll'
need 'picoo-desktop.exe'
need 'picoo-vcam-ring-reader.exe'
need 'regsvr32 /s'
need 'regsvr32 /u /s'
need 'RegisterVcamDll'
need 'UnregisterVcamDll'
need 'Return="check"'
need 'MajorUpgrade'
need 'StartMenuDesktop'
need 'Name="Picoo Camera"'
need 'Condition="NOT REMOVE"'
need 'Condition="REMOVE~=&quot;ALL&quot;"'
need_re 'Guid="A7C4E2F1-8B3D-4C6A-9E5F-1D2C3B4A5E7[123]"'
# Three product components: exe, VCam DLL, ring-reader
need 'Component Id="DesktopExe"'
need 'Component Id="VcamDll"'
need 'Component Id="RingReader"'
if [[ "$fail" -ne 0 ]]; then
  echo "WiX scaffold validation failed"
  exit 1
fi
echo "ok picoo-camera.wxs scaffold"
