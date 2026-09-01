#!/usr/bin/env bash
# Validate WiX installer scaffold contains VCam registration hooks (Linux-hostable).
# REQ-PICOO-VCAM-004 — full MSI build still requires windows-latest.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WXS="$ROOT/installers/windows/picoo-camera.wxs"
BUILD_MSI="$ROOT/installers/windows/build-msi.ps1"
CI_WORKFLOW="$ROOT/.github/workflows/ci.yml"
VCAM_IDS="$ROOT/extensions/windows-virtual-camera/mf-source/src/windows_source/mod.rs"
VCAM_MANIFEST="$ROOT/extensions/windows-virtual-camera/mf-source/Cargo.toml"
VCAM_RS="$ROOT/apps/desktop/src/vcam_register.rs"
WINDOWS_RESOURCE="$ROOT/build-support/windows_resource.rs"
DESKTOP_BUILD="$ROOT/apps/desktop/build.rs"
MF_SOURCE_BUILD="$ROOT/extensions/windows-virtual-camera/mf-source/build.rs"
RING_READER_BUILD="$ROOT/extensions/windows-virtual-camera/ring-reader/build.rs"
XTASK_WINDOWS="$ROOT/xtask/src/windows.rs"
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
need "$WXS" 'Version="$(PicooMsiVersion)"'
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
need "$WXS" 'FirewallMdns'
need "$WXS" 'KeyPath="yes"'
need "$WXS" 'MajorUpgrade'
need "$WXS" 'Schedule="afterInstallExecute"'
need "$WXS" 'Id="REINSTALLMODE"'
need "$WXS" 'Value="amus"'
need "$WXS" 'Before="CostFinalize"'
need "$WXS" 'Condition="WIX_UPGRADE_DETECTED"'
need "$WXS" 'StartMenuDesktop'
need "$WXS" 'Icon Id="PicooProductIcon"'
need "$WXS" 'ARPPRODUCTICON'
need "$WXS" 'PicooCamera.ico'
need "$BUILD_MSI" 'wix build $Wxs -arch x64'
need "$BUILD_MSI" '$env:PICOO_WINDOWS_MSI_VERSION'
need "$BUILD_MSI" '-d "PicooMsiVersion=$MsiVersion"'
need "$BUILD_MSI" 'PicooCamera.version'
need "$CI_WORKFLOW" 'PICOO_BUILD_NUMBER: ${{ github.run_number }}'
if grep -qE 'Version="[0-9]+\.[0-9]+\.[0-9]+"' "$WXS"; then
  echo "picoo-camera.wxs must not hard-code the MSI ProductVersion"
  fail=1
else
  echo "ok: picoo-camera.wxs has no hard-coded ProductVersion"
fi
need "$WXS" 'Component Id="DesktopExe" Guid="A7C4E2F1-8B3D-4C6A-9E5F-1D2C3B4A5E71"'
need "$WXS" 'Component Id="VcamDll" Guid="A7C4E2F1-8B3D-4C6A-9E5F-1D2C3B4A5E72"'
need "$WXS" 'Component Id="RingReader" Guid="A7C4E2F1-8B3D-4C6A-9E5F-1D2C3B4A5E73"'
need "$WXS" 'Component Id="FirewallQuic" Guid="A7C4E2F1-8B3D-4C6A-9E5F-1D2C3B4A5E74"'
need "$WXS" 'Component Id="FirewallMdns" Guid="A7C4E2F1-8B3D-4C6A-9E5F-1D2C3B4A5E75"'
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
if awk '/Id="RegisterVcamOnInstall"/{found=1} found && /Return=/{print; exit}' "$WXS" | grep -q 'Return="check"'; then
  echo "ok: MSI install fails when MF virtual-camera registration fails"
else
  echo "picoo-camera.wxs: RegisterVcamOnInstall must use Return=check"
  fail=1
fi
need "$WXS" 'xmlns:util='
need "$WXS" 'UnregisterVcamOnRemove'
need "$WXS" '--unregister-vcam'
need "$WXS" 'RollbackVcamRegistration'
need "$WXS" 'Execute="rollback"'
need "$WXS" 'Condition="NOT Installed AND NOT WIX_UPGRADE_DETECTED"'
need "$WXS" 'Custom Action="RegisterVcamOnInstall" After="RemoveExistingProducts" Condition="NOT REMOVE"'
need "$WXS" 'RestoreVcamOnUpgradeRollback'
need "$WXS" 'Custom Action="RestoreVcamOnUpgradeRollback" Before="RemoveExistingProducts" Condition="WIX_UPGRADE_DETECTED"'
need "$WXS" 'Custom Action="UnregisterVcamOnRemove" Before="RemoveRegistryValues" Condition="REMOVE~=&quot;ALL&quot; AND NOT UPGRADINGPRODUCTCODE"'
if awk '/Id="RollbackVcamRegistration"/{found=1} found && /Impersonate=/{print; exit}' "$WXS" | grep -q 'Impersonate="no"'; then
  echo "ok: rollback removal uses the AllUsers administrator context"
else
  echo "picoo-camera.wxs: RollbackVcamRegistration must be non-impersonated"
  fail=1
fi
if awk '/Id="UnregisterVcamOnRemove"/{found=1} found && /Return=/{print; exit}' "$WXS" | grep -q 'Return="check"'; then
  echo "ok: MSI keeps installed files when VCam removal fails"
else
  echo "picoo-camera.wxs: UnregisterVcamOnRemove must use Return=check"
  fail=1
fi
if awk '/Id="UnregisterVcamOnRemove"/{found=1} found && /Impersonate=/{print; exit}' "$WXS" | grep -q 'Impersonate="no"'; then
  echo "ok: uninstall removal uses the AllUsers administrator context"
else
  echo "picoo-camera.wxs: UnregisterVcamOnRemove must be non-impersonated"
  fail=1
fi
# LAN QUIC and mDNS discovery firewall exception scaffolding (PRD §19.3 / DISCOVERY-001)
need "$WXS" 'FirewallException'
need "$WXS" 'xmlns:fw='
need "$WXS" 'fw:FirewallException'
need "$WXS" 'Port="4433"'
need "$WXS" 'Picoo Camera QUIC'
need "$WXS" 'Port="5353"'
need "$WXS" 'Picoo Camera Discovery'
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
if grep -R -qF 'AgileReference::' "$ROOT/extensions/windows-virtual-camera/mf-source/src/windows_source"; then
  echo "MF Source must not wrap standard Media Foundation interfaces in RoGetAgileReference"
  fail=1
else
  echo "ok: MF Source stores direct Media Foundation COM references"
fi
if grep -R -qF 'MF_VIRTUALCAMERA_PROVIDE_ASSOCIATED_CAMERA_SOURCES' \
    "$ROOT/extensions/windows-virtual-camera/mf-source/src/windows_source"; then
  echo "synthetic MF Source must not claim associated physical camera sources"
  fail=1
else
  echo "ok: synthetic MF Source does not claim associated physical cameras"
fi
need "$ROOT/extensions/windows-virtual-camera/mf-source/src/windows_source/media_source.rs" 'IKsControl'
need "$VCAM_RS" 'MFEnumDeviceSources'
need "$VCAM_RS" 'MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK'
need "$VCAM_RS" 'MFVirtualCameraAccess_AllUsers'
need "$VCAM_RS" 'vcam_symbolic_link'
need "$VCAM_RS" 'camera_identity_matches'
need "$VCAM_RS" 'wait_for_registered_camera'
need "$VCAM_RS" 'self.camera.Shutdown()'

# Late major upgrades depend on new PE versions replacing the old maintenance
# binaries before the cached related product runs its uninstall command.
need "$WINDOWS_RESOURCE" 'PICOO_WINDOWS_FILE_VERSION'
need "$WINDOWS_RESOURCE" 'VersionInfo::FILEVERSION'
need "$WINDOWS_RESOURCE" 'VersionInfo::PRODUCTVERSION'
need "$DESKTOP_BUILD" 'windows_resource::apply_package_version'
need "$MF_SOURCE_BUILD" 'windows_resource::apply_package_version'
need "$RING_READER_BUILD" 'windows_resource::apply_package_version'
need "$XTASK_WINDOWS" 'PICOO_WINDOWS_FILE_VERSION'
if awk '/Id="RegisterVcamOnInstall"/{found=1} found && /Impersonate=/{print; exit}' "$WXS" | grep -q 'Impersonate="no"'; then
  echo "ok: per-machine VCam registration runs elevated for AllUsers access"
else
  echo "picoo-camera.wxs: RegisterVcamOnInstall must run non-impersonated"
  fail=1
fi

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
need "$VERIFY_PS1" 'ProductVersion'
need "$VERIFY_PS1" 'PicooCamera.version'

if [[ "$fail" -ne 0 ]]; then
  echo "WiX scaffold validation failed"
  exit 1
fi
echo "ok picoo-camera.wxs + CLSID sync scaffold"
