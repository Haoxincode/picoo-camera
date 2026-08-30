# REQ-PICOO-MEDIA：跨平台媒体管线

| ID | 状态 | 来源 | 描述 | 验收 |
| --- | --- | --- | --- | --- |
| REQ-PICOO-MEDIA-001 | implemented | PUC-005 | Android Camera2 + MediaCodec InputSurface 硬件 H.264 | `Camera2MediaEncoder` + `MediaBitrate` 单元测试（真机预览/编码仍待） |
| REQ-PICOO-MEDIA-002 | implemented | PUC-005 | 720p30 / 1080p30 能力协商与回退；中途分辨率切换 | Caps→`receiver_max_height` 钳制 pending StreamConfig；`capabilities_720_only_clamps_sender_stream_config`；JVM OEM 回退 |
| REQ-PICOO-MEDIA-003 | implemented | PUC-005 | 前后摄切换触发 stream_epoch 递增与 IDR，3s 内恢复 | Android `StreamEpoch` + Camera2；`stream_epoch_bump_recovers_openh264_framehub_under_three_seconds`；JVM `StreamEpochTest` |
| REQ-PICOO-MEDIA-004 | implemented | PUC-005 | 本机预览镜像与远端输出镜像独立 | Android `LocalPreviewMirror` 按前后摄默认；Receiver `nv12_mirror_horizontal` 应用 `StreamConfig.mirrored` |
| REQ-PICOO-MEDIA-005 | implemented | ARCH-PICOO-MEDIA-001 | Windows MF + D3D11 硬件解码 H.264；Linux/CI OpenH264 软解 | Windows：`windows-mf` MF 管线 + SPS/PPS；Linux：`OpenH264Decoder` + stub fixture 回退；真机 MF 验证仍待 |
| REQ-PICOO-MEDIA-006 | implemented | ARCH-PICOO-MEDIA-001 | Receiver 单次解码、FrameHub 多路消费 | `decode_invocations == access_units` 测试 |
| REQ-PICOO-MEDIA-007 | implemented | PUC-006 | 动态码率 720p 3–5 Mbps / 1080p 3–10 Mbps | rate-control + Android MediaCodec PARAMETER_KEY_VIDEO_BITRATE |
| REQ-PICOO-MEDIA-008 | implemented | PUC-005 | 手机端曝光补偿可调 | `Camera2MediaEncoder.setExposureCompensation` + Streaming EV±（真机验证仍待） |
| REQ-PICOO-MEDIA-009 | implemented | PUC-005 | StreamConfig.rotation 驱动 FrameHub/VCam 朝向 | `nv12_rotate_clockwise` 在 publish 前直立像素；metadata 清零；dims 随 90/270 交换 |
| REQ-PICOO-MEDIA-010 | implemented | PUC-006 | 持续拥塞 1080→720→480；健康后可回升；过热强制 ≤720p | rate-control + Android；`abr_downshift`（含 480）/`abr_upshift` FrameHub；`thermal_hold`；`sync_encode_height` + JNI |
| REQ-PICOO-MEDIA-011 | implemented | ARCH-PICOO-MEDIA-001 | iOS 使用 AVFoundation 采集并由 VideoToolbox 低延迟硬件编码 H.264；原始帧不跨 Rust FFI | `AVCaptureVideoDataOutput` 原生 420v → `VTPixelTransferSession` 854×480 缩放（按需）→ 必需硬件 `VTCompressionSession`（Realtime / 无 B 帧 / 2s IDR）→ AVCC AU + SPS/PPS → `PicooCore`；AU handoff 有界且连接/epoch 切换等待新 IDR，Rust 断连清空待发分片；Swift 6 App 与测试 bundle 已编译链接，测试执行待 CI；480p/720p/1080p30、方向、切换 3s 恢复和弱网 ABR 仍待 iPhone 真机升级为 `verified` |
| REQ-PICOO-MEDIA-012 | implemented | ARCH-PICOO-MEDIA-001 | macOS Receiver 使用 VideoToolbox 单次硬件解码至 NV12 FrameHub | `objc2` Apple Framework 绑定将 Annex-B/AVCC + SPS/PPS 封装为 CoreMedia Sample，要求硬件 `VTDecompressionSession`，同步解码并按 plane stride 复制紧凑 `420v` NV12；静态真实 H.264 IDR 覆盖 Annex-B、AVCC、无假帧错误、SPS/PPS 重建，以及 Receiver 720p→480p ABR/epoch/FrameHub 恢复；断连、StopStream、关闭均 flush session；`xtask test macos` 的产品和测试树均不需要 OpenH264/CMake。Android/iOS→GPUI/Camera Extension 真机链路仍待升级为 `verified` |
