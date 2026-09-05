#!/usr/bin/env bash
# Validate Linux-hostable installer and architecture contracts.
# REQ-PICOO-VCAM-004 — runtime registration/install behavior remains a Windows host test.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WXS="$ROOT/installers/windows/picoo-camera.wxs"
BUILD_MSI="$ROOT/installers/windows/build-msi.ps1"
CI_WORKFLOW="$ROOT/.github/workflows/ci.yml"
VCAM_IDS="$ROOT/extensions/windows-virtual-camera/mf-source/src/windows_source/mod.rs"
VCAM_MANIFEST="$ROOT/extensions/windows-virtual-camera/mf-source/Cargo.toml"
VCAM_RS="$ROOT/apps/desktop/src/vcam_register.rs"
VCAM_HOST_WORKFLOW="$ROOT/.github/workflows/windows-vcam-host.yml"
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
# Post-build MSI table and ICE checks live on windows-latest; no msiexec install.
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
need "$BUILD_MSI" 'wix msi validate $Msi -ice ICE27 -ice ICE63 -ice ICE77'
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
need "$WXS" 'Id="SharedRingDirectory"'
need "$WXS" 'Guid="A7C4E2F1-8B3D-4C6A-9E5F-1D2C3B4A5E76"'
need "$WXS" 'Id="PicooFrameDataFolder" Name="Picoo Camera"'
need "$WXS" 'Id="SharedRingDirectoryAcl"'
need "$WXS" ';;;LS)'
need "$WXS" ';;;BU)'
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
need "$WXS" 'Execute="commit"'
need "$WXS" 'Condition="NOT Installed AND NOT WIX_UPGRADE_DETECTED"'
need "$WXS" 'Custom Action="RegisterVcamOnInstall" Before="InstallExecute" Condition="NOT REMOVE"'
need "$WXS" 'RestoreVcamOnUpgradeRollback'
need "$WXS" 'Custom Action="RollbackVcamRegistration" Before="RestoreVcamOnUpgradeRollback" Condition="NOT Installed AND NOT WIX_UPGRADE_DETECTED"'
need "$WXS" 'Custom Action="RestoreVcamOnUpgradeRollback" Before="RegisterVcamOnInstall" Condition="WIX_UPGRADE_DETECTED"'
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
need "$WXS" 'xmlns:fw='
HOST_RS="$ROOT/crates/picoo-discovery/src/host.rs"
if python3 - "$WXS" "$HOST_RS" <<'PY'
import re
import sys
import xml.etree.ElementTree as ET

wxs_path, host_path = sys.argv[1:]
root = ET.parse(wxs_path).getroot()

def local_name(tag):
    return tag.rsplit("}", 1)[-1]

components = {
    component.attrib.get("Id"): component
    for component in root.iter()
    if local_name(component.tag) == "Component"
}

expected = {
    "FirewallQuic": ("PicooQuicUdp", "Picoo Camera QUIC", None),
    "FirewallMdns": ("PicooMdnsUdp", "Picoo Camera Discovery", 5353),
}

host_source = open(host_path, encoding="utf-8").read()
match = re.search(r"DEFAULT_QUIC_PORT:\s*u16\s*=\s*(\d+)", host_source)
if not match:
    raise SystemExit("host.rs: DEFAULT_QUIC_PORT constant not found")
expected["FirewallQuic"] = (*expected["FirewallQuic"][:2], int(match.group(1)))

for component_id, (exception_id, display_name, port) in expected.items():
    component = components.get(component_id)
    if component is None:
        raise SystemExit(f"picoo-camera.wxs: missing Component {component_id}")
    registry = next(
        (node for node in component if local_name(node.tag) == "RegistryValue"),
        None,
    )
    if registry is None or registry.attrib.get("KeyPath") != "yes":
        raise SystemExit(f"picoo-camera.wxs: {component_id} needs its own registry KeyPath")
    firewall = next(
        (node for node in component if local_name(node.tag) == "FirewallException"),
        None,
    )
    if firewall is None:
        raise SystemExit(f"picoo-camera.wxs: {component_id} has no FirewallException")
    actual = (
        firewall.attrib.get("Id"),
        firewall.attrib.get("Name"),
        firewall.attrib.get("Protocol"),
        firewall.attrib.get("Port"),
    )
    wanted = (exception_id, display_name, "udp", str(port))
    if actual != wanted:
        raise SystemExit(
            f"picoo-camera.wxs: {component_id} firewall {actual!r}, expected {wanted!r}"
        )
PY
then
  echo "ok: parsed WiX firewall components match Rust QUIC and mDNS port contracts"
else
  fail=1
fi

# COM CLSID must stay identical across the Rust cdylib and desktop maintenance command.
need "$VCAM_IDS" "$CLSID_RUST"
need "$VCAM_IDS" 'DllGetClassObject'
need "$VCAM_IDS" 'DllCanUnloadNow'
need "$VCAM_MANIFEST" 'crate-type = ["cdylib", "rlib"]'
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
need "$VCAM_HOST_WORKFLOW" 'runs-on: [self-hosted, Windows, X64, picoo-vcam]'
need "$VCAM_HOST_WORKFLOW" './scripts/test_windows_vcam_host.ps1'
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

if [[ "$fail" -ne 0 ]]; then
  echo "WiX scaffold validation failed"
  exit 1
fi
echo "ok picoo-camera.wxs + CLSID sync scaffold"
