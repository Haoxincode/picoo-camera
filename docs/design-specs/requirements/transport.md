# REQ-PICOO-TRANSPORT：QUIC 传输

| ID | 状态 | 来源 | 描述 | 验收 |
| --- | --- | --- | --- | --- |
| REQ-PICOO-TRANSPORT-001 | proposed | ARCH-PICOO-TRANSPORT-001 | 业务层只依赖 `PicooTransport` trait | 无 crate 直接 use quiche |
| REQ-PICOO-TRANSPORT-002 | proposed | ARCH-PICOO-TRANSPORT-001 | 控制消息走可靠 Stream，视频走 Datagram | testkit 模拟验证 |
| REQ-PICOO-TRANSPORT-003 | proposed | PUC-002 | Receiver 为 QUIC Server，Sender 为 Client | 连接角色测试 |
| REQ-PICOO-TRANSPORT-004 | proposed | PUC-006 | 重连退避 500ms→1s→2s→5s→5s | session 单元测试 |
| REQ-PICOO-TRANSPORT-005 | proposed | PRD §22 验证 A | Android/Windows quiche 可构建链接 | CI windows + android job |
