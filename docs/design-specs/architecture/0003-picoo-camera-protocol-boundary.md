# ARCH-PICOO-PROTOCOL-001: Picoo Camera Protocol 边界

Status: planned
Source: product PRD V1.0 / PUC-002 / PUC-005 / PUC-006

## 背景

Sender 与 Receiver 需要一套可测试、可 fuzz 的应用协议，分别承载会话建立、配对、能力交换、流控制、统计反馈和视频分片语义。若将控制消息与视频片段都放入 Protobuf 或 JSON，会给热路径带来不必要的编解码开销。

## 架构决策

协议名称：**Picoo Camera Protocol（PCP）**。仓库同时控制 Sender 与 Receiver，且允许使旧状态
失效，因此协议不维护数字版本、双解析器或兼容窗口。QUIC ALPN 固定为 `picoocam`；不符合当前
ALPN、Envelope、身份或信任存储契约的旧构建直接失败并要求重新安装/配对。

### 控制平面

控制消息使用 Protobuf 定义于 `proto/picoo_camera.proto`，由 Rust `prost` 生成类型，经 QUIC
Reliable Stream 传输。每个控制帧只允许解码一次 `ControlEnvelope`：

```protobuf
message ControlEnvelope {
  uint64 message_id = 1;
  uint64 connection_generation = 2;
  oneof payload { /* typed control messages */ }
}
```

`message_id` 在连接内严格单调，`connection_generation` 隔离旧连接事件。只有出现需要异步关联的
真实请求/响应语义时才为对应 payload 增加显式 transaction ID，不在通用 Envelope 中预留闲置字段。
接收方先辨识 oneof payload，再由当前 Trust/Stream 状态判断该消息是否允许。禁止逐个尝试解码为
不同 protobuf 类型，禁止 magic discriminator，也禁止裸 payload 绕过 Envelope。

双方都必须执行阶段门禁。Sender 只有在已固定 Receiver 公钥与 ServerHello 一致时才接受
`pairing_required=false`，不能把对端自行声明的“不需要配对”当作认证结果；CameraCommand、
EncoderCommand、Stats、Capabilities 和 StreamConfig 只能在相应的已认证阶段处理。配对挑战超时
关闭该连接，非活动 Session 的控制、媒体和断线事件直接丢弃。

主要消息：

- `ClientHello` / `ServerHello`
- `Capabilities`
- `PairingChallenge` / `PairingConfirm` / `PairingApproval` / `PairingCommit` / `PairingComplete`
- `StartStream` / `StopStream`
- `StreamConfig`
- `CameraCommand` / `EncoderCommand`；其中交互式「切换镜头」使用 `SWITCH_CAMERA`，由持有实际镜头状态的 Sender 决定目标朝向，`SWITCH_FRONT` / `SWITCH_BACK` 只表达调用方明确指定朝向的命令
- `ReceiverStats`
- `SenderStats`
- `RequestKeyframe`
- `Heartbeat`
- `SessionError`

### 视频平面

视频片段使用固定二进制 `VideoPacket` 头，经 QUIC Datagram 传输：

```text
VideoPacket {
  flags: u8
  stream_epoch: u32
  frame_id: u64
  pts_us: u64
  fragment_index: u16
  fragment_count: u16
  payload: bytes
}
```

固定头为 25 字节。Flags 包括：`KEYFRAME`、`START_OF_ACCESS_UNIT`、`END_OF_ACCESS_UNIT`、`DISCARDABLE`、
`FEC_PARITY`。

单个载荷约 **1150 字节**，控制在路径 MTU 内，避免 IP 分片。

单个 Access Unit 最多 1024 个系统数据片，即当前头部与 MTU 下约 1.1 MiB 原始 AU；额外校验片
不计入 `fragment_count`。Sender 在
入队前拒绝更大的 AU；Receiver 最多并行保留 8 个不完整 AU，因此真实 480p/720p/1080p
IDR 不会被早期 16 片测试上限误丢弃，同时异常 `fragment_count` 仍有明确内存边界。

每个 AU 的系统数据片按最多 6 片组成一个平衡组；每组额外生成 2 个 Reed-Solomon 校验片。Sender
按 shard position 在各组间轮转数据片，然后才发送校验片：这样连续丢包会分散到不同恢复组，而
健康路径能先用原始数据完成 AU，不会把正常在途尾片误当成丢失并消耗重建 CPU。
校验片复用 `fragment_index` 表示组起点，payload 前缀携带校验片序号与组内最后一个数据片长度。
Receiver 在 deadline 前收到足够片时，最多恢复组内任意 2 个缺片，不等待 RTT、不重传旧视频。
无丢片时数据片优先完成 AU，迟到校验片由 terminal tombstone 忽略；超过恢复能力仍丢弃整帧。

`SenderStats` 每秒通过可靠控制流上报完整 AU 提交数、Datagram 提交数、Sender 队列龄与整帧丢弃、
Quinn 待发字节和 Sender 端 QUIC sent/lost。Receiver 不得用自身控制流方向的 QUIC loss 推断
Android 视频方向的真实拥塞。

### StreamConfig 与 stream_epoch

`StreamConfig` 携带 `codec`、`profile`、`level`、`width`、`height`、`fps`、`bitrate`、`rotation`、`mirrored`、`color_range`、`sps`、`pps`、`stream_epoch`。

`stream_epoch` 在摄像头切换、分辨率变化、编码器重建、连接恢复或编码参数重大变化时递增。Receiver 按 `stream_epoch + frame_id` 重组帧，**不得**将不同 epoch 的片段组成同一帧。

### 关键帧策略

以下情况 Sender 必须请求或发出 IDR：

- 建立新连接；
- 解码器重新初始化；
- 丢失连续关键帧；
- 分辨率改变；
- 切换摄像头；
- Receiver 明确请求；
- 连续帧无法恢复。

未收到 SPS/PPS 时，Receiver 不得向解码器提交普通帧。

## 不采用的方案

### 每个视频片段使用 Protobuf

不采用。热路径应使用固定二进制头，避免 per-fragment 分配与解析成本。

### JSON / WebSocket 控制通道

不采用。与 QUIC Stream 重复，且不利于四端 Rust 统一实现与加密一致性。

### HEVC / AV1 作为第一版 codec

不采用。第一版固定 H.264/AVC Main（或 Baseline 回退），8-bit 4:2:0 SDR Progressive，无 B 帧。

## 约束

- 旧协议、旧 trust store 和裸控制 payload 必须 fail fast，不能静默降级或尝试猜测消息类型。
- 控制消息 Parser 与 VideoPacket Parser 必须可 fuzz。
- 配对完成前，控制面不得接受 StartStream 或 CameraCommand 中的敏感操作。
- 明文控制消息与视频不得在链路上裸传；QUIC/TLS 负责传输层保护。

## 相关 Use Case

- [PUC-002](../use-cases/product/puc-002-discover-and-connect-paired-receiver.md)
- [PUC-005](../use-cases/product/puc-005-adjust-camera-during-streaming.md)
- [PUC-006](../use-cases/product/puc-006-auto-reconnect-after-network-interruption.md)

## 相关 Architecture

- [ARCH-PICOO-TRANSPORT-001](0002-quic-transport-encapsulation-boundary.md)
- [ARCH-PICOO-MEDIA-001](0004-cross-platform-media-pipeline-boundary.md)
- [ARCH-PICOO-SESSION-001](0005-session-reconnect-jitter-bitrate-boundary.md)

## 相关 Requirements

- 待分解：`REQ-PICOO-PROTOCOL-*`
