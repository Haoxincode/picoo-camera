# Sign and verify the Windows release bundle — REQ-PICOO-STACK-008.
[CmdletBinding()]
param(
    [string]$BundlePath = "target/release/bundle"
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$Root = Split-Path -Parent $PSScriptRoot
if (-not [System.IO.Path]::IsPathRooted($BundlePath)) {
    $BundlePath = Join-Path $Root $BundlePath
}
$Bundle = [System.IO.Path]::GetFullPath($BundlePath)
$Msi = Join-Path $Bundle "msi/PicooCamera.msi"
$IdentityReport = Join-Path $Bundle "windows-release-identity.json"

function Require-Environment([string]$Name) {
    $value = [Environment]::GetEnvironmentVariable($Name)
    if ([string]::IsNullOrWhiteSpace($value)) {
        throw "$Name is required for a signed Windows release"
    }
    return $value
}

function Normalize-Fingerprint([string]$Value) {
    return ($Value -replace '[:\s]', '').ToUpperInvariant()
}

function Find-SignTool {
    $command = Get-Command signtool.exe -ErrorAction SilentlyContinue
    if ($null -ne $command) {
        return $command.Source
    }
    $kits = Join-Path ${env:ProgramFiles(x86)} "Windows Kits\10\bin"
    $candidate = Get-ChildItem -Path $kits -Filter signtool.exe -Recurse -File -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -match '\\x64\\signtool\.exe$' } |
        Sort-Object { [Version]$_.Directory.Parent.Name } -Descending |
        Select-Object -First 1
    if ($null -eq $candidate) {
        throw "signtool.exe was not found in PATH or the Windows 10/11 SDK"
    }
    return $candidate.FullName
}

function Invoke-SignTool([string[]]$Arguments, [string]$Operation) {
    & $script:SignTool @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "$Operation failed with exit code $LASTEXITCODE"
    }
}

function Assert-SignedFile([string]$Path, [string]$ExpectedSha256) {
    Invoke-SignTool @("verify", "/pa", "/all", "/v", $Path) "Authenticode verification for $Path"
    $signature = Get-AuthenticodeSignature -LiteralPath $Path
    if ($signature.Status -ne [System.Management.Automation.SignatureStatus]::Valid) {
        throw "Authenticode status for $Path is $($signature.Status): $($signature.StatusMessage)"
    }
    if ($null -eq $signature.SignerCertificate) {
        throw "Authenticode signer certificate is missing for $Path"
    }
    $actualSha256 = Normalize-Fingerprint $signature.SignerCertificate.GetCertHashString(
        [Security.Cryptography.HashAlgorithmName]::SHA256
    )
    if ($actualSha256 -ne $ExpectedSha256) {
        throw "Authenticode signer for $Path does not match PICOO_WINDOWS_SIGNER_SHA256"
    }
    if ($null -eq $signature.TimeStamperCertificate) {
        throw "Authenticode signature for $Path has no trusted RFC3161 timestamp"
    }
}

$Thumbprint = Normalize-Fingerprint (Require-Environment "PICOO_WINDOWS_CERT_THUMBPRINT")
$ExpectedSha256 = Normalize-Fingerprint (Require-Environment "PICOO_WINDOWS_SIGNER_SHA256")
$TimestampUrl = Require-Environment "PICOO_WINDOWS_TIMESTAMP_URL"
if ($Thumbprint -notmatch '^[0-9A-F]{40}$') {
    throw "PICOO_WINDOWS_CERT_THUMBPRINT must be a SHA-1 certificate thumbprint"
}
if ($ExpectedSha256 -notmatch '^[0-9A-F]{64}$') {
    throw "PICOO_WINDOWS_SIGNER_SHA256 must be exactly 32 SHA-256 bytes"
}
$TimestampUri = [Uri]$TimestampUrl
if (-not $TimestampUri.IsAbsoluteUri -or $TimestampUri.Scheme -notin @("http", "https")) {
    throw "PICOO_WINDOWS_TIMESTAMP_URL must be an absolute HTTP(S) RFC3161 service URL"
}

$CertificatePath = "Cert:\CurrentUser\My\$Thumbprint"
$Certificate = Get-Item -LiteralPath $CertificatePath -ErrorAction Stop
if (-not $Certificate.HasPrivateKey) {
    throw "Windows release certificate does not expose its private key"
}
if ($Certificate.NotBefore.ToUniversalTime() -gt [DateTime]::UtcNow -or
    $Certificate.NotAfter.ToUniversalTime() -le [DateTime]::UtcNow) {
    throw "Windows release certificate is not currently valid"
}
$CodeSigningOid = "1.3.6.1.5.5.7.3.3"
$HasCodeSigningEku = $Certificate.Extensions |
    Where-Object { $_ -is [Security.Cryptography.X509Certificates.X509EnhancedKeyUsageExtension] } |
    ForEach-Object { $_.EnhancedKeyUsages } |
    Where-Object { $_.Value -eq $CodeSigningOid } |
    Select-Object -First 1
if ($null -eq $HasCodeSigningEku) {
    throw "Windows release certificate does not authorize Code Signing"
}
$CertificateSha256 = Normalize-Fingerprint $Certificate.GetCertHashString(
    [Security.Cryptography.HashAlgorithmName]::SHA256
)
if ($CertificateSha256 -ne $ExpectedSha256) {
    throw "Imported Windows certificate does not match PICOO_WINDOWS_SIGNER_SHA256"
}

$SignTool = Find-SignTool
$PeFiles = @(
    (Join-Path $Bundle "picoo-desktop.exe"),
    (Join-Path $Bundle "picoo-vcam-ring-reader.exe"),
    (Join-Path $Bundle "PicooVirtualCameraSource.dll")
)
foreach ($path in $PeFiles) {
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "Windows release input is missing: $path"
    }
    Invoke-SignTool @(
        "sign", "/sha1", $Thumbprint, "/s", "My", "/fd", "SHA256", "/td", "SHA256",
        "/tr", $TimestampUrl, "/d", "Picoo Camera", $path
    ) "Authenticode signing for $path"
    Assert-SignedFile $path $ExpectedSha256
}

# WiX must package the already signed PE files. Signing an MSI first and then
# replacing embedded payloads would invalidate its package signature.
& powershell -ExecutionPolicy Bypass -File (Join-Path $Root "installers/windows/build-msi.ps1")
if ($LASTEXITCODE -ne 0) {
    throw "build-msi.ps1 failed with exit code $LASTEXITCODE"
}
if (-not (Test-Path -LiteralPath $Msi -PathType Leaf)) {
    throw "Windows MSI was not produced: $Msi"
}
Invoke-SignTool @(
    "sign", "/sha1", $Thumbprint, "/s", "My", "/fd", "SHA256", "/td", "SHA256",
    "/tr", $TimestampUrl, "/d", "Picoo Camera", $Msi
) "Authenticode signing for $Msi"
Assert-SignedFile $Msi $ExpectedSha256

$Files = @($PeFiles + $Msi) | ForEach-Object {
    [ordered]@{
        name = Split-Path -Leaf $_
        sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $_).Hash
    }
}
[ordered]@{
    verified_at_utc = [DateTime]::UtcNow.ToString("o")
    subject = $Certificate.Subject
    certificate_sha256 = $ExpectedSha256
    certificate_not_after_utc = $Certificate.NotAfter.ToUniversalTime().ToString("o")
    timestamp_url = $TimestampUrl
    product_version = $env:PICOO_WINDOWS_MSI_VERSION
    files = $Files
} | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $IdentityReport -Encoding UTF8

Write-Host "Signed Windows release verified: $IdentityReport"
