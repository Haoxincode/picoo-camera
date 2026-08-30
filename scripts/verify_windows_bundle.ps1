# Verify staged Windows bundle embeds Picoo Camera identity (REQ-PICOO-VCAM-001).
# Runs on windows-latest after `xtask package windows`. Does NOT install MSI or run regsvr32;
# Win11 perMachine install acceptance remains manual (see vcam-meeting-apps.md).
$ErrorActionPreference = "Stop"

# scripts/ → repo root (one level up). Do NOT double-parent like installers/windows/*.ps1.
$Root = Split-Path -Parent $PSScriptRoot
$Bundle = Join-Path $Root "target/release/bundle"
$Dll = Join-Path $Bundle "PicooVirtualCameraSource.dll"
$Exe = Join-Path $Bundle "picoo-desktop.exe"
$Msi = Join-Path $Bundle "msi/PicooCamera.msi"

Write-Host "Repo root: $Root"
Write-Host "Bundle:    $Bundle"

foreach ($path in @($Exe, $Dll)) {
    if (-not (Test-Path $path)) {
        Write-Error "Missing required bundle file: $path"
    }
    Write-Host "ok: $(Split-Path -Leaf $path) ($((Get-Item $path).Length) bytes)"
}

# REQ-PICOO-UI-002: Explorer/startup launches must not create a console window.
# The PE Optional Header subsystem value 2 is IMAGE_SUBSYSTEM_WINDOWS_GUI.
$exeBytes = [System.IO.File]::ReadAllBytes($Exe)
if ($exeBytes.Length -lt 64 -or $exeBytes[0] -ne 0x4d -or $exeBytes[1] -ne 0x5a) {
    Write-Error "picoo-desktop.exe has an invalid DOS header"
}
$peOffset = [BitConverter]::ToInt32($exeBytes, 0x3c)
if ($peOffset -lt 0 -or $peOffset + 24 -gt $exeBytes.Length) {
    Write-Error "picoo-desktop.exe has an invalid PE header offset"
}
if ($exeBytes[$peOffset] -ne 0x50 -or $exeBytes[$peOffset + 1] -ne 0x45 -or
    $exeBytes[$peOffset + 2] -ne 0 -or $exeBytes[$peOffset + 3] -ne 0) {
    Write-Error "picoo-desktop.exe has an invalid PE signature"
}
$optionalHeader = $peOffset + 24
$optionalHeaderSize = [BitConverter]::ToUInt16($exeBytes, $peOffset + 20)
if ($optionalHeaderSize -lt 70 -or $optionalHeader + $optionalHeaderSize -gt $exeBytes.Length) {
    Write-Error "picoo-desktop.exe has an invalid optional header size"
}
$optionalHeaderMagic = [BitConverter]::ToUInt16($exeBytes, $optionalHeader)
if ($optionalHeaderMagic -ne 0x10b -and $optionalHeaderMagic -ne 0x20b) {
    Write-Error "picoo-desktop.exe has an unsupported optional header magic: $optionalHeaderMagic"
}
$subsystem = [BitConverter]::ToUInt16($exeBytes, $optionalHeader + 68)
if ($subsystem -ne 2) {
    Write-Error "picoo-desktop.exe must use the Windows GUI subsystem (2), got $subsystem"
}
Write-Host "ok: picoo-desktop.exe uses Windows GUI subsystem"

if (-not (Test-Path $Msi)) {
    Write-Error "Missing MSI: $Msi (set PICOO_REQUIRE_MSI=1)"
}
Write-Host "ok: PicooCamera.msi ($((Get-Item $Msi).Length) bytes)"

# `wix build` defaults to x86 even when the authoring references
# ProgramFiles64Folder. Read PID_TEMPLATE (7) from the built MSI so CI proves
# that Windows Installer will use the 64-bit component/registry view.
$windowsInstaller = New-Object -ComObject WindowsInstaller.Installer
$summaryInfo = $windowsInstaller.GetType().InvokeMember(
    "SummaryInformation",
    "GetProperty",
    $null,
    $windowsInstaller,
    @($Msi, 0)
)
$templateSummary = $summaryInfo.GetType().InvokeMember(
    "Property",
    "GetProperty",
    $null,
    $summaryInfo,
    @(7)
)
[void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($summaryInfo)
[void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($windowsInstaller)
if (-not $templateSummary.StartsWith("x64;", [StringComparison]::OrdinalIgnoreCase)) {
    Write-Error "PicooCamera.msi must be an x64 package; Template Summary is '$templateSummary'"
}
Write-Host "ok: PicooCamera.msi Template Summary is $templateSummary"

# Post-build MSI smoke (REQ-PICOO-VCAM-004): COM registration is declarative WiX data.
# The Rust cdylib does not expose or require self-registration through regsvr32.
# Limitation: CI cannot run msiexec /i (perMachine admin + Win11 GUI); install acceptance
# remains manual — see docs/design-specs/verification/vcam-meeting-apps.md.
$msiBytes = [System.IO.File]::ReadAllBytes($Msi)
$msiAscii = [System.Text.Encoding]::ASCII.GetString($msiBytes)
$msiUnicode = [System.Text.Encoding]::Unicode.GetString($msiBytes)
$forbidden = @('RegisterVcamDll', 'RegisterVcamComDll', 'regsvr32.exe', 'DllRegisterServer')
foreach ($needle in $forbidden) {
    if ($msiAscii.Contains($needle) -or $msiUnicode.Contains($needle)) {
        Write-Error "MSI embeds forbidden self-registration pattern '$needle'; keep COM registration declarative"
    }
    Write-Host "ok: MSI lacks '$needle'"
}
$required = @('--register-vcam --no-wait', 'WixQuietExec', 'RegisterVcamOnInstall')
foreach ($needle in $required) {
    if (-not ($msiAscii.Contains($needle) -or $msiUnicode.Contains($needle))) {
        Write-Error "MSI missing required install hook '$needle'"
    }
    Write-Host "ok: MSI embeds '$needle'"
}
$clsid = 'A7C4E2F1-8B3D-4C6A-9E5F-1D2C3B4A5E6F'
if (-not ($msiAscii.Contains($clsid) -or $msiUnicode.Contains($clsid))) {
    Write-Error "MSI missing CLSID registry scaffold ($clsid)"
}
Write-Host "ok: MSI embeds CLSID $clsid"

# UTF-16LE "Picoo Camera" must appear in the VCam DLL (FRIENDLY_NAME).
$needle = [System.Text.Encoding]::Unicode.GetBytes("Picoo Camera")
$bytes = [System.IO.File]::ReadAllBytes($Dll)
$found = $false
for ($i = 0; $i -le $bytes.Length - $needle.Length; $i++) {
    $match = $true
    for ($j = 0; $j -lt $needle.Length; $j++) {
        if ($bytes[$i + $j] -ne $needle[$j]) { $match = $false; break }
    }
    if ($match) { $found = $true; break }
}
if (-not $found) {
    Write-Error "PicooVirtualCameraSource.dll does not embed UTF-16 'Picoo Camera' friendly name"
}
Write-Host "ok: DLL embeds UTF-16 friendly name 'Picoo Camera'"
Write-Host "Bundle smoke verification passed (meeting-app enum still requires Win11)."
