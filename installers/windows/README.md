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

`picoo-camera.wxs` installs to `Program Files\Picoo Camera` and writes COM CLSID registry keys (equivalent to `DllRegisterServer`) so Frame Server can load the DLL. **No deferred regsvr32** — that pattern failed on clean Win11 with `Return=check`. MF virtual camera registration is done on first launch via `picoo-desktop --register-vcam` (or automatically when GPUI starts).

If MSI install fails for other reasons, capture a verbose log:

```powershell
msiexec /i target/release/bundle/msi/PicooCamera.msi /l*v "$env:TEMP\picoo-camera-install.log"
```
