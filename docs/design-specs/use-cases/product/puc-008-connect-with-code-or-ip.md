# PUC-008：通过连接码授权或 IP 地址直连电脑

## 基本信息

- 类别：Product Connection and Pairing Fallback
- 参与者：会议与录制用户
- 目标：通过六位连接码授权首次配对，并在 mDNS 不可用时通过局域网 `IP:端口` 定位 Receiver

## 场景

Receiver 在等待连接页持续显示短期六位连接码和当前局域网 `IP:端口`。连接码是配对授权凭证，不是网络地址，也不承担 Endpoint 发现。

mDNS 正常时，Sender 从发现列表获得 Receiver Endpoint。用户选择尚未配对的电脑并输入其桌面端显示的连接码，Sender 建立 QUIC/TLS 连接后通过可靠控制 Stream 提交连接码。

mDNS 被企业网络、访客网络或路由器设置屏蔽时，用户先在 Sender 输入桌面端显示的 `IP:端口`。Sender 直接向该 Endpoint 建立 QUIC/TLS 连接，随后要求用户输入同一个六位连接码。Receiver 校验连接码并显示连接请求；用户确认后，双方固定公钥并建立可信设备关系。

## 可观察结果

- 桌面端等待连接页同时显示短期六位连接码与局域网 `IP:端口`，并支持刷新连接码和复制地址。
- 连接码只能提交给已经由 mDNS 或手动 IP 输入确定的 Receiver，不能被解释为全局地址查询码。
- mDNS 不可用时，用户可通过 `IP:端口` 到达 Receiver，并继续完成与自动发现路径一致的配对、视频传输和虚拟摄像头输出。
- 连接码过期、输入错误或尝试次数过多时，Receiver 拒绝配对且不泄露设备详细信息。
- 已配对设备仍需验证固定公钥；连接码不替代后续连接的身份校验。

## 边界

- 手动直连仅适用于同一局域网，不提供公网中继、账号查询或云端短码解析。
- 不提供二维码生成、二维码扫描、条码解析、URL 深链、NFC 或蓝牙辅助配对。
- 移动端不得为本流程引入 ZXing、ML Kit Barcode Scanning 或其他扫码 SDK；相机权限仅在正式进入视频采集时请求。
- Receiver 必须限制连接码尝试频率，并在成功使用、主动刷新或到期后使旧码失效。

## 相关 Architecture

- [ARCH-PICOO-DISCOVERY-001](../../architecture/0008-discovery-and-pairing-security-boundary.md)
- [ARCH-PICOO-TRANSPORT-001](../../architecture/0002-quic-transport-encapsulation-boundary.md)
- [ARCH-PICOO-UI-001](../../architecture/0009-desktop-gpui-mobile-native-ui-boundary.md)

## 相关 Requirements

- [REQ-PICOO-UI-0001](../../requirements/req-picoo-ui-0001-native-camera-and-desktop-gpui-acceptance.md)
- 待分解：`REQ-PICOO-DISCOVERY-*`、`REQ-PICOO-PAIRING-*`
