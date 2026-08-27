#!/usr/bin/env bash
# Validate WiX installer scaffold contains VCam registration hooks (Linux-hostable).
# REQ-PICOO-VCAM-004 — full MSI build still requires windows-latest.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WXS="$ROOT/installers/windows/picoo-camera.wxs"
REG_PS1="$ROOT/installers/windows/register-vcam.ps1"
VCAM_IDS="$ROOT/extensions/windows-virtual-camera/mf-source/include/picoo_vcam_ids.h"
VCAM_RS="$ROOT/apps/desktop/src/vcam_register.rs"
CLSID="A7C4E2F1-8B3D-4C6A-9E5F-1D2C3B4A5E6F"
fail=0
need() {
  local file="$1"
  local needle="$2"
  if ! grep -qF -- "$needle" "$file"; then
    echo "MISSING in $(basename "$file"): $needle"
    fail=1
  else
    echo "ok: $(basename "$file") :: $needle"
  fi
}
need_re() {
  local file="$1"
  local needle="$2"
  if ! grep -qE -- "$needle" "$file"; then
    echo "MISSING_RE in $(basename "$file"): $needle"
    fail=1
  else
    echo "ok_re: $(basename "$file") :: $needle"
  fi
}

need "$WXS" 'Name="Picoo Camera"'
need "$WXS" 'Manufacturer="Picoo"'
need "$WXS" 'UpgradeCode="A7C4E2F1-8B3D-4C6A-9E5F-1D2C3B4A5E70"'
need "$WXS" 'PicooVirtualCameraSource.dll'
need "$WXS" 'picoo-desktop.exe'
need "$WXS" 'picoo-vcam-ring-reader.exe'
need "$WXS" 'SystemFolder]regsvr32.exe'
need "$WXS" 'RegisterVcamDll'
need "$WXS" 'UnregisterVcamDll'
need "$WXS" 'FirewallQuic'
need "$WXS" 'KeyPath="yes"'
need "$WXS" 'Return="check"'
need "$WXS" 'MajorUpgrade'
need "$WXS" 'StartMenuDesktop'
need "$WXS" 'Condition="NOT REMOVE"'
need "$WXS" 'Condition="REMOVE~=&quot;ALL&quot;"'
need_re "$WXS" 'Guid="A7C4E2F1-8B3D-4C6A-9E5F-1D2C3B4A5E7[1234]"'
need "$WXS" 'Component Id="DesktopExe"'
need "$WXS" 'Component Id="VcamDll"'
need "$WXS" 'Component Id="RingReader"'
# LAN QUIC firewall exception scaffolding (PRD §19.3)
need "$WXS" 'FirewallException'
need "$WXS" 'xmlns:fw='
need "$WXS" 'fw:FirewallException'
need "$WXS" 'Port="4433"'
need "$WXS" 'Picoo Camera QUIC'
need "$WXS" 'Protocol="udp"'

# DEFAULT_QUIC_PORT in Rust must stay aligned with WiX FirewallException.
HOST_RS="$ROOT/crates/picoo-discovery/src/host.rs"
need "$HOST_RS" 'DEFAULT_QUIC_PORT: u16 = 4433'
if ! grep -qE 'Port="4433"' "$WXS"; then
  echo "port drift: WiX FirewallException must use Port=4433"
  fail=1
fi

# COM CLSID must stay identical across C++ / Rust / register script.
need "$VCAM_IDS" "$CLSID"
need "$VCAM_RS" "$CLSID"
need "$REG_PS1" "$CLSID"
need "$REG_PS1" 'regsvr32 /s'
need "$REG_PS1" 'regsvr32 /u /s'
need "$REG_PS1" '--register-vcam'
need "$REG_PS1" '--unregister-vcam'

if [[ "$fail" -ne 0 ]]; then
  echo "WiX scaffold validation failed"
  exit 1
fi
echo "ok picoo-camera.wxs + CLSID sync scaffold"
