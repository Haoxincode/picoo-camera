# REQ-PICOO-SESSION：会话与弹性

| ID | 状态 | 来源 | 描述 | 验收 |
| --- | --- | --- | --- | --- |
| REQ-PICOO-SESSION-001 | proposed | ARCH-PICOO-SESSION-001 | UI 可观察会话状态枚举完整 | `ReceiverStatus`/`SenderStatus` 导出 |
| REQ-PICOO-SESSION-002 | proposed | ARCH-PICOO-SESSION-001 | 抖动缓冲目标 50ms，最大 120ms | jitter crate 测试 |
| REQ-PICOO-SESSION-003 | implemented | ARCH-PICOO-SESSION-001 | 不完整非关键帧丢弃，关键帧丢则请求 IDR | packet `take_keyframe_loss` + EncoderCommand |
| REQ-PICOO-SESSION-004 | implemented | PUC-006 | 重连后恢复分辨率/镜像并请求 SPS/PPS/IDR | sender reconnect 集成测试 |
| REQ-PICOO-SESSION-005 | proposed | PRD §21 | 2h 长稳无崩溃、内存不增长 | 长稳测试脚本 |
