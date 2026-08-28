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
need "$WXS" 'Software\Classes\CLSID\{A7C4E2F1-8B3D-4C6A-9E5F-1D2C3B4A5E6F}'
need "$WXS" 'InprocServer32'
need "$WXS" 'ThreadingModel'
need "$WXS" 'Value="[#PicooVcamDll]"'
need "$WXS" 'Value="Both"'
need "$WXS" 'Value="Picoo Camera"'
if grep -qE 'RegistryValue[^>]*Name=""' "$WXS"; then
  echo "picoo-camera.wxs: RegistryValue Name must be omitted for default values, not empty string (WIX0006)"
  fail=1
else
  echo "ok: picoo-camera.wxs default RegistryValue names omitted"
fi
# Post-build MSI check lives in verify_windows_bundle.ps1 (windows-latest only; no msiexec).
need "$WXS" 'FirewallQuic'
need "$WXS" 'KeyPath="yes"'
need "$WXS" 'MajorUpgrade'
need "$WXS" 'StartMenuDesktop'
need_re "$WXS" 'Guid="A7C4E2F1-8B3D-4C6A-9E5F-1D2C3B4A5E7[1234]"'
need "$WXS" 'Component Id="DesktopExe"'
need "$WXS" 'Component Id="VcamDll"'
need "$WXS" 'Component Id="RingReader"'
# Deferred regsvr32 Return=check aborts MSI when DllRegisterServer fails on clean Win11.
if grep -qF 'RegisterVcamDll' "$WXS" || grep -qF 'regsvr32.exe' "$WXS"; then
  echo "picoo-camera.wxs must not use deferred regsvr32 (use declarative COM registry)"
  fail=1
else
  echo "ok: picoo-camera.wxs avoids deferred regsvr32"
fi
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

# Bundle smoke script must resolve repo root as parent of scripts/ (not grandparent).
VERIFY_PS1="$ROOT/scripts/verify_windows_bundle.ps1"
need "$VERIFY_PS1" 'Split-Path -Parent $PSScriptRoot'
if grep -qF 'Split-Path -Parent (Split-Path -Parent $PSScriptRoot)' "$VERIFY_PS1"; then
  echo "verify_windows_bundle.ps1 incorrectly double-parents repo root"
  fail=1
else
  echo "ok: verify_windows_bundle.ps1 repo root depth"
fi

if [[ "$fail" -ne 0 ]]; then
  echo "WiX scaffold validation failed"
  exit 1
fi
echo "ok picoo-camera.wxs + CLSID sync scaffold"