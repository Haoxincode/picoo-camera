# REQ-PICOO-PRIVACY：隐私与本地-only

| ID | 状态 | 来源 | 描述 | 验收 |
| --- | --- | --- | --- | --- |
| REQ-PICOO-PRIVACY-001 | implemented | PUC-007 | 不向公网发送设备信息或指标 | 无 HTTP/遥测客户端；指标仅 PCP 本地 `ReceiverStats` |
| REQ-PICOO-PRIVACY-002 | implemented | PUC-007 | 默认不写入视频文件 | 帧仅内存/Shared Frame Ring；诊断 `includes_video: false` |
| REQ-PICOO-PRIVACY-003 | implemented | PUC-007 | 日志不含视频帧；IP/设备名/指纹可脱敏 | `picoo-diagnostics` 默认脱敏；Android `exportDiagnosticsToPathWithSession` 含 session/hosts；桌面 Settings 导出 |
| REQ-PICOO-PRIVACY-004 | implemented | PRD §21 | QUIC/TLS 加密，抓包无明文控制/视频 | 控制+视频经 quiche ALPN `picoocam/1`（发现 mDNS/QR 明文属设计） |
| REQ-PICOO-PRIVACY-005 | implemented | PUC-007 | 断网外网后 LAN 功能仍可用 | 无云依赖；`paired_loopback_binds_lan_only_without_wan` + 架构无 HTTP 遥测 |
