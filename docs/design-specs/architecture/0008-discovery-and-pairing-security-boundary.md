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
- `quic_port`
- `pairing_state`
- `public_key_fingerprint_prefix`

不得包含用户身份、视频状态或完整密钥。

Sender 浏览该服务：

- Android：NSD / DNS-SD
- iOS：Bonjour
- Desktop：Rust mDNS/DNS-SD Adapter

### 配对短码核对与手动 IP 直连

Receiver 在等待连接页始终显示当前局域网 `IP:端口`。未配对 Sender 通过 mDNS 或手动地址建立
QUIC/TLS 连接后，双方才使用 OS CSPRNG 各自生成随机 nonce，并交换静态 Ed25519 公钥。

- Sender/Receiver 随机 nonce；
- 双方静态公钥；
- 当前 TLS exporter channel binding；
- 由以上完整 transcript 派生的六位 SAS。

配对短码只负责人工核对本次首次连接，不负责发现或解析 Receiver Endpoint。mDNS 正常时，Sender 从服务发现结果获得 Endpoint；mDNS 不可用时，用户必须输入 `IP:端口`，Sender 才能绕过服务发现直接连接。

SAS 必须由两端独立计算，禁止 Receiver 计算后把短码当作普通字段告知 Sender 显示。用户必须在
手机端和桌面端分别确认数字一致，确认先后顺序不得影响结果。双向确认后，双方分别使用 Ed25519
私钥对带 domain separation 的完整 transcript 签名，再执行
`PairingApproval → PairingCommit → PairingComplete` 持久化事务。所有消息都位于 ControlEnvelope，
绑定 connection generation、message ID、双方 nonce、公钥、设备 ID 与 TLS exporter。旧连接消息
不得完成新挑战；任一端持久化或发送失败不得形成自动推流的单边信任。

### 配对与公钥固定

每台设备持有一把 Ed25519 静态身份密钥，由 OS CSPRNG 生成；公钥派生稳定 `device_id`，私钥从不
跨平台边界或进入日志。Receiver 校验 Sender 针对本次 transcript 的真实私钥签名后保存：

- `device_id`
- `device_name`
- `public_key`
- `certificate_fingerprint`
- `paired_at`
- `last_connected_at`

后续连接必须验证固定公钥；同名但公钥不同的设备必须拒绝自动连接。

后续重连还必须执行随机 challenge-response 签名，证明当前连接方持有已固定公钥对应的私钥；
只比较 Hello 中重复发送的 public-key bytes 不构成认证。通过前不得接收媒体或特权控制。

### 身份存储与损坏处理

- Android：Android Keystore；iOS/macOS：Keychain；Windows：CNG/DPAPI；Rust/Linux 测试：权限受限、
  原子替换的文件 adapter。
- 身份加载、解密或一致性校验失败必须 fail closed，并向用户显示“身份损坏，可修复或重置配对”。
  禁止静默生成 ephemeral identity 后继续启动。
- 旧伪公钥和旧 trust store 不具备可验证私钥证明，不做静默迁移。切换到本安全契约时全部失效，
  用户重新配对。
- QUIC 端点临时自签名证书可以作为 TLS 加密载体，但应用身份认证必须由上述 Ed25519 transcript
  和 exporter binding 完成；`SkipServerVerification` 不能被描述为已验证设备身份。

### 身份重建与同名历史配对

`device_id + public_key` 是信任关系的身份边界，`device_name` 只是用户可见标签。系统不得使用设备名称、型号、Android ID、IMEI 或 MAC 地址自动合并身份或转移信任。

Sender 应用数据重建并生成新身份后，必须作为未配对设备完成完整的双端短码核对。只有此前未知的新身份已经提交到可信存储后，Receiver 才生成一次性“同名历史清理”决策；可信身份的自动重连不得从活动连接状态反向生成该决策。决策冻结 revision、候选 ID、公钥指纹与最近连接时间，并与新可信身份在同一次原子持久化中落盘，进程重启后恢复原快照，桌面端逐条展示指纹。用户确认后，Rust Session 在同一事务中保留当前身份并仅撤销该快照明确列出的同名身份；候选被修改或移除时拒绝过期决策，不得重新按名称扩大删除集合。持久化失败时恢复事务前的内存可信集合并保留决策以便重试。用户选择保留时同样持久化消费本次决策，各身份继续作为独立信任关系存在。

同名匹配只用于缩小用户清理历史凭据的范围，不证明两个身份来自同一物理设备。替换操作撤销旧信任，不向当前身份授予任何尚未通过短码配对获得的权限。

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
