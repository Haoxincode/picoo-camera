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
只有匹配 transaction、generation、epoch 和目标分辨率的首个 IDR 完整进入 packetization 后，Core
才允许 commit；原生层只从 Rust snapshot 观察结果，不发送 ACK/NACK。
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

Worker 队列最多保留两个尚未开始的 AU。新 IDR 取代全部排队 AU；reference delta 优先淘汰
discardable delta，自身无法入队时触发参考链恢复；discardable delta 可直接丢弃。Reset 清除所有
排队 Decode、推进 decoder generation，但不等待正在执行的平台调用，完成事件同时受 decoder、
connection 与 stream generation 门禁。Worker 析构只请求 Shutdown，不得为无法控制的平台调用执行
无界 join。队列需要检查并替换任意 pending job，标准库 `Mutex<VecDeque> + Condvar` 比只支持
端点背压的 channel 更贴合这项 Picoo 特有语义；macOS Worker 每个任务使用 `objc2` autorelease pool，
平台 Decoder panic 被收敛为可观察错误并由 Worker 重建。每个 job 共享 `Arc<StreamConfig>`，不得为
每个 AU 深拷贝 SPS/PPS 等配置字节。

### LatestFrameStore 与共享不可变帧

同进程 `FrameHub` 改名为 `LatestFrameStore`，使用 `Arc<VideoFrame>`。Receiver reducer 当前是
单一写入者，因此 `Option<Arc<VideoFrame>>` 已经满足容量一语义；只有 Store 本身需要跨线程独立
写入时才引入 ArcSwap，避免为不存在的并发所有权增加依赖。它不声称拥有 Shared Frame Ring 的
reader lease/原子协议。`VideoFrame` 内部使用可从 Decoder `Vec<u8>` 零复制接管的 `Bytes`，外层
`Arc` 同时共享完整时间线与像素 backing storage。
Decoder 输出只分配一次，Preview、Shared Ring Writer 和可选 Recorder Sink 只持有共享引用；
慢消费者不得反压 Decoder。需要反复分配的像素缓冲进入有界 `FrameBufferPool`。

`FrameBufferPool` 复用 `bytes 1.12.1+` 的 `Bytes::from_owner`，由最后一个不可变 `Bytes` 视图的
析构归还 backing `Vec`；无需引入面向连接或异步资源的通用对象池。`bytes` 为项目既有、维护活跃
且 MIT 许可的跨平台依赖；最低版本包含 `from_owner` 的已知内存泄漏修复。`BytesMut` 无法在最后
一个不可变消费者释放时自动回收到 Receiver 池，
`object-pool`/`deadpool` 等通用池则会增加依赖和不需要的阻塞/异步语义，因此不采用。池只限制
空闲保留的 buffer 数和总 capacity；所有 buffer 被慢消费者持有时临时分配并在归还时丢弃超额
存储，绝不能等待消费者或反压 Decoder。会话 teardown 清空池并使尚未归还的旧 lease 失效。

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
- macOS Preview 不允许保留 `NV12 BT.709 limited → BGRA → NV12 BT.601 full` 双转换。当前上游 GPUI
  `surface(CVPixelBuffer)` 只接受 `420f`，Metal Surface shader 固定使用 BT.601 full-range 矩阵，不能
  直接消费 Picoo 的 `420v` BT.709 limited；平台 adapter 因而使用既有 `fast_image_resize` 分别缩放
  NV12 双平面，再执行一次直接 YCbCr 矩阵/范围变换并复制到有界 IOSurface pool。该路径不分配 BGRA
  中间帧；若 GPUI 上游以后支持颜色附件或可配置 shader，应删除这层矩阵适配并直接提交 `420v`。
- Sender 码流侧以 Rust-owned AU buffer、fragment offset/length descriptor、预编码 Datagram 和
  buffer reuse 减少小对象；其优先级低于解码后 NV12 的全帧复制。
- FEC 必须按观测损伤自适应：健康链路可为 0 parity，轻微损伤 1 parity，burst loss 2 parity；
  IDR/重要参考帧允许比 discardable delta 更强保护。
- Android/iOS Session 正常运行由事件/wakeup 驱动；250/500 ms polling 只保留 timeout/stats 兜底。
- Network health 使用 episode 与进入/恢复滞回，不把单个统计窗口覆盖 Streaming 状态。
- 端到端延迟只有在 sender monotonic timestamp、ping/pong 与按 generation 重置的 affine clock
  mapping 建立后才可发布；RTT 不得冒充 glass-to-glass latency。

平台唤醒使用 Rust 标准库 `Mutex + Condvar` 保存进程内单调 event revision，并复用 Kotlin Coroutine
`Dispatchers.IO` 与 Swift detached task 的官方后台执行边界。这里不采用 Linux `eventfd`、Apple
`DispatchSource` 或 Android `ALooper`：它们会把同一 Session 契约拆成平台专用实现，而当前需求不涉及
跨进程句柄、文件描述符复用或 UI run-loop source。revision 而非一次性布尔信号保证事件先到、并发
encoder pump 先消费、或 waiter 稍后启动时仍不会丢失唤醒；Condvar 只等待，不拥有 Session 状态。

时钟同步复用已认证 PCP 控制流传递 NTP 风格四时间戳，并以有界低延迟样本拟合
`receiver_time = slope × sender_time + offset`。评审过 `ntp-proto 1.9.0`（Apache-2.0 OR MIT，
Rust 1.88）：它维护活跃且适合标准 NTP 报文、认证与系统校时，但会引入 Picoo 不使用的 UDP NTP、
墙钟与密码套件边界，默认 feature 体积也不适合四个移动/桌面目标。Picoo 因而只自行维护固定样本数、
低 RTT 筛选和 affine 拟合这一最小 estimator，不实现 NTP 网络栈或修改系统时钟。少于三个低延迟
样本、采样跨度不足或 generation 已切换时，映射必须不可用，UI/ABR 不得发布或消费伪造的总延迟。

NV12 方向变换优先保持一个跨平台 Rust 边界：`libyuv` 支持相关操作，但会为全部桌面目标引入
C/C++ 构建与 ABI 维护；`fast_image_resize` 不直接表达 NV12 双平面 rotate+mirror 融合语义。当前
`transform_nv12` 因而只维护坐标映射这一 Picoo 特有最小实现：无变换转移原 `Bytes`，有变换只
分配一个紧凑缓冲并直接写入最终方向。若未来 resize 与方向处理需要统一 SIMD pipeline，再依据
profile 重新评审 `libyuv`，不得仅因为已有自研代码而拒绝迁移。

FEC 继续采用已有 `reed-solomon-erasure 6.0.0`：该 crate 为 MIT 许可、纯 Rust、覆盖当前全部目标，
其系统码语义允许 Receiver 保持两槽恢复矩阵，同时让 Sender 只发送当前策略需要的 parity。Picoo
只自行维护 AU 分组、重要性策略和 Datagram 调度这些产品特有边界，不重复实现有限域编码。发送热
路径把原生借用 AU 复制到可复用的 Rust backing buffer，复用 fragment descriptor 容量，并直接
生成最终 wire Datagram；Transport 只接收完整 AU batch，不再次创建 `VideoPacket` 或编码 payload。

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

模拟边界评审过 `tokio::time::pause` 与 `turmoil`。前者只虚拟 Tokio timer，无法驱动当前以显式
单调时间戳为契约的 Reassembly/Jitter；后者适合 Tokio TCP/UDP 应用的网络故障注入，但不能直接
承载 Quinn Datagram、PCP 整 AU/FEC 语义和平台 Codec 事实。Picoo 因而保留一个无异步 runtime 的
最小离散事件适配器，并直接复用生产 `ControlEnvelope`、`SenderPipeline`、Reassembly、Jitter 和
`LatestFrameStore`。生产 Reassembly 同时提供显式 monotonic instant 入口，产品路径仍使用系统
`Instant`；模拟器不得复制 packet/FEC/jitter 算法。

补充 ControlEnvelope、pairing transcript、reassembly/FEC fuzz，Shared Ring kill/restart stress，unsafe
Miri，原子协议 Loom model，夜间网络损伤/soak，以及 Windows 安装、注册、枚举和启动 VCam 的
主机级 Runner。单纯 bundle smoke 不构成 VCam 验收。

原子协议模型采用 `loom 0.7.2`（MIT，Rust 1.65，tokio-rs 维护），仅作为
`picoo-frame-hub` dev-dependency 穷举 ready/sequence/reader lease 的关键交错；它不进入产品二进制，
也不替换跨进程 ABI 中必须使用的 `std::sync::atomic`。Miri 使用固定日期 nightly 提供的官方组件，
只解释执行原始 mapping 布局、共享 buffer 所有权和当前主机可运行的 C ABI 测试；OS mmap、文件锁和
COM/Objective-C API 仍由平台 contract/stress 测试验证，不能用 Miri 模拟结果替代。

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
