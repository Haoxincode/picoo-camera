# REQ-PICOO-UI：Android Compose + Windows GPUI

| ID | 状态 | 来源 | 描述 | 验收 |
| --- | --- | --- | --- | --- |
| REQ-PICOO-UI-001 | implemented | ARCH-PICOO-UI-001 | GPUI View 不直接持有 QUIC/Decoder/Frame Buffer | `ReceiverRuntime` + `ReceiverSnapshot`；View 只观察 |
| REQ-PICOO-UI-002 | implemented | PRD §16 | 桌面四页：首次启动/等待连接(含QR)/直播/设置；日志级别运行时可 reload | `DesktopPage`；改名→mDNS；`logging.rs` EnvFilter reload |
| REQ-PICOO-UI-003 | implemented | PRD §17 | 手机三页：设备列表/配对/传输 | Devices `Windows · Ready/Paired`；Pairing/Streaming 显示 Receiver 名；Cancel 取消配对 |
| REQ-PICOO-UI-004 | implemented | ARCH-PICOO-UI-001 | VideoSurface 只渲染纹理，不拥有解码器；Live 显示 Network Quality | `video_surface.rs`；`network_quality.rs` 与 Android `LinkQuality` 阈值对齐 |
| REQ-PICOO-UI-005 | implemented | PUC-005 | Sender 前台、防锁屏、深色低亮度、过热/低电量提示；API33+ 通知权限 | FGS + `POST_NOTIFICATIONS` + `FLAG_KEEP_SCREEN_ON` + Streaming `screenBrightness` 调暗 + `PowerHints`（真机验证仍待） |
| REQ-PICOO-UI-006 | implemented | PUC-001 | 权限在操作时请求，非启动一次性弹出 | Enable camera / Scan QR 时请求 CAMERA |
| REQ-PICOO-UI-007 | implemented | PRD §16 | 开机启动偏好写入 OS | `startup.rs` HKCU Run（Windows）+ MemoryStore 测试 |
| REQ-PICOO-UI-008 | implemented | PRD §16 | 关闭窗口时托盘策略 | `NotifyIconController` + tip 随 `ReceiverStatus`；`TrayMenuAction::apply` Show/Quit；HWND 注入或 `FindWindowW`；GPUI close + pump |
| REQ-PICOO-UI-009 | implemented | PUC-005 | 桌面直播页远程摄像头控制（前后摄 / 720p·1080p / 远端镜像）经 ReceiverSession → CameraCommand；Sender FFI/JNI 消费 | GPUI Live 前置/后置/720p/1080p/远端镜像 → `send_camera_command`；`picoo_sender_take_camera_command` + Android poll |
