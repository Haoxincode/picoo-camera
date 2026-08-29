# Windows Virtual Camera Extension

Independent IMFMediaSource DLL (`PicooVirtualCameraSource.dll`) consuming Shared Frame Ring.

## Components

| Artifact | Status | Description |
| --- | --- | --- |
| `picoo-vcam-ring-reader` | implemented | Polls Shared Frame Ring; validates VCam consumer path on Linux CI |
| `picoo-windows-vcam-source` | implemented | Rust `cdylib`：IMFActivate + IMFMediaSourceEx + NV12 stream from Shared Frame Ring |
| `register-vcam.ps1` | implemented | 调用桌面 CLI 修复声明式 COM 注册并执行 MFCreateVirtualCamera |

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

`picoo-windows-vcam-source` 由 Cargo 在 `windows-latest` 构建为
`picoo_virtual_camera_source.dll`，打包时重命名为 `PicooVirtualCameraSource.dll`。
COM/MF 接口通过 `windows-rs` 实现；仓库不维护 CMake、VCXPROJ 或等价 C++ Source。
详见 [ci-and-build.md](../../docs/development/ci-and-build.md)。

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

### DLL exports

- `DllGetClassObject`：创建 Media Foundation Source 的 COM class factory
- `DllCanUnloadNow`：在无活动 COM 对象与 server lock 时允许卸载

DLL 不提供自注册入口。MSI 使用 WiX 声明式写入 COM 注册表；开发态由桌面 CLI
以同一组键完成修复，然后调用 `MFCreateVirtualCamera`。
