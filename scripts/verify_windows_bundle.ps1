# Verify staged Windows bundle embeds Picoo Camera identity (REQ-PICOO-VCAM-001).
# Runs on windows-latest after `xtask package windows`. Does NOT install MSI or run regsvr32;
# Win11 perMachine install acceptance remains manual (see vcam-meeting-apps.md).
$ErrorActionPreference = "Stop"

# scripts/ → repo root (one level up). Do NOT double-parent like installers/windows/*.ps1.
$Root = Split-Path -Parent $PSScriptRoot
$Bundle = Join-Path $Root "target/release/bundle"
$Dll = Join-Path $Bundle "PicooVirtualCameraSource.dll"
$Exe = Join-Path $Bundle "picoo-desktop.exe"
$RingReader = Join-Path $Bundle "picoo-vcam-ring-reader.exe"
$ProductIcon = Join-Path $Bundle "PicooCamera.ico"
$Msi = Join-Path $Bundle "msi/PicooCamera.msi"
$MsiVersionFile = Join-Path $Bundle "msi/PicooCamera.version"

Write-Host "Repo root: $Root"
Write-Host "Bundle:    $Bundle"

foreach ($path in @($Exe, $Dll, $RingReader, $ProductIcon)) {
    if (-not (Test-Path $path)) {
        Write-Error "Missing required bundle file: $path"
    }
    Write-Host "ok: $(Split-Path -Leaf $path) ($((Get-Item $path).Length) bytes)"
}

# REQ-PICOO-UI-002: Explorer/startup launches must not create a console window.
# The PE Optional Header subsystem value 2 is IMAGE_SUBSYSTEM_WINDOWS_GUI.
$exeBytes = [System.IO.File]::ReadAllBytes($Exe)
if ($exeBytes.Length -lt 64 -or $exeBytes[0] -ne 0x4d -or $exeBytes[1] -ne 0x5a) {
    Write-Error "picoo-desktop.exe has an invalid DOS header"
}
$peOffset = [BitConverter]::ToInt32($exeBytes, 0x3c)
if ($peOffset -lt 0 -or $peOffset + 24 -gt $exeBytes.Length) {
    Write-Error "picoo-desktop.exe has an invalid PE header offset"
}
if ($exeBytes[$peOffset] -ne 0x50 -or $exeBytes[$peOffset + 1] -ne 0x45 -or
    $exeBytes[$peOffset + 2] -ne 0 -or $exeBytes[$peOffset + 3] -ne 0) {
    Write-Error "picoo-desktop.exe has an invalid PE signature"
}
$optionalHeader = $peOffset + 24
$optionalHeaderSize = [BitConverter]::ToUInt16($exeBytes, $peOffset + 20)
if ($optionalHeaderSize -lt 70 -or $optionalHeader + $optionalHeaderSize -gt $exeBytes.Length) {
    Write-Error "picoo-desktop.exe has an invalid optional header size"
}
$optionalHeaderMagic = [BitConverter]::ToUInt16($exeBytes, $optionalHeader)
if ($optionalHeaderMagic -ne 0x10b -and $optionalHeaderMagic -ne 0x20b) {
    Write-Error "picoo-desktop.exe has an unsupported optional header magic: $optionalHeaderMagic"
}
$subsystem = [BitConverter]::ToUInt16($exeBytes, $optionalHeader + 68)
if ($subsystem -ne 2) {
    Write-Error "picoo-desktop.exe must use the Windows GUI subsystem (2), got $subsystem"
}
Write-Host "ok: picoo-desktop.exe uses Windows GUI subsystem"

# REQ-PICOO-UI-013: the PE must expose the application icon used by Explorer,
# Start, taskbar, Alt-Tab, and the non-advertised Start Menu shortcut.
Add-Type -AssemblyName System.Drawing
$associatedIcon = [System.Drawing.Icon]::ExtractAssociatedIcon($Exe)
if ($null -eq $associatedIcon -or $associatedIcon.Width -lt 16 -or $associatedIcon.Height -lt 16) {
    Write-Error "picoo-desktop.exe does not expose an embedded application icon"
}
$associatedIcon.Dispose()
Write-Host "ok: picoo-desktop.exe exposes an embedded application icon"

foreach ($path in @($Msi, $MsiVersionFile)) {
    if (-not (Test-Path $path)) {
        Write-Error "Missing MSI output: $path (set PICOO_REQUIRE_MSI=1)"
    }
}
$ExpectedMsiVersion = (Get-Content -Raw $MsiVersionFile).Trim()
Write-Host "ok: PicooCamera.msi ($((Get-Item $Msi).Length) bytes)"

# `wix build` defaults to x86 even when the authoring references
# ProgramFiles64Folder. Read PID_TEMPLATE (7) from the built MSI so CI proves
# that Windows Installer will use the 64-bit component/registry view.
$windowsInstaller = New-Object -ComObject WindowsInstaller.Installer
$database = $null
$summaryInfo = $null
$productVersionView = $null
$productVersionRecord = $null
$fileVersionView = $null
$fileVersionRecord = $null
$sequenceView = $null
$sequenceRecord = $null
$customActionView = $null
$customActionRecord = $null
$FileVersions = @{}
$ExecuteSequence = @{}
$ExecuteConditions = @{}
$CustomActionTypes = @{}
try {
    # Open the package read-only, then obtain its SummaryInformation stream from
    # the Database object. Database.SummaryInformation requires maxProperties=0
    # for read-only access; passing the package path to this property is a COM
    # type mismatch on PowerShell 7.
    $database = $windowsInstaller.GetType().InvokeMember(
        "OpenDatabase",
        "InvokeMethod",
        $null,
        $windowsInstaller,
        @([string]$Msi, [int]0)
    )
    $summaryInfo = $database.GetType().InvokeMember(
        "SummaryInformation",
        "GetProperty",
        $null,
        $database,
        @([int]0)
    )
    $templateSummary = [string]$summaryInfo.GetType().InvokeMember(
        "Property",
        "GetProperty",
        $null,
        $summaryInfo,
        @([int]7)
    )
    $productVersionQuery = 'SELECT `Value` FROM `Property` WHERE `Property` = ''ProductVersion'''
    $productVersionView = $database.OpenView($productVersionQuery)
    $productVersionView.Execute()
    $productVersionRecord = $productVersionView.Fetch()
    if ($null -eq $productVersionRecord) {
        Write-Error "PicooCamera.msi does not contain ProductVersion"
    }
    $ProductVersion = [string]$productVersionRecord.StringData(1)

    $fileVersionView = $database.OpenView('SELECT `File`, `Version` FROM `File`')
    $fileVersionView.Execute()
    while ($null -ne ($fileVersionRecord = $fileVersionView.Fetch())) {
        $FileVersions[[string]$fileVersionRecord.StringData(1)] = [string]$fileVersionRecord.StringData(2)
        [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($fileVersionRecord)
        $fileVersionRecord = $null
    }

    $sequenceView = $database.OpenView('SELECT `Action`, `Condition`, `Sequence` FROM `InstallExecuteSequence`')
    $sequenceView.Execute()
    while ($null -ne ($sequenceRecord = $sequenceView.Fetch())) {
        $action = [string]$sequenceRecord.StringData(1)
        $ExecuteConditions[$action] = [string]$sequenceRecord.StringData(2)
        $ExecuteSequence[$action] = [int]$sequenceRecord.IntegerData(3)
        [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($sequenceRecord)
        $sequenceRecord = $null
    }

    $customActionView = $database.OpenView('SELECT `Action`, `Type` FROM `CustomAction`')
    $customActionView.Execute()
    while ($null -ne ($customActionRecord = $customActionView.Fetch())) {
        $CustomActionTypes[[string]$customActionRecord.StringData(1)] = [int]$customActionRecord.IntegerData(2)
        [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($customActionRecord)
        $customActionRecord = $null
    }
} finally {
    if ($null -ne $customActionRecord) {
        [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($customActionRecord)
    }
    if ($null -ne $customActionView) {
        [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($customActionView)
    }
    if ($null -ne $sequenceRecord) {
        [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($sequenceRecord)
    }
    if ($null -ne $sequenceView) {
        [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($sequenceView)
    }
    if ($null -ne $fileVersionRecord) {
        [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($fileVersionRecord)
    }
    if ($null -ne $fileVersionView) {
        [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($fileVersionView)
    }
    if ($null -ne $productVersionRecord) {
        [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($productVersionRecord)
    }
    if ($null -ne $productVersionView) {
        [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($productVersionView)
    }
    if ($null -ne $summaryInfo) {
        [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($summaryInfo)
    }
    if ($null -ne $database) {
        [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($database)
    }
    [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($windowsInstaller)
}
if (-not $templateSummary.StartsWith("x64;", [StringComparison]::OrdinalIgnoreCase)) {
    Write-Error "PicooCamera.msi must be an x64 package; Template Summary is '$templateSummary'"
}
Write-Host "ok: PicooCamera.msi Template Summary is $templateSummary"
if ($ProductVersion -ne $ExpectedMsiVersion) {
    Write-Error "MSI ProductVersion '$ProductVersion' does not match generated version '$ExpectedMsiVersion'"
}
Write-Host "ok: PicooCamera.msi ProductVersion is $ProductVersion"

# REQ-PICOO-VCAM-009: the late MajorUpgrade bridge only works if Windows
# Installer can replace all maintenance binaries before the old product runs.
$ExpectedFileVersion = "$ExpectedMsiVersion.0"
$expectedVersionedFiles = @{
    PicooDesktop = $Exe
    PicooVcamDll = $Dll
    PicooRingReader = $RingReader
}
foreach ($fileId in $expectedVersionedFiles.Keys) {
    if ($FileVersions[$fileId] -ne $ExpectedFileVersion) {
        Write-Error "MSI File version for '$fileId' is '$($FileVersions[$fileId])', expected '$ExpectedFileVersion'"
    }
    $peVersion = (Get-Item $expectedVersionedFiles[$fileId]).VersionInfo.FileVersion.Trim()
    if ($peVersion -ne $ExpectedFileVersion) {
        Write-Error "PE FileVersion for '$fileId' is '$peVersion', expected '$ExpectedFileVersion'"
    }
    Write-Host "ok: $fileId PE/MSI FileVersion is $ExpectedFileVersion"
}

foreach ($action in @('InstallFiles', 'InstallExecute', 'RemoveExistingProducts', 'RegisterVcamOnInstall', 'InstallFinalize', 'RollbackVcamRegistration', 'RestoreVcamOnUpgradeRollback', 'UnregisterVcamOnRemove', 'RemoveRegistryValues')) {
    if (-not $ExecuteSequence.ContainsKey($action)) {
        Write-Error "MSI InstallExecuteSequence is missing '$action'"
    }
}
if (-not ($ExecuteSequence.InstallFiles -lt $ExecuteSequence.RollbackVcamRegistration -and
          $ExecuteSequence.RollbackVcamRegistration -lt $ExecuteSequence.RestoreVcamOnUpgradeRollback -and
          $ExecuteSequence.RestoreVcamOnUpgradeRollback -lt $ExecuteSequence.RegisterVcamOnInstall -and
          $ExecuteSequence.RegisterVcamOnInstall -lt $ExecuteSequence.InstallExecute -and
          $ExecuteSequence.InstallExecute -lt $ExecuteSequence.RemoveExistingProducts -and
          $ExecuteSequence.RemoveExistingProducts -lt $ExecuteSequence.InstallFinalize)) {
    Write-Error "MajorUpgrade must queue rollback/commit actions before InstallExecute, then run InstallExecute < RemoveExistingProducts < InstallFinalize"
}
$lateUpgradeWindow = @(
    $ExecuteSequence.GetEnumerator() |
        Where-Object {
            $_.Value -gt $ExecuteSequence.InstallExecute -and
            $_.Value -lt $ExecuteSequence.InstallFinalize
        } |
        Sort-Object Value, Key |
        ForEach-Object Key
)
if ($lateUpgradeWindow.Count -ne 1 -or $lateUpgradeWindow[0] -ne 'RemoveExistingProducts') {
    Write-Error "Late MajorUpgrade window must contain only RemoveExistingProducts; found: $($lateUpgradeWindow -join ', ')"
}
if ($ExecuteConditions.UnregisterVcamOnRemove -ne 'REMOVE~="ALL" AND NOT UPGRADINGPRODUCTCODE') {
    Write-Error "UnregisterVcamOnRemove has unsafe condition '$($ExecuteConditions.UnregisterVcamOnRemove)'"
}
if ($ExecuteSequence.UnregisterVcamOnRemove -ge $ExecuteSequence.RemoveRegistryValues) {
    Write-Error "UnregisterVcamOnRemove must run before RemoveRegistryValues"
}
$registerType = $CustomActionTypes.RegisterVcamOnInstall
if (($registerType -band 0x200) -eq 0 -or ($registerType -band 0x400) -eq 0 -or
    ($registerType -band 0x800) -eq 0 -or ($registerType -band 0x100) -ne 0) {
    Write-Error "RegisterVcamOnInstall must be commit, in-script, and non-impersonated; CustomAction.Type=$registerType"
}
$unregisterType = $CustomActionTypes.UnregisterVcamOnRemove
if (($unregisterType -band 0x400) -eq 0 -or ($unregisterType -band 0x800) -eq 0 -or
    ($unregisterType -band 0x100) -ne 0 -or ($unregisterType -band 0x200) -ne 0) {
    Write-Error "UnregisterVcamOnRemove must be deferred and non-impersonated; CustomAction.Type=$unregisterType"
}
$restoreType = $CustomActionTypes.RestoreVcamOnUpgradeRollback
if (($restoreType -band 0x100) -eq 0 -or ($restoreType -band 0x400) -eq 0 -or
    ($restoreType -band 0x800) -eq 0 -or ($restoreType -band 0x200) -ne 0) {
    Write-Error "RestoreVcamOnUpgradeRollback must be rollback, in-script, and non-impersonated; CustomAction.Type=$restoreType"
}
$freshRollbackType = $CustomActionTypes.RollbackVcamRegistration
if (($freshRollbackType -band 0x100) -eq 0 -or ($freshRollbackType -band 0x400) -eq 0 -or
    ($freshRollbackType -band 0x800) -eq 0 -or ($freshRollbackType -band 0x200) -ne 0) {
    Write-Error "RollbackVcamRegistration must be rollback, in-script, and non-impersonated; CustomAction.Type=$freshRollbackType"
}
if ($ExecuteConditions.RestoreVcamOnUpgradeRollback -ne 'WIX_UPGRADE_DETECTED') {
    Write-Error "RestoreVcamOnUpgradeRollback has unsafe condition '$($ExecuteConditions.RestoreVcamOnUpgradeRollback)'"
}
if ($ExecuteConditions.RollbackVcamRegistration -ne 'NOT Installed AND NOT WIX_UPGRADE_DETECTED') {
    Write-Error "RollbackVcamRegistration has unsafe condition '$($ExecuteConditions.RollbackVcamRegistration)'"
}
if ($ExecuteConditions.RegisterVcamOnInstall -ne 'NOT REMOVE') {
    Write-Error "RegisterVcamOnInstall has unexpected condition '$($ExecuteConditions.RegisterVcamOnInstall)'"
}
Write-Host "ok: MSI late MajorUpgrade queues commit/rollback work before the restricted execution window"

# Post-build MSI smoke (REQ-PICOO-VCAM-004): COM registration is declarative WiX data.
# The Rust cdylib does not expose or require self-registration through regsvr32.
# Limitation: CI cannot run msiexec /i (perMachine admin + Win11 GUI); install acceptance
# remains manual — see docs/design-specs/verification/vcam-meeting-apps.md.
$msiBytes = [System.IO.File]::ReadAllBytes($Msi)
$msiAscii = [System.Text.Encoding]::ASCII.GetString($msiBytes)
$msiUnicode = [System.Text.Encoding]::Unicode.GetString($msiBytes)
$forbidden = @('RegisterVcamDll', 'RegisterVcamComDll', 'regsvr32.exe', 'DllRegisterServer')
foreach ($needle in $forbidden) {
    if ($msiAscii.Contains($needle) -or $msiUnicode.Contains($needle)) {
        Write-Error "MSI embeds forbidden self-registration pattern '$needle'; keep COM registration declarative"
    }
    Write-Host "ok: MSI lacks '$needle'"
}
$required = @(
    '--register-vcam --no-wait',
    'WixQuietExec',
    'RegisterVcamOnInstall',
    'RestoreVcamOnUpgradeRollback',
    'PicooProductIcon'
)
foreach ($needle in $required) {
    if (-not ($msiAscii.Contains($needle) -or $msiUnicode.Contains($needle))) {
        Write-Error "MSI missing required install hook '$needle'"
    }
    Write-Host "ok: MSI embeds '$needle'"
}
$clsid = 'A7C4E2F1-8B3D-4C6A-9E5F-1D2C3B4A5E6F'
if (-not ($msiAscii.Contains($clsid) -or $msiUnicode.Contains($clsid))) {
    Write-Error "MSI missing CLSID registry scaffold ($clsid)"
}
Write-Host "ok: MSI embeds CLSID $clsid"

# UTF-16LE "Picoo Camera" must appear in the VCam DLL (FRIENDLY_NAME).
$needle = [System.Text.Encoding]::Unicode.GetBytes("Picoo Camera")
$bytes = [System.IO.File]::ReadAllBytes($Dll)
$found = $false
for ($i = 0; $i -le $bytes.Length - $needle.Length; $i++) {
    $match = $true
    for ($j = 0; $j -lt $needle.Length; $j++) {
        if ($bytes[$i + $j] -ne $needle[$j]) { $match = $false; break }
    }
    if ($match) { $found = $true; break }
}
if (-not $found) {
    Write-Error "PicooVirtualCameraSource.dll does not embed UTF-16 'Picoo Camera' friendly name"
}
Write-Host "ok: DLL embeds UTF-16 friendly name 'Picoo Camera'"
Write-Host "Bundle smoke verification passed (meeting-app enum still requires Win11)."
