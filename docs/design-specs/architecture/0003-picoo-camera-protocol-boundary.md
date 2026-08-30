# ARCH-PICOO-PROTOCOL-001: Picoo Camera Protocol (PCP/1) 边界

Status: planned
Source: product PRD V1.0 / PUC-002 / PUC-005 / PUC-006

## 背景

Sender 与 Receiver 需要一套版本可协商、可测试、可 fuzz 的应用协议，分别承载会话建立、配对、能力交换、流控制、统计反馈和视频分片语义。若将控制消息与视频片段都放入 Protobuf 或 JSON，会给热路径带来不必要的编解码开销。

## 架构决策

协议名称：**Picoo Camera Protocol**，版本 **PCP/1**。

### 控制平面

控制消息使用 Protobuf 定义于 `proto/picoo_camera.proto`，由 Rust `prost` 生成类型，经 QUIC Reliable Stream 传输。

主要消息：

- `ClientHello` / `ServerHello`
- `Capabilities`
- `PairingChallenge` / `PairingConfirm`
- `StartStream` / `StopStream`
- `StreamConfig`
- `CameraCommand` / `EncoderCommand`
- `ReceiverStats`
- `RequestKeyframe`
- `Heartbeat`
- `SessionError`

### 视频平面

视频片段使用固定二进制 `VideoPacket` 头，经 QUIC Datagram 传输：

```text
VideoPacket {
  version: u8
  flags: u8
  stream_epoch: u32
  frame_id: u64
  pts_us: u64
  fragment_index: u16
  fragment_count: u16
  payload: bytes
}
```

Flags 包括：`KEYFRAME`、`START_OF_ACCESS_UNIT`、`END_OF_ACCESS_UNIT`、`DISCARDABLE`。

单个载荷约 **1150 字节**，控制在路径 MTU 内，避免 IP 分片。

单个 Access Unit 最多 1024 个 Datagram，即当前头部与 MTU 下约 1.1 MiB。Sender 在
入队前拒绝更大的 AU；Receiver 最多并行保留 8 个不完整 AU，因此真实 480p/720p/1080p
IDR 不会被早期 16 片测试上限误丢弃，同时异常 `fragment_count` 仍有明确内存边界。

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

- 协议版本协商失败时必须 fail fast，不能静默降级到未定义行为。
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
