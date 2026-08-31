# REQ-PICOO-DISCOVERY：设备发现

| ID | 状态 | 来源 | 描述 | 验收 |
| --- | --- | --- | --- | --- |
| REQ-PICOO-DISCOVERY-001 | implemented | PUC-002 | Receiver 广播 `_picoocam._udp.local`，广播出口必须绑定所选局域网地址，不得漂移到 VPN/Hyper-V/WSL 虚拟接口；Windows 安装后必须允许局域网 mDNS 流量 | MdnsAdvertiser 注册/改名/信任变更后 re-advertise，并以 `IfKind::All` disable + LAN IP enable 限定 mDNS daemon；LAN `select_advertise_ipv4`（禁 loopback）；Windows WiX 同时放行 mDNS UDP `5353` 与默认 QUIC UDP `4433` |
| REQ-PICOO-DISCOVERY-002 | implemented | ARCH-PICOO-DISCOVERY-001 | 广播仅含白名单字段；fingerprint_prefix 来自公钥 | TXT 白名单 + 指纹前缀测试 |
| REQ-PICOO-DISCOVERY-005 | implemented | PUC-002 | Android 使用系统 NSD 浏览 Receiver，但 TXT 协议字段由 Rust Core 统一校验；API 33+ 发现明确绑定 Wi-Fi NetworkRequest，不随默认 VPN 路由漂移；系统浏览异常停止后自动恢复，网络切换或 Receiver 刷新 TXT 导致的瞬时 ServiceLost 不立即删除设备 | `NsdReceiverBrowser` 将原始 TXT 交给 JNI `parseDiscoveryTxt`；Rust `ReceiverAdvertisement::from_txt_properties` 强制完整字段、白名单与 pairing_state；Wi-Fi `NetworkRequest`；NSD 失败延迟重启、ServiceLost 设 10s 宽限且重新解析后取消；Devices 行展示 Ready/Paired |
| REQ-PICOO-DISCOVERY-006 | implemented | PUC-002 / PRD §8.1 | 健康网络发现 P50&lt;2s；已配对连接建立 &lt;3s | `paired_connect_to_streaming_under_three_seconds`；`apply_resolved_txt`→list P50（`synthetic_advertise_to_list_p50_under_two_seconds`）；真 mDNS `--ignored` |
| REQ-PICOO-DISCOVERY-007 | implemented | PUC-008 / PRD FR-DISC-003 | mDNS 不可用时，Sender 可输入局域网 `IP:端口` 直连 Receiver；最近一次有效输入作为 Endpoint 定位信息保存在本机，但不建立信任 | Android `DevicesScreen` 在连接失败、弹窗重开与进程重启后保留最近 Endpoint；Rust Sender 缓存连接身份并在异步 QUIC 建连后发送 ClientHello；桌面等待页显示连接地址；`client_hello_queued_before_async_connect_is_sent_when_connected`；`manual_endpoint_connects_to_streaming` |
| REQ-PICOO-DISCOVERY-008 | implemented | PUC-002 / ARCH-PICOO-DISCOVERY-001 | iOS 在用户发起连接流程时请求 Local Network 权限并发现 Receiver | `PicooDiscoveryBrowser` 轮询 C ABI discovery browser；`NSLocalNetworkUsageDescription` + `_picoocam._udp` Bonjour 声明；发现不可用/拒绝权限时保留经校验的 IPv4 或方括号 IPv6 `IP:端口` 直连；真局域网发现时延仍待真机验证 |
