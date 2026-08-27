# Windows staging bundle — REQ-PICOO-VCAM-004 scaffold

Creates `target/release/bundle/` with desktop exe, ring-reader, VCam DLL, and registration script.

```powershell
cargo xtask package windows
powershell -ExecutionPolicy Bypass -File target/release/bundle/register-vcam.ps1
picoo-desktop --register-vcam
```

MSI/WiX installer is not yet implemented; `register-vcam.ps1` performs COM registration via `regsvr32`.
