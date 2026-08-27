# PUC-003：通过二维码连接电脑

## 基本信息

- 类别：Product Connection Fallback
- 参与者：会议与录制用户
- 目标：在 mDNS 被企业网络、访客网络或客户端隔离开关屏蔽时，通过扫描二维码直接建立 QUIC 连接

## 场景

用户打开桌面端 Receiver，但手机端无法通过自动发现看到任何电脑。常见原因包括企业 Wi-Fi 阻止 mDNS、访客网络客户端隔离、路由器未转发多播或 DNS-SD 记录被过滤。

桌面端在等待连接页提供 `Show QR Code` 入口。二维码包含局域网地址、QUIC 端口、Receiver ID、协议版本、公钥指纹和短期连接随机数。用户使用手机端 `Scan QR Code` 功能扫码后，Sender 绕过 mDNS 直接向目标 Endpoint 发起 QUIC 连接。

若该 Receiver 尚未配对，扫码连接仍进入与 [PUC-001](puc-001-first-install-and-pairing.md) 相同的六位短码确认流程；若已配对，则验证固定公钥后直接恢复会话。

## 可观察结果

- 桌面端能在等待连接页生成并显示有效二维码；二维码内容足以让 Sender 建立 QUIC 连接并完成能力协商。
- 手机端扫码后能显示目标电脑名称，并进入配对或已配对连接流程。
- 二维码连接成功后，视频传输、预览、虚拟摄像头输出和连接质量显示与 mDNS 发现路径一致。
- 短期连接随机数过期后，旧二维码不能无限期复用建立新会话。

## 边界

- 二维码仍要求手机与电脑处于同一局域网；不能用于跨公网远程连接。
- 本 Use Case 不提供 URL 深链、NFC 或蓝牙辅助配对。
- 二维码内容不应包含完整私钥或长期凭证；仅携带建立连接和验证身份所需的公开信息。

## 相关 Architecture

- [ARCH-PICOO-DISCOVERY-001](../../architecture/0008-discovery-and-pairing-security-boundary.md)
- [ARCH-PICOO-TRANSPORT-001](../../architecture/0002-quic-transport-encapsulation-boundary.md)

## 相关 Requirements

- 待分解：`REQ-PICOO-DISCOVERY-*`
