# REQ-PICOO-VCAM：桌面虚拟摄像头

| ID | 状态 | 来源 | 描述 | 验收 |
| --- | --- | --- | --- | --- |
| REQ-PICOO-VCAM-001 | implemented | PUC-004 | 注册统一名称 `Picoo Camera` | `FRIENDLY_NAME` / `PICOO_VCAM_FRIENDLY_NAME`；CI `verify_windows_bundle.ps1` UTF-16 嵌入校验；真机枚举仍待 |
| REQ-PICOO-VCAM-002 | planned | ARCH-PICOO-VCAM-001 | MFCreateVirtualCamera + 独立 Rust IMFMediaSource DLL | Session 生命周期由桌面进程持有；`windows-rs` 实现 COM/MF；`MENewStream`/`MEUpdatedStream`；480p/720p/1080p NV12；输出格式跟随 Shared Frame Ring；`windows-latest` Cargo 产出 `PicooVirtualCameraSource.dll`；Windows bundle export/load smoke；真机枚举仍待 |
| REQ-PICOO-VCAM-003 | implemented | PUC-004 | VCam 进程只读 Shared Frame Ring，不持有 QUIC/解码器 | Rust VCam Source 只依赖 `picoo-frame-hub` 与 Windows 系统 API；直接复用共享环与占位帧实现 |
| REQ-PICOO-VCAM-004 | implemented | PUC-001 | 安装器注册 COM/MF 组件与卸载清理 | WiX 声明式 COM 注册表 + FirewallException QUIC(4433) + 安装/卸载时 WixQuietExec 调用 `--register-vcam --no-wait` / `--unregister-vcam`；CLSID 三方同步；`validate_wix_scaffold.sh`；CI 钉扎 WiX 5.0.2；MSI 真机仍待 |
| REQ-PICOO-VCAM-005 | proposed | PUC-004 | Zoom/Teams/腾讯会议/OBS/浏览器可选用 | [会议软件验收清单](../verification/vcam-meeting-apps.md)（需 Win11） |
| REQ-PICOO-VCAM-006 | planned | ARCH-PICOO-VCAM-001 | macOS 使用 Core Media I/O Camera Extension 注册 `Picoo Camera` | Swift 6 CMIO 扩展与 App Group mmap 消费边界已可编译；`package macos` 将与 Bundle ID 同名的扩展嵌入 Host `.app` 标准目录；仍需 `OSSystemExtensionRequest`、用户批准、重启枚举、卸载清理与会议软件真机验收；扩展不持有 QUIC/Decoder |
| REQ-PICOO-VCAM-007 | planned | ARCH-PICOO-VCAM-001 / ci-and-build.md | macOS 发布产物满足 Hardened Runtime、Developer ID 签名与公证 | CI 归档 ARM64 未签名 Host `.app` 与已展开的 Host 签名输入 scaffold，并校验 sandbox/network/System Extension 能力及 App Group 一致性；实际 codesign entitlement、Developer ID 与公证仍需发布 workflow 通过 GitHub Secrets 注入凭据并完成真机验收 |
