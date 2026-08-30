# PUC-008：通过配对短码核对或 IP 地址直连电脑

## 基本信息

- 类别：Product Connection and Pairing Fallback
- 参与者：会议与录制用户
- 目标：通过六位配对短码人工核对首次连接，并在 mDNS 不可用时通过局域网 `IP:端口` 定位 Receiver

## 场景

Receiver 在等待连接页持续显示当前局域网 `IP:端口`。六位配对短码只在未配对 Sender 建立加密连接后出现，是本次握手的人工核对值，不是网络地址，也不承担 Endpoint 发现。

mDNS 正常时，Sender 从发现列表获得 Receiver Endpoint。用户选择尚未配对的电脑并建立 QUIC/TLS 连接后，两端显示由同一挑战派生的六位配对短码。用户确认数字一致，双方才提交本次配对确认。

mDNS 被企业网络、访客网络或路由器设置屏蔽时，用户先在 Sender 输入桌面端显示的 `IP:端口`。Sender 直接向该 Endpoint 建立 QUIC/TLS 连接，随后进入与自动发现相同的双端短码核对流程。两端分别确认一致后，双方固定公钥并建立可信设备关系。

## 可观察结果

- 桌面端等待连接页始终显示局域网 `IP:端口`，收到未配对请求后才显示六位配对短码；地址支持复制。
- 配对短码由已建立的加密连接派生并通过控制 Stream 传递，不能被用户输入或解释为全局地址查询码。
- mDNS 不可用时，用户可通过 `IP:端口` 到达 Receiver，并继续完成与自动发现路径一致的配对、视频传输和虚拟摄像头输出。
- 配对挑战过期、两端任一方拒绝或连接中断时，Receiver 拒绝配对且不泄露设备详细信息。
- 已配对设备仍需验证固定公钥；配对短码不替代后续连接的身份校验。

## 边界

- 手动直连仅适用于同一局域网，不提供公网中继、账号查询或云端短码解析。
- 不提供二维码生成、二维码扫描、条码解析、URL 深链、NFC 或蓝牙辅助配对。
- 移动端不得为本流程引入 ZXing、ML Kit Barcode Scanning 或其他扫码 SDK；相机权限仅在正式进入视频采集时请求。
- Receiver 必须将挑战绑定到单次连接，并在成功、拒绝、连接中断或到期后使配对短码失效。

## 相关 Architecture

- [ARCH-PICOO-DISCOVERY-001](../../architecture/0008-discovery-and-pairing-security-boundary.md)
- [ARCH-PICOO-TRANSPORT-001](../../architecture/0002-quic-transport-encapsulation-boundary.md)
- [ARCH-PICOO-UI-001](../../architecture/0009-desktop-gpui-mobile-native-ui-boundary.md)
- [ARCH-PICOO-UI-002](../../architecture/0010-cross-platform-design-system-boundary.md)

## 相关 Requirements

- [REQ-PICOO-UI-0001](../../requirements/req-picoo-ui-0001-native-camera-and-desktop-gpui-acceptance.md)
- 待分解：`REQ-PICOO-DISCOVERY-*`、`REQ-PICOO-PAIRING-*`
