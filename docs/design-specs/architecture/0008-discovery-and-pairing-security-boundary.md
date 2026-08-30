# ARCH-PICOO-DISCOVERY-001: 设备发现、配对与安全边界

Status: planned
Source: product PRD V1.0 / PUC-001 / PUC-002 / PUC-007 / PUC-008

## 背景

用户通常不应理解 IP、端口或公钥，但系统仍需在局域网内安全地发现 Receiver、建立 QUIC 连接并维护可信设备关系。企业网络可能屏蔽 mDNS，因此系统提供手动 IP 直连作为显式兜底，且广播内容不得泄露敏感信息。

## 架构决策

### mDNS / DNS-SD 自动发现

Receiver 启动后广播服务类型：

```text
_picoocam._udp.local
```

广播记录 **只允许** 包含：

- `receiver_id`
- `display_name`
- `protocol_version`
- `quic_port`
- `pairing_state`
- `public_key_fingerprint_prefix`

不得包含用户身份、视频状态或完整密钥。

Sender 浏览该服务：

- Android：NSD / DNS-SD
- iOS：Bonjour
- Desktop：Rust mDNS/DNS-SD Adapter

### 配对短码核对与手动 IP 直连

Receiver 在等待连接页始终显示当前局域网 `IP:端口`。未配对 Sender 通过 mDNS 或手动地址建立 QUIC/TLS 连接后，Receiver 才针对本次握手生成：

- 随机挑战 nonce；
- 由挑战、Receiver ID 与 Sender ID 派生的六位配对短码。

配对短码只负责人工核对本次首次连接，不负责发现或解析 Receiver Endpoint。mDNS 正常时，Sender 从服务发现结果获得 Endpoint；mDNS 不可用时，用户必须输入 `IP:端口`，Sender 才能绕过服务发现直接连接。

Receiver 通过可靠控制 Stream 将挑战和配对短码发给 Sender，两端同时显示同一短码。用户必须在手机端和桌面端分别确认数字一致；任一端拒绝、挑战到期或连接中断都应终止本次配对。短码不作为密码提交，因此不存在输入错误与尝试次数模型；它必须绑定本次握手，不能跨连接复用。未完成双向确认前不得建立信任关系。

### 配对与公钥固定

首次连接时，两端核对相同的 **六位配对短码** 并分别确认。Receiver 校验 Sender 针对本次挑战发送的 `PairingConfirm` 后保存：

- `device_id`
- `device_name`
- `public_key`
- `certificate_fingerprint`
- `paired_at`
- `last_connected_at`

后续连接必须验证固定公钥；同名但公钥不同的设备必须拒绝自动连接。

未完成配对的设备：

- 不得接收视频；
- 不得切换摄像头；
- 不得读取设备详细信息；
- 不得修改码率或分辨率；
- 不得触发虚拟摄像头输出。

### 撤销配对

手机端与桌面端都必须支持删除已配对设备。删除必须由持有活动信任状态的 Rust
Session 完成内存更新与持久化；持久化失败时回滚内存信任状态，平台层不得自行拼接
“删除、保存、重新挂载”形成可部分成功的事务。删除成功后下一次连接必须重新配对。

### 隐私

- 不向公网发送视频或设备信息。
- 不需要登录或云端连接。
- 抓包不能看到明文控制消息或视频（QUIC/TLS 保护）。
- 日志不得包含视频数据；IP、设备名和公钥指纹应支持脱敏。

## 不采用的方案

### mDNS 广播完整证书或会话密钥

不采用。广播只携带指纹前缀与连接所需公开信息。

### 仅依赖 IP 直连、无配对确认

不采用。未配对隔离是安全与隐私基线。

### 云端账号同步配对关系

不采用。第一版本地-only。

### 二维码生成与扫码连接

不采用。连接所需信息已由 mDNS、手动 `IP:端口` 与六位配对短码核对覆盖；二维码会额外引入生成、解析、扫码 UI 与移动端扫码 SDK，增加包体积和权限路径。

## 约束

- Apple 平台必须提供 Local Network 用途说明，并声明 Bonjour 服务类型。
- Android 必须处理摄像头、网络与局域网发现相关权限；权限在操作时请求。不得为连接流程引入扫码 SDK 或提前请求相机权限。
- 删除配对后必须重新确认，不能 silent trust。

## 相关 Use Case

- [PUC-001](../use-cases/product/puc-001-first-install-and-pairing.md)
- [PUC-002](../use-cases/product/puc-002-discover-and-connect-paired-receiver.md)
- [PUC-007](../use-cases/product/puc-007-manage-paired-devices.md)
- [PUC-008](../use-cases/product/puc-008-connect-with-code-or-ip.md)

## 相关 Architecture

- [ARCH-PICOO-TRANSPORT-001](0002-quic-transport-encapsulation-boundary.md)
- [ARCH-PICOO-PROTOCOL-001](0003-picoo-camera-protocol-boundary.md)
- [ARCH-PICOO-STACK-001](0001-rust-core-monorepo-boundary.md)

## 相关 Requirements

- 待分解：`REQ-PICOO-DISCOVERY-*`、`REQ-PICOO-PAIRING-*`、`REQ-PICOO-PRIVACY-*`
