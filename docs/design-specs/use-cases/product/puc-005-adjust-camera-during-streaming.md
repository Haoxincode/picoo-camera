# PUC-005：会议中调整摄像头、分辨率与镜像

## 基本信息

- 类别：Product Live Control
- 参与者：会议与录制用户
- 目标：在传输过程中从手机端或桌面端查看状态，并调整前后摄像头、分辨率、镜像和曝光补偿

## 场景

传输建立后，Sender 显示直播页：本机摄像头预览、连接质量、当前分辨率与帧率、目标 Receiver 名称，以及前后摄像头、480p/720p/1080p、镜像和断开控制。

用户切换前置或后置摄像头时，Sender 允许短暂重建编码器和视频会话；Receiver 收到新的 `stream_epoch` 和 `StreamConfig`，丢弃旧 epoch 片段并请求 IDR。切换完成后，桌面预览与虚拟摄像头在 3 秒内恢复可用画面。

用户切换 480p、720p 或 1080p 时，编码器按协商能力回退；Receiver 自适应调整解码和 FrameHub 缓冲。用户开启远端镜像时，Receiver 在输出到虚拟摄像头和桌面预览的路径上应用镜像；本机预览镜像与远端输出镜像相互独立——默认前置摄像头本机预览镜像，传输到会议软件的画面不镜像。

用户可在手机端查看 Wi-Fi 质量、延迟和连接状态；桌面端可查看分辨率、帧率、码率、延迟、丢包和网络质量。

## 可观察结果

- 手机端可在 `Front` / `Back` 间切换；Receiver 与虚拟摄像头在 3 秒内恢复。
- 系统支持 854×480@30、1280×720@30 与 1920×1080@30；设备不支持目标规格时通过能力协商回退。
- 本机预览镜像与远端输出镜像可独立配置；默认行为符合 PRD 约定。
- 视频帧携带旋转和方向信息；桌面端输出到会议软件的方向稳定可消费。
- 传输期间 Sender 保持前台，提供防锁屏、深色低亮度界面、过热提示和电量不足提示。
- 桌面端直播页显示手机名称、分辨率、帧率、码率和延迟；虚拟摄像头状态为 `Active`。

## 边界

- 4K、60 FPS、HDR、HEVC 不在当前范围。
- AI 美颜、背景替换、虚化不在当前范围。
- 曝光补偿属于 Sender 侧采集控制；Receiver 不代替手机做 ISP 级处理。
- 本 Use Case 不描述断网恢复，见 [PUC-006](puc-006-auto-reconnect-after-network-interruption.md)。

## 相关 Architecture

- [ARCH-PICOO-MEDIA-001](../../architecture/0004-cross-platform-media-pipeline-boundary.md)
- [ARCH-PICOO-PROTOCOL-001](../../architecture/0003-picoo-camera-protocol-boundary.md)
- [ARCH-PICOO-SESSION-001](../../architecture/0005-session-reconnect-jitter-bitrate-boundary.md)
- [ARCH-PICOO-UI-001](../../architecture/0009-desktop-gpui-mobile-native-ui-boundary.md)

## 相关 Requirements

- 待分解：`REQ-PICOO-MEDIA-*`、`REQ-PICOO-PROTOCOL-*`
