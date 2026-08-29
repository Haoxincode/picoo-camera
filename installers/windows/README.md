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

`picoo-camera.wxs` installs to `Program Files\Picoo Camera`, writes the COM CLSID and
`InprocServer32` values declaratively, then runs
`picoo-desktop --register-vcam --no-wait` for system-lifetime MF registration.

At runtime, `picoo-desktop` checks that the COM registration points at the adjacent DLL and
repairs the same registry values directly when needed. Repairing HKLM requires Administrator.

The development bundle uses `register-vcam.ps1`, which delegates both repair and MF
registration to the desktop CLI. The Rust DLL intentionally has no self-registration export.

If MSI install fails for other reasons, capture a verbose log:

```powershell
msiexec /i target/release/bundle/msi/PicooCamera.msi /l*v "$env:TEMP\picoo-camera-install.log"
```
