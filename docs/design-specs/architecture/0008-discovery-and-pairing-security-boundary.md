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

### 连接码授权与手动 IP 直连

Receiver 在等待连接页显示：

- 短期六位连接码；
- 当前局域网 `IP:端口`。

连接码只负责首次配对授权，不负责发现或解析 Receiver Endpoint。mDNS 正常时，Sender 从服务发现结果获得 Endpoint；mDNS 不可用时，用户必须输入 `IP:端口`，Sender 才能绕过服务发现直接连接。

Sender 建立 QUIC/TLS 连接后，通过可靠控制 Stream 提交用户输入的连接码。Receiver 必须限制失败尝试频率；连接码成功使用、主动刷新或到期后立即失效。桌面端仍需向用户显示连接请求，未确认前不得建立信任关系。

### 配对与公钥固定

首次连接时，Sender 提交 Receiver 显示的 **短期六位连接码**，桌面端用户确认连接请求后保存：

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

手机端与桌面端都必须支持删除已配对设备。删除后下一次连接必须重新配对。

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

不采用。连接所需信息已由 mDNS、手动 `IP:端口` 与六位连接码覆盖；二维码会额外引入生成、解析、扫码 UI 与移动端扫码 SDK，增加包体积和权限路径。

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
