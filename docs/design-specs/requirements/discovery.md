# REQ-PICOO-DISCOVERY：设备发现

| ID | 状态 | 来源 | 描述 | 验收 |
| --- | --- | --- | --- | --- |
| REQ-PICOO-DISCOVERY-001 | implemented | PUC-002 | Receiver 广播 `_picoocam._udp.local` | MdnsAdvertiser 注册测试 |
| REQ-PICOO-DISCOVERY-002 | implemented | ARCH-PICOO-DISCOVERY-001 | 广播仅含白名单字段；fingerprint_prefix 来自公钥 | TXT 白名单 + 指纹前缀测试 |
| REQ-PICOO-DISCOVERY-003 | implemented | PUC-003 | 桌面 Show QR Code，含地址/端口/ID/版本/指纹/nonce | QR payload 使用真实指纹 |
| REQ-PICOO-DISCOVERY-004 | implemented | PUC-003 | 短期 nonce 过期后二维码不可复用 | QR TTL + Receiver `active_qr` 校验/消费；桌面过期自动刷新；Android 传 `qr_nonce` |
| REQ-PICOO-DISCOVERY-005 | implemented | PUC-002 | Android NSD 浏览 Receiver 列表 | `NsdReceiverBrowser` + TXT 白名单解析测试 |
| REQ-PICOO-DISCOVERY-006 | implemented | PUC-002 / PRD §8.1 | 健康网络发现 P50&lt;2s；已配对连接建立 &lt;3s | `paired_connect_to_streaming_under_three_seconds`；`synthetic_advertise_to_list_p50_under_two_seconds`；真 mDNS `--ignored` |
