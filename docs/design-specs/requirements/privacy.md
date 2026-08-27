# REQ-PICOO-PRIVACY：隐私与本地-only

| ID | 状态 | 来源 | 描述 | 验收 |
| --- | --- | --- | --- | --- |
| REQ-PICOO-PRIVACY-001 | proposed | PUC-007 | 不向公网发送设备信息或指标 | 网络抓包/代码审查 |
| REQ-PICOO-PRIVACY-002 | proposed | PUC-007 | 默认不写入视频文件 | 存储扫描 |
| REQ-PICOO-PRIVACY-003 | proposed | PUC-007 | 日志不含视频帧；IP/设备名/指纹可脱敏 | 日志导出测试 |
| REQ-PICOO-PRIVACY-004 | proposed | PRD §21 | QUIC/TLS 加密，抓包无明文控制/视频 | TLS 验证 |
| REQ-PICOO-PRIVACY-005 | proposed | PUC-007 | 断网外网后 LAN 功能仍可用 | 离线测试 |
