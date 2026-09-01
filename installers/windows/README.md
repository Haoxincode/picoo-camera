# Windows staging bundle — REQ-PICOO-VCAM-004

Creates `target/release/bundle/` with the desktop exe, ring-reader, VCam DLL, and MSI.
The loose bundle is for build/export/load smoke only; it is not a portable system installer.

```powershell
cargo xtask package windows
msiexec /i target/release/bundle/msi/PicooCamera.msi
```

## MSI

Requires [WiX Toolset v5](https://wixtoolset.org/) on PATH (using the v4 schema):

```powershell
dotnet tool install --global wix
powershell -ExecutionPolicy Bypass -File installers/windows/build-msi.ps1
```

`picoo-camera.wxs` installs to `Program Files\Picoo Camera`, writes the COM CLSID and
`InprocServer32` values declaratively, then commits
`picoo-desktop --register-vcam --no-wait` for system-lifetime MF registration. Late major
upgrades keep `InstallExecute -> RemoveExistingProducts -> InstallFinalize` free of other
in-script actions; the build runs ICE27/ICE63/ICE77 in addition to direct MSI table checks.

At runtime, ordinary `picoo-desktop` startup only reads the installed COM registration. The
explicit “安装或修复…” action invokes the installed maintenance command through Windows UAC.
System registration is accepted only from the per-machine `Program Files\Picoo Camera`
installation. The Rust DLL intentionally has no self-registration export.

If MSI install fails for other reasons, capture a verbose log:

```powershell
msiexec /i target/release/bundle/msi/PicooCamera.msi /l*v "$env:TEMP\picoo-camera-install.log"
```
