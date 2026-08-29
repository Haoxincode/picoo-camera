# REQ-PICOO-TRANSPORT：QUIC 传输

| ID | 状态 | 来源 | 描述 | 验收 |
| --- | --- | --- | --- | --- |
| REQ-PICOO-TRANSPORT-001 | implemented | ARCH-PICOO-TRANSPORT-001 | 业务层只依赖 `PicooTransport` trait | Quinn/Rustls 仅在 `picoo-transport` |
| REQ-PICOO-TRANSPORT-002 | implemented | ARCH-PICOO-TRANSPORT-001 | 控制消息走可靠 Stream，视频走 Datagram | loopback / testkit 验证 |
| REQ-PICOO-TRANSPORT-003 | implemented | PUC-002 | Receiver 为 QUIC Server，Sender 为 Client | QuicReceiver/QuicSender 角色测试 |
| REQ-PICOO-TRANSPORT-004 | implemented | PUC-006 | 重连退避 500ms→1s→2s→5s→5s | `ReconnectBackoff` + `reconnect_backoff_escalates_across_failed_attempts` |
| REQ-PICOO-TRANSPORT-005 | planned | PRD §22 验证 A / ARCH-PICOO-TRANSPORT-001 | Android/Windows Quinn + Rustls 可构建链接，QUIC 构建不依赖 CMake/BoringSSL/NASM | Android：`cargo ndk`→Rust JNI `.so` + Gradle APK/AAB（含 16KB 门禁）；Windows：`xtask build/package windows` 产出 exe/DLL/MSI（CI `windows-latest`） |
