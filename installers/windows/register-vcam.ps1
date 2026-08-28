# Register Picoo Camera virtual camera (COM + MFCreateVirtualCamera) - REQ-PICOO-VCAM-004
param(
    [switch]$Unregister,
    [switch]$AllowComOnly
)

$ErrorActionPreference = "Stop"

# Must match picoo_vcam_ids.h / vcam_register.rs (REQ-PICOO-VCAM-002/004).
$VcamClsid = "A7C4E2F1-8B3D-4C6A-9E5F-1D2C3B4A5E6F"
Write-Host "Picoo Camera VCam CLSID: {$VcamClsid}"

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
    if (-not (Test-Path $Desktop)) {
        Write-Error "picoo-desktop.exe not found; COM unregistered but MFCreateVirtualCamera cleanup skipped."
    }
    Write-Host "Removing MF virtual camera registration via picoo-desktop --unregister-vcam"
    & $Desktop --unregister-vcam
    if ($LASTEXITCODE -ne 0) {
        Write-Error "picoo-desktop --unregister-vcam failed with exit code $LASTEXITCODE"
    }
    Write-Host "Picoo Camera virtual camera unregistered."
    exit 0
}

Write-Host "Registering COM server: $Dll"
& regsvr32 /s $Dll
if ($LASTEXITCODE -ne 0) {
    Write-Error "regsvr32 failed with exit code $LASTEXITCODE for $Dll"
}

if (-not (Test-Path $Desktop)) {
    if ($AllowComOnly) {
        Write-Warning "picoo-desktop.exe not found; COM registered but MFCreateVirtualCamera not invoked (-AllowComOnly)."
        Write-Host "Run picoo-desktop --register-vcam after building the desktop app."
        exit 0
    }
    Write-Error "Missing picoo-desktop.exe (needed for MFCreateVirtualCamera). Build desktop or pass -AllowComOnly."
}

Write-Host "Starting MF virtual camera via picoo-desktop --register-vcam"
Write-Host "Note: on Windows 11, Frame Server must be able to read the DLL path (prefer Program Files install)."
& $Desktop --register-vcam
if ($LASTEXITCODE -ne 0) {
    Write-Error "picoo-desktop --register-vcam failed with exit code $LASTEXITCODE"
}
Write-Host "Picoo Camera virtual camera registered (COM + MF)."
