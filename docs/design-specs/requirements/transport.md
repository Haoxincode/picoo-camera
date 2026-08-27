# REQ-PICOO-TRANSPORT：QUIC 传输

| ID | 状态 | 来源 | 描述 | 验收 |
| --- | --- | --- | --- | --- |
| REQ-PICOO-TRANSPORT-001 | implemented | ARCH-PICOO-TRANSPORT-001 | 业务层只依赖 `PicooTransport` trait | quiche 仅在 `picoo-quiche`/`picoo-transport` |
| REQ-PICOO-TRANSPORT-002 | implemented | ARCH-PICOO-TRANSPORT-001 | 控制消息走可靠 Stream，视频走 Datagram | loopback / testkit 验证 |
| REQ-PICOO-TRANSPORT-003 | implemented | PUC-002 | Receiver 为 QUIC Server，Sender 为 Client | QuicReceiver/QuicSender 角色测试 |
| REQ-PICOO-TRANSPORT-004 | implemented | PUC-006 | 重连退避 500ms→1s→2s→5s→5s | `ReconnectBackoff` 单元测试 |
| REQ-PICOO-TRANSPORT-005 | proposed | PRD §22 验证 A | Android/Windows quiche 可构建链接 | CI windows + android job（待 Actions 恢复） |
