# REQ-PICOO-DISCOVERY：设备发现

| ID | 状态 | 来源 | 描述 | 验收 |
| --- | --- | --- | --- | --- |
| REQ-PICOO-DISCOVERY-001 | proposed | PUC-002 | Receiver 广播 `_picoocam._udp.local` | Windows mDNS adapter 测试 |
| REQ-PICOO-DISCOVERY-002 | proposed | ARCH-PICOO-DISCOVERY-001 | 广播仅含 receiver_id/display_name/version/port/state/fingerprint_prefix | 字段白名单测试 |
| REQ-PICOO-DISCOVERY-003 | proposed | PUC-003 | 桌面 Show QR Code，含地址/端口/ID/版本/指纹/nonce | QR payload 编解码测试 |
| REQ-PICOO-DISCOVERY-004 | proposed | PUC-003 | 短期 nonce 过期后二维码不可复用 | 过期单元测试 |
| REQ-PICOO-DISCOVERY-005 | proposed | PUC-002 | Android NSD 浏览 Receiver 列表 | Android 仪器测试 |
