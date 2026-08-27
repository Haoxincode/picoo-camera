# REQ-PICOO-PROTOCOL：PCP/1 协议

| ID | 状态 | 来源 | 描述 | 验收 |
| --- | --- | --- | --- | --- |
| REQ-PICOO-PROTOCOL-001 | proposed | ARCH-PICOO-PROTOCOL-001 | VideoPacket 固定二进制头（26 字节 + payload） | 单元测试编解码往返 |
| REQ-PICOO-PROTOCOL-002 | proposed | ARCH-PICOO-PROTOCOL-001 | QUIC ALPN 为 `picoocam/1` | 常量导出且 transport 使用 |
| REQ-PICOO-PROTOCOL-003 | proposed | ARCH-PICOO-PROTOCOL-001 | 单包最大 1150 字节，payload 不超 MTU | 超限返回错误 |
| REQ-PICOO-PROTOCOL-004 | proposed | ARCH-PICOO-PROTOCOL-001 | stream_epoch 隔离重组 | 不同 epoch 片段不可组帧 |
| REQ-PICOO-PROTOCOL-005 | implemented | PUC-005 | StreamConfig 携带 codec/分辨率/SPS/PPS/epoch | Android 提取 SPS/PPS → FFI → StreamConfig；packet 解析测试 |
| REQ-PICOO-PROTOCOL-006 | implemented | PUC-006 | ReceiverStats 每秒上报 RTT/丢包/帧龄等 | transport link_stats + receiver 上报；sender ABR |
| REQ-PICOO-PROTOCOL-007 | proposed | PRD §20.2 | VideoPacket Parser 可 fuzz | cargo fuzz 目标存在 |
