#!/usr/bin/env bash
# Validate WiX installer scaffold contains VCam registration hooks (Linux-hostable).
# REQ-PICOO-VCAM-004 — full MSI build still requires windows-latest.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WXS="$ROOT/installers/windows/picoo-camera.wxs"
VCAM_IDS="$ROOT/extensions/windows-virtual-camera/mf-source/src/windows_source/mod.rs"
VCAM_MANIFEST="$ROOT/extensions/windows-virtual-camera/mf-source/Cargo.toml"
VCAM_RS="$ROOT/apps/desktop/src/vcam_register.rs"
CLSID="A7C4E2F1-8B3D-4C6A-9E5F-1D2C3B4A5E6F"
CLSID_RUST="0xa7c4e2f1_8b3d_4c6a_9e5f_1d2c3b4a5e6f"
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
# WIX0104: XML comments cannot contain '--' (WiX rejects invalid comment bodies).
if python3 - "$WXS" <<'PY'
import re, sys
path = sys.argv[1]
text = open(path, encoding="utf-8").read()
for match in re.finditer(r"<!--(.*?)-->", text, re.DOTALL):
    body = match.group(1)
    if "--" in body:
        snippet = body.strip().replace("\n", " ")[:120]
        print(f"picoo-camera.wxs: XML comment contains '--' (WIX0104): {snippet}")
        sys.exit(1)
sys.exit(0)
PY
then
  echo "ok: picoo-camera.wxs XML comments avoid '--' (WIX0104)"
else
  fail=1
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
# COM registration is declarative; self-registration is intentionally absent.
for forbidden in RegisterVcamDll RegisterVcamComDll regsvr32.exe DllRegisterServer; do
  if grep -qF "$forbidden" "$WXS"; then
    echo "picoo-camera.wxs must not contain self-registration path: $forbidden"
    fail=1
  fi
done
echo "ok: picoo-camera.wxs uses declarative COM registration only"
need "$WXS" 'RegisterVcamOnInstall'
need "$WXS" '--register-vcam --no-wait'
need "$WXS" 'WixQuietExec'
need "$WXS" 'xmlns:util='
need "$WXS" 'UnregisterVcamOnRemove'
need "$WXS" '--unregister-vcam'
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

# COM CLSID must stay identical across the Rust cdylib and desktop maintenance command.
need "$VCAM_IDS" "$CLSID_RUST"
need "$VCAM_IDS" 'DllGetClassObject'
need "$VCAM_IDS" 'DllCanUnloadNow'
need "$VCAM_MANIFEST" 'crate-type = ["cdylib", "rlib"]'
need "$VCAM_MANIFEST" 'windows-core = "0.62.2"'
need "$VCAM_MANIFEST" 'windows = { version = "0.62.2"'
need "$VCAM_RS" "$CLSID"

if find "$ROOT/extensions/windows-virtual-camera/mf-source" \
    \( -name '*.cpp' -o -name '*.h' -o -name '*.vcxproj' -o -name 'CMakeLists.txt' \) \
    -print -quit | grep -q .; then
  echo "Rust VCam Source must not retain C++/VCXPROJ/CMake files"
  fail=1
else
  echo "ok: Rust VCam Source has no C++/VCXPROJ/CMake files"
fi

# Bundle smoke script must resolve repo root as parent of scripts/ (not grandparent).
VERIFY_PS1="$ROOT/scripts/verify_windows_bundle.ps1"
need "$VERIFY_PS1" 'Split-Path -Parent $PSScriptRoot'
if grep -qF 'Split-Path -Parent (Split-Path -Parent $PSScriptRoot)' "$VERIFY_PS1"; then
  echo "verify_windows_bundle.ps1 incorrectly double-parents repo root"
  fail=1
else
  echo "ok: verify_windows_bundle.ps1 repo root depth"
fi
need "$VERIFY_PS1" "'RegisterVcamComDll'"
need "$VERIFY_PS1" "'regsvr32.exe'"
need "$VERIFY_PS1" "'DllRegisterServer'"

if [[ "$fail" -ne 0 ]]; then
  echo "WiX scaffold validation failed"
  exit 1
fi
echo "ok picoo-camera.wxs + CLSID sync scaffold"
