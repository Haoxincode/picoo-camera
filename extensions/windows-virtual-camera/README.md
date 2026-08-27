# Windows Virtual Camera Extension

Independent IMFMediaSource DLL (`PicooVirtualCameraSource.dll`) consuming Shared Frame Ring.

## Components

| Artifact | Status | Description |
| --- | --- | --- |
| `picoo-vcam-ring-reader` | implemented | Polls Shared Frame Ring; validates VCam consumer path on Linux CI |
| `PicooVirtualCameraSource.dll` | ring-reader | CMake DLL + Shared Frame Ring C++ consumer; IMFMediaSource pending |

## Requirement 映射

- REQ-PICOO-VCAM-001..005
- REQ-PICOO-FRAME-003, REQ-PICOO-FRAME-004

## Local test (Linux / Windows)

With desktop receiver running (`picoo-desktop --serve` or GPUI):

```bash
cargo run -p picoo-vcam-ring-reader
```

Expect NV12 placeholder frames (`1280x720`) until live H.264 decode lands.

Built on `windows-latest` for MF DLL — see [ci-and-build.md](../../docs/development/ci-and-build.md).

### DLL exports (dev / VCam bootstrap)

- `PicooVcamSourceVersion()` — build label
- `PicooVcamAttachRing(name)` — open `%TEMP%\\picoo-frame-ring-{name}.link` mapping (same flink as Rust)
- `PicooVcamPollFrame(out)` — latest NV12 frame view for IMFMediaSource sample path
