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

完整帧进入抖动缓冲；QUIC Datagram 允许跨 AU 乱序，因此“看到更新 frame_id 的 START”不能作为旧 AU 丢失的证据。Receiver Reassembly Map 在处理每个入站片段前，按首片到达的 120 ms 墙钟截止时间判定不完整帧，过期后建立单调丢弃边界，迟到尾片不得重建旧 AU；容量淘汰或已经完成的 AU 也保留有界 terminal tombstone。截止前完成且尚未越过播放点的 AU 按 PTS 排序进入抖动缓冲；若更旧 AU 在更新 PTS 已输出后才完成，则直接丢弃，迟到关键帧触发新 IDR。超过截止时间仍不完整的帧丢弃，不请求重传旧视频片段。

### 丢包处理

```text
完整帧                 → 解码
不完整 DISCARDABLE 帧  → 丢弃，不破坏参考链
不完整非 DISCARDABLE 帧 → 丢弃、停止解码后续 delta 并请求 IDR
解码器报错             → 清空当前 epoch 缓冲并请求 IDR
```

Receiver 必须把预测链恢复建模为显式状态，而不是只在错误分支零散发送
`RequestKeyframe`：

```text
Healthy
  → 不完整/迟到的非 DISCARDABLE AU、Decoder error、Decoder reset、stream_epoch 变化
AwaitingRefresh
  → 清空当前 reassembly 与 jitter 中尚未播放的 AU
  → reset Decoder，丢弃旧参考链和延迟输出
  → 合并并限频发送 RequestKeyframe
  → 丢弃普通 delta AU，不再反复喂给已知损坏的预测链
  → 完整且匹配当前 StreamConfig/stream_epoch 的 IDR 被 Decoder 接受
Healthy
```

`flush` 与 `reset` 语义必须分离：`flush` 可以排空 Decoder 已产生的延迟输出，
`reset` 必须丢弃所有参考状态并准备从新 IDR 开始。平台 Decoder 不得用“排空残帧”
冒充恢复重置。自动关键帧请求需要合并和限频；限频期间仍保持 `AwaitingRefresh`，
不能因为请求被抑制而恢复输入 delta AU。用户主动“画面修复”可以强制发出一次新请求。

### 队列上限

以下队列必须有固定上限，满时优先丢弃最旧的非关键帧、已过播放期限的帧、依赖已丢失参考帧的帧：

- Sender Access Unit Queue
- QUIC Datagram Queue
- Receiver Reassembly Map
- Jitter Buffer
- Decoded Frame Queue
- Shared Frame Ring

Sender 的 Access Unit 是视频发送队列的最小原子项，不是单个 Datagram 分片。提交前必须完成整帧
分片，并确认整组能够进入应用队列；Quinn send buffer 空间不足时丢弃完整非关键 AU，不能先发送
头部再丢尾部。关键帧可以替换陈旧 delta 队列以恢复解码，但关键帧自身也必须完整进入发送缓冲。
视频队列深度必须按实时预算限制，不能用数百个分片的队列把拥塞转化为持续延迟。

### 自适应码率

Receiver 每秒向 Sender 发送 `ReceiverStats`：

`RTT`、`packet_loss`、`jitter`、`reassembly_drop`、`decoder_drop`、`frame_age`、`receive_bitrate`、`jitter_buffer_depth`

其中 `packet_loss` 描述 Receiver 能从已决 AU 观察到的缺失视频 fragment 比例。完整 AU 在完成时、
不完整 AU 在超时或淘汰时整体归入一个统计窗口；分子是这些 AU 中确认缺失的 fragment 数，分母是
这些 AU 的期望 fragment 总数（收到 + 确认缺失）。不得把同一 AU 的收到片与缺片拆到两个窗口，
也不得把主动 recovery/epoch 清空的仍在途 AU 计作网络丢包。没有任何 fragment 到达的完整 AU
在 PCP/2 增加独立
packet sequence 前无法由该指标观察，因此该指标不是 transport-wide 精确丢包率。QUIC 路径的
`lost_packets / sent_packets` 只描述本端发出的包；在 Receiver 端它主要是控制流，不能作为
Android 视频丢包率反馈给 ABR。未建立双端时钟同步前，桌面只能把 QUIC RTT 命名为链路延迟；
Receiver 解码完成后的本地 `frame_age` 不得与 RTT 相加并标成端到端延迟。

控制策略：

- 丢包 > 3%，或帧龄持续增加，或发送队列持续堆积 → 码率降低 20%
- 丢包 < 1%，且缓冲稳定，且持续 5 秒有余量 → 码率提高 10%
- 降级顺序：降低码率 → 降低图像复杂度 → 1080p 降至 720p，再降至 480p

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
