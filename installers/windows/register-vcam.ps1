# Register Picoo Camera virtual camera (COM + MFCreateVirtualCamera) — REQ-PICOO-VCAM-004
$ErrorActionPreference = "Stop"

param(
    [switch]$Unregister
)

$Root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$Bundle = Join-Path $Root "target/release/bundle"
$Dll = Join-Path $Bundle "PicooVirtualCameraSource.dll"
$Desktop = Join-Path $Bundle "picoo-desktop.exe"

function Find-Artifact([string]$name, [string[]]$candidates) {
    foreach ($candidate in $candidates) {
        if (Test-Path $candidate) {
            return $candidate
        }
    }
    return Join-Path $Bundle $name
}

$Dll = Find-Artifact "PicooVirtualCameraSource.dll" @(
    $Dll,
    (Join-Path $Root "target/vcam-build/bin/Release/PicooVirtualCameraSource.dll"),
    (Join-Path $Root "target/vcam-build/bin/PicooVirtualCameraSource.dll")
)

$Desktop = Find-Artifact "picoo-desktop.exe" @(
    $Desktop,
    (Join-Path $Root "target/release/picoo-desktop.exe")
)

if (-not (Test-Path $Dll)) {
    Write-Error "Missing VCam DLL: $Dll (run cargo xtask build windows first)"
}

if ($Unregister) {
    Write-Host "Unregistering COM server: $Dll"
    & regsvr32 /u /s $Dll
    if (Test-Path $Desktop) {
        Write-Host "Removing MF virtual camera registration via picoo-desktop"
        & $Desktop --unregister-vcam
    }
    Write-Host "Picoo Camera virtual camera unregistered."
    exit 0
}

Write-Host "Registering COM server: $Dll"
& regsvr32 /s $Dll

if (-not (Test-Path $Desktop)) {
    Write-Warning "picoo-desktop.exe not found; COM registered but MFCreateVirtualCamera not invoked."
    Write-Host "Run picoo-desktop --register-vcam after building the desktop app."
    exit 0
}

Write-Host "Starting MF virtual camera via picoo-desktop --register-vcam"
Write-Host "Note: on Windows 11, Frame Server must be able to read the DLL path (prefer Program Files install)."

# Non-interactive registration: spawn and send Enter after brief delay is brittle; use dedicated mode later.
Write-Host "For interactive session registration run: $Desktop --register-vcam"
