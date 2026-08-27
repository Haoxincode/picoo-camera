# REQ-PICOO-PROTOCOL：PCP/1 协议

| ID | 状态 | 来源 | 描述 | 验收 |
| --- | --- | --- | --- | --- |
| REQ-PICOO-PROTOCOL-001 | implemented | ARCH-PICOO-PROTOCOL-001 | VideoPacket 固定二进制头（26 字节 + payload） | 编解码往返 + header size 测试 |
| REQ-PICOO-PROTOCOL-002 | implemented | ARCH-PICOO-PROTOCOL-001 | QUIC ALPN 为 `picoocam/1` | `ALPN` 常量且 transport/discovery 使用 |
| REQ-PICOO-PROTOCOL-003 | implemented | ARCH-PICOO-PROTOCOL-001 | 单包最大 1150 字节，payload 不超 MTU | `rejects_oversized_datagram` |
| REQ-PICOO-PROTOCOL-004 | implemented | ARCH-PICOO-PROTOCOL-001 | stream_epoch 隔离重组 | packet crate epoch 测试 |
| REQ-PICOO-PROTOCOL-005 | implemented | PUC-005 | StreamConfig 携带 codec/分辨率/SPS/PPS/epoch；AU AVCC→Annex-B 规范化 | Android 提取 SPS/PPS → FFI → StreamConfig；`access_unit_to_annex_b`；`paired_avcc_length_prefixed_au_reaches_frame_hub` |
| REQ-PICOO-PROTOCOL-006 | implemented | PUC-006 / PUC-005 | ReceiverStats 每秒上报 RTT/丢包/帧龄等；Sender 直播页展示链路质量 | transport link_stats + ABR；`picoo_sender_last_receiver_stats` + Android `LinkQuality` |
| REQ-PICOO-PROTOCOL-007 | implemented | PRD §20.2 | VideoPacket Parser 可 fuzz | `fuzz/fuzz_targets/video_packet_decode.rs` + 随机解码非 panic 测试 |
