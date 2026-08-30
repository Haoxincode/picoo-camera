# Stage Windows release artifacts into target/release/bundle - REQ-PICOO-STACK-004
$ErrorActionPreference = "Stop"

$Root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$Release = Join-Path $Root "target/release"
$Bundle = Join-Path $Release "bundle"

New-Item -ItemType Directory -Force -Path $Bundle | Out-Null

$ObsoleteRegisterScript = Join-Path $Bundle "register-vcam.ps1"
if (Test-Path $ObsoleteRegisterScript) {
    Remove-Item -Force $ObsoleteRegisterScript
    Write-Host "Removed obsolete register-vcam.ps1 from bundle"
}

$Artifacts = @(
    "picoo-desktop.exe",
    "picoo-vcam-ring-reader.exe"
)

foreach ($name in $Artifacts) {
    $src = Join-Path $Release $name
    if (-not (Test-Path $src)) {
        Write-Error "Missing release artifact: $src (run cargo xtask build windows first)"
    }
    Copy-Item -Force $src (Join-Path $Bundle $name)
    Write-Host "Staged $name"
}

$ProductIcon = Join-Path $Root "assets/brand/windows/PicooCamera.ico"
if (-not (Test-Path $ProductIcon)) {
    Write-Error "Missing Windows product icon: $ProductIcon"
}
Copy-Item -Force $ProductIcon (Join-Path $Bundle "PicooCamera.ico")
Write-Host "Staged PicooCamera.ico"

$VcamCandidates = @(
    (Join-Path $Release "picoo_virtual_camera_source.dll"),
    (Join-Path $Release "PicooVirtualCameraSource.dll")
)

$VcamDll = $null
foreach ($candidate in $VcamCandidates) {
    if (Test-Path $candidate) {
        $VcamDll = $candidate
        break
    }
}

if ($null -ne $VcamDll) {
    Copy-Item -Force $VcamDll (Join-Path $Bundle "PicooVirtualCameraSource.dll")
    Write-Host "Staged PicooVirtualCameraSource.dll"
} else {
    if ($env:PICOO_REQUIRE_MSI -eq "1") {
        Write-Error "PicooVirtualCameraSource.dll not found (PICOO_REQUIRE_MSI=1)"
    }
    Write-Warning "PicooVirtualCameraSource.dll not found - Rust cdylib build skipped or failed"
}

$BuildMsi = Join-Path $Root "installers/windows/build-msi.ps1"
if (Test-Path $BuildMsi) {
    Write-Host "Attempting MSI build (optional; requires WiX)"
    & powershell -ExecutionPolicy Bypass -File $BuildMsi
    if ($LASTEXITCODE -ne 0) {
        if ($env:PICOO_REQUIRE_MSI -eq "1") {
            Write-Error "build-msi.ps1 failed with exit code $LASTEXITCODE (PICOO_REQUIRE_MSI=1)"
        }
        Write-Warning "build-msi.ps1 failed with exit code $LASTEXITCODE"
    }
}

Write-Host "Bundle ready: $Bundle"
