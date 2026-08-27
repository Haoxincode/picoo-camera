# Build MSI from staged bundle (requires WiX v4 on PATH) — REQ-PICOO-VCAM-004
$ErrorActionPreference = "Stop"

$Root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$Bundle = Join-Path $Root "target/release/bundle"
$OutDir = Join-Path $Bundle "msi"
$Wxs = Join-Path $PSScriptRoot "picoo-camera.wxs"

if (-not (Test-Path (Join-Path $Bundle "picoo-desktop.exe"))) {
    Write-Error "Missing staged bundle. Run: cargo xtask package windows"
}

if (-not (Get-Command wix -ErrorAction SilentlyContinue)) {
    Write-Warning "WiX Toolset (wix) not found on PATH. Staging bundle only; MSI skipped."
    Write-Host "Install: dotnet tool install --global wix"
    Write-Host "Then:    wix extension add WixToolset.UI.wixext"
    exit 0
}

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$Msi = Join-Path $OutDir "PicooCamera.msi"

Write-Host "Building MSI from $Wxs with bindpath Bundle=$Bundle"
& wix build $Wxs -o $Msi -b Bundle=$Bundle
if ($LASTEXITCODE -ne 0) {
    Write-Error "wix build failed with exit code $LASTEXITCODE"
}

Write-Host "MSI ready: $Msi"
