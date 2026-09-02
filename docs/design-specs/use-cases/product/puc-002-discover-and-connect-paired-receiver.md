# PUC-002：自动发现并连接已配对电脑

## 基本信息

- 类别：Product Connection and Session Establishment
- 参与者：会议与录制用户
- 目标：在后续日常使用中，通过 mDNS 自动发现已配对或未配对的 Receiver，并建立 QUIC 会话与视频传输

## 场景

用户打开桌面端 Receiver。Receiver 启动后通过 mDNS/DNS-SD 广播 `_picoocam._udp.local` 服务，广播记录仅包含 `receiver_id`、`display_name`、`protocol_version`、`quic_port`、`pairing_state` 和 `public_key_fingerprint_prefix`，不包含用户身份、视频状态或完整密钥。

用户打开手机端 Sender。Sender 浏览局域网内的 Receiver 列表，显示电脑显示名称、平台类型和就绪状态。对于已配对设备，Sender 可自动连接或等待用户点击连接；对于未配对设备，进入 [PUC-001](puc-001-first-install-and-pairing.md) 的配对流程。

连接建立后，双方通过 PCP/4 可靠 Stream 完成 Hello、Capabilities、StartStream 等控制消息交换；Sender 通过 QUIC Datagram 发送受 FEC 保护的 H.264 视频片段；Receiver 重组、自适应 deadline 播放、解码并将画面提供给 FrameHub 与虚拟摄像头。

## 可观察结果

- 桌面端启动后，局域网内 Sender 在 P50 小于 2 s、P95 小于 5 s 内可发现 Receiver（健康网络条件下）。
- 发现列表显示 `display_name`、平台信息和就绪状态，不泄露敏感密钥或当前是否在会议中。
- 已配对设备连接时，系统验证固定公钥；公钥变化时拒绝自动连接并要求重新配对。
- 连接建立后，Receiver UI 显示 `Connecting` → `Negotiating` → `Streaming` 等可区分状态。
- 已配对设备从发起到开始传输的目标时间小于 3 s（健康局域网条件）。
- 健康 5 GHz 或 Wi-Fi 6 网络下，端到端延迟 P50 小于 150 ms，P95 小于 250 ms。

## 边界

- 本 Use Case 假设 mDNS 在局域网内可用；不可用时的手动 IP 直连见 [PUC-008](puc-008-connect-with-code-or-ip.md)。
- 一台手机同时连接多台电脑、多手机同时输入一台电脑不在当前范围。
- Sender 应用必须保持前台；后台或锁屏持续传输不在当前范围。
- 本 Use Case 不描述会议软件内的选用行为，见 [PUC-004](puc-004-use-virtual-camera-in-meeting-apps.md)。

## 相关 Architecture

- [ARCH-PICOO-DISCOVERY-001](../../architecture/0008-discovery-and-pairing-security-boundary.md)
- [ARCH-PICOO-TRANSPORT-001](../../architecture/0002-quic-transport-encapsulation-boundary.md)
- [ARCH-PICOO-PROTOCOL-001](../../architecture/0003-picoo-camera-protocol-boundary.md)
- [ARCH-PICOO-SESSION-001](../../architecture/0005-session-reconnect-jitter-bitrate-boundary.md)
- [ARCH-PICOO-UI-001](../../architecture/0009-desktop-gpui-mobile-native-ui-boundary.md)
- [ARCH-PICOO-UI-002](../../architecture/0010-cross-platform-design-system-boundary.md)

## 相关 Requirements

- 待分解：`REQ-PICOO-DISCOVERY-*`、`REQ-PICOO-TRANSPORT-*`、`REQ-PICOO-SESSION-*`
