# ARCH-PICOO-SESSION-001: 会话状态、重连、抖动缓冲与码率控制边界

Status: planned
Source: product PRD V1.0 / PUC-006

## 背景

无线局域网存在丢包、乱序、抖动和短暂断网。Picoo Camera 的目标是 **实时性优先**：可以丢弃过期视频帧，但不能因重传旧帧或无限缓冲导致延迟持续累积。

## 架构决策

### 会话状态

Receiver 与 Sender UI 必须能反映以下状态（至少）：

`Discovering`、`Pairing`、`Connecting`、`Negotiating`、`Streaming`、`Reconnecting`、`Disconnected`、`Permission Required`、`Virtual Camera Unavailable`、`Network Unstable`。

会话状态由 Rust Session 层维护，通过事件更新 UI；UI 不直接持有 QUIC Connection 或解码器。

### 抖动缓冲

目标：

- 目标缓冲：50 ms
- 正常范围：30–80 ms
- 最大缓冲：120 ms

完整帧进入抖动缓冲；超过截止时间仍不完整的帧丢弃，不请求重传旧视频片段。

### 丢包处理

```text
完整帧                 → 解码
不完整非关键帧         → 丢弃
不完整关键帧           → 丢弃并请求 IDR
解码器报错             → 清空当前 epoch 缓冲并请求 IDR
```

### 队列上限

以下队列必须有固定上限，满时优先丢弃最旧的非关键帧、已过播放期限的帧、依赖已丢失参考帧的帧：

- Sender Access Unit Queue
- QUIC Datagram Queue
- Receiver Reassembly Map
- Jitter Buffer
- Decoded Frame Queue
- Shared Frame Ring

### 自适应码率

Receiver 每秒向 Sender 发送 `ReceiverStats`：

`RTT`、`packet_loss`、`jitter`、`reassembly_drop`、`decoder_drop`、`frame_age`、`receive_bitrate`、`jitter_buffer_depth`

控制策略：

- 丢包 > 3%，或帧龄持续增加，或发送队列持续堆积 → 码率降低 20%
- 丢包 < 1%，且缓冲稳定，且持续 5 秒有余量 → 码率提高 10%
- 降级顺序：降低码率 → 降低图像复杂度 → 1080p 降至 720p

不能通过扩大缓冲区掩盖带宽不足。

### 重连恢复

重连成功后顺序：

1. 验证固定公钥；
2. 重新协商能力；
3. 恢复上次分辨率与镜像设置；
4. 请求新的 SPS/PPS；
5. 请求 IDR；
6. 恢复虚拟摄像头画面。

## 不采用的方案

### 可靠传输视频片段

不采用。视频走 QUIC Datagram，丢包时等待重传会累积延迟。

### 无上限重组 Map

不采用。必须防止恶意或异常 `frame_id` / `fragment_count` 导致内存增长。

### UI 线程直接 poll QUIC

不采用。Session 与 Transport 在 Rust Core 后台事件循环中运行，UI 只观察状态快照。

## 约束

- 网络丢包 5% 时仍保持可用。
- 恢复后延迟不应累积到 1 秒以上。
- 连续传输 2 小时无崩溃；内存无持续增长。
- 旧 epoch 帧不得污染新会话。

## 相关 Use Case

- [PUC-002](../use-cases/product/puc-002-discover-and-connect-paired-receiver.md)
- [PUC-006](../use-cases/product/puc-006-auto-reconnect-after-network-interruption.md)

## 相关 Architecture

- [ARCH-PICOO-TRANSPORT-001](0002-quic-transport-encapsulation-boundary.md)
- [ARCH-PICOO-PROTOCOL-001](0003-picoo-camera-protocol-boundary.md)
- [ARCH-PICOO-FRAME-001](0006-framehub-shared-frame-ring-boundary.md)

## 相关 Requirements

- 待分解：`REQ-PICOO-SESSION-*`
