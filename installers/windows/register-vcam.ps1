# Register Picoo Camera virtual camera (COM + MFCreateVirtualCamera) - REQ-PICOO-VCAM-004
param(
    [switch]$Unregister
)

$ErrorActionPreference = "Stop"

# Must match the Rust VCam Source / vcam_register.rs (REQ-PICOO-VCAM-002/004).
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
    (Join-Path $Root "target/release/PicooVirtualCameraSource.dll"),
    (Join-Path $Root "target/release/picoo_virtual_camera_source.dll")
)

$Desktop = Find-Artifact "picoo-desktop.exe" @(
    $Desktop,
    (Join-Path $Root "target/release/picoo-desktop.exe")
)

if (-not (Test-Path $Dll)) {
    Write-Error "Missing VCam DLL: $Dll (run cargo xtask build windows first)"
}

if ($Unregister) {
    if (-not (Test-Path $Desktop)) {
        Write-Error "picoo-desktop.exe not found; virtual-camera cleanup cannot continue."
    }
    Write-Host "Removing MF virtual camera and declarative COM registration via picoo-desktop"
    & $Desktop --unregister-vcam
    if ($LASTEXITCODE -ne 0) {
        Write-Error "picoo-desktop --unregister-vcam failed with exit code $LASTEXITCODE"
    }
    Write-Host "Picoo Camera virtual camera unregistered."
    exit 0
}

if (-not (Test-Path $Desktop)) {
    Write-Error "Missing picoo-desktop.exe (needed for MFCreateVirtualCamera). Build the Windows bundle first."
}

Write-Host "Starting MF virtual camera via picoo-desktop --register-vcam --no-wait"
Write-Host "The desktop command repairs COM registry values directly when needed; run as Administrator outside MSI."
& $Desktop --register-vcam --no-wait
if ($LASTEXITCODE -ne 0) {
    Write-Error "picoo-desktop --register-vcam failed with exit code $LASTEXITCODE"
}
Write-Host "Picoo Camera virtual camera registered (COM + MF)."
