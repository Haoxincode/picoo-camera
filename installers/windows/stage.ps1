# Stage Windows release artifacts into target/release/bundle — REQ-PICOO-STACK-004
$ErrorActionPreference = "Stop"

$Root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$Release = Join-Path $Root "target/release"
$Bundle = Join-Path $Release "bundle"

New-Item -ItemType Directory -Force -Path $Bundle | Out-Null

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

$VcamDll = Join-Path $Root "target/vcam-build/bin/PicooVirtualCameraSource.dll"
if (Test-Path $VcamDll) {
    Copy-Item -Force $VcamDll (Join-Path $Bundle "PicooVirtualCameraSource.dll")
    Write-Host "Staged PicooVirtualCameraSource.dll"
} else {
    Write-Warning "PicooVirtualCameraSource.dll not found — mf-source CMake build skipped or failed"
}

Write-Host "Bundle ready: $Bundle"
