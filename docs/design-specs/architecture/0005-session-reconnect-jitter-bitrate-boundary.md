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

Receiver 不使用固定播放缓冲。每个 stream epoch 建立一条 Receiver 单调时钟到 Sender
媒体 PTS 的相对映射；映射只依赖到达时间与 PTS 的差值，不要求两端墙钟同步。播放控制器根据
近期完整 AU 的到达延迟变化分位数和平台解码耗时分位数计算 target：

- 启动 target：33 ms；
- 自适应 target：16–80 ms，网络恶化时快速增加，稳定后缓慢减小；
- 异常恢复 deadline：取 `2 × target + 一帧周期` 与 `RTT + 3 × 网络抖动 + 一帧周期` 较大者，
  再限制在 200–300 ms。

恢复 deadline 是不完整 AU 和完整 AU 本地排队的丢弃边界，不是健康播放 target，也不得被解释为
人眼无感延迟。它必须同时覆盖当前播放预算、一次 Receiver/OS 调度停顿和路径往返波动，同时受
300 ms 上限约束，避免过短固定值在真实 Wi-Fi 上把仍会及时到达的片段误判为永久丢失。

`target` 表示从基准网络到达到帧可供下游消费的总时序预算；Receiver 在扣除解码与渲染余量后
决定何时把完整 AU 交给 Decoder。Sender PTS 只用于帧顺序和相对节奏，不能与 Receiver 单调时钟
直接比较后判定一帧在到达前已经“过期”：采集、硬编延迟和两端时钟漂移都会形成同样的数值。
完整且尚未越过本地播放点的 AU 必须接受；恢复 deadline 只约束 Receiver 本地仍未重组完成的数据，或完整
AU 在 Receiver 队列中继续等待的时间。需要超过该 deadline 才能维持顺序时，必须丢弃并进入
预测链恢复，不能继续扩大缓冲。

完整帧进入抖动缓冲；QUIC Datagram 允许跨 AU 乱序，因此“看到更新 frame_id 的 START”不能作为旧 AU 丢失的证据。Receiver Reassembly Map 在处理每个入站片段前，按首片到达的自适应恢复 deadline 判定不完整帧，过期后建立单调丢弃边界，迟到尾片不得重建旧 AU；容量淘汰或已经完成的 AU 也保留有界 terminal tombstone。截止前完成且尚未越过播放点的 AU 按 PTS 排序进入抖动缓冲。若 Reassembly Map 中仍有更旧 frame_id 的未决 AU，播放控制器不得先输出更新 AU；由 FEC、完整重组或恢复 deadline 先确定旧 AU 结果。每个最多 64 个 Datagram 的入站批次处理后必须立即给播放队列一次调度机会，不能等待 transport event queue 全部变空；否则持续流会把线程调度边界误当成 jitter 容量溢出。Jitter 的数量上限还必须覆盖 30 FPS 下最大 300 ms 恢复 deadline、80 ms 播放 target 和一帧余量；V1 使用 16 个完整 AU 的硬上限，由 wall-clock deadline 先承担低延迟保护，数量上限只负责限制内存。只有完全不可观测的旧 AU，或已经越过截止边界的迟到数据，才允许形成播放缺口。若更旧 AU 在更新 PTS 已输出后才完成，则直接丢弃，迟到关键帧触发新 IDR。超过截止时间仍不完整的帧丢弃，不请求重传旧视频片段。

以下三个量必须独立：

- `jitter_buffer_target_ms`：控制器当前总播放 target；
- `jitter_buffer_actual_delay_ms`：本窗口已输出帧从首片到达至离开缓冲的平均实际停留时间；
- `jitter_buffer_occupancy_ms`：当前队列最新与最旧 PTS 的跨度。

occupancy 在 30 FPS 下天然以约 33.3 ms 离散变化，不能当成 target，也不能单独作为 ABR
升降依据。

### 丢包处理

Receiver 在判定 AU 不完整前，先按 PCP 的平衡 6+2 Reed-Solomon 组尝试就地恢复。每组最多
6 个数据片和 2 个校验片，可在不等待重传的情况下恢复任意 2 个缺失数据片；FEC 无法恢复时才进入
下列丢帧与参考链恢复语义。桌面诊断同时显示 FEC 已恢复 fragment 累计值和 FEC 后残余丢片，避免
把“已修复的损伤”误判为健康网络。

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

Android Sender 必须在每次编码帧回调后消费可靠控制流中的自动关键帧请求，并立即调用
MediaCodec 的 sync-frame 请求；该检查不依赖当前 AU 是否成功进入有界发送队列。500 ms 的会话维护
轮询只作为兜底，不得成为正常恢复路径的时延下界。

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
视频队列深度必须按实时预算限制，不能用数百个分片的队列把拥塞转化为持续延迟。产品基线中
Quinn Datagram receive buffer 为 2 MiB，send buffer 仅为 256 KiB；send buffer 待发字节必须
通过 `SenderStats` 可观测，并参与 ABR，不能让几秒旧视频隐藏在传输库内部。

### 自适应码率

Receiver 每秒向 Sender 发送 `ReceiverStats`：

`RTT`、`packet_loss`、`jitter`、`reassembly_drop`、`decoder_drop`、`frame_age`、`receive_bitrate`、
`jitter_buffer_target`、`jitter_buffer_actual_delay`、`jitter_buffer_occupancy`

Sender 每秒向 Receiver 发送 `SenderStats`：

`access_units`、`submitted_datagrams`、`video_queue_age`、`video_dropped_access_units`、
`video_buffered_bytes`、`quic_sent_packets`、`quic_lost_packets`

`jitter` 表示完整 AU 的到达间隔相对 PTS 间隔的 EWMA 变化量，不依赖两端墙钟同步；
三个 jitter buffer 指标按上一节定义。它们与网络 `jitter` 不得复用同一数值，桌面端也不得把
occupancy 标成网络抖动或实际播放延迟。

其中 `packet_loss` 描述 Receiver 能从已决 AU 观察到的缺失视频 fragment 比例。完整 AU 在完成时、
不完整 AU 在超时或淘汰时整体归入一个统计窗口；分子是这些 AU 中确认缺失的 fragment 数，分母是
这些 AU 的期望 fragment 总数（收到 + 确认缺失）。不得把同一 AU 的收到片与缺片拆到两个窗口，
也不得把主动 recovery/epoch 清空的仍在途 AU 计作网络丢包。没有任何 fragment 到达的完整 AU
在 PCP 增加独立
packet sequence 前无法由该指标观察，因此该指标不是 transport-wide 精确丢包率。QUIC 路径的
`lost_packets / sent_packets` 只描述本端发出的包；在 Receiver 端它主要是控制流，不能作为
Android 视频丢包率反馈给 ABR。未建立双端时钟同步前，桌面只能把 QUIC RTT 命名为链路延迟；
Receiver 解码完成后的本地 `frame_age` 不得与 RTT 相加并标成端到端延迟。

控制策略：

- 丢包 > 3%，或帧龄持续增加，或发送队列持续堆积 → 码率降低 20%
- 丢包 < 1%、重组/解码零丢帧、frame age 与 Sender 发送队列不增长，且持续 5 秒有余量
  → 码率提高 10%
- 降级顺序：降低码率 → 降低图像复杂度 → 1080p 降至 720p，再降至 480p

不能通过扩大缓冲区掩盖带宽不足。ABR 不读取瞬时 occupancy 作为健康门槛；target 达到上限是
诊断信号，但只有媒体丢帧、队列/延迟趋势和有效吞吐等证据才能触发质量调整。

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
