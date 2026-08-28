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

`picoo-camera.wxs` installs to `Program Files\Picoo Camera` and runs deferred `regsvr32` (via `SystemFolder` + `[#PicooVcamDll]`) for COM registration so Frame Server can load the DLL. MF virtual camera registration is done on first launch via `picoo-desktop --register-vcam`.

If MSI install fails at the custom action step, capture a verbose log:

```powershell
msiexec /i target/release/bundle/msi/PicooCamera.msi /l*v "$env:TEMP\picoo-camera-install.log"
```
