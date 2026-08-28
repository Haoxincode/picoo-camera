# Windows Virtual Camera Extension

Independent IMFMediaSource DLL (`PicooVirtualCameraSource.dll`) consuming Shared Frame Ring.

## Components

| Artifact | Status | Description |
| --- | --- | --- |
| `picoo-vcam-ring-reader` | implemented | Polls Shared Frame Ring; validates VCam consumer path on Linux CI |
| `PicooVirtualCameraSource.dll` | imf-media-source | IMFActivate + IMFMediaSourceEx + NV12 stream from Shared Frame Ring |
| `register-vcam.ps1` | scaffold | `regsvr32` COM registration + MFCreateVirtualCamera bootstrap |

## Requirement mapping

- REQ-PICOO-VCAM-001..005
- REQ-PICOO-FRAME-003, REQ-PICOO-FRAME-004

## Local test (Linux / Windows)

With desktop receiver running (`picoo-desktop --serve` or GPUI):

```bash
cargo run -p picoo-vcam-ring-reader
```

Expect NV12 placeholder frames (`1280x720`) until a phone streams; after OpenH264/MF
decode, live frames appear at negotiated resolution (see
`paired_openh264_publishes_to_shared_frame_ring`).

Built on `windows-latest` for MF DLL — see [ci-and-build.md](../../docs/development/ci-and-build.md).

### Windows registration (Win11)

```powershell
cargo xtask build windows
cargo xtask package windows
powershell -ExecutionPolicy Bypass -File target/release/bundle/register-vcam.ps1
# Dev interactive (session lifetime, waits for Enter):
picoo-desktop --register-vcam
# MSI / headless (system lifetime):
picoo-desktop --register-vcam --no-wait
```

COM CLSID: `{A7C4E2F1-8B3D-4C6A-9E5F-1D2C3B4A5E6F}` — friendly name **Picoo Camera**.

### DLL exports (dev / diagnostics)

- `PicooVcamSourceVersion()` — build label
- `PicooVcamAttachRing(name)` — probe Shared Frame Ring mapping
- `PicooVcamPollFrame(out)` — latest NV12 frame view
- Standard COM: `DllGetClassObject`, `DllRegisterServer`, `DllUnregisterServer`
