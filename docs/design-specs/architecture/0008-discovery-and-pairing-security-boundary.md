# ARCH-PICO-DISCOVERY-001: 设备发现、配对与安全边界

Status: planned
Source: product PRD V1.0 / PUC-001 / PUC-002 / PUC-003 / PUC-007

## 背景

用户不应理解 IP、端口或公钥，但系统仍需在局域网内安全地发现 Receiver、建立 QUIC 连接并维护可信设备关系。企业网络可能屏蔽 mDNS，因此发现机制必须有兜底，且广播内容不得泄露敏感信息。

## 架构决策

### mDNS / DNS-SD 自动发现

Receiver 启动后广播服务类型：

```text
_picocam._udp.local
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

### 二维码兜底

桌面端在等待连接页显示二维码，内容包含：

- 局域网地址
- QUIC 端口
- Receiver ID
- 协议版本
- 公钥指纹
- 短期连接随机数

Sender 扫码后绕过 mDNS 直接连接。短期随机数过期后旧二维码不能无限期复用。

### 配对与公钥固定

首次连接时，双方显示由握手上下文派生的 **六位短认证码**。用户确认后保存：

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

## 约束

- Apple 平台必须提供 Local Network 用途说明，并声明 Bonjour 服务类型。
- Android 必须处理摄像头、网络与局域网发现相关权限；权限在操作时请求。
- 删除配对后必须重新确认，不能 silent trust。

## 相关 Use Case

- [PUC-001](../use-cases/product/puc-001-first-install-and-pairing.md)
- [PUC-002](../use-cases/product/puc-002-discover-and-connect-paired-receiver.md)
- [PUC-003](../use-cases/product/puc-003-qr-code-fallback-connection.md)
- [PUC-007](../use-cases/product/puc-007-manage-paired-devices.md)

## 相关 Architecture

- [ARCH-PICO-TRANSPORT-001](0002-quic-transport-encapsulation-boundary.md)
- [ARCH-PICO-PROTOCOL-001](0003-pico-camera-protocol-boundary.md)
- [ARCH-PICO-STACK-001](0001-rust-core-monorepo-boundary.md)

## 相关 Requirements

- 待分解：`REQ-PICO-DISCOVERY-*`、`REQ-PICO-PAIRING-*`、`REQ-PICO-PRIVACY-*`
