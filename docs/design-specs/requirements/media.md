# REQ-PICOO-MEDIA：媒体管线（Android + Windows）

| ID | 状态 | 来源 | 描述 | 验收 |
| --- | --- | --- | --- | --- |
| REQ-PICOO-MEDIA-001 | proposed | PUC-005 | Android Camera2 + MediaCodec InputSurface 硬件 H.264 | 真机预览+编码 |
| REQ-PICOO-MEDIA-002 | proposed | PUC-005 | 720p30 / 1080p30 能力协商与回退 | 分辨率切换测试 |
| REQ-PICOO-MEDIA-003 | proposed | PUC-005 | 前后摄切换触发 stream_epoch 递增与 IDR | 3s 内恢复测试 |
| REQ-PICOO-MEDIA-004 | proposed | PUC-005 | 本机预览镜像与远端输出镜像独立 | 镜像行为测试 |
| REQ-PICOO-MEDIA-005 | proposed | ARCH-PICOO-MEDIA-001 | Windows MF + D3D11 硬件解码 H.264 | Windows 解码测试 |
| REQ-PICOO-MEDIA-006 | proposed | ARCH-PICOO-MEDIA-001 | Receiver 单次解码、FrameHub 多路消费 | 解码次数断言 |
| REQ-PICOO-MEDIA-007 | proposed | PUC-006 | 动态码率 720p 3–5 Mbps / 1080p 3–10 Mbps | rate-control 测试 |
