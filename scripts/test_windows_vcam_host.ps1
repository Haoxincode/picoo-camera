# Installed Windows 11 VCam host contract — REQ-PICOO-VCAM-001/004/009/012.
# This script is destructive only to the exact MSI passed by the caller and is
# intended for a dedicated, clean, administrator self-hosted runner.
[CmdletBinding()]
param(
    [string]$MsiPath = "target/release/bundle/msi/PicooCamera.msi",
    [string]$BundleExePath = "target/release/bundle/picoo-desktop.exe",
    [string]$LogDirectory = "target/vcam-host-logs"
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$Root = Split-Path -Parent $PSScriptRoot
function Resolve-RepoPath([string]$Path) {
    if ([System.IO.Path]::IsPathRooted($Path)) {
        return [System.IO.Path]::GetFullPath($Path)
    }
    return [System.IO.Path]::GetFullPath((Join-Path $Root $Path))
}

$Msi = Resolve-RepoPath $MsiPath
$BundleExe = Resolve-RepoPath $BundleExePath
$Logs = Resolve-RepoPath $LogDirectory
$InstallDir = Join-Path $env:ProgramFiles "Picoo Camera"
$InstalledExe = Join-Path $InstallDir "picoo-desktop.exe"
$InstalledDll = Join-Path $InstallDir "PicooVirtualCameraSource.dll"
$ComKey = "Registry::HKEY_LOCAL_MACHINE\SOFTWARE\Classes\CLSID\{A7C4E2F1-8B3D-4C6A-9E5F-1D2C3B4A5E6F}\InprocServer32"
$ProductKey = "Registry::HKEY_LOCAL_MACHINE\SOFTWARE\Picoo\PicooCamera"
$IdentityName = "vcam_symbolic_link"

function Assert-SuccessExit([int]$ExitCode, [string]$Operation, [int[]]$Allowed = @(0, 3010)) {
    if ($Allowed -notcontains $ExitCode) {
        throw "$Operation failed with exit code $ExitCode"
    }
}

function Invoke-Msi([string[]]$Arguments, [string]$Operation) {
    $process = Start-Process -FilePath "msiexec.exe" -ArgumentList $Arguments -Wait -PassThru
    Assert-SuccessExit $process.ExitCode $Operation
}

function Invoke-Picoo([string]$Executable, [string[]]$Arguments, [string]$Operation) {
    $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = $Executable
    $startInfo.UseShellExecute = $false
    foreach ($argument in $Arguments) {
        [void]$startInfo.ArgumentList.Add($argument)
    }
    $process = [System.Diagnostics.Process]::Start($startInfo)
    if ($null -eq $process) {
        throw "$Operation did not create a process"
    }
    $process.WaitForExit()
    Assert-SuccessExit $process.ExitCode $Operation @(0)
}

function Read-DefaultRegistryValue([string]$Path) {
    return (Get-Item -LiteralPath $Path).GetValue("")
}

if (-not (Test-Path -LiteralPath $Msi -PathType Leaf)) {
    throw "MSI does not exist: $Msi"
}
if (-not (Test-Path -LiteralPath $BundleExe -PathType Leaf)) {
    throw "Bundle verifier does not exist: $BundleExe"
}

$windows = Get-CimInstance Win32_OperatingSystem
if ([int]$windows.ProductType -ne 1 -or [int]$windows.BuildNumber -lt 22000) {
    throw "VCam host contract requires Windows 11 client; found $($windows.Caption) build $($windows.BuildNumber)"
}
$principal = [Security.Principal.WindowsPrincipal]::new([Security.Principal.WindowsIdentity]::GetCurrent())
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw "VCam host contract requires an elevated administrator runner"
}
if ([System.Diagnostics.Process]::GetCurrentProcess().SessionId -eq 0) {
    throw "VCam host contract requires an interactive Windows session; a Session 0 runner cannot prove user-camera publication"
}
$null = Get-Service -Name "FrameServer" -ErrorAction Stop

New-Item -ItemType Directory -Path $Logs -Force | Out-Null
$InstallLog = Join-Path $Logs "install.log"
$RepairLog = Join-Path $Logs "repair.log"
$UninstallLog = Join-Path $Logs "uninstall.log"
$CleanupLog = Join-Path $Logs "cleanup.log"
$EvidencePath = Join-Path $Logs "contract-evidence.json"

# A dedicated runner must begin clean. Failing here avoids mutating an install
# that was not created by this workflow invocation.
if ((Test-Path -LiteralPath $InstallDir) -or
    (Test-Path -LiteralPath $ComKey) -or
    (Test-Path -LiteralPath $ProductKey)) {
    throw "Dedicated VCam runner is not clean; remove the existing Picoo Camera installation manually"
}

$InstallAttempted = $false
$Uninstalled = $false
$SavedIdentity = $null
try {
    $InstallAttempted = $true
    Invoke-Msi @("/i", "`"$Msi`"", "/qn", "/norestart", "/l*v", "`"$InstallLog`"") "MSI install"

    foreach ($path in @($InstalledExe, $InstalledDll)) {
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
            throw "Installed file is missing: $path"
        }
    }
    $registeredDll = [string](Read-DefaultRegistryValue $ComKey)
    if (-not [string]::Equals(
        [System.IO.Path]::GetFullPath($registeredDll),
        [System.IO.Path]::GetFullPath($InstalledDll),
        [StringComparison]::OrdinalIgnoreCase
    )) {
        throw "COM registration points to '$registeredDll' instead of '$InstalledDll'"
    }
    $SavedIdentity = [string](Get-ItemPropertyValue -LiteralPath $ProductKey -Name $IdentityName)
    if ([string]::IsNullOrWhiteSpace($SavedIdentity)) {
        throw "Installed VCam identity is missing"
    }
    Invoke-Picoo $InstalledExe @("--verify-vcam-host") "installed host activation"

    Invoke-Msi @("/fa", "`"$Msi`"", "/qn", "/norestart", "/l*v", "`"$RepairLog`"") "same-version MSI repair"
    Invoke-Picoo $InstalledExe @("--verify-vcam-host") "host activation after repair"
    $RepairedIdentity = [string](Get-ItemPropertyValue -LiteralPath $ProductKey -Name $IdentityName)
    if (-not [string]::Equals($SavedIdentity, $RepairedIdentity, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Same-version repair replaced the committed VCam identity"
    }

    Invoke-Picoo $InstalledExe @("--unregister-vcam") "first unregister"
    Invoke-Picoo $InstalledExe @("--unregister-vcam") "idempotent second unregister"
    Invoke-Picoo $BundleExe @("--verify-vcam-absent", $SavedIdentity) "device absence after unregister"

    Invoke-Picoo $InstalledExe @("--register-vcam", "--no-wait") "system re-registration"
    Invoke-Picoo $InstalledExe @("--verify-vcam-host") "host activation after re-registration"
    $FinalIdentity = [string](Get-ItemPropertyValue -LiteralPath $ProductKey -Name $IdentityName)

    Invoke-Msi @("/x", "`"$Msi`"", "/qn", "/norestart", "/l*v", "`"$UninstallLog`"") "MSI uninstall"
    $Uninstalled = $true
    Invoke-Picoo $BundleExe @("--verify-vcam-absent", $FinalIdentity) "device absence after uninstall"

    foreach ($path in @($InstalledExe, $InstalledDll, $ComKey, $ProductKey)) {
        if (Test-Path -LiteralPath $path) {
            throw "Uninstall left Picoo-owned state behind: $path"
        }
    }

    [ordered]@{
        verified_at_utc = [DateTime]::UtcNow.ToString("o")
        windows = $windows.Caption
        windows_build = $windows.BuildNumber
        msi_sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $Msi).Hash
        installed_dll_sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $Root "target/release/bundle/PicooVirtualCameraSource.dll")).Hash
        initial_symbolic_link = $SavedIdentity
        final_symbolic_link = $FinalIdentity
        contracts = @(
            "install", "exact-com-path", "mf-enumerate", "activate", "start-stop-shutdown",
            "same-version-repair", "idempotent-unregister", "re-register", "uninstall-cleanup"
        )
    } | ConvertTo-Json -Depth 3 | Set-Content -LiteralPath $EvidencePath -Encoding UTF8
    Write-Host "Installed Windows VCam host contract passed. Evidence: $EvidencePath"
} finally {
    if ($InstallAttempted -and -not $Uninstalled) {
        try {
            $cleanup = Start-Process -FilePath "msiexec.exe" -ArgumentList @(
                "/x", "`"$Msi`"", "/qn", "/norestart", "/l*v", "`"$CleanupLog`""
            ) -Wait -PassThru
            if (@(0, 1605, 3010) -notcontains $cleanup.ExitCode) {
                Write-Warning "Scoped MSI cleanup failed with exit code $($cleanup.ExitCode)"
            }
        } catch {
            Write-Warning "Scoped MSI cleanup failed: $_"
        }
    }
}
