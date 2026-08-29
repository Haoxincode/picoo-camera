# REQ-PICOO-UI：手机原生 UI + 桌面 GPUI

| ID | 状态 | 来源 | 描述 | 验收 |
| --- | --- | --- | --- | --- |
| REQ-PICOO-UI-001 | implemented | ARCH-PICOO-UI-001 | GPUI View 不直接持有 QUIC/Decoder/Frame Buffer | `ReceiverRuntime` + `ReceiverSnapshot`；View 只观察 |
| REQ-PICOO-UI-002 | implemented | PRD §16 | 桌面首次启动/等待连接/直播/设置；等待页显示手动连接地址；日志级别运行时可 reload | `DesktopPage`；`render_manual_endpoint_card`；改名→mDNS；`logging.rs` EnvFilter reload |
| REQ-PICOO-UI-003 | implemented | PRD §17 / PUC-008 | 手机设备列表、手动 IP 直连、配对、传输页 | Devices `Windows · Ready/Paired`；`ManualConnectScreen`；Pairing/Streaming 显示 Receiver 名；Cancel 取消配对 |
| REQ-PICOO-UI-004 | implemented | ARCH-PICOO-UI-001 | VideoSurface 只渲染纹理，不拥有解码器；Live 显示 Network Quality | `video_surface.rs`；`network_quality.rs` 与 Android `LinkQuality` 阈值对齐 |
| REQ-PICOO-UI-005 | implemented | PUC-005 | Sender 前台、防锁屏、深色低亮度、过热/低电量提示；API33+ 通知权限 | FGS + `POST_NOTIFICATIONS` + `FLAG_KEEP_SCREEN_ON` + Streaming `screenBrightness` 调暗 + `PowerHints`；`ManifestPermissionsTest`（真机验证仍待） |
| REQ-PICOO-UI-006 | implemented | PUC-001 / PUC-008 | 权限在操作时请求，非启动一次性弹出；连接流程不请求相机权限 | 开始视频采集时请求 CAMERA；Nearby/Notifications 按需；`ManifestPermissionsTest` |
| REQ-PICOO-UI-007 | implemented | PRD §16 | 开机启动偏好写入 OS | `startup.rs` HKCU Run（Windows）+ MemoryStore 测试 |
| REQ-PICOO-UI-008 | implemented | PRD §16 | Windows 关闭窗口时可最小化到系统托盘；macOS 不伪装 Windows 托盘，关闭后保留在 Dock/后台 | Windows `NotifyIconController` + tip 随 `ReceiverStatus`；`TrayMenuAction::apply` Show/Quit；message-only HWND 或 `FindWindowW`；macOS 构建不包含 Notify Icon 状态 |
| REQ-PICOO-UI-009 | implemented | PUC-005 | 桌面直播页远程摄像头控制（前后摄 / 480p·720p·1080p / 远端镜像）经 ReceiverSession → CameraCommand；Sender FFI/JNI 消费 | GPUI Live 前置/后置/480p/720p/1080p/远端镜像 → `send_camera_command`；`picoo_sender_take_camera_command` + Android poll |
| REQ-PICOO-UI-010 | implemented | ARCH-PICOO-UI-001 | iOS Sender 使用 SwiftUI 复现设备列表、手动连接、配对与传输页面，不复制 Rust 业务状态机 | `SenderAppModel` 以 Observation 单向驱动设备/手动连接/配对/等待/传输页；状态来自 Rust C ABI，Camera 权限只在进入传输页时请求；Reicon 使用本地 SVG Asset Catalog；完整视觉与真机交互仍按 `REQ-PICOO-UI-0001` 验收 |
| REQ-PICOO-UI-011 | planned | ARCH-PICOO-UI-001 | macOS Receiver 复用 `apps/desktop` 的 GPUI 页面与主题，仅隔离启动项、窗口后台行为、虚拟摄像头状态等平台适配 | `xtask build macos` 编译同一 GPUI View；macOS 不构建 Win32 Notify Icon；不新增 macOS 专属 WebView/SwiftUI 桌面 UI；平台适配具备独立测试 |
