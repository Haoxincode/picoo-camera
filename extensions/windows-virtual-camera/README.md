# Windows Virtual Camera Extension

Independent IMFMediaSource DLL (`PicooVirtualCameraSource.dll`) consuming Shared Frame Ring.

## Components

| Artifact | Status | Description |
| --- | --- | --- |
| `picoo-vcam-ring-reader` | implemented | Polls Shared Frame Ring; validates VCam consumer path on Linux CI |
| `picoo-windows-vcam-source` | implemented | Rust `cdylib`：IMFActivate + IMFMediaSourceEx + NV12 stream from Shared Frame Ring |
| `PicooCamera.msi` | implemented | perMachine 安装、声明式 COM 注册与 MFCreateVirtualCamera 维护命令 |

## Requirement mapping

- REQ-PICOO-VCAM-001..005, REQ-PICOO-VCAM-008..012
- REQ-PICOO-FRAME-003, REQ-PICOO-FRAME-004, REQ-PICOO-FRAME-007

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
Frame Provider 每 250 ms 检查 Shared Frame Ring 的 Producer 代际；Receiver 重启或重建
损坏映射后会自动重新附着，并允许新代际从 sequence 1 重新开始。
Source 每秒通过 Windows debug output 输出 `requests_per_sec`、fresh/cached/placeholder/failed
样本数与 delivery 平均/最大耗时。该指标用于识别 Frame Server 异常 request pump；当前
不主动 sleep 或改变 pacing，是否限速以 Win11 会议软件真机记录为依据。
Frame Server 选择 480p/720p/1080p 后，该运行周期的输出类型保持稳定；Shared Frame Ring
画面在 Source 内等比缩放并以黑边补齐，placeholder/live 切换不会触发动态格式重协商。
详见 [ci-and-build.md](../../docs/development/ci-and-build.md)。

### Windows registration (Win11)

```powershell
cargo xtask build windows
cargo xtask package windows
msiexec /i target/release/bundle/msi/PicooCamera.msi
```

松散 `windows-bundle` 仅用于编译、导出与加载 smoke，不能从用户可写目录完成系统注册。安装后如需修复，请使用桌面“虚拟摄像头”页的“安装或修复…”入口。

COM CLSID: `{A7C4E2F1-8B3D-4C6A-9E5F-1D2C3B4A5E6F}` — friendly name **Picoo Camera**.

专用 Win11 管理员 Runner 使用以下 Host Contract 验证已安装设备，而不是把
`windows-latest` 的 DLL 进程内测试当作系统发布证明：

```powershell
./scripts/test_windows_vcam_host.ps1
```

Harness 会从安装目录调用 `picoo-desktop --verify-vcam-host`，按注册时持久化的 exact
symbolic link 执行 `MFEnumDeviceSources → ActivateObject → Start → Stop → Shutdown`，并覆盖
同版 repair、幂等 unregister、重新注册和卸载后设备消失。它要求运行前不存在 Picoo 安装，
且只清理由本次传入 MSI 创建的产品；会议软件兼容性仍按独立真机清单验收。

### DLL exports

- `DllGetClassObject`：创建 Media Foundation Source 的 COM class factory
- `DllCanUnloadNow`：在无活动 COM 对象与 server lock 时允许卸载

DLL 不提供自注册入口。MSI 使用 WiX 声明式写入 COM 注册表；安装目录中的桌面维护命令可经 UAC 修复同一组键，然后调用 `MFCreateVirtualCamera`。
