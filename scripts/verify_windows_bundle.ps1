# Verify staged Windows bundle embeds Picoo Camera identity (REQ-PICOO-VCAM-001).
# Runs on windows-latest after `xtask package windows`. Does NOT replace Win11 meeting-app acceptance (VCAM-005).
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

if (-not (Test-Path $Msi)) {
    Write-Error "Missing MSI: $Msi (set PICOO_REQUIRE_MSI=1)"
}
Write-Host "ok: PicooCamera.msi ($((Get-Item $Msi).Length) bytes)"

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
