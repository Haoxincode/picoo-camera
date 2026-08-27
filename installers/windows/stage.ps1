# Stage Windows release artifacts into target/release/bundle - REQ-PICOO-STACK-004
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

$VcamCandidates = @(
    (Join-Path $Root "target/vcam-build/bin/Release/PicooVirtualCameraSource.dll"),
    (Join-Path $Root "target/vcam-build/bin/PicooVirtualCameraSource.dll")
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
    Write-Warning "PicooVirtualCameraSource.dll not found - mf-source CMake build skipped or failed"
}

$RegisterScript = Join-Path $Root "installers/windows/register-vcam.ps1"
if (Test-Path $RegisterScript) {
    Copy-Item -Force $RegisterScript (Join-Path $Bundle "register-vcam.ps1")
    Write-Host "Staged register-vcam.ps1"
}

$BuildMsi = Join-Path $Root "installers/windows/build-msi.ps1"
if (Test-Path $BuildMsi) {
    Write-Host "Attempting MSI build (optional; requires WiX)"
    & powershell -ExecutionPolicy Bypass -File $BuildMsi
}

Write-Host "Bundle ready: $Bundle"
