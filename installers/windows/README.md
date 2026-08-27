# Windows staging bundle — REQ-PICOO-VCAM-004 scaffold

Creates `target/release/bundle/` with desktop exe, ring-reader, and VCam DLL when built.

```powershell
powershell -ExecutionPolicy Bypass -File installers/windows/stage.ps1
```

MSI/COM registration is not yet implemented; this script only stages release artifacts for CI upload.
