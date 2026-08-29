# REQ-PICOO-DISCOVERY：设备发现

| ID | 状态 | 来源 | 描述 | 验收 |
| --- | --- | --- | --- | --- |
| REQ-PICOO-DISCOVERY-001 | implemented | PUC-002 | Receiver 广播 `_picoocam._udp.local` | MdnsAdvertiser 注册/改名/信任变更后 re-advertise；LAN `select_advertise_ipv4`（禁 loopback）；默认 QUIC `4433` |
| REQ-PICOO-DISCOVERY-002 | implemented | ARCH-PICOO-DISCOVERY-001 | 广播仅含白名单字段；fingerprint_prefix 来自公钥 | TXT 白名单 + 指纹前缀测试 |
| REQ-PICOO-DISCOVERY-005 | implemented | PUC-002 | Android NSD 浏览 Receiver 列表 | `NsdReceiverBrowser` + TXT 白名单；Devices 行展示 Ready/Paired（TXT + 本地 trust）；`scripts/check_discovery_txt_keys.sh` 与 Rust 对齐 |
| REQ-PICOO-DISCOVERY-006 | implemented | PUC-002 / PRD §8.1 | 健康网络发现 P50&lt;2s；已配对连接建立 &lt;3s | `paired_connect_to_streaming_under_three_seconds`；`apply_resolved_txt`→list P50（`synthetic_advertise_to_list_p50_under_two_seconds`）；真 mDNS `--ignored` |
| REQ-PICOO-DISCOVERY-007 | implemented | PUC-008 / PRD FR-DISC-003 | mDNS 不可用时，Sender 可输入局域网 `IP:端口` 直连 Receiver；该地址只定位 Endpoint，不建立信任 | Android `ManualConnectScreen`；桌面等待页显示连接地址；`manual_endpoint_connects_to_streaming` |
