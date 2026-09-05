# ARCH-PICOO-RUNTIME-001：显式状态、媒体所有权与性能边界

Status: implemented
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

### 每会话单一运行时所有者

每个 Receiver 会话由 Rust Core 的专用单线程 owner 创建并独占。QUIC、重组、播放队列、Decoder
completion 和平台输出 adapter 都不能进入 GPUI `Entity` 或 CLI 输入循环；桌面只实现
`ReceiverRuntimeAdapter`，通过命令、oneshot reply、`Arc<ReceiverSnapshot>` 与独立发布的 latest frame
交互。owner 以 transport/event revision 和最近媒体 deadline 等待，每轮命令处理受 64 条/2 ms
公平预算约束。完整快照最多每 100 ms 构建一次，命令后立即构建，内容无变化时保持同一个 `Arc`；
可信设备排序/指纹摘要按进程内 trust revision 缓存，历史指标摘要只在样本集合变化时重算。销毁句柄
只请求 Shutdown，join 交给清理线程，不能阻塞 UI teardown。GPUI 与 `--serve` 必须使用同一个 Core
owner；CLI 可同步等待自身命令 reply，但不得直接调用 `pump()` 或恢复固定媒体轮询。

owner 的命令邮箱必须有界且由提交方非阻塞写入；容量耗尽或 owner 已关闭时，要求 reply 的命令连同其
oneshot sender 返回给平台 adapter，由 adapter 立即发送明确的 `Full`/`Closed` 错误，不能让 UI
误判为普通 reply channel 取消。`Disconnect` 与其他有副作用命令一样必须返回 reply，队列满或关闭时
不得只记录日志。展示名、自动接受、占位图与虚拟摄像头状态等可合并设置共用一个
capacity-one latest mailbox，但平台 adapter 必须先按字段合并最新值，避免不同设置之间互相覆盖。
UI 线程不得使用阻塞 `send()` 向 owner 施加反压。CLI stdin EOF 只表示不再有控制台命令，
不表示 Receiver 服务退出；`--serve` 只能由 `quit`、显式关闭信号或 runtime 终止结束。

`ReceiverRuntimeAdapter` 不要求 `Send`：adapter 和 `ReceiverSession` 必须在 owner 线程内构造。这一
边界用于阻止为了移动 mmap、COM 或其他平台资源而增加宽泛 `unsafe impl Send`。Sender 同样遵循
“一个会话一个原生媒体 owner”的原则；平台 callback 只交付事实，不同时承担 Session 编排。

### Rust 统一编码器事务

Rust Core 是编码重配置事务的唯一语义所有者，显式保存 transaction ID、候选 stream epoch、
预期 encoder generation、目标配置与 rollback 状态。Android/iOS 只执行
`ApplyEncoderConfiguration`，再报告 `EncoderStarted`、`EncodedAccessUnit` 或 `EncoderFailed`。
只有匹配 transaction、generation、epoch 和目标分辨率的首个 IDR 完整进入 packetization 后，Core
才允许 commit；原生层只从 Rust snapshot 观察结果，不发送 ACK/NACK。
原生层继续拥有 Camera2/MediaCodec/AVFoundation/VideoToolbox 生命周期，但不独立决定协议提交。

Android JNI 与 iOS C ABI 对每个原生编码 AU 只暴露一个完整事件入口。事件同时携带
generation/transaction、时间线、关键帧事实和可选参数集；Core 在一次 Session 独占中完成 started
事实、StreamConfig stage/可靠控制推进、packetization、flush、pump 与 IDR 请求消费，并返回类型化
结果。FFI 只做参数转换和错误映射，不得重新编排这些业务步骤。MediaCodec/VideoToolbox 借用内存
不能越过平台释放边界；异步 handoff 中的数据必须已经拥有所有权。

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

Decoder 公共输出把帧描述与存储能力分开：`DecodedFrameDescription` 固定表达尺寸、stride、旋转、
像素格式、色彩矩阵与范围，`DecodedFrame` 另持时间戳，`DecodedFrameStorage` 表达实际 backing。当前产品只启用安全的
`CpuNv12(Bytes)`，保持 Shared Ring 和软件消费者的明确生命周期；未来 CVPixelBuffer 或 D3D11
surface 只有在存在显式、可跨线程转移的 owner 时才能加入非穷尽存储枚举，禁止把裸平台指针直接
标记为 `Send`。需要 CPU 像素的消费者按能力生成并缓存 fallback，不要求跨进程 GPU surface。

跨 generation 旧帧不能发布；同一 AU 最多解码一次。Decoder 通过有界、reference-aware 的 Effect
队列在 Worker 上执行，不能同步阻塞控制消息、统计、IDR 请求和断线处理。

Worker 队列最多保留两个尚未开始的 AU。新 IDR 取代全部排队 AU；reference delta 优先淘汰
discardable delta；队列暂满且没有可替换项时，尚未过硬期限的 reference delta 留在 Jitter 等待
Decoder capacity event，真正超期才触发参考链恢复；饱和时的 discardable delta 可直接丢弃。
准入检查发生在 `pop_ready()` 之前，且只有 Session owner 能提交 Decode，因此 Worker 并发取走任务
只会让检查后的容量增加，不会被第二个生产者抢占。Reset 清除所有排队 Decode、推进 decoder
generation，但不等待正在执行的平台调用，完成事件同时受 decoder、
connection 与 stream generation 门禁。Worker 析构只请求 Shutdown，不得为无法控制的平台调用执行
无界 join。队列需要检查并替换任意 pending job，标准库 `Mutex<VecDeque> + Condvar` 比只支持
端点背压的 channel 更贴合这项 Picoo 特有语义；macOS Worker 每个任务使用 `objc2` autorelease pool，
平台 Decoder panic 被收敛为可观察错误并由 Worker 重建。每个 job 共享 `Arc<StreamConfig>`，不得为
每个 AU 深拷贝 SPS/PPS 等配置字节。

### 统一媒体调度决策

Reassembly、Jitter 和 Decoder 队列继续保留各自专用数据结构，但“下一步解码、丢弃恢复阻塞帧、丢弃可抛帧、
硬过期、等待旧 AU、等待恢复 IDR completion、等待 Decoder 容量、等待播放点或等待事件”只能由纯
`MediaScheduler` 决定。输入是完整 AU 队首身份、最旧未完成 frame ID、正常播放 delay、绝对帧龄 deadline、
恢复准入与 Decoder 准入事实；
输出是 `DecodeReadyFrame`、`DiscardReadyFrame`、`DiscardExpired`、`WaitUntil`、`WaitForEvent` 或
`Idle`。生产 Receiver 的 drain 与 next wake 必须调用同一个函数，不能各自复制一套
阻塞判断；`picoo-sim` 也直接依赖该纯决策，从而让乱序、旧 AU 阻塞与硬过期的模拟证据覆盖生产
语义，而不是覆盖一个相似实现。

### LatestFrameStore 与共享不可变帧

同进程使用 `LatestFrameStore` 与 `Arc<VideoFrame>`。Receiver reducer 当前是
单一写入者，因此 `Option<Arc<VideoFrame>>` 已经满足容量一语义；只有 Store 本身需要跨线程独立
写入时才引入 ArcSwap，避免为不存在的并发所有权增加依赖。它不声称拥有 Shared Frame Ring 的
reader lease/原子协议。`VideoFrame` 内部使用可从 Decoder `Vec<u8>` 零复制接管的 `Bytes`，外层
`Arc` 同时共享完整时间线与像素 backing storage。
Decoder 输出只分配一次，Preview、Shared Ring Writer 和可选 Recorder Sink 只持有共享引用；
慢消费者不得反压 Decoder。需要反复分配的像素缓冲进入有界 `FrameBufferPool`。

Shared Ring Writer 位于 `LatestFrameStore` 之后并拥有独立线程。Receiver 发布完成后只提交
`Arc<VideoFrame>`；Writer 的 pending 容量为一，新帧覆盖尚未开始的旧工作，实际 mmap Producer 在
Writer 线程内构造并终身留在该线程。发布成功/失败通过事件回报诊断，像素复制失败不破坏 Decoder
参考链，也不能同步阻塞 Session owner。所有槽位忙时 Producer 返回显式 `Busy`，Writer 保留最新
未落地帧并限频重试；新帧可替换它，一次性占位帧不能把旧 sequence 伪报成新发布。Preview 与
Shared Ring 是 LatestFrameStore 的两个独立消费者，任何一方变慢都不能阻塞另一方。

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
            → Arc<Prepared(active output formats)>
            → RequestSample → 一次最终 MF buffer copy
```

`RequestSample` 不打开 Ring、不检测 generation、不 resize/letterbox、不生成 placeholder、不深拷贝
缓存帧，也不等待长期 mutex。CPU resize 只在新源帧到达时执行，并优先采用维护活跃、许可兼容、
目标平台可构建的 SIMD 图像库；只有 profile 证明必要时才评审 GPU 路径。
只有处于 Running 的协商格式构成准备需求；Stop 后 reader 与 resize 必须暂停。多消费者只能把各自
正在使用的格式加入集合，不能恢复为每帧固定生成全部三档。

VCam 持有独立 `SampleClock`。sample time 单调、duration 固定；请求过快时复用缓存内容但按输出
节拍推进，请求过慢时跳过旧输出时刻而不补积压。最终策略必须由 Frame Server、Teams、Zoom、
OBS 和浏览器实测确认。

### 其余可观测性能边界

- Quinn 在读到每批首个 Datagram 时记录 Receiver 本地单调时间。该时间贯穿 TransportEvent 与
  Reassembly，使入口队列等待包含在同一媒体截止线内；超龄批次按完整 AU 建立 terminal boundary，
  并复用参考链恢复，容量上界不得冒充时间上界。
- GPUI 只以命令和不可变快照访问独占 Receiver 的线程；网络、重组、Decoder completion、deadline
  共用 revision wake。16 ms UI timer 只负责展示，不能驱动媒体正确性或发布节奏。
- Preview demand 由 Live 视频元素最近实际绘制的物理尺寸和页面可见性决定，目标 30 FPS；隐藏、
  设置页或非 Live 状态不准备像素，恢复后只提交 LatestFrameStore 当前帧。缩小预览必须先缩 NV12
  双平面再做目标尺寸色彩转换。
- Receiver 无旋转/镜像时直接转移 Decoder buffer 所有权；需要变换时合并整帧遍历，避免
  `rotation → mirror → copy → ring copy` 的连续全帧扫描。
- macOS Preview 不允许保留 `NV12 BT.709 limited → BGRA → NV12 BT.601 full` 双转换。当前上游 GPUI
  `surface(CVPixelBuffer)` 只接受 `420f`，Metal Surface shader 固定使用 BT.601 full-range 矩阵，不能
  直接消费 Picoo 的 `420v` BT.709 limited；平台 adapter 因而使用既有 `fast_image_resize` 分别缩放
  NV12 双平面，再执行一次直接 YCbCr 矩阵/范围变换并复制到有界 IOSurface pool。该路径不分配 BGRA
  中间帧；若 GPUI 上游以后支持颜色附件或可配置 shader，应删除这层矩阵适配并直接提交 `420v`。
- Sender 码流侧在同步分包期间直接借用原生 AU，以可复用的 fragment offset/length descriptor、
  预编码 Datagram 和 buffer reuse 减少复制与小对象；最终 Datagram 必须在调用返回前完成自持有。
  其优先级低于解码后 NV12 的全帧复制。
- Android MediaCodec callback 只复制有效压缩字节、释放 codec buffer 并提交有界 GOP-aware handoff。
  单一 Sender media worker 执行 generation/transaction 校验、StreamConfig、分包、flush、pump 与 IDR
  消费；一次原子 JNI 调用只取得一次 Session mutex。handoff 同时限制 AU 数、字节和 250 ms 帧龄。
- FEC 必须按观测损伤自适应：健康链路可为 0 parity，轻微损伤 1 parity，burst loss 2 parity；
  IDR/重要参考帧允许比 discardable delta 更强保护。
- Reassembly 在 PartialFrame 建立平衡分组索引和组内位图，每个数据片或 parity 只检查受影响组；
  Sender 只计算实际发送的 parity，并复用 padded shard scratch。轻量 parity 0 必须与强保护 parity 0
  保持逐字节 wire 一致。
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
路径在同步调用期间直接读取原生借用 AU，复用 fragment descriptor 容量，并直接生成自持有的最终
wire Datagram；Transport 只接收完整 AU batch，不再次创建 `VideoPacket` 或编码 payload。借用 AU
不会跨越 packetization 调用边界，因此不得为异步生命周期假设额外复制整帧。

## 验证边界

建立使用虚拟时钟的 `picoo-sim`：Scripted Camera/Encoder → Sender Core → Simulated Transport →
Receiver Core → Scripted Decoder → Fake Frame Sink。它必须覆盖丢包、乱序、重复、burst loss、
IDR fragment 丢失、StreamConfig 晚到、epoch 连续变化、前后台切换、编码重配置失败、快速重连、
Receiver 重启、控制消息重复/越序/非法阶段、UI 不消费和 VCam 快慢消费。

快速场景可使用零播放等待和同步完成 Decoder；涉及 deadline、背压和代际切换的生产等价场景必须
启用自适应 Jitter，并以虚拟完成时刻模拟 Decoder active/pending 容量、开始、完成和 reset。完整 AU
在 Reassembly 完成后直接进入与产品一致的 Jitter/`MediaScheduler` 路径，不得在模拟器外层另建
`completed_access_units` 队列提前复制旧 AU 阻塞决策。模拟器的恢复 IDR 也必须在 Decoder completion
且 candidate 身份匹配时才恢复参考链，不得在 `accept_access_unit()` 或 Decoder submit 时提前恢复。

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
`Instant`；模拟器不得复制 packet/FEC/jitter 算法，也不得复制生产 `MediaScheduler` 的跨层推进决策。
Scripted Decoder 只模拟平台 adapter 的时序和资源边界，不模拟硬件像素算法。

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

- `REQ-PICOO-SESSION-011..017`
- `REQ-PICOO-TRANSPORT-011`
- `REQ-PICOO-PROTOCOL-010`
- `REQ-PICOO-MEDIA-005 / 016..023`
- `REQ-PICOO-FRAME-008..011`
- `REQ-PICOO-VCAM-010..013`
- `REQ-PICOO-UI-004`
- `REQ-PICOO-STACK-009 / 011`
