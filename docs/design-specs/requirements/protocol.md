# REQ-PICOO-PROTOCOL：PCP/1 协议

| ID | 状态 | 来源 | 描述 | 验收 |
| --- | --- | --- | --- | --- |
| REQ-PICOO-PROTOCOL-001 | implemented | ARCH-PICOO-PROTOCOL-001 | VideoPacket 固定二进制头（26 字节 + payload） | 编解码往返 + header size 测试 |
| REQ-PICOO-PROTOCOL-002 | implemented | ARCH-PICOO-PROTOCOL-001 | QUIC ALPN 为 `picoocam/1` | `ALPN` 常量；Hello `protocol_version` 不匹配 fail-fast |
| REQ-PICOO-PROTOCOL-003 | implemented | ARCH-PICOO-PROTOCOL-001 | 单包最大 1150 字节，payload 不超 MTU | `rejects_oversized_datagram` |
| REQ-PICOO-PROTOCOL-004 | implemented | ARCH-PICOO-PROTOCOL-001 | stream_epoch 隔离重组 | packet crate epoch 测试 |
| REQ-PICOO-PROTOCOL-005 | implemented | PUC-005 | StreamConfig 携带 codec/分辨率/SPS/PPS/epoch；AU AVCC→Annex-B 规范化 | Android 提取 SPS/PPS → FFI → StreamConfig；`access_unit_to_annex_b`；`paired_avcc_length_prefixed_au_reaches_frame_hub` |
| REQ-PICOO-PROTOCOL-006 | implemented | PUC-006 / PUC-005 | ReceiverStats 每秒上报 RTT/丢包/帧龄等；Sender 直播页展示链路质量 | transport link_stats + ABR；`picoo_sender_last_receiver_stats` + Android `LinkQuality` |
| REQ-PICOO-PROTOCOL-008 | implemented | ARCH-PICOO-PROTOCOL-001 | StartStream/StopStream 使用 magic 判别（1/2），避免空 message 误解码；未配对 StopStream 不拆除配对 | `unpaired_start_stream_is_rejected` + `unpaired_stop_stream_is_ignored_without_teardown` + `paired_start_stop_stream_and_camera_command_roundtrip` |
