# REQ-PICOO-MEDIA：媒体管线（Android + Windows）

| ID | 状态 | 来源 | 描述 | 验收 |
| --- | --- | --- | --- | --- |
| REQ-PICOO-MEDIA-001 | implemented | PUC-005 | Android Camera2 + MediaCodec InputSurface 硬件 H.264 | `Camera2MediaEncoder` + `MediaBitrate` 单元测试（真机预览/编码仍待） |
| REQ-PICOO-MEDIA-002 | implemented | PUC-005 | 720p30 / 1080p30 能力协商与回退；中途分辨率切换 | Capabilities + Android 钳制；`midstream_resolution_change_openh264_updates_framehub` |
| REQ-PICOO-MEDIA-003 | implemented | PUC-005 | 前后摄切换触发 stream_epoch 递增与 IDR，3s 内恢复 | Android epoch++；`stream_epoch_bump_recovers_openh264_framehub_under_three_seconds` |
| REQ-PICOO-MEDIA-004 | implemented | PUC-005 | 本机预览镜像与远端输出镜像独立 | Android local vs remote；Receiver `nv12_mirror_horizontal` 应用 `StreamConfig.mirrored` |
| REQ-PICOO-MEDIA-005 | implemented | ARCH-PICOO-MEDIA-001 | Windows MF + D3D11 硬件解码 H.264；Linux/CI OpenH264 软解 | Windows：`windows-mf` MF 管线 + SPS/PPS；Linux：`OpenH264Decoder` + stub fixture 回退；真机 MF 验证仍待 |
| REQ-PICOO-MEDIA-006 | implemented | ARCH-PICOO-MEDIA-001 | Receiver 单次解码、FrameHub 多路消费 | `decode_invocations == access_units` 测试 |
| REQ-PICOO-MEDIA-007 | implemented | PUC-006 | 动态码率 720p 3–5 Mbps / 1080p 3–10 Mbps | rate-control + Android MediaCodec PARAMETER_KEY_VIDEO_BITRATE |
| REQ-PICOO-MEDIA-008 | implemented | PUC-005 | 手机端曝光补偿可调 | `Camera2MediaEncoder.setExposureCompensation` + Streaming EV±（真机验证仍待） |
| REQ-PICOO-MEDIA-009 | implemented | PUC-005 | StreamConfig.rotation 驱动 FrameHub/VCam 朝向 | Sender SENSOR_ORIENTATION → FFI → Receiver 覆盖解码器 rotation |
| REQ-PICOO-MEDIA-010 | implemented | PUC-006 | 持续拥塞降至 720p；健康后可回升 1080p；过热强制 720p | `Downshift/UpshiftResolution` + 720/1080 ladder + `PowerHints.shouldForce720p` |
