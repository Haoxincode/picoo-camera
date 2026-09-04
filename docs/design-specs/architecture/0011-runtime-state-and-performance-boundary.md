# ARCH-PICOO-RUNTIME-001：显式状态、媒体所有权与性能边界

Status: planned
Source: ARCH-PICOO-SESSION-001 / ARCH-PICOO-MEDIA-001 / ARCH-PICOO-FRAME-001 / ARCH-PICOO-VCAM-001

## 背景

Picoo Camera 的 H.264 + QUIC Datagram 数据面已经具备整 AU 丢弃、有界队列、IDR 恢复、
自适应抖动缓冲、单次解码、latest-only Preview、Shared Frame Ring 和 VCam 合法帧兜底。
长期风险不在传输技术选型，而在状态语义分散、帧所有权不清晰和成熟组件组合边界上的同步复制。

本 Architecture 约束运行时重构的目标形态。它不要求 WebRTC、SRT、HEVC 或 GPU 跨进程零复制，
也不以扩大队列换取表面吞吐。

## 架构决策

### 正交状态与单一 Reducer

Receiver 和 Sender 不再用一个展示枚举同时表达连接、信任、媒体、输出和健康。Core 持有正交状态：

```rust
struct ReceiverState {
    connection: ConnectionState,
    trust: TrustState,
    stream: StreamState,
    output: OutputState,
    health: HealthState,
}
```

UI 标签只能从该状态派生，`NetworkUnstable` 和 `VirtualCameraUnavailable` 不得覆盖仍然存在的
Streaming/Connected 事实。状态转换收敛到纯 reducer：

```rust
fn reduce(state: ReceiverState, event: ReceiverEvent)
    -> (ReceiverState, Vec<ReceiverEffect>);
```

QUIC、Decoder、时钟、磁盘可信存储、Frame Sink 和平台生命周期都是 Effect adapter。
旧 connection generation 的事件不得修改新会话；teardown 只有一个幂等入口。上层不构造
`SessionId`，Transport 提供 `close_active(reason)`。

### Rust 统一编码器事务

Rust Core 是编码重配置事务的唯一语义所有者，显式保存 transaction ID、候选 stream epoch、
预期 encoder generation、目标配置与 rollback 状态。Android/iOS 只执行
`ApplyEncoderConfiguration`，再报告 `EncoderStarted`、`EncodedAccessUnit` 或 `EncoderFailed`。
只有匹配 transaction、generation、epoch 和目标分辨率的首个 IDR 才允许 Core commit 并发送 ACK。
原生层继续拥有 Camera2/MediaCodec/AVFoundation/VideoToolbox 生命周期，但不独立决定协议提交。

### 类型化媒体时间线

Receiver 的 Decoder Effect 前后使用拥有所有权的类型，而不是把媒体身份散落在调用栈之外：

```rust
struct EncodedAccessUnit {
    generation: StreamGeneration,
    frame_id: FrameId,
    source_pts_us: u64,
    received_at: MonoInstant,
    frame_kind: FrameKind,
    data: Bytes,
}

struct VideoFrame {
    generation: StreamGeneration,
    frame_id: FrameId,
    source_pts_us: u64,
    decoded_at: MonoInstant,
    width: u32,
    height: u32,
    stride: u32,
    rotation: Rotation,
    pixels: Bytes,
}
```

跨 generation 旧帧不能发布；同一 AU 最多解码一次。Decoder 通过有界、reference-aware 的 Effect
队列在 Worker 上执行，不能同步阻塞控制消息、统计、IDR 请求和断线处理。

### LatestFrameStore 与共享不可变帧

同进程 `FrameHub` 改名为 `LatestFrameStore`，使用 `Arc<VideoFrame>`。Receiver reducer 当前是
单一写入者，因此 `Option<Arc<VideoFrame>>` 已经满足容量一语义；只有 Store 本身需要跨线程独立
写入时才引入 ArcSwap，避免为不存在的并发所有权增加依赖。它不声称拥有 Shared Frame Ring 的
reader lease/原子协议。`VideoFrame` 内部使用可从 Decoder `Vec<u8>` 零复制接管的 `Bytes`，外层
`Arc` 同时共享完整时间线与像素 backing storage。
Decoder 输出只分配一次，Preview、Shared Ring Writer 和可选 Recorder Sink 只持有共享引用；
慢消费者不得反压 Decoder。需要反复分配的像素缓冲进入有界 `FrameBufferPool`。

Shared Frame Ring 保留现有跨进程三槽、ready state、sequence、reader lease 与进程崩溃恢复协议。
Consumer 只读取 `data_length`，Producer 因而不得每帧清零最大 slot：正常帧只覆盖有效范围；
创建、关闭和 generation 重建时清零，若隐私评审要求，则仅在大帧切换为小帧时清理一次尾部。

### Virtual Camera 输出热路径

Windows VCam 使用以下边界：

```text
Shared Ring → RingReaderWorker → Arc<SourceFrame>
            → OutputPreparationWorker
            → Arc<Prepared480/720/1080>
            → RequestSample → 一次最终 MF buffer copy
```

`RequestSample` 不打开 Ring、不检测 generation、不 resize/letterbox、不生成 placeholder、不深拷贝
缓存帧，也不等待长期 mutex。CPU resize 只在新源帧到达时执行，并优先采用维护活跃、许可兼容、
目标平台可构建的 SIMD 图像库；只有 profile 证明必要时才评审 GPU 路径。

VCam 持有独立 `SampleClock`。sample time 单调、duration 固定；请求过快时复用缓存内容但按输出
节拍推进，请求过慢时跳过旧输出时刻而不补积压。最终策略必须由 Frame Server、Teams、Zoom、
OBS 和浏览器实测确认。

### 其余可观测性能边界

- Receiver 无旋转/镜像时直接转移 Decoder buffer 所有权；需要变换时合并整帧遍历，避免
  `rotation → mirror → copy → ring copy` 的连续全帧扫描。
- macOS Preview 不允许长期保留 `NV12 BT.709 limited → BGRA → NV12 BT.601 full` 双转换；
  GPUI Surface 应直接消费 NV12/CVPixelBuffer，或由 GPU 只转换一次。
- Sender 码流侧以 Rust-owned AU buffer、fragment offset/length descriptor、预编码 Datagram 和
  buffer reuse 减少小对象；其优先级低于解码后 NV12 的全帧复制。
- FEC 必须按观测损伤自适应：健康链路可为 0 parity，轻微损伤 1 parity，burst loss 2 parity；
  IDR/重要参考帧允许比 discardable delta 更强保护。
- Android/iOS Session 正常运行由事件/wakeup 驱动；250/500 ms polling 只保留 timeout/stats 兜底。
- Network health 使用 episode 与进入/恢复滞回，不把单个统计窗口覆盖 Streaming 状态。
- 端到端延迟只有在 sender monotonic timestamp、ping/pong 与按 generation 重置的 affine clock
  mapping 建立后才可发布；RTT 不得冒充 glass-to-glass latency。

## 验证边界

建立使用虚拟时钟的 `picoo-sim`：Scripted Camera/Encoder → Sender Core → Simulated Transport →
Receiver Core → Scripted Decoder → Fake Frame Sink。它必须覆盖丢包、乱序、重复、burst loss、
IDR fragment 丢失、StreamConfig 晚到、epoch 连续变化、前后台切换、编码重配置失败、快速重连、
Receiver 重启、控制消息重复/越序/非法阶段、UI 不消费和 VCam 快慢消费。

持续验证以下 invariant：

- 未认证设备永远不能发送媒体或特权控制；
- 同一时刻只有一个 committed stream generation；
- 新 generation 首个可接受 AU 必须是 IDR；
- AU 只能整体排队或整体丢弃，参考链损坏后不提交 delta；
- Preview/VCam 永远不能反压 Decoder，且 VCam 始终输出 negotiated type 的合法帧；
- 每个 AU 最多解码一次；
- 旧 connection generation 的事件不能修改新会话。

补充 ControlEnvelope、pairing transcript、reassembly/FEC fuzz，Shared Ring kill/restart stress，unsafe
Miri，原子协议 Loom model，夜间网络损伤/soak，以及 Windows 安装、注册、枚举和启动 VCam 的
主机级 Runner。单纯 bundle smoke 不构成 VCam 验收。

## 明确排除

- 不替换 H.264、QUIC reliable control + Datagram media、Native Camera/Codec、单次桌面解码、
  Shared Frame Ring、latest-only Preview、小容量整 AU 背压和断流合法帧。
- 本轮不做 P3：D3D11 shared texture、IOSurface 跨进程共享、AHardwareBuffer 全链路、
  自定义 GPU decoder output sharing 或其他复杂零复制。只有 profile 证明 CPU 内存路径仍是热点时
  才建立新的 Architecture 评审。

## 相关 Requirements

- `REQ-PICOO-SESSION-011..014`
- `REQ-PICOO-MEDIA-016..019`
- `REQ-PICOO-FRAME-008..010`
- `REQ-PICOO-VCAM-010..012`
- `REQ-PICOO-STACK-009`
