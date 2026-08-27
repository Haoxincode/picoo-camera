# Build MSI from staged bundle (requires WiX v4 on PATH) — REQ-PICOO-VCAM-004
$ErrorActionPreference = "Stop"

$Root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$Bundle = Join-Path $Root "target/release/bundle"
$OutDir = Join-Path $Bundle "msi"
$Wxs = Join-Path $PSScriptRoot "picoo-camera.wxs"
$RequireMsi = $env:PICOO_REQUIRE_MSI -eq "1"

if (-not (Test-Path (Join-Path $Bundle "picoo-desktop.exe"))) {
    Write-Error "Missing staged bundle. Run: cargo xtask package windows"
}

if (-not (Get-Command wix -ErrorAction SilentlyContinue)) {
    if ($RequireMsi) {
        Write-Error "WiX Toolset (wix) not found on PATH (PICOO_REQUIRE_MSI=1)."
    }
    Write-Warning "WiX Toolset (wix) not found on PATH. Staging bundle only; MSI skipped."
    Write-Host "Install: dotnet tool install --global wix"
    Write-Host "Then:    wix extension add WixToolset.UI.wixext"
    Write-Host "         wix extension add WixToolset.Firewall.wixext"
    exit 0
}

# Ensure Firewall extension for fw:FirewallException (REQ-PICOO-VCAM-004).
& wix extension add WixToolset.Firewall.wixext 2>$null | Out-Null

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$Msi = Join-Path $OutDir "PicooCamera.msi"

Write-Host "Building MSI from $Wxs with bindpath Bundle=$Bundle"
& wix build $Wxs -ext WixToolset.Firewall.wixext -o $Msi -b Bundle=$Bundle
if ($LASTEXITCODE -ne 0) {
    Write-Error "wix build failed with exit code $LASTEXITCODE"
}

if (-not (Test-Path $Msi)) {
    Write-Error "MSI was not produced at $Msi"
}

Write-Host "MSI ready: $Msi"
