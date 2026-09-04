# Picoo Recording：本地多轨录制、桌面后处理与 Agent 剪辑研究

日期：2026-09-03
状态：Research，非规范性
范围：飞书继续负责实时会议；Picoo 负责独立素材录制、汇集、桌面离线处理、非破坏性编辑与 Agent 剪辑
明确不包含：会议中实时把抠图人像叠加到投屏画面的 `Picoo Stage`

> 本文用于保存需求收敛、仓库审计、候选架构、依赖边界和验证计划，不直接定义稳定产品行为。
> 经验证并确认的决策仍应分别进入 Use Case、Architecture 和 Requirement；实现不得只引用本文作为验收依据。

## 1. 研究问题

本次研究回答以下问题：

1. 团队继续使用飞书开会时，Picoo 应如何增加高质量多轨录制能力，而不变成另一套会议系统？
2. 摄像头、麦克风和纯屏幕内容应在哪里录制，如何避免飞书与 Picoo 争抢同一设备？
3. 一场 2～3 人、1080p、30～60 分钟会议会产生多少素材，如何可靠保存并汇集？
4. 多个人分别录制的视频和音频如何映射到同一时间轴？
5. 人像抠图、布局、字幕、音频混合和最终编码为什么应集中在桌面端离线处理？
6. Agent 应修改什么，才能安全、可撤销地完成长视频剪辑和画面编排？
7. 这项能力如何进入当前仓库，又不污染正在稳定的实时 Sender → Receiver → Virtual Camera 主链路？

## 2. 结论摘要

### 2.1 产品边界

`Candidate Decision`

团队继续使用飞书完成邀请、通话、实时视频、屏幕共享和协作。Picoo 不实现会议房间、SFU、聊天、举手或日程系统。

新增能力建议拆成三个连续但独立的对象：

```text
Picoo Recording
  保存本地独立原始 Track

Picoo Edit Project
  保存非破坏性的剪辑与布局决定

Picoo Processor
  在一台桌面电脑上同步、抠图、转录、混音和渲染
```

现有 `Picoo Camera` 继续解决实时摄像头输入；新能力解决会议结束后的可编辑素材与成片。

### 2.2 架构边界

`Candidate Decision`

新能力不扩展 PCP 实时 Datagram 数据面，也不把 `FrameHub` 当作录像存储。实时视频和归档素材的目标不同：

- 实时链路追求最新帧、低延迟、允许丢弃过期内容；
- Recording 追求完整、可恢复、可校验、可续传和可重复处理。

两者可以共享设备身份、平台媒体能力和部分 QUIC 基础设施，但必须使用独立的 Session 语义、文件格式和传输协议。

### 2.3 录制与处理位置

`Candidate Decision`

- 原始素材尽量在源设备本地录制；网络失败不能破坏录像。
- 后处理集中在一台用户选择的 Windows 或 macOS 电脑。
- 云端只承担可选的协调与临时交换，不承担默认 GPU 处理。
- 原始素材是事实，任何抠图、剪辑、布局、字幕和最终视频都是可重新生成的派生产物。

### 2.4 Agent 边界

`Candidate Decision`

Agent 不直接修改 MP4，不直接拼接二进制文件，也不生成任意 shell/FFmpeg 命令。Agent 只读取转录、Track 元数据、关键帧和现有工程，然后输出经过 Schema 校验的编辑操作。

```text
Raw Tracks
  ↓
Transcript / Index / Keyframes
  ↓
Agent 生成 Edit Operations
  ↓
Picoo Edit Project
  ↓
确定性 Renderer
  ↓
final.mp4
```

## 3. 当前仓库审计

### 3.1 当前 V1 明确排除本需求

`Confirmed Fact`

现有 [产品 PRD V1.0](../product/picoo-camera-prd-v1.0-2026-08-27.md) 明确规定：

- 视频默认不保存；
- 不做本地录像；
- 不做手机麦克风传输；
- 不做多手机同时输入；
- 不做 AI 背景替换；
- 不做账号、组织和云同步。

因此 Picoo Recording 是一个新的产品能力，不应以“小改动”的方式塞进 V1 Requirement。

### 3.2 当前 Workspace 没有 Recording / Edit / Render 模块

`Confirmed Fact`

当前根 [Cargo.toml](../../Cargo.toml) 包含协议、分包、配对、发现、会话、传输、抖动、码率、解码、FrameHub、Sender、Receiver、FFI、桌面端与虚拟摄像头模块，没有录像资产、编辑时间线、后处理任务或渲染模块。

当前 [桌面应用](../../apps/desktop/) 主要负责 Receiver Runtime、实时预览、诊断、托盘和虚拟摄像头管理。新增处理任务不应运行在 GPUI 渲染线程或现有 Receiver 热路径中。

### 3.3 `FrameHub` 不适合作为归档层

`Confirmed Fact`

[ARCH-PICOO-FRAME-001](../design-specs/architecture/0006-framehub-shared-frame-ring-boundary.md) 与 `picoo-frame-hub` 面向实时最新帧消费和固定容量共享帧环。它适合把解码后的 NV12 交给预览和虚拟摄像头，不负责历史帧、持久化、校验、断点恢复或多小时素材。

Recording 必须在 FrameHub 之外保存独立 Track。

### 3.4 当前实时媒体约束会影响手机本地录像

`Confirmed Fact`

[ARCH-PICOO-MEDIA-001](../design-specs/architecture/0004-cross-platform-media-pipeline-boundary.md) 规定 Android 使用 Camera2、EGL/GLES 与单个硬件 H.264 MediaCodec Encoder，并特别考虑只支持单个硬件编码器的设备。当前实时链路还允许 ABR 在 1080p、720p 和 480p 之间变化。

因此手机端不能未经验证就增加“第二个固定 1080p Encoder”。手机本地母版至少存在两个候选方案：

1. 同一份 H.264 Access Unit 同时发送并写入本地分段文件；实现轻，但母版会继承实时 ABR 的码率和分辨率变化；
2. 能力探测通过时启用独立录像 Encoder；母版稳定，但增加发热、功耗和设备兼容风险。

本文不锁定该选择，第一轮 PoC 应优先验证桌面 Webcam 录制和现有手机码流旁路保存。

### 3.5 实时主线与 Recording 必须隔离

`Repository Observation`

以下 V1 连通性与虚拟摄像头问题已按真机验证关闭，不应再当作进行中的阻塞：

- [#25](https://github.com/Haoxincode/picoo-camera/issues/25) Windows 虚拟摄像头闪烁并回退到 Disconnected
- [#23](https://github.com/Haoxincode/picoo-camera/issues/23) Android 无法稳定发现 macOS Receiver 的 mDNS 服务
- [#22](https://github.com/Haoxincode/picoo-camera/issues/22) macOS Receiver 无法接收入站 UDP/QUIC
- [#21](https://github.com/Haoxincode/picoo-camera/issues/21) Android 手动 IP 连接失败会被自动直连其他 Receiver 掩盖

当前仍开着的 V1 问题是 Android 发布 APK 缺少稳定签名、无法可靠覆盖升级（[#24](https://github.com/Haoxincode/picoo-camera/issues/24)）。这是发布供应链边界，不是实时媒体生命周期问题。

即使实时连通性和虚拟摄像头输出已按真机验证，Recording 仍不应改变 PCP、Receiver Recovery、FrameHub 或 VCam 的生命周期。第一轮应通过 feature boundary 和独立 Worker 隔离。

## 4. 目标用户场景

一次三人飞书会议：

```text
飞书
├── 实时语音与视频
├── 正常共享 PPT / 网页 / Demo
└── 现有会议协作

Picoo Recording（并行）
├── Host Camera + Local Mic
├── Guest A Camera + Local Mic
├── Guest B Camera + Local Mic
├── Clean Screen Track
└── Optional Feishu Reference Audio
```

会议过程中，Picoo 只保证每个本地 Track 被可靠保存。人物不会提前烧录进纯屏幕画面。

会议结束以后：

```text
独立 Track 汇集到 Processor Desktop
  ↓
时间映射与同步
  ↓
转录、关键帧、代理素材
  ↓
人物 Matting
  ↓
自动或 Agent 生成 Edit Project
  ↓
用户调整人物大小、位置、出现时间和 Screen 比例
  ↓
渲染一个或多个输出版本
```

同一组原始素材可生成演示版、双人访谈版、竖屏精华版等不同成片。

## 5. 候选总体架构

```text
                         飞书会议
              实时交流 / 实时视频 / Screen Share
                             │
─────────────────────────────┼─────────────────────────────
                             │ 独立运行

Participant A Desktop                   Participant B Desktop
Camera / Mic                            Camera / Mic
     ↓                                       ↓
Local Segmented Recorder                Local Segmented Recorder
     ↓                                       ↓
Local Authoritative Store               Local Authoritative Store
     │                                       │
     └────────── Resumable Asset Transfer ───┘
                             ↓
                      Processor Desktop
                             │
             ┌───────────────┼────────────────┐
             ↓               ↓                ↓
          Ingest          Media Index       Time Map
             ↓               ↓                ↓
         Matting         Transcript       Audio Sync
             └───────────────┼────────────────┘
                             ↓
                       Picoo Edit Project
                             ↓
                 Agent / GUI / CLI Edit Operations
                             ↓
                     Deterministic Renderer
                             ↓
                         final.mp4
```

控制服务是可选能力：

```text
Picoo Coordinator
├── Session 邀请与临时授权
├── Participant / Track Manifest
├── Peer 地址协商
└── 临时对象存储签名

不承载默认视频中继，不执行后处理。
```

## 6. 核心领域模型

Recording 领域不应复用现有实时 `picoo-session::Session` 名称和状态机。建议先定义独立模型：

| 对象 | 职责 |
| --- | --- |
| `RecordingSession` | 一次会议素材集合及其生命周期 |
| `Participant` | 参与者身份与本次录制角色 |
| `Track` | 一路连续媒体语义，如 camera、microphone、screen |
| `AssetSegment` | Track 的可校验、可续传物理分段 |
| `TimeMap` | Local PTS 到 Session Time 的映射与漂移修正 |
| `DerivedArtifact` | Proxy、缩略图、转录、Alpha、波形等派生产物 |
| `EditProject` | 非破坏性剪辑、布局、字幕和混音决定 |
| `RenderJob` | 将指定 Project 编译并渲染为输出文件的任务 |

候选生命周期：

```text
Created
  → Recording
  → Finalizing
  → Collecting
  → Ready
  → Processing
  → Editable
  → Rendering
  → Completed
```

每个阶段允许 `Partial`、`Failed`、`Cancelled`，但已经完成的本地 Segment 不能因网络或后处理失败而丢失。

## 7. Capture 设计

### 7.1 桌面 Webcam：Picoo 应优先成为设备拥有者

`Candidate Decision`

最稳定的路径是 Picoo Desktop 打开物理 Webcam 一次，再分发到两个消费者：

```text
Physical Webcam
      ↓
Platform-native Capture
      ├──→ FrameHub / Picoo Virtual Camera → 飞书
      └──→ Local Encoder / Segmented Recorder
```

这样飞书选择的是 `Picoo Camera` 虚拟摄像头，而不是与 Picoo 同时直接打开物理设备。

Windows 的 `MediaCaptureSharingMode.SharedReadOnly` 确实允许只读获取已被其他应用使用的 Camera Frames，但不能控制格式，并可能受到另一个独占客户端的配置影响。它可以作为兼容模式，不应成为跨平台基线：

- [Process media frames with MediaFrameReader](https://learn.microsoft.com/en-us/windows/apps/develop/camera/process-media-frames-with-mediaframereader)
- [MediaCaptureInitializationSettings.SharingMode](https://learn.microsoft.com/en-us/uwp/api/windows.media.capture.mediacaptureinitializationsettings.sharingmode)

### 7.2 手机 Camera：保持可选增强，不成为团队准入条件

参与者可以使用 Picoo Mobile，也可以直接使用电脑内置或 USB Webcam。Recording 的要求是得到本地独立 Track，不是要求所有人必须使用手机。

手机端第一轮只验证以下两种路径，不立即承诺双 Encoder：

```text
A. Existing H.264 AU
   ├── QUIC Live Stream
   └── Local Segmented File

B. Camera Source
   ├── Low-latency Live Encoder
   └── Independent Recording Encoder（能力探测后启用）
```

### 7.3 纯屏幕 Track

Screen Track 保存未叠加人物、Logo 和字幕的原始演示内容：

- Windows 使用 `Windows.Graphics.Capture` 获取窗口或显示器帧；微软官方文档也给出了将捕获帧编码成视频文件的路径：
  - [Screen capture](https://learn.microsoft.com/en-us/windows/apps/develop/media-authoring-processing/screen-capture)
  - [Screen capture to video](https://learn.microsoft.com/en-us/windows/uwp/audio-video-camera/screen-capture-video)
- macOS 使用 `ScreenCaptureKit` 获取窗口、显示器和系统音频：
  - [ScreenCaptureKit](https://developer.apple.com/documentation/screencapturekit)

Screen Capture 是平台 Adapter，不进入通用 Rust Domain。

### 7.4 音频 Track

每个参与者至少保存自己的本地麦克风 Track；视频和麦克风必须共享同一 Local Monotonic Timebase。

Processor Desktop 可选保存飞书参考音频：

- Windows 使用 WASAPI Loopback；如需隔离飞书进程，可评估微软的 `ActivateAudioInterfaceAsync` Application Loopback Sample：
  - [Loopback Recording](https://learn.microsoft.com/en-us/windows/win32/coreaudio/loopback-recording)
  - [Application loopback audio capture sample](https://learn.microsoft.com/en-us/samples/microsoft/windows-classic-samples/applicationloopbackaudio-sample/)
- macOS 使用 ScreenCaptureKit 的应用/系统音频输出，麦克风仍可作为独立 Track 采集。

飞书参考音频只用于校验、辅助同步和故障兜底。它包含网络延迟和会议端处理，不能被定义为所有本地 Track 的唯一主时钟。

## 8. 媒体规格、数据量与本地存储

### 8.1 第一版规格

`Candidate Decision`

- Camera：1920×1080、30 FPS、H.264、SDR；
- 建议 Camera 码率：6～8 Mbps；
- Screen：按内容复杂度约 2～4 Mbps；
- Audio：48 kHz，录制阶段可保存无损/低损中间格式，输出阶段统一混音；
- 不以 4K 为目标。

按十进制 GB 粗算：

| 素材 | 30 分钟 | 60 分钟 |
| --- | ---: | ---: |
| 1 路人物 @ 6～8 Mbps | 1.35～1.80 GB | 2.70～3.60 GB |
| 1 路屏幕 @ 2～4 Mbps | 0.45～0.90 GB | 0.90～1.80 GB |
| 2 人 + 屏幕 | 3.15～4.50 GB | 6.30～9.00 GB |
| 3 人 + 屏幕 | 4.50～6.30 GB | 9.00～12.60 GB |
| 每路压缩音频 @ 128～256 kbps | 0.03～0.06 GB | 0.06～0.12 GB |
| 每路 PCM @ 48 kHz / 16-bit / mono | 约 0.17 GB | 约 0.35 GB |

因此三人一小时会议在常用压缩音频下通常约为 9～13 GB；若所有本地麦克风都保留单声道 PCM，应额外预留约 1 GB。Manifest、转录和缩略图相对于媒体仍较小。2～3 人、半小时到一小时属于普通桌面视频处理规模，主要挑战是自动汇集和故障恢复，不是总容量本身。

### 8.2 存储不变量

`Candidate Decision`

1. 本地副本是录制事实源，上传不能反压 Camera 或影响飞书。
2. 不把一个长时间、未完成的单体 MP4 作为唯一事实源。
3. Track 应写成可独立校验和恢复的 Segment，并使用原子 Manifest 记录完成状态。
4. 每个 Segment 保存内容哈希、字节数、媒体时间范围和 Codec 配置引用。
5. 原始 Track 只追加，不被 Matting、Agent 或 Renderer 覆盖。
6. 派生产物可删除并重建。

容器候选为 fragmented MP4 或短时独立 MP4 Segment。最终选择必须通过断电/强杀、长时间录制、Seek、拼接和跨平台兼容实验确认。

候选目录：

```text
recordings/<session-id>/
├── session.json
├── tracks/
│   ├── <participant-id>-camera/
│   │   ├── manifest.json
│   │   └── segments/
│   ├── <participant-id>-mic/
│   ├── screen/
│   └── meeting-reference/
├── derived/
│   ├── proxies/
│   ├── waveforms/
│   ├── transcripts/
│   ├── thumbnails/
│   └── mattes/
├── edit/
│   └── default.picoo-edit.json
└── outputs/
```

## 9. 时间同步与最终音频

### 9.1 主时间基准

`Candidate Decision`

每个 Track 必须记录：

- 本地单调时钟下的 PTS；
- Track 开始时的 Local Monotonic Anchor；
- 与 Session Coordinator 多次交换得到的时钟偏移、RTT 和不确定度；
- Codec / Capture Pipeline 引入的已知延迟；
- 后处理估计出的漂移修正。

禁止只用墙上时钟或文件创建时间对齐长视频。

### 9.2 精同步

```text
Clock Mapping 粗同步
  ↓
本地 Camera 与 Mic 的共同 PTS 保持口型同步
  ↓
音频特征 / 波形用于偏移和漂移校验
  ↓
生成每个 Track 的 TimeMap
```

飞书参考音频可以辅助发现缺口和对齐事件，但远端声音到达参考 Track 时已经经过网络，不能直接把该到达时间当作远端摄像头的采集时间。

### 9.3 最终音频

最终成片不选择某一个人的整条音频作为唯一来源，而是生成新的 Mix：

```text
Host Local Mic
+ Guest A Local Mic
+ Guest B Local Mic
+ Optional Screen / Media Audio
  ↓
TimeMap
  ↓
Noise / Level / Bleed Control
  ↓
final-audio
```

Track 身份天然提供 Speaker 信息；转录不必依赖单路混音再做完整说话人分离。若本地麦克风录到扬声器串音，后处理需通过活动说话 Track、门限、回声抑制或用户选择降低重复声音。

## 10. 素材传输与临时存储

### 10.1 Local-first, Transfer-later

`Candidate Decision`

录制先落本地，会议中可使用剩余带宽渐进传输，带宽不足时自动降速或暂停。传输失败只改变 `TransferState`，不能改变 `RecordingState`。

### 10.2 目标路径选择

```text
1. 同一局域网
   → 可靠 QUIC / TCP 直传

2. 不同网络且双方在线
   → 协调服务交换身份和候选地址
   → NAT Traversal 后 P2P 直传

3. P2P 失败或 Processor 不在线
   → R2 / S3-compatible 临时对象存储
   → Processor 下载并校验
   → TTL / 完成后删除
```

Coordinator 只负责身份、授权、Peer Discovery、Manifest 和临时上传凭证，不承载默认大文件 Relay。

### 10.3 与现有 QUIC 的关系

当前 `picoo-transport` 使用 Quinn + Rustls，可以复用证书、身份和 QUIC 工程经验，但 Recording 文件传输应使用独立 ALPN、可靠 Stream、Chunk Manifest 和断点协议，不能塞进 PCP 实时媒体 Datagram。

远程 P2P 可评估 [iroh](https://github.com/n0-computer/iroh) 与 [iroh-blobs](https://github.com/n0-computer/iroh-blobs)。当前 `iroh-blobs` 主线 README 自己标记为尚未达到 production quality，并建议生产需求暂用 0.35，因此它只能作为候选依赖，不能在 Research 阶段直接锁入稳定架构。

### 10.4 R2 的定位

R2 只作为失败兜底和异步交换区，不是长期媒体库。Cloudflare 当前 Standard R2 存储为按 GB-month 和操作量计费，Internet egress 免费；临时保存 10 GB 级素材的成本主要来自短期存储和请求，而不是下载流量。见 [R2 Pricing](https://developers.cloudflare.com/r2/pricing/)。

对象存储必须通过 Provider 接口隔离，不能把 Recording Domain 绑定到 Cloudflare 账号、Bucket 或 SDK。

## 11. Desktop Processing Pipeline

`Candidate Decision`

Processor 是独立 Job Runtime，不在 GPUI View 或实时 Receiver Loop 内执行。推荐顺序：

```text
1. Ingest / Validate
   校验 Manifest、Hash、Codec 和时长

2. Time Mapping
   生成统一 Session Time 与每个 Track 的 TimeMap

3. Media Index
   生成 Proxy、Thumbnail、Waveform、Scene / Screen Activity Index

4. Transcription
   每个 Mic Track 独立转录并保留 source time

5. Matting
   为选中的 Camera Track 生成 foreground + alpha

6. Edit Project
   自动布局或 Agent / GUI 修改非破坏性工程

7. Render Plan
   校验工程并编译成确定性处理图

8. Render
   输出 1080p MP4、字幕与可选平台版本
```

所有中间结果按输入 Hash、Provider 版本和参数缓存。某个 Matting 或 Transcription Worker 升级后，可以只使相关 Derived Artifact 失效，不必重新录制或重新传输原始素材。

## 12. Agent 剪辑接口

### 12.1 Canonical Project 由 Picoo 定义

`Candidate Decision`

Picoo 应定义版本化 JSON Schema 作为事实源，不直接把 FFmpeg Filtergraph、OTIO Python 对象或某个 Agent SDK 当作工程格式。

最小编辑操作包括：

- `keep_source_range`
- `remove_source_range`
- `set_layout`
- `set_subject_transform`
- `set_screen_transform`
- `set_track_visibility`
- `select_audio_source`
- `set_gain`
- `add_caption`
- `add_chapter`

示意：

```json
{
  "schema_version": 1,
  "operations": [
    {
      "op": "remove_source_range",
      "from_ms": 0,
      "to_ms": 92000,
      "reason": "删除会议前寒暄"
    },
    {
      "op": "set_layout",
      "from_ms": 540000,
      "to_ms": 780000,
      "layout": "screen_with_two_cutouts"
    },
    {
      "op": "set_subject_transform",
      "track_id": "guest-a-camera",
      "from_ms": 610000,
      "to_ms": 690000,
      "scale": 1.25,
      "anchor": "bottom-right"
    }
  ]
}
```

所有操作引用原始 Session Time；输出时间由 Renderer 根据保留区间推导。这样用户撤销删除或换布局时不会重新切割原始文件。

### 12.2 Agent 输入

Agent 默认读取：

- 带 Track ID 和 Source Time 的 Transcript；
- Session Chapters；
- Screen Activity；
- 关键帧与缩略图；
- 当前 Edit Project；
- 用户目标，如“保留架构讨论，控制在 15 分钟”。

只有需要视觉判断时才读取对应时间段的低分辨率 Proxy 或 Keyframe，避免反复处理一小时原始视频。

### 12.3 Agent 安全边界

1. Agent 输出必须通过 JSON Schema 和引用完整性校验。
2. Agent 不能得到原始素材删除权限。
3. Agent 不能输出任意路径或 shell 命令。
4. 每次修改生成新的 Project Revision，并保留原因、输入范围和生成者。
5. GUI、CLI 和 Agent 操作同一份 Project，而不是维护三套编辑逻辑。

### 12.4 OpenTimelineIO 的位置

[OpenTimelineIO](https://opentimeline.io/) 是编辑剪切信息的交换格式，保存 Cut 顺序、时长和外部媒体引用，并不是媒体容器。建议把它作为 Premiere / Resolve / 其他工具的导入导出 Adapter，而不是 Picoo 第一版内部 Domain 的唯一模型。

## 13. 候选依赖与隔离方式

| 能力 | 候选依赖 | 进入 Picoo 的方式 |
| --- | --- | --- |
| Recording / Edit Domain | `serde`、`serde_json` | Rust Core crate，版本化 Schema |
| Segment 校验 | BLAKE3 | Rust Core，内容哈希与断点校验 |
| Windows Camera / Screen / Audio | `windows-rs` + Media Foundation + Windows.Graphics.Capture + WASAPI | 平台 Adapter；沿用当前 Rust 原生系统绑定路线 |
| macOS Camera / Screen / Audio | AVFoundation、ScreenCaptureKit、CoreMedia/CoreAudio；优先评估 `objc2-*` 生成式绑定 | 平台 Adapter；不让原始帧跨移动 FFI |
| Media Probe / Compose / Encode | FFmpeg 外部可执行文件 | Provider / Worker；不直接成为默认 Cargo 链接依赖 |
| Video Matting | MatAnyone 2 及后续模型 | Python/uv 外部 Worker，通过版本化 Job JSON 和目录交换 |
| First-frame Mask | 人物检测 / SAM2 / 手工确认 | Matting Provider 的前处理，不进入 Recording Domain |
| Transcription | whisper.cpp 或其他本地 ASR | 可替换 Worker；默认 Workspace 不依赖 CMake |
| Timeline Interchange | OpenTimelineIO | 可选导入导出 Adapter |
| LAN Transfer | Quinn reliable streams | 独立 Asset Transfer Protocol |
| Remote P2P | iroh / iroh-blobs 候选 | Adapter，须先固定版本并验证成熟度 |
| Temporary Object Storage | R2 / S3-compatible | Provider；使用短时凭证与 TTL |
| Desktop UI | 现有 GPUI / gpui-component | 仅展示 Session、Track、进度和 Edit Project |

### 13.1 FFmpeg License 边界

FFmpeg 官方说明默认使用 LGPL 2.1-or-later，但启用部分 GPL 组件后，整个 FFmpeg 构建适用 GPL。正式分发前必须固定 configure flags、来源、版本和 NOTICE；不能把系统中任意 FFmpeg 二进制视为可直接打包的依赖。见 [FFmpeg License and Legal Considerations](https://ffmpeg.org/legal.html)。

第一轮 PoC 可使用开发者提供的外部 FFmpeg；正式发行再决定使用平台原生 Encoder、LGPL-compatible FFmpeg build，或只提供 Provider 接口。

### 13.2 MatAnyone 2 边界

[MatAnyone 2](https://github.com/pq-yang/MatAnyone2) 当前提供 Python/uv 安装、视频 + 首帧 Mask 输入，以及 foreground / alpha 输出，适合作为离线质量 Benchmark。它采用 NTU S-Lab License 1.0，允许非商业用途的使用与再分发，但必须保留许可声明；它不是 Picoo `MIT OR Apache-2.0` Core 的直接依赖。

因此建议：

- Core 只定义 `MattingProvider` Job Contract；
- 模型、Checkpoint、Python Environment 与输出缓存由独立 Worker 管理；
- 用户可以替换为其他模型；
- 默认发行包是否携带模型需要单独的分发与体积评审。

### 13.3 Transcription 边界

[whisper.cpp](https://github.com/ggml-org/whisper.cpp) 可作为本地 ASR 候选，但不应让默认 Cargo Workspace 或仓库构建依赖 CMake。优先使用可下载、带版本与校验值的独立 Worker，或保留 Provider 接口由用户自行配置。

## 14. 候选仓库结构

经 Research 和 PoC 确认后，可以评审以下边界；现在不应直接创建全部 crate：

```text
crates/
├── picoo-recording/       # Session、Track、Manifest、TimeMap、状态机
├── picoo-asset-store/     # 本地 Segment、Hash、原子 Manifest、Cache
├── picoo-edit/            # 非破坏性 Project、Operation、Schema
├── picoo-render/          # Provider-neutral Render Plan 与 Job 状态
└── picoo-transfer/        # Chunk / Resume / LAN / P2P / Object adapters

apps/desktop/src/
├── recording/             # GPUI orchestration，不放媒体算法
├── platform_capture/      # Windows/macOS Camera、Screen、Audio adapters
└── processor/             # Worker supervision、Job progress、recovery

tools/processors/
├── matting-matanyone2/    # 可选外部 Worker，不加入默认 Cargo Workspace
└── transcription/        # 可选外部 Worker
```

边界规则：

- `picoo-recording` 不依赖 GPUI、FFmpeg、Python、R2 或某个平台 API；
- `picoo-edit` 不读取视频文件，只操作稳定 ID 和 Source Time；
- `picoo-render` 生成受控 Render Plan，不拼接任意 shell 字符串；
- 平台采集 Adapter 不复制 Session / Project 业务状态；
- 外部 Worker 通过版本化 JSON/JSONL、stdin/stdout 或 Job Directory 通信；
- 当前 PCP、Sender、Receiver、Jitter、FrameHub 和 VCam 不因 Recording 引入新的阻塞路径。

## 15. 第一轮 PoC

建议先做一个不会影响 V1 主线的桌面 PoC，回答最高风险，而不是直接实现完整 UI 和远程服务。

### 15.1 PoC 范围

1. Windows Picoo Desktop 独占一个物理 Webcam；
2. 同一 Capture 同时输出到现有 FrameHub / VCam 和本地 1080p30 分段录像；
3. 录制一个纯 Screen Track、本地 Mic 和飞书参考音频；
4. 用两到三组测试 Track 建立 `RecordingSession` 与 `TimeMap`；
5. 通过 MatAnyone 2 Worker 为 10 分钟人物素材生成 Alpha；
6. 建立最小 `picoo-edit.json`，支持删除范围、Screen + 两人布局、人物缩放和位置；
7. 通过受控 FFmpeg Worker 输出 1080p MP4；
8. 用脚本模拟 Agent 生成 Operations，并验证撤销后可以无损恢复。

### 15.2 必须测量的证据

- 60 分钟 1080p30 本地录像是否有掉帧、时间戳跳变和不可恢复文件；
- 强杀进程后已完成 Segment 是否可读取，Manifest 是否可恢复；
- VCam 与 Local Recorder 并行时是否影响现有实时延迟和稳定性；
- 2～3 人 Track 的时钟偏移和一小时漂移；
- 音频同步后口型与对话是否自然；
- Matting 在 NVIDIA、Apple Silicon 和 CPU-only 环境的质量、速度和显存；
- 同一 Raw Session 重渲染两种 Layout 是否无需重新录制；
- Agent 删除、恢复、换布局是否只修改 Edit Project；
- Transfer 限速、暂停和失败是否完全不影响本地录像；
- 真实数据量是否落在本文估算范围。

## 16. 暂不确认的决策

以下问题需要 PoC 证据，不能在本文中伪装为稳定架构：

1. 手机母版是否复用实时 Encoder，还是能力探测后运行第二 Encoder；
2. Windows/macOS 是否都要求 Picoo 独占物理 Webcam，还是允许 Shared Read-only 兼容模式；
3. fMP4、短 MP4、Matroska 中哪一种作为长期 Segment Container；
4. Quinn 自研 Asset Protocol 与 iroh-blobs 的最终取舍；
5. 是否需要默认 Coordinator，还是本地 Session 可完全离线使用；
6. R2 Provider 的凭证、TTL、隐私提示和自托管替代；
7. MatAnyone 2 是否只作为 Benchmark，还是作为官方可选 Worker；
8. Windows/macOS 的默认硬件加速与无独显降级；
9. 最终 FFmpeg 分发方式及 LGPL/GPL 边界；
10. Agent Runtime、模型和 Prompt 不在本研究中锁定，只锁定 Edit Project Contract。

## 17. 后续规范化路径

若 PoC 证明方向成立，再依次进入稳定文档：

1. Business Use Case：团队继续使用飞书，将普通会议转为可编辑的高质量多轨素材；
2. Product Use Cases：创建/加入 Recording Session、本地录制、汇集素材、编辑、Agent 剪辑、渲染与恢复；
3. Architecture：
   - Recording Domain 与本地 Asset Store；
   - 平台 Capture 与设备拥有权；
   - Timebase、Audio Sync 与 Final Mix；
   - Asset Transfer 与 Object Storage Adapter；
   - Edit Project 与 Agent Operation Contract；
   - Processor、Provider 与 Render Boundary；
4. Requirements：为每项可验收行为分配 `REQ-PICOO-RECORDING-*`、`REQ-PICOO-EDIT-*`、`REQ-PICOO-RENDER-*` 等 ID；
5. 最后创建 crate、feature flag 和实现 Issue。

## 18. 推荐结论

本研究建议先接受以下方向，暂不批准完整实现：

```text
飞书负责 Meeting
Picoo Camera 负责实时 Camera
Picoo Recording 负责独立 Raw Tracks
Picoo Edit Project 负责非破坏性编辑
Picoo Processor 负责桌面离线处理与输出
```

这条路线可以保留团队现有会议习惯，也让人像抠图、布局调整和 Agent 剪辑拥有足够高的素材质量。它与实时 `Picoo Stage` 是两个不同的问题，应继续分别评审。
