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

`picoo-camera.wxs` installs to `Program Files\Picoo Camera`, writes COM CLSID registry keys, then runs **`regsvr32 /s` on the installed DLL** (`Return=ignore`, SYSTEM context) followed by `picoo-desktop --register-vcam --no-wait` for system-lifetime MF registration. Declarative registry alone can leave `IMFVirtualCamera::Start` failing with **0x80040154 (REGDB_E_CLASSNOTREG)** on some Win11 builds; regsvr32 ensures `InprocServer32` matches the installed path.

At runtime, `picoo-desktop` also calls `regsvr32` automatically when COM is missing (requires Administrator if HKLM write is denied).

Development bundle still uses `register-vcam.ps1` (regsvr32 + `--register-vcam --no-wait`).

If MSI install fails for other reasons, capture a verbose log:

```powershell
msiexec /i target/release/bundle/msi/PicooCamera.msi /l*v "$env:TEMP\picoo-camera-install.log"
```
