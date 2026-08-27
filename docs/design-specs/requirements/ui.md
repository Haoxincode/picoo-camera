# REQ-PICOO-UI：Android Compose + Windows GPUI

| ID | 状态 | 来源 | 描述 | 验收 |
| --- | --- | --- | --- | --- |
| REQ-PICOO-UI-001 | implemented | ARCH-PICOO-UI-001 | GPUI View 不直接持有 QUIC/Decoder/Frame Buffer | `ReceiverRuntime` + `ReceiverSnapshot`；View 只观察 |
| REQ-PICOO-UI-002 | implemented | PRD §16 | 桌面四页：首次启动/等待连接(含QR)/直播/设置 | `DesktopPage` FirstLaunch/Waiting/Live/Settings |
| REQ-PICOO-UI-003 | implemented | PRD §17 | 手机三页：设备列表/配对/传输 | `SenderTab` Devices/Pairing/Streaming 导航 |
| REQ-PICOO-UI-004 | implemented | ARCH-PICOO-UI-001 | VideoSurface 只渲染纹理，不拥有解码器 | `video_surface.rs` 仅 `nv12_preview_rgba` |
| REQ-PICOO-UI-005 | implemented | PUC-005 | Sender 前台、防锁屏、深色低亮度、过热/低电量提示 | FGS + `FLAG_KEEP_SCREEN_ON` + Streaming `screenBrightness` 调暗 + `PowerHints`（真机验证仍待） |
| REQ-PICOO-UI-006 | implemented | PUC-001 | 权限在操作时请求，非启动一次性弹出 | Enable camera / Scan QR 时请求 CAMERA |
| REQ-PICOO-UI-007 | implemented | PRD §16 | 开机启动偏好写入 OS | `startup.rs` HKCU Run（Windows）+ MemoryStore 测试 |
| REQ-PICOO-UI-008 | implemented | PRD §16 | 关闭窗口时托盘策略 | `tray.rs` CloseOutcome + `NotifyIconController` ADD/MODIFY/DELETE；GPUI `on_window_should_close`；Win32 `Shell_NotifyIconW` 待 HWND |
