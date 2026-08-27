# REQ-PICOO-FRAME：FrameHub 与 Shared Frame Ring

| ID | 状态 | 来源 | 描述 | 验收 |
| --- | --- | --- | --- | --- |
| REQ-PICOO-FRAME-001 | implemented | ARCH-PICOO-FRAME-001 | FrameHub 三槽环形缓冲，最新完整帧优先 | frame-hub 单元测试 |
| REQ-PICOO-FRAME-002 | implemented | ARCH-PICOO-FRAME-001 | 消费者慢时丢弃旧帧 | ring rapid overwrite 测试 |
| REQ-PICOO-FRAME-003 | implemented | PUC-004 | Shared Frame Ring（Win Named SHM / Linux flink） | producer/consumer + 并发 poll |
| REQ-PICOO-FRAME-004 | implemented | PUC-004 | 无连接时输出占位画面（黑底+标志+Waiting for phone...） | placeholder 测试 |
| REQ-PICOO-FRAME-005 | proposed | PUC-006 | 中断时最多短暂重复最后一帧，随后重连占位 | 状态切换测试 |
