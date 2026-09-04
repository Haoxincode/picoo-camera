# Picoo Camera 媒体可靠性与 Windows 虚拟摄像头研究

日期：2026-08-31

状态：Research，非规范性

范围：Android/iOS Sender → PCP → Windows Receiver → Media Foundation Virtual Camera

> 本文用于保存研究证据、仓库审计结果和待决策问题，不直接定义产品行为。
> 已确认的产品与架构决策仍应进入对应 Use Case、Architecture 和 Requirement；实现不得只引用本文作为验收依据。

## 1. 研究问题

本次研究回答四个问题：

1. Picoo 是否需要从 QUIC Datagram 改成 RTSP、WebRTC 或 SRT？
2. 当前卡顿、马赛克、首帧失败、颜色异常和会议软件黑屏，是否说明 H.264 或局域网视频本身不成熟？
3. 外部项目中哪些经验适合 Picoo，哪些只是特定项目的实现或故障记录？
4. Picoo 当前已经具备什么，真正尚未闭合的架构与实现缺口是什么？

## 2. 结论摘要

### 2.1 主架构不需要更换

Picoo 当前主线成立：

```text
同一 Wi-Fi
  → mDNS / 手动 Endpoint
  → QUIC reliable Stream：控制、配置、反馈
  → QUIC Datagram：H.264 Access Unit 分片
  → Receiver 重组、抖动缓冲、解码
  → FrameHub / Shared Frame Ring
  → Windows MF Virtual Camera
```

QUIC Datagram 的不可靠语义并不是架构错误。实时视频本来就需要在应用层定义帧边界、截止时间、丢弃、预测链恢复和媒体反馈。Picoo 已经实现了其中大部分，不应为了获得这些语义而直接引入完整 RTSP、WebRTC 或 SRT 生产栈。

### 2.2 当前主要问题是边界没有完全闭合

Picoo 不是缺少一套全新的“媒体协议栈”，而是存在以下已确认缺口：

- Design Spec 要求解码失败后清理缓冲并请求 IDR，当前代码尚未执行该恢复状态转换。
- `ReceiverStats.packet_loss` 当前把 Datagram 数量与 AU 丢弃数量放在同一比率中，统计单位不一致。
- Windows VCam 没有 RequestSample 请求频率、新鲜帧和缓存帧指标，无法判断是否存在 request pump。
- 缺少只验证当前 PCP 网络媒体面的独立 `picoo-probe`。
- 当前 ABR 支持码率和 1080p/720p/480p 分辨率阶梯，但没有帧率降档。

### 2.3 优先建立证据，再扩大实现

最有价值的工作不是先增加更多协议字段或替换底层，而是：

1. 把采集、编码、发送、重组、解码、FrameHub 和 VCam 指标串起来；
2. 闭合 Decoder Recovery 状态；
3. 获得 Windows 真机上的 RequestSample 节拍证据；
4. 再决定是否增加 packet sequence、提高反馈频率或加入 FPS 阶梯。

### 2.4 本轮修复落地状态

本文后续的 `Repository Gap` 保留研究启动时的仓库快照。基于本研究启动的第一轮实现已经：

- 增加 Receiver `AwaitingRefresh`：非 `DISCARDABLE` AU 在 reassembly/jitter 丢失或
  Decoder error 后立即停止后续 delta 解码，reset Decoder，并以全局一秒窗口合并 IDR 请求；
- 将 `ReceiverStats.packet_loss` 改为缺失 fragment ÷（收到 fragment + 缺失 fragment），
  不再混用 AU drop 和 Datagram 数；完全无片到达的 AU 仍等待后续 packet sequence 决策；
- 增加 Windows MF Source 每秒 request/fresh/cached/placeholder 与 delivery time debug 指标，
  但没有在缺少 Win11 证据时引入同步 sleep；
- 将 recovery drop、Decoder reset 与 IDR request 计数写入桌面诊断导出。

尚未完成的主项是独立 `picoo-probe`、跨端 monotonic 时间关联、Windows 会议软件真机指标和
Android→Windows 弱网 E2E 验收。

## 3. 证据等级

本文使用以下标签，避免把外部经验或推测误写成架构事实：

| 标签 | 含义 |
| --- | --- |
| `Confirmed Fact` | 由标准、官方文档、仓库代码或可重复测试直接支持 |
| `Repository Gap` | 当前 Design Spec 与实现之间存在可定位的缺口，或实现缺少目标所需能力 |
| `Hypothesis` | 现象与机制相符，但仍需指标、日志或真机实验确认 |
| `Candidate Decision` | 可进入 Architecture/Requirement 评审的候选，不是当前已确认决策 |

证据优先级为：标准与官方平台文档 → Picoo 代码和测试 → 外部项目源码 → 外部项目说明 → 推断。

## 4. 第一性原理与系统不变量

Picoo 的目标不是可靠交付每个历史视频字节，而是在局域网波动和平台生命周期变化下，持续输出最新、可解码、时间合法的画面。

应保持以下不变量：

1. Camera 和硬件 Encoder 线程不得等待网络发送。
2. 一个 AU 超过实时截止时间后不再等待；不完整 AU 永远不进入解码器。
3. 预测链损坏后，不继续把已知不可恢复的 delta AU 喂给解码器；恢复点是匹配当前配置的完整 IDR。
4. Sender AU 队列、QUIC 发送队列、Receiver 重组、jitter、FrameHub 和 Shared Frame Ring 都必须有固定上限。
5. VCam 与网络、解码器属于独立故障域；无实时帧时仍输出完整 NV12、时间戳和 duration。
6. `stream_epoch` 隔离配置和编码世代；旧世代数据不得污染新世代。
7. 任何“延迟”“丢包”“帧龄”指标必须说明测量边界和单位，不能把不同方向或不同层的数字混为一谈。

这些不变量比采用某个特定传输协议更重要。

## 5. 标准与官方平台证据

### 5.1 QUIC Datagram

`Confirmed Fact`

[RFC 9221](https://www.rfc-editor.org/rfc/rfc9221.html) 明确规定：

- DATAGRAM frame 不要求重传；
- DATAGRAM 没有显式 flow control，也不计入 QUIC stream/connection data limit；
- DATAGRAM 仍使用 QUIC connection 的拥塞控制；
- 拥塞控制可能让实现延迟发送或直接丢弃 Datagram；
- 即使传输层确认 Datagram 被接收，也不能证明接收端应用已经成功处理媒体数据。

因此，QUIC 可以提供安全连接、拥塞控制和可靠控制流，但不会替 Picoo 定义视频帧完成、过期、解码恢复或应用层反馈。

### 5.2 H.264 分片语义

`Confirmed Fact`

[RFC 6184](https://www.rfc-editor.org/rfc/rfc6184.html) 定义了 H.264 over RTP 的 NAL fragmentation、顺序编号、Start/End 和重组语义。Picoo 不需要采用完整 RTP wire format，但可以借鉴以下语义：

- 片段具有明确顺序；
- 只有完整媒体单元才能交给后续处理；
- 乱序、缺片和解码顺序必须有一致定义；
- 时间戳与媒体单元而不是任意网络写调用绑定。

### 5.3 预测链恢复

`Confirmed Fact`

[RFC 4585 的 Picture Loss Indication](https://www.rfc-editor.org/rfc/rfc4585.html#section-6.3.1) 表示 Decoder 已观察到一个或多个 picture 的数据丢失，Encoder 应意识到 inter-picture prediction chain 可能已经损坏。

PLI 的核心价值是恢复语义，而不是要求每次反馈都立即生成超大 IDR。具体 Encoder 行为、请求合并和限频仍属于应用策略。

### 5.4 媒体反馈

`Confirmed Fact`

[RFC 8888](https://www.rfc-editor.org/rfc/rfc8888.html) 说明 sender-based congestion control 通常需要比汇总 RTT 更细的反馈，包括 packet sequence、到达时间、丢包和 ECN。文档提到一些算法可使用 50–200 ms 的反馈间隔，但间隔必须与实际拥塞算法、媒体码率和反馈开销共同选择。

这支持为 Picoo 增加准确 packet sequence 和媒体反馈，但不等于当前简单 AIMD 必须每 100 ms 调整一次 Encoder。

### 5.5 Windows Frame Server

`Confirmed Fact`

[Microsoft Frame Server Custom Media Source](https://learn.microsoft.com/en-us/windows-hardware/drivers/stream/frame-server-custom-media-source) 要求 Custom Media Source 为样本提供 buffer、sample time 和 sample duration。微软的 [Windows-Camera Virtual Camera 示例](https://github.com/microsoft/Windows-Camera/tree/master/Samples/VirtualCamera) 同时说明：

- Media Source 会被 Frame Server / Frame Server Monitor 加载；
- stream state、event sequence、allocator 和线程模型都是契约的一部分；
- 虚拟摄像头应当通过专门测试加载到真实 Frame Server 中验证，而不能只用普通进程内单元测试替代。

## 6. 外部项目与复用适用性

### 6.1 已核对项目

| 项目 | 已核对价值 | 适用性 | 许可证边界 |
| --- | --- | --- | --- |
| [CatCam](https://github.com/igorfyago/CatCam) | Android Camera2/MediaCodec、单发送者队列、弱网降级、Windows Frame Server、共享内存、`probe.py` 和故障记录 | 适合阅读行为、状态机和诊断经验；不作为成熟底座 | GPL-2.0；只借鉴思想并独立实现，不复制到非 GPL 分发代码 |
| [RootEncoder](https://github.com/pedroSG94/RootEncoder) / [RootEncoder-iOS](https://github.com/pedroSG94/RootEncoder-iOS) | 长期维护的移动采集、硬编码、动态码率和多协议实现 | 适合参考 OEM Encoder 降级、设备兼容和测试策略；不替换 Picoo 当前原生管线 | Apache-2.0；实际复用仍需保留许可与 NOTICE 要求 |
| [Microsoft Windows-Camera](https://github.com/microsoft/Windows-Camera) | 官方 `IMFVirtualCamera`、Custom Media Source、测试和 Frame Server 调试入口 | Windows MF 契约的第一参考 | MIT |
| [VCamSample](https://github.com/smourier/VCamSample) | 独立的 Windows 11 MF Virtual Camera、NV12、CPU/GPU 路径 | 适合交叉验证接口和客户端差异；项目自己也记录了 Teams 兼容限制 | MIT |

截至本研究快照，CatCam 的 GitHub 元数据表明仓库创建于 2026-07-26，规模与历史都不足以证明长期成熟性。它的价值是高相关性的工程记录，而不是统计意义上的稳定性证据。

### 6.2 CatCam 中可借鉴但不能直接泛化的经验

CatCam 源码记录了两类高价值经验：

- 网络写阻塞 Encoder drain，继续反压 EGL，最终使 Camera HAL 饿死；
- Windows `RequestSample` 立即完成后，特定环境测得约 1700 requests/s，并通过同步等待控制 cadence。

第一条与 Picoo 的线程边界高度一致，可以作为架构不变量。第二条只能视为 `Hypothesis` 的来源：微软官方 synthetic Virtual Camera 示例也会立即完成 `RequestSample`，因此“所有立即完成都错误”并不是平台标准结论。Picoo 应先测量自己的 request/fresh/cached 频率，再选择 pacing 实现，且不能未经验证直接在持有全局 stream mutex 时复制 CatCam 的 sleep 方案。

### 6.3 暂不作为决策依据的线索

原始材料还提到 VCamdroid/Nexora、OpenStream、DroidCam OBS Plugin、VanCamera、str0m 和 webrtc-rs。本文没有逐一固定仓库 URL、研究 commit、许可证和复现实验，因此不使用这些项目的具体行为作为当前架构证据。

WebRTC/RTP 的恢复语义已经可以从 RFC 获取；SRT 可作为后续诊断对照，但不应因未复核的项目案例进入生产依赖。

## 7. Picoo 当前能力审计

### 7.1 PCP VideoPacket

[当前 VideoPacket](../../crates/picoo-protocol/src/video_packet.rs) 已包含：

| 通用媒体字段 | Picoo 当前字段 | 状态 |
| --- | --- | --- |
| wire compatibility marker | 无 | 同仓同步发布，不维护数字协议版本或兼容分支 |
| session/config generation | `stream_epoch` | 已有，覆盖连续编码世代 |
| frame identity | `frame_id` | 已有 |
| media timestamp | `pts_us` | 已有，但不等同于跨设备可比较的 capture timestamp |
| fragment position | `fragment_index` / `fragment_count` | 已有 |
| key/start/end/discardable | `flags` | 已有 |
| transport-wide packet sequence | 无 | 候选缺口 |
| stream ID | 无 | V1 单视频流暂无必要 |
| independent config ID | 无 | 与 `StreamConfig + stream_epoch` 职责重叠，暂不建议增加 |

`packet_sequence` 对当前帧重组不是必需的，因为 `stream_epoch + frame_id + fragment_index` 已能重组完整 AU；它的主要价值是精确丢包、乱序和反馈统计。

### 7.2 重组、截止时间和队列

`Confirmed Fact`

[picoo-packet](../../crates/picoo-packet/src/lib.rs) 已实现：

- 只输出 fragment 齐全的 AU；
- 不同 `stream_epoch` 隔离；
- 重组容量上限；
- 超时 AU 的单调丢弃边界；
- terminal tombstone，防止迟到尾片重建旧帧；
- 优先淘汰最旧非关键帧；
- 不完整关键帧丢失信号。

[Receiver Session](../../crates/picoo-receiver/src/session/mod.rs) 使用 120 ms 重组截止时间，并且只有完整 AU 才进入 jitter。

[Quinn transport](../../crates/picoo-transport/src/quinn_backend.rs) 已将完整 AU 作为发送队列项；应用队列上限为三个 AU，发送前检查 Quinn Datagram send buffer 空间，避免由 Picoo 主动制造“头片进入、尾片因队列满而丢弃”的半帧。

因此，原始建议中“增加完整帧重组、截止时间和有界 Sender 队列”已经不属于待实现事项。

### 7.3 StreamConfig 与关键帧请求

`Confirmed Fact`

Picoo 已经在以下条件请求关键帧：

- 当前 epoch 首次开始或 epoch 变化；
- 不完整关键帧超过重组 deadline；
- 关键帧在 jitter 中超过播放点；
- 新 epoch Datagram 先于可靠 `StreamConfig` 到达。

`Repository Gap`

[ARCH-PICOO-SESSION-001](../design-specs/architecture/0005-session-reconnect-jitter-bitrate-boundary.md) 规定：

```text
解码器报错 → 清空当前 epoch 缓冲并请求 IDR
```

但 [Receiver media publish](../../crates/picoo-receiver/src/session/media.rs) 当前只记录 decoder drop 和错误，然后继续处理后续 AU；[RequestKeyframe](../../crates/picoo-receiver/src/session/control.rs) 也没有自动请求合并或限频状态。

这意味着预测链损坏后，Receiver 可能继续向 Decoder 输入依赖损坏参考帧的 delta AU，产生连续错误和无意义工作。

### 7.4 ReceiverStats 与 ABR

`Confirmed Fact`

Picoo 当前每秒反馈：

```text
rtt_ms
packet_loss
jitter_ms
reassembly_drop
decoder_drop
frame_age_ms
receive_bitrate
jitter_buffer_depth_ms
```

Sender 使用反馈进行码率增减，并在持续拥塞时按 1080p → 720p → 480p 降档。

`Repository Gap`

[Receiver StatsReporter](../../crates/picoo-receiver/src/session/mod.rs) 中：

- `window_packets` 是收到的 Datagram 数量；
- `reassembly_drop` 是 ReassemblyMap 丢弃的 AU/帧数量；
- 当前 `packet_loss = reassembly_drop / (window_packets + reassembly_drop)`。

分子和分母的单位不同。该统计比读取 Receiver 本端发出控制包的 Quinn loss 更接近目标，但仍不能称为精确视频 packet loss，而且无法观察完全没有任何片段到达的 AU。

`Repository Gap`

[picoo-rate-control](../../crates/picoo-rate-control/src/lib.rs) 只有码率和分辨率动作，没有 30 → 15 → 10 FPS 阶梯。是否加入 FPS 降档应在准确指标和真机行为稳定后评审，不应先于恢复语义与可观测性。

### 7.5 Windows Virtual Camera

`Confirmed Fact`

当前 Windows MF Source 已具备：

- 网络、Decoder 与 VCam DLL 进程边界分离；
- 480p/720p/1080p NV12 media type；
- BT.709 limited range media attributes；
- 合法 sample buffer、sample time 和 sample duration；
- Shared Frame Ring 新世代重连；
- 短暂保留最后完整帧，之后输出统一占位帧；
- `MEStreamStarted`、`MEStreamStopped`、format changed 和 media sample events。

这些实现位于 [media_stream.rs](../../extensions/windows-virtual-camera/mf-source/src/windows_source/media_stream.rs) 和 [frame_provider.rs](../../extensions/windows-virtual-camera/mf-source/src/frame_provider.rs)。

`Repository Gap`

`RequestSample` 当前同步获取/复制一帧并立即投递 `MEMediaSample`，没有以下指标：

```text
vcam_requests_per_sec
vcam_fresh_samples_per_sec
vcam_cached_samples_per_sec
vcam_placeholder_samples_per_sec
vcam_sample_delivery_time_ms
```

因此，当前不能判断 Teams、Zoom、微信、OBS 或浏览器是否把 Source 拉入异常 request pump，也不能证明需要哪一种 pacing。

### 7.6 Discovery 与平台生命周期

`Confirmed Fact`

原始建议中相当一部分已经实现：

- Android NSD 使用系统发现，并在 API 33+ 绑定 Wi-Fi Network；
- Receiver mDNS 广播绑定选定 LAN 地址，避免 VPN/Hyper-V/WSL 接口漂移；
- 网络切换与瞬时 `ServiceLost` 有恢复策略；
- `device_id` 是身份，Endpoint 只是定位信息；
- 支持手动 IP fallback；
- Windows 安装器包含 QUIC 与 mDNS 防火墙规则；
- iOS 已声明 Local Network 和 Bonjour 服务，并保留手动 Endpoint。

对应证据见 [discovery requirements](../design-specs/requirements/discovery.md)。设备发现仍需要真局域网、企业 Wi-Fi、访客网络与 AP isolation 验证，但不应重写成另一套发现架构。

## 8. 已确认缺口

### F-01 Decoder Recovery 状态缺失

标签：`Repository Gap`

需要评审明确的 Receiver 恢复状态，而不只是散落的 `RequestKeyframe` 调用：

```text
Healthy
  → 关键帧丢失 / 连续解码错误 / Decoder 重建 / epoch 变化
AwaitingRefresh
  → 清空旧 reassembly 与 jitter
  → flush 或重建 Decoder（按错误类型）
  → 合并并限频发送 RequestKeyframe
  → 丢弃不满足恢复条件的 delta AU
  → 收到匹配当前 StreamConfig 的完整 IDR
Healthy
```

恢复条件是否必须显式观察 SPS/PPS，取决于当前 `StreamConfig` 和 IDR 内联 config 契约，需在 Architecture 中写成唯一规则。

### F-02 视频丢包测量缺少同单位序号

标签：`Repository Gap`、`Candidate Decision`

候选方案是在下一次 PCP VideoPacket 变更中加入单调 `packet_sequence`。Receiver 按窗口统计 received/missing/reordered，Sender 用它进行媒体健康判断。

在确定字段前需要同时决定：

- sequence 是每个 connection、每个 `stream_epoch` 还是整个 Sender 进程递增；
- wrap 规则；
- 重连后是否由新的 connection/session generation 隔离；
- 反馈是否返回 missing ranges，还是只返回汇总计数；
- 完全丢失 AU 如何由 `frame_id` gap 补充统计。

### F-03 VCam cadence 没有证据闭环

标签：`Repository Gap`、`Hypothesis`

先增加 request/fresh/cached/placeholder 指标，在 Windows Frame Server 与至少两个会议客户端中记录。只有真机证明请求频率显著超过声明 FPS，或出现重复 GPU/CPU 工作，才决定同步 pacing、异步 work queue、token queue 或其他实现。

不能把 CatCam 的同步 sleep 直接变成 Picoo 的平台规则。

### F-04 缺少当前协议的独立网络 Probe

标签：`Repository Gap`

建议的 `picoo-probe` 应使用 Picoo 自己的 PCP/QUIC 实现，只接收并重组，不解码、不写 FrameHub、不启动 VCam。至少输出：

```text
session_epoch
packets_received
packets_missing          # sequence 可用后
packets_reordered        # sequence 可用后
access_units_completed
access_units_expired
keyframes_completed
keyframes_expired
receive_bitrate
reassembly_queue_ms
stall_duration_ms
```

这比先增加 RootEncoder → SRT → FFmpeg 链路更能隔离 Picoo 当前传输与重组问题，因为不会同时更换 Encoder、容器、packetization 和 transport。

### F-05 端到端延迟缺少统一时钟边界

标签：`Repository Gap`

当前 `pts_us` 是媒体时间，不能在没有时钟同步的情况下直接与 Receiver wall clock 相减并称为 capture-to-render 延迟。需要先定义：

- Sender monotonic capture/encode 时间；
- Receiver monotonic arrival/decode/publish 时间；
- 双端 clock offset/uncertainty，或仅测量单端分段延迟；
- UI 中 RTT、frame age 和 E2E 的不同名称。

在此之前，200 ms 的用户观感不能仅归因于网络。

### F-06 全链路结构化关联不足

标签：`Repository Gap`

诊断记录应能按以下标识关联：

```text
monotonic_timestamp
session_id
stream_epoch
device_id
frame_id
packet_sequence          # 字段引入后
state_transition_reason
```

日志不得包含配对密钥、完整证书或敏感网络凭据。

## 9. 待验证假设

| 假设 | 为什么合理 | 验证方式 | 不能先下的结论 |
| --- | --- | --- | --- |
| H-01 VCam 存在高频 request pump | 当前每次立即返回；CatCam 在特定环境观察过 | Win11 记录 requests/fresh/cached per second | 不能先认定同步 sleep 是唯一修复 |
| H-02 200+ ms 主要来自排队 | Encoder、50 ms jitter、Sender/Quinn/VCam 都可能排队 | 分段 monotonic timestamp 与 queue age | 不能只看 QUIC RTT |
| H-03 马赛克主要来自 Datagram 丢失 | 不完整或参考链损坏会产生类似现象 | packet/AU loss + decoder recovery 日志 | 不能排除码率、Decoder、NV12、颜色和裁切问题 |
| H-04 反馈提高到 100–200 ms 会改善 ABR | RFC 8888 支持某些算法使用该范围 | replay 仿真与弱网真机 A/B | 不能让当前 1 秒 AIMD 直接以 10 Hz 震荡 |
| H-05 SRT 对照能辅助定位 | 可靠重传和固定 latency 可提供不同基准 | 相同编码 AU 的受控实验 | 使用不同 Encoder/容器时不能归因到 QUIC 单一变量 |

## 10. 架构候选判断

### 10.1 保留 QUIC，借媒体语义，不引入完整 RTP/WebRTC

建议保持现有 [ARCH-PICOO-TRANSPORT-001](../design-specs/architecture/0002-quic-transport-encapsulation-boundary.md) 与 [ARCH-PICOO-PROTOCOL-001](../design-specs/architecture/0003-picoo-camera-protocol-boundary.md)。

借鉴内容：sequence、完整媒体单元、prediction-chain recovery、feedback。

不引入内容：RTP/RTCP wire compatibility、ICE、STUN/TURN、SRTP、WebRTC signaling 和公网 NAT traversal。

### 10.2 `stream_epoch` 继续作为唯一编码世代

不建议新增并行的 `config_id`。重大编码配置变化继续通过可靠 `StreamConfig` 与 `stream_epoch` 提交；Receiver 只接受匹配已确认 epoch 的 Datagram。

如果未来允许一个 connection 内多路视频，再评审 `stream_id`，而不是为 V1 单路提前增加字段。

### 10.3 快速恢复信号与慢速 ABR 分离

关键帧恢复需要低延迟，但码率决策需要稳定窗口。候选模型：

```text
即时/事件驱动：关键帧损坏、Decoder 重建、epoch/config 变化
较快健康反馈：packet/AU completion、queue age、stall
稳定 ABR 窗口：码率、分辨率、FPS 决策
```

具体间隔必须通过 replay 与真机测试确定，不能从 RFC 的区间直接复制。

### 10.4 VCam 作为独立故障域

继续保持现有边界：VCam DLL 不持有网络和 Decoder。网络重连不得重新注册虚拟摄像头；断流只改变 Shared Frame Ring 内容和 Receiver 状态。

### 10.5 SRT 只作为可选诊断实验

在 `picoo-probe`、Decoder Recovery 和 VCam 指标完成前，不建议投入 SRT 对照线。若后续实施，应尽可能复用同一 H.264 AU，避免同时替换 Camera/Encoder 造成错误归因。

## 11. 分层验证入口

为了把相同的“卡住/黑屏/花屏”拆成独立故障域，建议保留以下验证矩阵：

| 验证入口 | 输入 | 输出 | 排除范围 |
| --- | --- | --- | --- |
| Sender 本地编码 | Camera → Encoder | `.h264` / AU 统计 | 排除网络、Receiver、VCam |
| `picoo-probe` | PCP QUIC | 分包、重组、stall 指标 | 排除 Decoder、FrameHub、VCam |
| Receiver 普通预览 | QUIC → Decoder → FrameHub | GPUI 预览和解码指标 | 排除 VCam |
| 合成 NV12 → VCam | 生成器 → Shared Ring | Teams/Zoom/OBS/浏览器 | 排除 Camera、Encoder、网络、Decoder |
| 完整真机链路 | Android/iOS → 会议软件 | 用户可见结果与全链路日志 | 集成验收 |

Windows VCam 最终证据必须来自 GitHub Actions 构建产物与 Win11 真机；Linux 上的 Rust 测试不能替代 Frame Server 行为验收。

## 12. Design Spec 转化建议

本文不直接分配新的稳定 Requirement ID。进入实现前，应按以下映射评审：

| 研究发现 | 应评审的长期文档 | Requirement 处理建议 |
| --- | --- | --- |
| F-01 Decoder Recovery | `ARCH-PICOO-SESSION-001`、`ARCH-PICOO-MEDIA-001` | 扩展 `REQ-PICOO-SESSION-003` 或新增独立恢复 Requirement |
| F-02 packet sequence / loss feedback | `ARCH-PICOO-PROTOCOL-001`、`ARCH-PICOO-SESSION-001` | 更新 `REQ-PICOO-PROTOCOL-001/006` 前先确定协议兼容与验收 |
| F-03 VCam cadence | `ARCH-PICOO-VCAM-001` | 新增可测量 pacing/合法 sample Requirement，不能只写实现方式 |
| F-04 `picoo-probe` | `ARCH-PICOO-STACK-001`、verification | 作为开发诊断入口和 CI/testkit 能力，不冒充产品真机验收 |
| F-05 E2E clock boundary | `ARCH-PICOO-SESSION-001` | 修订 `REQ-PICOO-SESSION-007` 的测量定义 |
| F-06 structured correlation | Session/Privacy/Diagnostics 相关 Architecture | 明确字段、保留周期和脱敏验收 |

只有完成 Architecture 与 Requirement 对齐后，才进入代码实现。实现验收应覆盖：正常流、缺 delta 片、缺 IDR 片、连续 Decoder error、epoch 变化、断网重连、会议软件重复开关和无 Sender fallback。

## 13. 参考资料

### 标准与官方文档

- [RFC 9221: An Unreliable Datagram Extension to QUIC](https://www.rfc-editor.org/rfc/rfc9221.html)
- [RFC 6184: RTP Payload Format for H.264 Video](https://www.rfc-editor.org/rfc/rfc6184.html)
- [RFC 4585: Extended RTP Profile for RTCP-Based Feedback](https://www.rfc-editor.org/rfc/rfc4585.html)
- [RFC 8888: RTCP Feedback for Congestion Control](https://www.rfc-editor.org/rfc/rfc8888.html)
- [Microsoft: Frame Server Custom Media Source](https://learn.microsoft.com/en-us/windows-hardware/drivers/stream/frame-server-custom-media-source)
- [Microsoft Windows-Camera VirtualCamera Sample](https://github.com/microsoft/Windows-Camera/tree/master/Samples/VirtualCamera)
- [Microsoft: MFCreateVirtualCamera](https://learn.microsoft.com/en-us/windows/win32/api/mfvirtualcamera/nf-mfvirtualcamera-mfcreatevirtualcamera)

### 外部实现

- [CatCam](https://github.com/igorfyago/CatCam)
- [CatCam MediaStream RequestSample](https://github.com/igorfyago/CatCam/blob/f707f87365b442100247d31866fa101a337d4652/windows/MediaStream.cpp#L181-L258)
- [RootEncoder](https://github.com/pedroSG94/RootEncoder)
- [RootEncoder-iOS](https://github.com/pedroSG94/RootEncoder-iOS)
- [VCamSample](https://github.com/smourier/VCamSample)

### Picoo 设计上下文

- [Design Specs context](../design-specs/context.md)
- [ARCH-PICOO-TRANSPORT-001](../design-specs/architecture/0002-quic-transport-encapsulation-boundary.md)
- [ARCH-PICOO-PROTOCOL-001](../design-specs/architecture/0003-picoo-camera-protocol-boundary.md)
- [ARCH-PICOO-MEDIA-001](../design-specs/architecture/0004-cross-platform-media-pipeline-boundary.md)
- [ARCH-PICOO-SESSION-001](../design-specs/architecture/0005-session-reconnect-jitter-bitrate-boundary.md)
- [ARCH-PICOO-VCAM-001](../design-specs/architecture/0007-virtual-camera-platform-boundary.md)
- [PUC-004：在会议软件中使用虚拟摄像头](../design-specs/use-cases/product/puc-004-use-virtual-camera-in-meeting-apps.md)
- [PUC-006：网络中断后自动恢复](../design-specs/use-cases/product/puc-006-auto-reconnect-after-network-interruption.md)
