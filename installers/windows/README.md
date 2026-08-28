# Windows staging bundle — REQ-PICOO-VCAM-004

Creates `target/release/bundle/` with desktop exe, ring-reader, VCam DLL, and registration script.

```powershell
cargo xtask package windows
powershell -ExecutionPolicy Bypass -File target/release/bundle/register-vcam.ps1
picoo-desktop --register-vcam
```

## MSI (optional)

Requires [WiX Toolset v4](https://wixtoolset.org/) on PATH:

```powershell
dotnet tool install --global wix
powershell -ExecutionPolicy Bypass -File installers/windows/build-msi.ps1
```

`picoo-camera.wxs` installs to `Program Files\Picoo Camera`, writes COM CLSID registry keys (equivalent to `DllRegisterServer`), and runs `picoo-desktop --register-vcam --no-wait` after `InstallFiles` (system-lifetime MF registration via `WixQuietExec`). **No deferred regsvr32** — that pattern failed on clean Win11 with `Return=check`.

Development bundle still uses `register-vcam.ps1` (regsvr32 + `--register-vcam --no-wait`).

If MSI install fails for other reasons, capture a verbose log:

```powershell
msiexec /i target/release/bundle/msi/PicooCamera.msi /l*v "$env:TEMP\picoo-camera-install.log"
```
