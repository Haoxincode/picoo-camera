# Picoo Camera Next：完整需求与技术实现方案（修订版 v2）

> 归档说明：2026-09-06 用户授权按本方案开始实施，不要求兼容。以下原始提案正文保留；当前契约与实现状态见 [架构](../design-specs/architecture/0012-native-media-multi-output-boundary.md) 和 [需求追溯](../design-specs/requirements/next-media.md)。D01 为未随本次提供的上一版附件，不是仓库文件。

**方案版本：v2 · GPU 主链路 + 虚拟摄像头 CPU 输出桥接**  
**状态：Proposed · 允许破坏性修改 · 尚未实现、编译或真机验收**  
**修订日期：2026-09-06**  
**沿用代码基线：`6a214b34df619f407faf3b1fa7760a3123e4049e`；本次没有重新审查 main，不宣称该提交仍是当前最新。**  
**产品平台：Android、iOS 发送端；Windows、macOS 接收端与虚拟摄像头。**  
**取代：上一版 `Picoo-Camera-Next-Requirements-and-Implementation.md` 的设计提议，及更早 `picoo-native-preview-implementation-6a214b3.md`。不表示仓库规范或代码已被修改。**

本次按用户明确决定保留 CPU 输出。它是新版输出架构内的正式后端，不是对旧协议、旧 FFI 或旧共享内存 ABI 的兼容。软件编解码回退、CPU 预览流水线不恢复。

**阅读依据：**本方案继承上一版需求、平台范围、模块组织与固定提交事实，重新编写受 CPU 输出影响的帧类型、路由、IPC、资源预算、安全、测试和实施门槛。标有 R 的引用为上一轮固定提交源码依据，U 为当时核对的依赖上游，P 为平台资料；本次重新查阅了与 CPU 桥接直接相关的官方文档，并新增 P25—P28；D01 标识本次修订的原始附件。需求、接口、阈值、目录和默认策略均为设计提议。没有承诺任何第三方软件内部全程使用 GPU，也没有把拟议指标当作实测结果。

**阅读导航：**第 3 节给出 40 条完整需求；第 7、14—16 节是新增 CPU 输出的主要实现；第 19—20 节给出性能、错误与安全；第 21—25 节给出逐文件修改、实施、追溯与完成定义。


## 0. 执行结论

采用 **H.264/H.265 双硬件编解码 + 原生 GPU 视频帧 + 多输出架构**。预览、虚拟摄像头和录像共享源帧及时间线，不共享一套会互相阻塞的队列。

虚拟摄像头有两个必须交付的后端：

- `GpuNative`：原生表面交接，允许必要的 GPU 内复制与格式转换。
- `CpuBridge`：从 GPU 主链路导出目标输出图像，用 CPU 可访问内存交接，并填充系统认可的 sample。

默认先验证原生输出；因资源导入、adapter 不一致、sample 分配要求等无法建立时，验证并选择 CPU 输出。**切换只影响相应摄像头输出，不更换手机 codec、不重启正常 Decoder、不把 GPUI 预览或录像改成 CPU 路径。** CPU 输出不是错误状态，也不等于低画质模式。

原码流录像在解码前保存已经编码的 AU；处理后录像仍采用 GPU 图像处理和硬件编码。CPU 输出不能弥补缺失的硬件编解码器，不能在整个 GPU 不可用时凭空恢复新视频。

不维护旧协议、旧 FFI、旧 IPC/配置迁移和软件编解码回退；应用与扩展作为同主版本集合发布。旧 CPU Shared Ring 的安全、lease、Busy 重试等经验证逻辑可以复用，但数据格式、命名空间和接口按新版本重构，不接入旧端点。

### 0.1 已确认的产品约束

网络稳定、带宽充足是体验验收前提；对照物为 **Picoo 手机端自身预览**。先同尺寸对齐，再验证大窗口/全屏，最后验证会议和录像并用。细节/颜色、动作连贯、反应跟手三项一起达标，不降低手机端参考画质，不通过少显示帧伪造性能改善。

不新增监看/会议/拍摄等用途模式，也不增加要求普通用户选择 GPU/CPU 的设置。录制保存方式、分辨率、帧率、编码格式仍是明确的输出配置；后台路径在诊断中可见。

小米 15→Windows 是日常体验首要验收组合；小米 15→Mac mini M4 用于自动化及真实 macOS 回归；iOS 到两种接收端分别验证。优化覆盖全部产品平台，Windows 优先只是验证顺序。

### 0.2 范围之外

本期不承诺 4K、HDR、10-bit、4:2:2、多手机同时输入、AI 抠图模型、Agent 剪辑、手机本地母版补传和音频系统。内建录像按视频轨定义，完整音视频录制仍需要单独确定音频源与同步。不增加软件编码/解码、CPU 预览或广泛 CPU 特效流水线。

### 0.3 本版修订边界

| 上一版决定 | 本版替换决定 |
|---|---|
| 所有生产视频路径禁止 CPU 完整帧 | 原生源帧与预览/录像保持 GPU；仅已授权 CPU 输出及显式诊断可物化整图 |
| 摄像头只有 GPU-native 后端 | 两个平台都交付 GpuNative 和 CpuBridge |
| 不同 adapter 的摄像头输出直接拒绝 | 优先保持现有源 GPU，尝试该输出的 CPU 桥接；不打断活动录像 |
| 删除全部 CPU ring 实现 | Windows 保留并重构为本代 CPU IPC；Mac 优先复用 CMIO sink 交接 CPU 准备的原生容器 |
| 全局 readback 必须为 0 | Preview-only/GPU-native 输出为 0；CPU 输出按需求统计并限制 |
| 缺少 native sample 直接判摄像头不可用 | 有合法 CPU sample/可上传目标时可用，否则明确失败 |
| “不做兼容”同时等于 GPU-only | 不做旧版本兼容；新版本支持两种合法输出存储方式 |

保留 CPU 桥接不保证零开销。线程隔离只能避免逻辑反压，GPU copy、内存带宽、总线和 CPU 仍共享实际机器资源；必须在多输出测试中验证其影响。


## 1. 本版最终架构选择

| 方面 | 最终决定 |
|---|---|
| 编码与解码 | AVC 与 HEVC 都是正式原生硬件能力，失败不选择软件 codec |
| 公共源帧 | 有完整身份、颜色/几何描述和资源所有权的 NativeVideoFrame |
| Picoo 预览 | GPU 视频 surface，无逐帧 CPU 图像或静态 RenderImage 中转 |
| GPU 虚拟摄像头 | 合法的原生表面交接、系统 manager/allocator 和独立采样 |
| CPU 虚拟摄像头 | GPU 完成目标图像处理，输出专用 Exporter 提供 CPU 可访问图像；只做布局整理、复制和必要的目标上传 |
| 录像 | 原码流不重编码；处理后录像使用 GPU 和硬件 Encoder，不从预览缓存抽帧 |
| 版本 | PCP/FFI/本地 IPC 同主版本，不接受旧接口；文档 v2 不等于网络协议再升一次 |
| 故障处理 | 输出能力不足可局部选择 CPU；权限拒绝、硬件 codec 缺失、整个 GPU 丢失不被这种选择掩盖 |
| UI | 不增加用途/后端模式；默认自动，诊断记录路径与原因 |

**CpuBridge 是输出路径，不是另一套媒体系统。** 它不持有 codec 事务，不拥有连接状态，不改变源帧率/码率，不允许把 CPU 帧反向塞回 Native FrameBus。


## 2. 沿用的基线事实与对应改造

下列是上一轮对固定提交的检查记录，本次按该基线修订方案，未重新逐文件确认当前 main。

| 基线事实 | 源码依据 | 新版处理 |
|---|---|---|
| owner、MediaScheduler、RAP 确认、Decoder 准入已经存在 | R02 | 保留正确行为并扩展双 codec |
| DecodedFrameStorage 仅有 CpuNv12 / into_cpu_nv12 | R03 | Decoder 公共输出改原生 lease；CPU 类型放在输出边界 |
| VideoFrame 保存 pixel_data: Bytes | R04 | Native FrameBus 分发原生图像；CpuOutputFrame 只由 Exporter 产生 |
| 发布阶段执行 CPU 方向变换 | R07 | GPU RenderSpec 执行，owner 仅校验与分发 |
| Windows Decoder 没有接入 D3D11 manager | R08 | 真正 native 输出，独立于 VCam 当前是否选 CpuBridge |
| Windows 视频逐帧创建 RenderImage | R05 | GPU video surface |
| Mac Decoder 输出 CopiedNv12 | R19 | retain 原始 PixelBuffer，CPU 读取仅在 exporter |
| 30fps preview cadence 与 16ms UI poll 并存 | R06、R09 | 帧事件＋显示刷新，不混入 VCam sample clock |
| codec 字符串、仅 SPS/PPS、尺寸与 fps 分列 | R23 | typed codec、完整能力组合和 HEVC VPS |
| Windows SetD3DManager 忽略参数 | R24 | 验证 manager；可用则原生；无可用 manager 时探测合法 CPU sample |
| Windows Lock buffer 后 CPU copy | R25 | 从全局必经改为 CpuBridge 专有路径，并修正 stride/allocator 生命周期 |
| Mac 由 CPU ring copy 到 PixelBuffer | R26 | 原生 sink 为主；CPU backend 可填独立 PixelBuffer 后走相同 sink |
| 1080p 6—10Mbps、256KiB Datagram buffer | R13—R15 | codec/fps 感知质量与有界大 AU 策略，和 CPU 输出选择无关 |

CPU output 的存在不能被用来宣布 native 路径完成；native 的存在也不能代替 CPU 输出的真实端到端验证。


## 3. 正式需求

需求编号 `NEXT-REQ-*` 是提案编号，实施时接入仓库追溯系统，避免覆盖原有编号。

### 3.1 产品与质量

| 编号 | 必须满足的要求 | 验收证据 |
|---|---|---|
| NEXT-REQ-001 | Android/iOS→Windows/macOS 四种组合完整实现 | 四组合发布矩阵，不能互相代替 |
| NEXT-REQ-002 | 不新增用途性能模式；默认配置无需反复调参 | UI 及用户流程测试 |
| NEXT-REQ-003 | H.264 High/Constrained High 与 HEVC Main，均支持 8-bit 4:2:0 SDR | 两种真实码流、参数集与输出检查 |
| NEXT-REQ-004 | 1080p60 是完整功能目标；同时提供 1080p30、720p60、720p30 正式配置 | 每个 codec、配置组合的真机测试 |
| NEXT-REQ-005 | 默认选择通过准入的 1080p60 原生配置；不在运行中静默改帧率/分辨率/codec | 配置事务、异常日志和 UI 实际配置 |
| NEXT-REQ-006 | 同尺寸对比时保留手机端细节、颜色和构图，不以降画质/减帧换取指标 | 同源、同视场、同物理显示尺寸人工对照 |
| NEXT-REQ-007 | 大窗口和全屏不改变源配置，视频连贯且跟手 | DPI/resize/fullscreen 回归 |
| NEXT-REQ-008 | 预览、虚拟摄像头、一个处理后录像任务可并用；同时允许一个原码流录像，不承诺多路并发重编码 | 同时工作与热稳态测试 |

“产品支持两种编码”指两种都有正式实现和验证；不是所有设备必须在任意尺寸/帧率均能同时运行两个编码器。正常只启动一个手机编码会话。被明确指定且不满足要求的配置直接报错；自动选择仅在同代受支持配置中进行一次协商，不是旧后端回退。

### 3.2 GPU、输出与资源

| 编号 | 必须满足的要求 | 验收证据 |
|---|---|---|
| NEXT-REQ-009 | 生产主链路硬件编解码、原生帧、GPU 预览/处理后录像；整图 CPU 访问只允许在显式 CpuBridge 或诊断 | 按 sink/backend 归因的 map/readback 计数，生产工厂无软件 codec |
| NEXT-REQ-010 | Intel/AMD/NVIDIA 显卡走统一平台接口；不要求核显 | 包含 AMD 独显且无核显、单核显、双 GPU 的矩阵 |
| NEXT-REQ-011 | 每个 AU 在同一 Decoder generation 最多提交一次；输出携带原始 token | 延迟/多输出/reset 回归 |
| NEXT-REQ-012 | 预览 latest-only，窗口隐藏停止预览工作，不停止其他输出 | 非可见时预览提交计数为零 |
| NEXT-REQ-013 | Windows/macOS VCam 都实现 GPU 原生与 CPU 桥接；两者提供相同协商内容和时间语义 | 强制两后端的 capture harness 及实际客户端验证 |
| NEXT-REQ-014 | VCam 按协商格式与独立时钟输出；支持 30/60fps，不把重复 sample 算新画面 | sample PTS/duration 与源帧 ID 分别检查 |
| NEXT-REQ-015 | 任意慢消费者不能阻塞其他输出；无无界 surface 或任务队列 | 饱和/悬挂消费者/显存压力测试 |
| NEXT-REQ-016 | CPU 引用释放与 GPU 完成分离；未完成 GPU 工作不能复用表面 | 平台调试层、lease 状态机测试 |
| NEXT-REQ-017 | GPU 丢失/重建推进资源代际，旧帧不能进入新资源 | device-lost、睡眠恢复、adapter 改变测试 |

### 3.3 录像与可靠性

| 编号 | 必须满足的要求 | 验收证据 |
|---|---|---|
| NEXT-REQ-018 | 原码流录像保存接收到的 AVC/HEVC，不重编码，不从预览取帧 | AU 归一化摘要对照；两种文件回放 |
| NEXT-REQ-019 | 处理后录像由 GPU 图像和硬件编码器生成，目标尺寸独立于窗口 | 隐藏/缩小预览期间录像内容与尺寸不变 |
| NEXT-REQ-020 | 录像使用独立有界队列与源时间线；丢帧/写入失败必须明确 | 慢磁盘、缺 AU、编码器拒绝、低存储测试 |
| NEXT-REQ-021 | 录制结果区分完整、含缺口和失败；普通成功不能掩盖中断 | manifest 与 UI 完成状态一致 |
| NEXT-REQ-022 | codec、分辨率、配置变化时安全分段，拒绝把不兼容 sample description 硬塞同段 | 切配置后每段可独立解码 |
| NEXT-REQ-023 | 重组与恢复 codec-aware；IDR/CRA 不能混同 | AVC/HEVC 各类随机访问样本与缺片注入 |
| NEXT-REQ-024 | 只有摄像头输出边界可自动选择 CpuBridge；硬件 codec 或 GPU 主链路不足明确失败 | 能力组合故障注入、路由原因和依赖检查 |
| NEXT-REQ-025 | 网络、命令、媒体、输出全部有容量和时限；控制命令有明确结果 | 过载状态机与预算断言 |
| NEXT-REQ-026 | 旧协议、旧 IPC、旧配置、旧 FFI 不被新产品接受 | 混版本拒绝测试 |
| NEXT-REQ-027 | 配对认证、加密、资源权限、输入边界、隐私行为继续有效 | 未授权访问与恶意输入测试 |
| NEXT-REQ-028 | 度量自动采集并区分请求值/实际值、提交/呈现、新帧/重复帧 | 结构化诊断完整性检查 |

### 3.4 CPU 输出专用需求（新增）

| 编号 | 必须满足的要求 | 验收证据 |
|---|---|---|
| NEXT-REQ-029 | CPU 整图仅由 Output Exporter 产生；不进入 Decoder API 或 Preview | 构建依赖审查、preview-only 导出数为零 |
| NEXT-REQ-030 | 各 VCam sink 独立选择 GpuNative/CpuBridge，默认自动，不增加用户模式 | 同时两 sink 不同后端；原因可见 |
| NEXT-REQ-031 | CPU 路径不降低既定分辨率、帧率、色彩或重复执行镜像 | 同源同 RenderSpec 对照、两后端格式一致 |
| NEXT-REQ-032 | 无 CPU 输出需求时无持续 readback；重复源帧不反复导出 | demand/unique-source/export 计数与缓存测试 |
| NEXT-REQ-033 | readback、PixelBuffer 锁定、CPU copy 不在 UI/Receiver owner/实时 RequestSample 回调等待 | 执行器约束、延迟注入 |
| NEXT-REQ-034 | CPU staging、工作帧、IPC、系统 sample 均有资源上限和明确寿命 | 槽位占满、慢消费、内存高水位测试 |
| NEXT-REQ-035 | 相同输出内容可共享一次 CPU 物化，不同规格独立准备；低频 sink 不增加高频无用导出 | 一源两消费同/异规格、source60→output30 |
| NEXT-REQ-036 | 后端切换为事务，推进 backend generation；配置和 SampleClock 连续，旧完成不可提交 | 切换中 stop/格式改变/旧任务完成 |
| NEXT-REQ-037 | 跨 GPU 原生交接不可用时可用 CPU bridge，不为新增 sink 重置正在录像的源 device | 双 adapter GPU→CPU→合法目标 sample |
| NEXT-REQ-038 | CPU IPC 使用新版 ABI、最小权限和崩溃可恢复 lease；隐私时限适用所有缓存 | 混版本/未授权/崩溃/断开占位测试 |
| NEXT-REQ-039 | CPU 输出按所选30/60fps规格验收，并验证其对 GPU 预览与录像的影响 | 同配置 A/B、p95/p99、热稳态并用 |
| NEXT-REQ-040 | CPU bridge 不能绕过身份/权限失败，也不能冒充硬件 codec 或 GPU 故障的修复 | 拒绝后无回退访问；缺 decoder 仍失败 |


## 4. 平台支持契约

下表是**项目建议的最低支持策略**，不是平台 API 的最早出现版本，也不是所有该系统设备都已通过测试。

| 平台 | 新版建议基线 | 运行时准入 |
|---|---|---|
| Android | Android 13/API 33+ | 当前 camera/尺寸/fps 输出组合可用；硬件 AVC/HEVC 编码；Surface 输入；实际时序通过 |
| iOS | iOS 17+ | AVFoundation 目标采集格式可用；VideoToolbox 硬件 AVC/HEVC 编码；原生缓冲可交接 |
| Windows | Windows 11 25H2+ | D3D11 视频与图形能力；所选 codec 的 MF/DXVA 后端；主链路所需 GPU 能力；VCam 至少一条合法 GpuNative 或 CpuBridge 输出成立 |
| macOS | macOS 14+ | Metal、VideoToolbox、CMIO Extension sink/source；至少一条受支持的样本准备/交接路径 |

Windows 25H2 是收窄实现面的产品提议，不是说更早 Windows 11 没有 GPU 摄像头能力。它也使需要时使用 `MF_READWRITE_USE_ONLY_HARDWARE_TRANSFORMS` 的版本要求明确；本方案核心仍直接控制 Decoder/Encoder，而非把选路全部交给自动拓扑。[P15]

Apple Silicon 是 macOS 首发认证重点，M4 为自动化设备；不为 Intel Mac 维护另一套旧路径，满足相同能力与测试要求的 Intel Mac 可使用同一实现。Windows 不按核显/独显或 CPU 名称判断能力，使用实际 adapter、codec profile、视频处理、共享格式及资源分配结果。

预览/VCam 需要电脑解码与 GPU 图像处理；处理后录像额外需要硬件编码。电脑不能硬件编码不阻止其原码流录像。不同功能报告独立状态，不用一个 `gpu_supported: bool` 代替全部事实。

HEVC 系统组件没有安装、所选 codec 硬解不可用，仍然报告 codec 错误；摄像头当前无法使用 native sample，则先验证 CpuBridge 是否能满足合法 sample 合同，只有两条输出都不成立才报告该摄像头输出不可用。检测结果不足以形成硬件证明时标记 unknown，而不误报 hardware=true。Windows 的 HEVC MFT 必须实际创建验证，显卡支持 HEVC 不替代系统组件存在性。[P02]

CPU bridge 不扩张上述最低系统版本，也不额外承诺无 GPU 的虚拟机/纯软件远程会话。它处理的是“已有可用原生图像，某个输出不能直接接收”的情况。M4 的统一内存不能使 CPU lock/copy 自动变成零成本；AMD 独显无核显是正常支持目标，和是否使用 CPU output 是两个判断。


## 5. 目标架构与责任边界

```text
Camera2 / AVFoundation
        ↓
AVC / HEVC 硬件编码
        ↓
Sender owner → QUIC Datagram → Receiver 完整 AU
                                ├── EncodedRecorder → 文件/缺口记录
                                ↓
                       MediaScheduler → NativeDecoder
                                ↓
                     Native FrameBus（不可变源帧）
                     ├── PreviewPresenter → GPU → GPUI
                     ├── RenderedRecorder → GPU → HW Encoder → Muxer
                     └── VirtualCameraSink（每实例独立）
                          ├── GpuNative → GPU IPC / CMIO → 系统 sample
                          └── CpuBridge → GPU 输出准备
                                       → CpuFrameExporter
                                       → CpuOutputFrame / CPU-filled PixelBuffer
                                       → 新版 CPU IPC 或同一 CMIO sink
                                       → 系统 sample（CPU 或必要的目标 GPU 上传）
```

会话 owner 保留连接、配置、恢复与控制；输出 coordinator 保留输出需求和后端选择；平台 worker 执行 codec、GPU、readback、copy、录像工作。FrameBus 分发不等待消费者。

CPU 输出不会反向改变 Native FrameBus，也不要求 Decoder 返回一份 GPU＋CPU 的双份图像。仅有 CPU sink 时仍使用同一个硬件 Decoder，按需要导出其结果；无 CPU sink 时 exporter 可以完全不启动。

预览 latest-only；VCam 采用独立 sample clock；处理后录像有界有序；原码流录像在解码前。任何 sink 不能以无限保留源 lease 的方式阻塞 Decoder 池。逻辑隔离不消除硬件带宽竞争，因此“独立”不是“性能开销为零”。


## 6. 代码边界与依赖

最多新增三个有明确职责的 crate：

| crate | 职责 | 不包含 |
|---|---|---|
| `picoo-bitstream`（新增） | AVC/HEVC NAL 与参数集、AU 描述、随机访问判定、Annex-B/length-prefix 适配 | 网络、GPU、UI、完整软件解码器 |
| `picoo-gpu`（新增） | 平台 GPU context、图像变换、资源池、表面准备、输出专用异步 readback | 会话业务、codec 协商、通用节点编辑器 |
| `picoo-recording`（新增） | 两种 Recorder、录制时间线、manifest、平台 Mux/硬编适配 | 网络连接管理、UI、音频系统 |

复用 `picoo-frame-hub` 作为公共 frame description/identity/native ownership/FrameBus；`picoo-media-decode` 依赖它。`picoo-gpu` 依赖 frame-hub；frame-hub 不反向依赖 gpu、decoder 或 gpui。GPU context 通过明确的 lease/interface 注入，而不是放入全局可变 GPUI 对象。

AVC helper 从 `picoo-packet` 移到 bitstream；packet 保留碎片、FEC、AU 重组。生产端禁止默认链接 OpenH264/StubDecoder；测试替身放在 `picoo-testkit` 或仅 test feature。Linux 运行纯核心/模拟测试，不在本期新增 Linux 产品后端。

不为 CPU 输出再新增通用 fallback 框架或第四个大 crate。建议 `picoo-gpu::export` 实现 readback；`picoo-frame-hub::output` 提供不可变 CpuOutputFrame、bounded lease 及新版 CPU IPC 基础。输出 coordinator 放在 receiver/output 边界；Windows/Mac 系统 sample 适配仍在各自扩展。

`picoo-frame-hub` 可定义 CpuOutputFrame，但不得在 NativeImage 中加入“为了方便”自动物化的 CPU variant；Preview 与 Recorder 不依赖 CPU Exporter。保留旧 ring 的算法或测试不等于保留旧 wire layout。


## 7. 帧、时间与所有权模型

以下接口是设计骨架，基础类型和平台 lease 必须随实现定义，不是可直接编译的补丁。非产品 CI 平台可通过测试专用 feature 提供 FakeNativeLease，正式构建不能启用；CPU byte fixture 不代表软件解码后端。

```rust
pub struct FrameIdentity {
    pub connection: ConnectionGeneration,
    pub stream_epoch: StreamEpoch,
    pub decoder_generation: DecoderGeneration,
    pub frame_id: FrameId,
}

pub struct FrameDescription {
    pub coded_size: SizeU32,
    pub visible_rect: RectU32,
    pub pixel_aspect_ratio: Rational,
    pub color: ColorDescription,
    pub transform: PresentationTransform,
    pub config_revision: u64,
}

pub enum NativeImage {
    #[cfg(target_os = "windows")]
    D3d11(D3d11SampleLease),
    #[cfg(target_os = "macos")]
    Apple(ApplePixelBufferLease),
}

pub struct NativeVideoFrame {
    pub id: FrameIdentity,
    pub source_pts: MediaTime,
    pub description: FrameDescription,
    pub image: NativeImage,
    pub ready: GpuReadiness,
    pub timeline: FrameTimeline,
}
```

原生源帧模型不含 `CpuNv12`、`into_cpu_nv12()`、`as_mut_pixels()`。CPU 完整视频帧通过下游专用 CpuOutputFrame 表达；只有显式 ExportRequest 可创建，不提供任意消费者都能调用的隐式 readback getter。离线诊断/测试与静态资产初始化单独计数。

图像格式与存储分离：NV12 可以是 GPU 图像；不能为 GPU 图像虚构 CPU stride。描述区分 encoded/coded、visible crop、texture allocation、pixel aspect。首发输出语义为 8-bit BT.709 SDR；采集输出若不同必须在原生图像路径上做实际转换，不能只换标签。范围、primaries、transfer、chroma siting 都显式表达，unknown 必须在配置阶段解决。

变换仅表达剩余未应用的 crop/rotate/mirror。Android compositor 已经应用的操作不再在接收端重复。比较手机预览时考虑明确的本地自拍镜像差异；不可无声改变镜像规则。

每个 AU 准入时绑定配置快照；Decoder completion 不用“现在的 StreamConfig”重新解释旧帧。图片 identity、输出 demand revision、scene revision、device generation 一起组成缓存键。同一源帧在窗口 resize 后可以产生新的呈现结果。

### 7.1 所有权与池

Windows lease 保留必要的 IMFSample、纹理、subresource 和资源代际；只 AddRef 纹理不等于 Decoder 的纹理数组位置不会复用。[P09]

Apple lease retain CVPixelBuffer 及用于映射的 CVMetalTexture，直到使用它的 GPU command buffer 完成。不能把 CPU 侧 Arc 释放作为 GPU 完成信号。[P10]

公共安全接口不暴露无生命周期约束的裸指针，不对整类平台对象一概 `unsafe impl Send`。只有已审查的不可变 handle owner 可以跨线程；立即上下文和队列由指定执行器使用，析构按平台约束回到所有者。

Decoder DPB 与应用输出池分开计数。必要时将 Decoder surface 在 GPU 内复制到应用池，尽快释放 decoder sample，避免 Recorder 把硬解器所有输出表面占满。

建议初始边界：preview 待处理 1；每个 VCam 消费端待处理 1＋最多 3 个输出槽；RenderedRecorder 队列最多 8 帧且不超过 150ms；在途 GPU 完成对象同样计入预算。最终容量由启动时明确上限管理，不根据消费者变慢无限扩容。单流应用所有自有 native 图像的初始预算 256MiB（工程配置，不含驱动内部池），达到上限触发对应输出错误/丢弃规则。

### 7.2 CPU 输出对象（新增）

```rust
pub struct CpuOutputFrame {
    pub source: FrameIdentity,
    pub output_revision: u64,
    pub backend_generation: u64,
    pub source_device_generation: u64,
    pub source_pts: MediaTime,
    pub prepared_at: MonoInstant,
    pub description: CpuFrameDescription,
    pub planes: [PlaneLayout; 2],
    pub pixels: ImmutableBufferLease,
}

pub struct PlaneLayout {
    pub offset_bytes: usize,
    pub stride_bytes: usize,
    pub row_bytes: usize,
    pub rows: usize,
}

pub enum VcamBackend {
    GpuNative,
    CpuBridge,
}
```

类型是接口草案，不是现成可编译补丁。首发 CPU 输出选 NV12 8-bit BT.709 SDR；像素已完成目标几何、方向与镜像，description 中剩余变换为 identity，避免两端重复处理。优先导出紧凑帧，但平面描述保留真实 offset/stride，系统目标缓冲不要求同一 stride。

CPU 物化身份与系统 sample 身份分开：一张 CpuOutputFrame 可用于多个合法 sample 时间，不能每重复一个 sample 就捏造一个新 source frame。序列化只发送固定宽度、明确字节序字段，不能直接把 usize、Rust enum 或进程地址写入 IPC。

每个异步 export 持有源 lease 至 GPU copy/读取结束；把目标 bytes 发布后不可变，最后一个消费者释放才归池。可以用 `Bytes::from_owner` 复用受限像素池，不通过 Vec→Arc<[u8]> 的额外分配隐藏整帧搬运。缓存键为 source identity＋RenderSpec＋output revision＋device generation；backend generation 用于结果接纳，不把其他 sink 的生命周期与其绑死。

CPU-only placeholder 是允许的例外：预先生成对应格式的静态占位图，供桥接输出在源丢失/重建期间使用；不要求为了输出占位而等待失效 GPU 恢复。占位缓冲受相同隐私与版本控制，不是一套 CPU 视频渲染器。


## 8. 破坏性协议与 FFI

### 8.1 版本与发布

使用新 ALPN（提议 `picoocam/2`）和新的发现 TXT 协议主版本。新版本不同时接受旧 ALPN。配对 transcript 必须绑定协议/算法和端点身份；新的本地配置/IPC namespace 明确分离。应用、手机端和扩展必须匹配。旧设备明确提示升级，不发模糊的网络错误。

旧个人设置与配对记录不设计迁移器；新版本首次配置与配对。卸载旧扩展只清理本项目拥有的注册和 IPC 资源；不触碰录像素材。新旧桌面同名虚拟摄像头不同时保留为正式部署方式。

### 8.2 完整配置，不使用尺寸与 fps 的笛卡尔积

```text
StreamOffer {
  codec: AVC | HEVC
  profile, level, bit_depth=8, chroma=420
  coded_size, visible_rect, fps_num, fps_den
  bitrate_envelope, max_access_unit_bytes
  color_description, presentation_transform
  random_access_policy, input_clock_description
}
CodecConfig = Avc{ SPS[], PPS[], nal_length_size }
            | Hevc{ VPS[], SPS[], PPS[], nal_length_size }
```

每条 offer 都是该相机＋原生编码器实际支持的组合；接收端按 Decoder 能力交集选择。选择顺序以实测可满足同一质量/延迟目标为准，HEVC 是正常首选候选，但不宣称总比 AVC 快。指定格式失败不静默换另一种格式。

配置事务保留 prepare→native started→有效随机访问 AU→commit 的主线，新增 codec/尺寸/fps/颜色/参数集的完整校验。可靠配置与 Datagram 可乱序，接收端暂存至多一个未来配置的随机访问 AU，其余受严格容量/时限约束。收到新码流不能继续用旧参数集猜测。

码率变化可使用可验证的动态参数更新，不必每次重建 Decoder；改变 codec、coded dimensions、参数集等必须推进 stream epoch。UI 与 FFI 不独立决定事务提交。

FFI 只暴露完整 `submit_encoder_event` 操作，返回 `Accepted/Rejected/Error` 与配置提交、关键帧请求等独立事实。不再用负错误码与成功位掩码混用。异步交接的数据必须已拥有所有权；MediaCodec output buffer 释放后不保留借用指针。

### 8.3 AU wire 格式

统一“一条 AU 包含一张图像的全部相关 NAL”，内部采用 4-byte big-endian NAL length 表达，平台边界按需转换；不在每个阶段来回转格式。配置含 codec、NAL长度约束、配置摘要。header 至少携带 epoch、AU ID、PTS、片序/片数、AU总字节数与经过验证的参考/随机访问信息。控制消息封装大小、NAL长度、AU长度、片数都做上限和溢出校验。

当前 `keyframe: bool` 不再是唯一恢复依据，见下一节。

## 9. 双编码与参考链恢复

### 9.1 Codec-neutral 核心，codec-specific 位流解析

`picoo-bitstream` 分别实现 AVC 与 HEVC 的 NAL header、参数集、picture/RAP 描述和格式转换。HEVC 不能只把字符串改成 h265，继续使用 AVC 的一字节 NAL header、SPS/PPS 和 `nal_type == 5` 判断。

建议将 `RandomAccessPoint` 表达为 `AvcIdr / HevcIdr / HevcCra`，由经过验证的码流内容产生；原生编码器的 keyframe 标志只是输入证据之一。未知依赖的非 RAP 帧一律视作 reference；只有能够证明不被后续帧依赖的帧才标记 discardable。

本期请求低延迟、无 B 帧/无显示重排配置；不能仅凭请求参数就假定驱动接受。HEVC 的 IDR、CRA 及其 leading pictures 语义不同，进入 CRA 随机访问点时必须识别并丢弃关联的不可解码 RASL，不能任意丢掉 RADL。解析/恢复配置不支持的结构明确拒绝，而不是把所有 IRAP 都当成 AVC IDR。[P05]

首选闭合、易验证的随机访问序列；如果硬件返回 CRA，只有通过相应 CRA/RASL 验收的后端才允许提交该配置。参数集、输入 NAL 大小、图像数与 PTS 关系有边界检查，不实现未经需要的完整软件解码器。

### 9.2 明确在途恢复候选

恢复状态至少区分：

```text
Healthy
  → AwaitingRefresh
  → RefreshCandidateInFlight(candidate identity/config)
  → Healthy
```

在途候选后面的 reference AU 不得先丢掉，再因为候选的迟到完成而放行依赖它们的后续帧。默认在剩余 deadline 内保留后续 AU；如果具体原生 Decoder 必须接收后续输入才能产生候选输出，允许在同一有界候选链中顺序提交，但候选确认前不公布未经确认的链结果。这样避免“一定等待候选出图、而后端必须多收输入”形成死锁。

候选失败、参考丢失、配置变化或超期时，使整个候选身份失效；其旧 completion 不能重新打开 gate。`accepted_without_frame`、确认了合法 RAP、产生图像三个事实分开。每个 Decoder 输出根据 token 查回原始 AU，而不是绑定最近一次调用。

### 9.3 生产与模拟一致

MediaScheduler 接收重组缺口、reference/RAP 状态、Decoder 准入、目标时间和硬 deadline，返回唯一的可执行动作及等待原因。生产 drain 与 next wake 使用同一决策。模拟器不得在外层先把复杂条件过滤掉，再声称调用同一函数等于验证生产语义。

## 10. 时钟、调度、传输与有界性

### 10.1 统一媒体时间，不统一所有输出定时器

给每个时钟域编号：Camera capture clock、Sender monotonic、Receiver monotonic、GPU/present clock、Recorder media time。端内先校准 capture→monotonic，跨端再进行带不确定度的时钟映射。不可比较的时间返回 unavailable，而不是减出一个看似准确的延迟。

接收 deadline 从应用接收入口记录的本地到达时间开始，带着这条截止时间穿过重组、Jitter 和 Decoder 准入；进入下一阶段不重新获得一整份预算。绝对截止时间与正常目标时间分别建模。正常链路的完整 AU 在依赖与容量允许时尽快解码，不再固定添加一层 16/33ms 的等待来对齐 UI。

乱序的短暂等待、恢复候选等待与 Decoder 资源等待仍然需要存在。目标是“预算内有理由地等，超期明确结束”，不是机械执行“任何旧帧永远不等”。预览、VCam 和录像共享时间含义，但各自采用显示刷新、系统采样和录像媒体时间，不互相驱动。

### 10.2 初始队列预算

以下数值是工程起点，必须在 PR 中通过压力测试校准；不是已测得的最佳值。所有条目同时受字节/资源上限约束。

| 阶段 | 初始边界 | 饱和/超期处理 |
|---|---|---|
| 手机压缩 AU handoff | 2 个待处理 AU，另有总字节上限 | 不阻塞 Camera/codec callback；reference-aware 中止并恢复 |
| Sender 网络发送 | 1 个 active AU＋2 个 pending；每 AU 独立绝对期限 | 未开始可整体淘汰；已开始中止则留下 tombstone 并修复参考链 |
| Receiver 未完成/完整 AU | 帧数按 fps 与 deadline 计算，同时限制总字节；不保留旧 30fps 专属计数假设 | 保留现有丢失归因，达到硬期限再结束等待 |
| Decoder 应用待提交 | 2 个，额外区分 backend 正在使用的 DPB/in-flight | 未超期的 reference 留在 scheduler，不先 pop 再推回 |
| Preview | 1 个 pending＋有界 GPU in-flight | 替换未开始旧画面，不修改解码依赖 |
| 每个 VCam sink | 1 个 pending＋最多 3 个输出槽作为初始配置，两后端共同预算 | 暂用已完成缓存/占位；不无限持有 Decoder 样本 |
| CPU Exporter | 每个活跃 RenderSpec 最多1个未开始请求、3个 staging/读取槽；有全局字节上限 | 忙则替换未开始旧请求，完成回调释放；不能循环强制等待 |
| CPU IPC / 最终 sample | 每个实例初始3个IPC槽，系统样本遵守allocator预算 | lease持有时禁止覆盖；全忙不假报发布，终态占位限频重试 |
| RenderedRecorder | 最多 8 帧、最老 150ms、显存限额 | 明确 Overrun/HasGaps 或停止该录像，不能悄悄覆盖 |
| EncodedRecorder 写入队列 | 初始 16MiB 且最老 2s | 单独录制失败/缺口，不反压实时视频 |

Receiver 本地 ingress→提交可呈现结果的硬预算可从 100ms 起试，异常恢复另有明确期限。不是保证端到端 100ms，也不是给每个阶段分别增加 100ms。

### 10.3 大 AU 不再受固定 256KiB 整批入队限制

旧实现要求完整 AU 全部 Datagram 字节一次性装入 Quinn 可用发送缓冲；当前 256KiB 限制可能拒绝高质量关键帧。[R14][R15] 本版改为**逻辑完整 AU 所有权＋受控分片发射**，而不是无条件扩大 Quinn 内部队列。

1. 分包器先确认完整 AU 合法、总长度符合协商上限，再交给唯一的 AU sender。建议协议初始最大 AU 2MiB，明确这只是安全上限，不是正常帧大小目标。
2. 每次只发射一个 active AU；pending 有界。active AU 的数据、FEC 与身份保持一致。
3. 使用不任意挤掉旧 Datagram 的等待式发送接口，在专门的媒体任务中等待可用空间；控制收发和 Close 在独立任务中推进，不能卡在一次媒体 await 后面。
4. 每次等待都受 AU 的绝对期限与 cancel 约束。发生中止时发送可靠的 `AccessUnitAborted(epoch, au_id, reason)` 事实，Receiver 为该 AU 建立终止记录，迟到尾片不得重新组帧；依赖链按照 codec 规则恢复。
5. 已经进入 Quinn 或网络的分片不可能因本地取消而凭空消失。小型发送缓冲限制残留量，Receiver 的 deadline 与 tombstone 处理残留，不声称网络物理发送原子化。
6. 链路层安全缓冲与应用 AU 内存分开预算。按相同码率下队列时间而不是“能放几帧”的直觉定上限。

Quinn 文档区分 `send_datagram` 的旧数据淘汰行为与 `send_datagram_wait` 的空间等待行为；具体接口按仓库锁定版本核对，不为这项改动顺便升级所有依赖。[P19]

这会改变原来“发送前全部字节一次准入”的内部契约，但仍保证**不完整 AU 永远不交给 Decoder，应用中止有明确归因**。任何无线丢片本来也可能导致不完整 AU，因此不能将“整批入队”误写成网络传输原子保证。

必须测试：大于 256KiB 的合法 RAP 正常完成；active AU 中途取消；Abort 比数据早/晚到；FEC 尾片晚到；配置升代；控制消息不被空间等待饿死；发送缓冲年龄不持续增长。

### 10.4 CPU 输出成本与准入

CPU bridge 没有压缩或重新编码，不因复制本身损失细节；但存在回读同步、CPU 内存搬运和可能的目标 GPU 上传。允许的工作只有输出所需几何/格式的 GPU 准备、读取、平面整理、IPC 和 sample 填充，不借此把共享 CPU 转色/resize 插回所有消费者前面。

按紧凑 NV12 计算：1080p 一帧为 3,110,400 字节，30fps 每遍完整搬运为 93,312,000 字节/秒，60fps 为 186,624,000 字节/秒；720p60 为 82,944,000 字节/秒。这只是尺寸与频率计算，不是实测带宽或性能损失，实际可能有多次 copy 和 pitch padding。

初始 CPU 输出内存预算建议为：桌面进程共64MiB、每个扩展/source实例16MiB，另计系统allocator实际使用量；native应用资源预算继续初始256MiB。都属于可调工程上限，不是设备最低配置或保证达到60fps的依据。在分辨率切换和旧代际仍在途时同样计入限额；不够就拒绝新增需求或报告该输出资源不足，不能无限建立新池。

同规格多个sink可共享已经物化的不可变像素，但每个消费路径有自己的lease与完成。源60fps、唯一CPU请求30fps时最多准备30fps的新输出；一个30fps和一个60fps且图像规格相同的请求，可按合并采样时刻复用一次物化，不重复导出同一source。不同crop/size/color或镜像不复用错误缓存。

CPU输出与GPU录像争用同一硬件时，保留既定源配置，优先取消尚未开始的过期CPU输出、复用合法缓存并报告该sink错过期限。不能改成全局source降码率/降分辨率/降帧率。此类重复sample有独立计数，不能宣称CPU路径已达完整输出规格。


## 11. Android 发送端

将 `MediaCodecH264Encoder` 改为 `MediaCodecVideoEncoder`，`codec` 映射为 `video/avc` 或 `video/hevc`，其余生命周期仍由现有原生适配器执行。

使用实际 codec 列表和硬件标志筛选，检查目标尺寸、速率、profile、bitrate 与 Surface 输入的组合。`createEncoderByType()` 成功不等于硬件证明；记录组件名、请求配置、实际 output format，并校验原生输出参数集。没有受支持硬件实例时返回能力错误，不选择软件编码器。

默认保留现有 Camera2→OES/EGL→MediaCodec Surface 的一次 GPU 合成路径，以维持固定画幅、方向和裁剪。不再并行维护一条“为了兼容旧机型”的 CPU YUV 编码路径。手机预览继续来自同一采集 session 的显示目标；采集控制、有效区域、镜像意图从同一个配置模型派生，避免以降低手机基准来制造对齐。

Camera2 目标 FPS Range 先查询支持组合，不能直接请求不存在的 60—60 区间；采集、compositor、编码三处分别统计唯一帧时间戳。帧通知允许合并，但同一 SurfaceTexture timestamp 不重复编码成两张“新帧”。资源丢失与 Activity/surface 销毁不能复用旧 generation。

已有 `KEY_LATENCY=0`、`KEY_MAX_B_FRAMES=0`、operating rate 等请求保留并验证，不能把设置调用成功当作生效。Android 明确允许忽略不支持的 encoder latency，并要求查看 output format。[P03]

HEVC 的 codec-config/CSD 包含 VPS/SPS/PPS 的解析交给 Rust bitstream 层，避免 Kotlin/Rust 两份 NAL 解释。压缩 AU 交接保留所有权和尺寸上限；允许一次压缩数据复制，不让 MediaCodec output buffer 在网络排队期间被持有，不允许释放后仍借用指针。

时钟记录 `SENSOR_INFO_TIMESTAMP_SOURCE`，REALTIME 使用对应的 elapsed realtime 域；UNKNOWN 采用可验证的端内映射，不能直接拿 `System.nanoTime()` 相减充当编码耗时。[P24]

## 12. iOS 发送端

统一 `VideoEncoderConfiguration.codec` 和完整采集配置。AVFoundation 的 active format、min/max frame duration、输出 pixel format 和展示几何均来自已选 offer；不以更新 Swift 默认 fps 代替实际采集切换。

VTCompressionSession 使用 H.264 或 HEVC codec type，要求硬件编码。尺寸匹配时直接使用同一源 PixelBuffer；需要裁剪/缩放时只走一次 GPU/原生像素处理，并使用受限池。借用图像一直持有至编码器完成，不让回调引用失效的配置对象。

**低延迟配置不能混用。** Apple 的低延迟视频会议会话明确针对 H.264；AVC 可使用支持的 `EnableLowLatencyRateControl` 路径。HEVC 使用可支持的 RealTime、禁止帧重排及经过测量的缓冲控制，不直接假设同一个低延迟开关支持 HEVC。[P04][P23]

不再固定 High/Main/Baseline level 4.0。使用符合所选尺寸/fps 的 profile/auto level，并以实际码流参数集验证。任何“force keyframe”完成回调都经过 codec-specific RAP 解析，不把 HEVC keyframe 标志自动视为 IDR。

保留 Rust 编码事务唯一所有者；Swift 完整消费 `encoderAccepted / streamConfigured / refreshRequested` 等事实。回调只交付拥有所有权的压缩事件，重配置、发布与恢复由 Core 决定。Android/iOS 对同一种故障要返回同一业务错误类别。

## 13. 桌面硬件 Decoder

### 13.1 接口与事件

建议接口以 configure/submit/poll/reset 及带 token 的事件表达，不要求一次 submit 必须立刻返回同一张图像。异步与同步后端共享外部契约：

```text
configure(CodecConfig, NativeDeviceLease)
submit(DecodeInputToken, EncodedAccessUnit)
→ Accepted(token) / Output(token, native_frame) / Failed(token, reason)
reset(new_decoder_generation)
```

原生线程里创建 codec 对象。输入、输出 stream change、NEED_MORE_INPUT、暂不能接受、codec reset、设备资源代际各自明确；不能把“暂时无图像”都计为解码丢帧。

### 13.2 Windows

使用 Media Foundation AVC/HEVC 后端与 D3D11/DXGI device manager。AVC 可以先沿用系统 Decoder MFT，HEVC 使用实际可创建且能接受所选输入的 MFT；创建、协商、输出 IMFDXGIBuffer 与实际 DXVA 行为共同验证。[P02][P08]

`MFT_ENUM_FLAG_HARDWARE` 不能作为唯一判断：某些系统 Decoder MFT 是使用 DXVA 的软件宿主 MFT，而不是厂商独立 hardware MFT。不能因此错误排除正常硬解，也不能把纯软件输出标为 hardware。

如果选中的后端是 asynchronous MFT，必须处理 NeedInput/HaveOutput 事件和相应 unlock/消息契约；不可以拿同步 ProcessInput/ProcessOutput 循环直接套用。对 MF_E_NOTACCEPTING，应取出旧输出再重试同一输入，不能重复提交 AU 或把旧输出绑定新 token。

媒体 device 创建使用所需 video/BGRA 能力。立即上下文操作串行化或按 API 契约保护；解码 sample lease 包含资源 subresource，必要时 GPU copy 到应用池释放 Decoder 池。[P08][P09]

### 13.3 macOS

使用 VideoToolbox，AVC/HEVC 分别创建对应的 format description，要求硬件 Decoder，输出 Metal-compatible、IOSurface-backed PixelBuffer。回调保留原始 buffer，不执行 `CopiedNv12` 或 CPU 平面整理。[R19]

输出颜色、clean aperture、平面布局和 token 在回调完成时验证。第一版可保留同步 decode API，只改变输出所有权；若需异步提高吞吐，必须先通过跨 epoch/在途 reset/缓冲满测试，不能为并行度跳过代际门禁。

## 14. GPU 处理与 GPUI 集成

### 14.1 共用处理语义，不制造通用媒体图框架

`picoo-gpu` 接受不可变源帧及 `RenderSpec`：目标尺寸、crop/contain、方向、镜像、色彩、简单 overlay。每个输出记录 spec revision。CPU 生成参数和 draw commands，GPU 完成像素处理。

预览可以输出 BGRA texture；VCam 和硬件编码使用它们实际接受的 NV12/其他原生 surface。GpuNative 优先采用 GPU→GPU 复制/变换；不能直接共享 NV12 时先验证 GPU 内格式桥。若该sink不能合法建立原生交接，允许切换 CpuBridge，由同一 RenderSpec 生成目标图像后导出。CPU bridge 是显式输出计划，不是任意 shader 失败时偷偷回读所有源帧。

颜色处理包含矩阵、范围、transfer、primaries、色度位置与目标显示解释；采样器线性插值不是自动完成这些语义。对齐 BT.709 SDR 基线，禁止仅把 limited 标签改成 full 来“修色”。合成结果按输出要求保持正确颜色，不默认存在 HDR 或广色域转换。

缓存键为源 identity＋scene/output revision＋size＋device generation。相同 source/transform 的重复输出可复用已完成结果；不同线程/进程的生命周期不能凭缓存键共用一把排他锁。

### 14.2 窄范围 GPUI 补丁

以当前锁定 `gpui-pre 0.3.3` 包族为起点维护一致版本的补丁；公共视频 surface/scene primitive、Windows renderer 和 Apple renderer 必须一起核对。不要只改 `gpui-kit` 的版本约束，实际 Cargo.lock 和类型 identity 才决定链接的实现。[R16][R17]

对应上游 Surface 只实现 Apple buffer；Windows 用 DirectXRenderer，Apple 已有 texture cache，但其特定表面格式/色彩约束需扩展。[U01][U02][U03] 新增正式的视频 surface primitive，覆盖 texture source、source rect、transform、color、clip 和完成资源 lease。

Windows 不再逐帧创建 RenderImage/图集 tile；macOS 不再为 Surface 固定色彩假设做 CPU 709→601 中转。允许 native surface 自身使用静态 GPU 创建的 placeholder，但它不属于逐帧静态图片上传。

继续使用 GPUI 的裁剪、DPI、绘制次序、弹窗和窗口生命周期；不通过独立置顶 HWND/NSView 蒙住 UI 来跳过 scene 集成。合成色卡、裁剪、多层遮挡和资源释放测试属于这项补丁的交付条件。

### 14.3 PreviewPresenter

新帧到达只合并一个 redraw 请求；窗口下一次显示刷新取最新的**已完成** native frame。不得在 UI 线程等 GPU fence，未完成就继续显示上一张已完成帧。

删除固定 30fps prepare cadence＋16ms video poll 组合。状态摘要可以独立低频刷新；平台显示刷新支持 30/60、分数帧率和非整倍数情况。重复显示必须标为 repeated presentation，不计新帧。

可见性来自页面、窗口状态与有效视频区域，不靠最近 100ms 是否 paint 维持自身工作。resize 后同一 source frame 可重绘，不能被 sequence 去重挡住。窗口重新显示时直接取当前最新帧，不补旧帧。隐藏窗口仅暂停 Preview consumer，VCam 和 Recorder 独立继续。

### 14.4 输出计划、能力检测与选择（新增）

输出协议同时传递 `OutputDemand` 和 `OutputCapabilities`：需求含size、rational fps、NV12/color、crop/mirror意图；能力含可用native输入、CPU sample/CPU-filled容器、目标adapter/device、allocator世代、传输权限及各类上限。不使用`gpu_supported: bool`作全部决定。

建议状态为：

```text
Inactive → Probing → ActiveGpu
                 └→ ActiveCpu
                 └→ Unavailable
ActiveGpu → PreparingCpu → ActiveCpu
ActiveGpu / ActiveCpu → Recovering → ActiveGpu / ActiveCpu / Unavailable
任意状态 → Stopping → Inactive
```

`GpuNative`探测包括格式导入、同adapter或可用原生共享、目标allocator及可观察的一次图像准备/交付。若无法建立，只有授权控制面仍成立且CPU exporter/目标sample实际可用，才选择`CpuBridge`。常见触发是`NativeImportUnsupported`、`AdapterMismatch`、`NativeAllocatorUnavailable`、`SharingUnavailable`；普通单次槽忙、暂时无源帧不触发后端切换。

`UnauthorizedProducer`、设备权限被用户撤销、协议主版本不匹配、不受支持输出格式，不通过CPU路径绕开。原生视频decode失败、整个GPU丢失时先恢复主资源，CPU桥接不能生成不存在的新帧。静态占位可以继续输出，但必须明确新视频当前不可用。

默认自动选择；诊断工具可以在测试构建或显式诊断命令中强制GpuNative/CpuBridge复现，不增加普通用户需要理解的性能模式。后端选择不依赖“是Intel还是AMD”这类名称判断，基于实际能力与错误。

### 14.5 切换为输出事务（新增）

切换保留既有源codec、stream epoch、Decoder、scene和活跃录像。只为受影响sink创建候选OutputPlan，持有新的backend generation与output revision。准备并验证首个可用sample后，原子校验版本并替换活动计划；旧在途结果允许完成释放，但不能再次发布。系统SampleClock不因后端切换清零。

同一流内是否允许更换buffer backing必须由实际系统sample合同决定，不能假设只要NV12相同就一定支持无缝交换。允许原地切换时保持PTS单调；不允许时执行合法stop/restart/重新协商并对用户报告短暂中断，其他sink不受这次控制操作影响。若候选失败且旧输出仍可工作，保留旧计划；两者都失败进入明确Unavailable。

默认在一次sink活动期间锁定已选后端。GpuNative失败切CPU后，不逐帧探测并来回切换；下一次Start、有效device/allocator变化或显式诊断时才重新评估。持久化失败缓存需要关联OS、driver、adapter及合同版本，升级后失效，不能永久给整张显卡贴不支持标签。

### 14.6 CpuFrameExporter（新增）

输入为 `ExportRequest(native_frame, output_plan, absolute_deadline)`。只有活跃CPU demand才产生请求；没有CPU需求且没有待完成工作时，释放闲置大缓冲，进入事件等待。Exporter的输出是第7.2节的不可变CpuOutputFrame，或者平台能直接填充的独立CPU可访问PixelBuffer。

基本流程：

```text
挑选满足输出时刻的最新源图像
    → GPU按RenderSpec准备目标size/方向/color/NV12
    → 提交受限readback或等待原生缓冲可CPU访问
    → 完成事件到达后短时映射/锁定
    → 按真实plane layout复制到受限目标
    → 解锁，归还GPU/staging资源
    → 校验source/device/output/backend版本
    → 提交该sink的CPU交接缓存
```

GPU几何/颜色处理复用同一实现，避免GPU输出与CPU输出各写一套算法。Exporter不等待前一张被系统长期持有的CPU帧；有空槽才开始新工作。重复source+spec复用已准备结果。普通视频过期可丢弃；一次性隐私占位/终态帧仍保留最新任务并在事件/限频timeout后重试，不能把Busy计成Published。

CPU/IPC最终copy仍要计量。不存在跨Windows、Mac、所有allocator都保证“整个链路只复制一次”的承诺；只保证不产生无目的中间图像、不重复计算、每次搬运可归因。


## 15. Windows 虚拟摄像头：原生和 CPU 双后端

两个后端共享同一个media source、协商格式、控制、身份认证、SampleClock和业务状态。差异仅在输入资源的交接与系统sample填充。CPU后端正式发布并测试，不作为临时调试开关。

### 15.1 系统合同

重写 `media_source.rs::SetD3DManager()`，保存并检查系统传入的manager、device/adapter和allocator代际。原生输入可用且目标sample合同成立时选GpuNative；不能建立时验证CpuBridge，不直接把“没有manager”升级为整个摄像头不可用。

Windows Frame Server文档分别描述系统内存和DirectX surface的标准媒体buffer；未压缩系统内存视频推荐使用MF的2D buffer，不允许用自制IMFMediaBuffer把任意共享内存冒充系统合法sample。[P01] 本地CPU IPC的内存只是中间输入；最终必须填入提供的allocator或被合同允许的标准MF buffer。

`RequestSample`只验证状态、接收token并安排有界工作，不做GPU等待、readback、IPC打开或整帧CPUcopy。后台可用sample准备好后，经lifecycle revalidation发出MEMediaSample；Stop/Shutdown保证旧请求不再出帧。真实API约束若要求某步同步执行，应保持该步轻量并有测量，不以UI或Receiver owner代跑。

输出支持已协商的720p/1080p、30/60fps、NV12 SDR。后端选择不能暗改这些参数。

### 15.2 GpuNative

Picoo在媒体GPU准备独立输出表面，通过受权限保护的本机control channel交接句柄、generation、源帧身份及同步信息。先验证同adapter下的NV12共享，不成立但BGRA共享可用时用GPU格式桥。通过合法句柄复制/共享API传递资源，不把数值handle当作另一进程的有效handle。

Frame Server侧在与系统manager一致的device上导入输入，并GPU填充目标allocator所提供的sample。保留必要的源lease至GPU使用结束；系统下游持有输出sample不继续占用手机Decoder的DPB。CPU引用释放不等于GPU已完成。[P07][P09]

每个实际source实例独立有界共享池；不让一把keyed mutex承担所有消费者广播。正确处理timeout、abandoned与device lost，旧代际不能复用。普通槽忙只复用合法缓存或跳过本sink的旧准备，不切换codec、不重置其他输出。[P11]

### 15.3 CpuBridge：GPU 回读与跨进程交接

路径：

```text
源GPU原生帧
   → GPU输出准备（协商size/NV12/color/已应用方向）
   → 输出专用staging/可读Y、UV平面
   → CPU访问并整理有效行
   → 新版SharedCpuFrameRing
   → Frame Server后台填充合法MF sample
   → 同一个media stream交付
```

`picoo-gpu::export::windows`维护有界staging池，提交GPU copy后通过完成查询/事件推进，不紧接着阻塞Map。合法的CPU-read资源使用`D3D11_MAP_FLAG_DO_NOT_WAIT`，`DXGI_ERROR_WAS_STILL_DRAWING`是尚未就绪，安排后续有界唤醒，不能busy-spin。[P25] 立即上下文由明确owner使用；readback的Map不能错误放在不支持该读取方式的deferred context。

Map时读取真实RowPitch和有效平面布局，释放时Unmap。不能把1920×1088的分配高度当成1080可见高度，也不能仅依据buffer总长度推导UV起点。若目标NV12 staging能力缺失，可在GPU上输出独立Y/UV平面再按序复制，不以CPU大循环重新实施颜色/缩放算法。CopyResource/CopySubresourceRegion不是格式转换器，变换需在之前完成。[P28]

CPU输出按需求采样，先在GPU上降到客户端要求的尺寸，再回读；不能先回读完整1080p/60fps，再在CPU端缩为720p/30fps。两sink同source/spec时共享不可变物化结果，IPC、目标样本copy按实际实例分别计数。

### 15.4 SharedCpuFrameRing：新版本，不恢复旧兼容

重构/复用旧ring成熟的槽位所有权、进程崩溃恢复、Busy重试与不覆盖读者的逻辑；新版ABI仅服务CpuBridge。建议每个实例初始三槽，固定header包含magic、IPC主版本、instance/session generation、slot容量及布局；每槽包含backend/output revision、source identity、色彩/size、两平面offset/stride/rows、数据长度与序列。

创建时验证checked arithmetic、文件/共享映射长度和协商最大值；layout在该generation内不可变，格式改变新建generation。发布数据与读写lease必须有可证明的同步关系。**仅比较前后sequence不是内存安全同步，不能无锁读写同一普通字节数组形成数据竞争。** 保留已验证的进程锁/原子lease机制和对应压力测试。

control channel用于Start/Stop、需求、心跳和generation，不逐帧通过JSON/pipe发送大图。进程有独立的最小权限；Frame Server可能运行在Local Service/Session 0，不能只按桌面用户名授予访问，也不能开放Everyone可写。[P01] 认证失败不通过CPU channel绕开。

consumer取到稳定完整帧后，在后台完成向自身缓存或合法sample的有界copy，释放ring lease；不让会议软件持有MF sample时继续占着生产者槽位。全槽忙返回Busy并保留最新请求，不递增published、不清除失败事实。producer/consumer崩溃后通过内核租约与generation恢复，不能强行清零仍存活读者的lease。

已断开或撤销授权时清除真实画面保留资格；静态CpuPlaceholder按合法采样继续。复用缓冲时不泄露上一张大图的尾部，跨实例/权限边界回收时清理敏感内容。

### 15.5 系统 sample 填充

sample来自管线指定allocator，或在合同允许时使用标准MFCreate2DMediaBuffer等API。不能为省copy把任意mmap裸地址包装成自定义buffer交给Frame Server。

写入优先使用`IMF2DBuffer2::Lock2DSize`取得可访问范围与pitch；必要时遵守文档列出的2D锁/普通锁回退顺序，RAII保证解锁。扫描线起点、实际pitch、UV平面偏移和buffer边界必须验证，不能把紧凑源一次memcpy到有padding目标。负pitch通用情况需检查；对本产品NV12未支持的布局明确拒绝或标准重分配，不能盲用RGB规则。[P26]

同一源图像重复输出时复用准备缓存，不重复GPU readback；但下游占用着的可变sample不能被覆盖。新的sample按目标buffer合同设置有效长度，并有合法时间戳、duration和请求token；不能把分配容量直接当成有效图像长度。重复图像保留原source identity。

如果目标allocator只提供GPU sample，而跨device native输入失败，CPU后端可以从新版CPU IPC在目标device完成一次上传，再填入该allocator。这必须报告为`CpuBridge + native-upload`，不是GpuNative/零回读。若目标buffer既不可合法CPU写，也无可行上传路径，则该sink仍不可用。

### 15.6 多 GPU、切换与故障

普通D3D11共享要求匹配adapter。[P12] 新增VCam的目标adapter与Picoo源GPU不同，默认保持源GPU和活动录像，验证CpuBridge；不为新客户端搬迁整个Decoder。仅在用户显式重启会话或计划允许的资源重建时重新选主GPU。

manager/allocator变化推进output资源代际，按第14.5节切换。原生输出失败但source GPU正常，可以局部切CPU；source GPU已经lost，须恢复源GPU，CPU仅可输出合法缓存/静态占位，不能继续新画面。中断显式可见，重试有界。

安全/权限、协议拒绝是终止条件。无法原生共享的合法授权参与者可以用CPU，未授权参与者不行。

### 15.7 Windows 验收

覆盖GpuNative、CpuBridge system-memory、必要的CpuBridge native-upload；两adapter、同adapter、无核显独显、缺manager、只提供CPU buffer、布局padding、30/60fps、多客户端、所有槽忙、consumer崩溃、Stop后完成、重新连接、低内存。强制CPU测试不能因开发机原生能力强而跳过。

第三方软件可能在系统外部自行转换图像；Picoo只声明自己路径的后端和copy计数，不将外部行为当作自身原生后端不存在的证明。


## 16. macOS 虚拟摄像头：同一 CMIO 合同，两种准备方式

原生路径继续用CMIO Extension sink/source。CPU后端改变的是图像准备方式，不要求取消IOSurface、另造一套公共虚拟摄像头，也不假定“CPU可访问”与“原生PixelBuffer”互斥。

### 16.1 共同控制与交接

应用通过Core Media IO接口找到本项目受控sink并提交CMSampleBuffer；扩展consume并回报scheduled-output，source按自己的合法采样时钟输出。[P06] Start/Stop、需求、格式、身份、generation和sample时间在两个后端相同。

使用系统提供的client身份和签名信息进行授权，不能相信消息内自报PID或名称。[P21][P22] 无权限、sink不存在或CMIO交接本身不可用，不意味着换CPU就能修复，应按功能错误报告。

### 16.2 GpuNative

VT输出原始PixelBuffer由GPU准备到sink支持的目标图像，保留IOSurface/native backing。GPU完成可见后才交给系统；正确retain至平台及GPU使用结束，不只传surface ID就立即复用。[P06][P10]

source重复缓存图像时只生成新的sample时刻，不重复颜色处理。需求消失后停止持续准备。格式变化推进output revision，不能让旧buffer进入新合同。

### 16.3 CpuBridge

```text
原生源帧
  → GPU准备目标size/NV12/颜色/方向
  → 输出执行器确认GPU完成
  → 对可CPU访问缓冲执行只读Lock
  → 按Y/UV真实stride复制到sink可接受的独立池化PixelBuffer
  → Unlock
  → 使用与GpuNative相同的CMIO sink/source交接
```

目标PixelBuffer仍可由IOSurface支撑，以满足系统传输；它由CPU填入像素并不使其失去原生容器属性。只有当前source/target图像对象直接交接无法成立、而CPU重建的标准目标buffer可以合法交接时，这条路径才有意义。不能宣称它绕开CMIO系统限制。

源图像若已具备正确几何并能安全直接CPU读，可省掉中间GPU变换；否则在独立输出池完成变换后再读，避免对仍被预览使用的可变图像写入。CPU访问采用匹配的Lock/Unlock标志，读锁不允许写源像素。Apple明确要求CPU访问前锁定，并说明单纯GPU访问不需要这种锁。[P27]

CPU lock/copy放在输出工作线程，不在Receiver、GPUI或CMIO实时回调中等待。M4统一内存并不消除同步成本，因此同时记录lock耗时、读取字节与copy次数。目标缓冲发布后不可变，系统持有期间不得复写。能直接填目标PixelBuffer就不额外分配Vec绕一圈。

本版优先不在Mac另建与CMIO并存的像素文件ring；旧SharedRingReader与SharedRingAtomic退出该平台产品。需要的CPU plane-copy、边界测试可以移入新adapter复用。如果真实测试证明某个系统场景连CPU填充的标准sink sample也不接收，明确报错或另立协议变更，不能写成自动回退已经成功。

### 16.4 后端选择、占位与采样

输出router与Windows遵守同一状态机，但平台能力证据不同；native输送/导入失败才测试CPU准备。CPU版本通过相同RenderSpec与颜色验证，不自动降30fps。sink下游若需要CPU访问但当前GpuNative sample已被正常接收，不为猜测消费者内部行为预先复制。

当前图像重复使用不重复CPU准备。无输入、device重建或权限撤销后的占位可预先CPU填入标准PixelBuffer，以确保静态合法输出不依赖失效GPU；隐私hold上限适用两条路径。30/60fps sample时间与unique source frame统计分别维护。

### 16.5 macOS 验收

真实VT AVC/HEVC→GpuNative/强制CpuBridge→CMIO sink/source→consumer全链测试，包含CPU填入的IOSurface-backed目标、输入/输出pool上限、锁定失败、格式变化、source重启、旧sample、客户停止、GPU completion迟到与权限失败。离屏Metal测试不能替代CMIO跨进程验证。


## 17. 录像：独立的两个消费者

### 17.1 原码流录像 `EncodedRecorder`

输入为接收端重组完成的 AU 事件，位于 live scheduler 丢弃/Decoder 之前。Recorder 有自己的短有序队列和依赖/缺口记录；不能因预览恢复清空队列而悄悄删掉已经可写入的完整编码数据。连接代际与 AU ID 保持连续可追溯。

开始录制进入 `Arming`，请求并等待有效随机访问点和参数集后才建立可独立解码的第一段；UI 显示“正在准备录制”，actual start 记录真正第一张可写入 AU 的时间。HEVC CRA 入段遵守第 9 节的 leading-picture 规则。

原码流录像不重新编码，也不从 LatestFrameStore 抽帧。保存的是**手机编码后、电脑实际接收到的内容**，不是手机本地可靠母版。无线丢失或 AU 已在重组期限结束前不完整时，记录 gap，结束受影响依赖链，等待下一 RAP 恢复；文件结果为 `HasGaps`，不能显示普通完整成功。

只在预览上应用的镜像、字幕、裁剪不会自动写入压缩像素。原码流文件可写支持的方向元数据，并在 manifest 保存完整呈现意图；需要所有播放器看到完全一致效果时选处理后录像，不能依赖播放器一定解释自定义镜像/crop 信息。

### 17.2 处理后录像 `RenderedRecorder`

从 FrameBus 的发布事件订阅，不从容量一的 Preview cache 轮询。使用自己的有界有序队列、RenderSpec、输出尺寸、帧率和源媒体时间；窗口隐藏/resize/设置页切换不改变录像。

共享源帧的所有权，但不无限持有 Decoder DPB。进入 Recorder 的图像必要时 GPU copy 到 Recorder pool。GPU 完成场景处理后将目标 surface 送给指定的硬件 AVC/HEVC 编码器；录像 codec 不必等于传输 codec。

源 60、录像 30 时，按照固定的媒体时间采样；这是用户/配置明确选择的输出速率，不是性能变差后的静默降级。源图像确实缺失时记录 gap/duration，不能仅用重复帧和连续 PTS 宣称原始每帧齐全。

编码器或磁盘来不及，触发 `Overrun`，结束当前段或停止此 Recorder，向 UI 和 manifest 报告。不得反压 Camera、Decoder、Preview 或 VCam。控制中断后清理该录制资源，但保留已经成功落盘的部分。

### 17.3 硬件编码与容器封装

Windows 使用显式选定、能接收原生图像的硬件 Encoder MFT；若使用 Sink Writer，则必须同时设置对应 D3D manager、允许硬件及 hardware-only 策略，并验证实际后端。只有 enable-hardware 标志不足以排除软件编码；hardware-only 的版本及前置条件以官方文档为准。[P14][P15]

macOS 使用要求硬件的 VTCompressionSession，再把压缩 CMSampleBuffer 交给 AVAssetWriter。原码流路径使用不重编码写入，两个路径汇合到 Muxer，不必为 Recorder 再解码。[P13]

第一版选 MP4 为输出容器，平台 mux adapter 分开实现。Windows 的 MPEG-4 sink 文档只保证自动生成 AVC/AAC/MP3 的 sample description；HEVC 需要正确构造并提供 sample description 与 codec configuration，不能仅把 subtype 改为 HEVC 就宣布支持。[P16] bitstream 模块输出经过独立文件校验的 AVC/HEVC configuration record，由 mux adapter 按 API 要求封装。AVC 的 Annex-B、HEVC 的 sample entry/参数集存放方式以各后端契约适配，内部 wire 格式不因此反复变化。

HEVC MP4 必须验证参数集、长度字段、时间戳、RAP flags、段首和色彩，分别经 Windows 与 Apple 系统 Decoder 以及离线独立校验器读取。平台 mux 的特殊情况是实现与验收事项，不允许借用未经检查的“MP4 都支持”概括替代。

### 17.4 可恢复落盘与文件结果

录制工作目录使用 bundle：

```text
recording/
  manifest.json
  segments/
    000001.mp4
    000002.mp4
    000003.partial
  exports/                 # 正常结束后的单文件导出，可选
```

每段从独立 RAP 开始；初始按约 10 秒请求一个段边界。编码配置变化、确认的缺口立即切段，不能混用 sample description。写入临时文件，只有正常 finalize 成功后才改为完成文件并更新 manifest。低延迟编码模式可能不提供固定周期 RAP，因此必须主动请求并验证，而非仅依赖 GOP 默认值。

在支持的后端内使用 fragmented MP4 降低正在写入部分的损失，但“设置 fragment interval”不等于任意崩溃后文件必然完整；fragment/finalize 行为必须故障注入验证。Apple 提供 movieFragmentInterval，Windows 提供 fragmented MPEG-4 sink，这些是可用机制，不是耐久性保证。[P17][P18]

正常结束后，相同配置的连续段可以不重编码地 remux 成单个 MP4。多配置录制先交付分段与 manifest，不静默转码成一个规格。失败时仍保留成功 finalize 的段，未完成 `.partial` 保留为可检查对象，不冒充完整成片。

manifest 至少包括：recording ID、模式、开始/结束/状态、codec/配置分段、源与输出时间映射、source frame/AU 范围、gap 区间和原因、scene revision、文件摘要及软件版本。状态为 `Complete / HasGaps / Failed`，任何 gap 都是粘性的，不被后续成功覆盖。

本期只有视频轨。音频和手机本地补传不在此方案中被默认“顺便解决”；也不承诺 QUIC Datagram 接收录像在丢包时与手机原始编码完全一致。

### 17.5 与 CPU 摄像头输出的隔离（本版补充）

EncodedRecorder仍取完整AU，不经过CpuFrameExporter。RenderedRecorder仍订阅Native FrameBus，GPU生成录像图像并交给硬件Encoder。虚拟摄像头切到CpuBridge不改变它们的输入、codec、时间线或完成状态。

没有硬件Encoder时不能借用CpuOutputFrame启用软件录像；没有硬件Decoder时CPU bridge也不会成为软件解码器。通过其他软件录摄像头，可以使用本版CPU摄像头输出，但外部录制的编码选择、音频和文件状态不由Picoo内建Recorder保证。

CPU路径占用共享内存/总线可能影响系统负载，必须纳入并用测试。若CPU输出超期，记录该sink错误/重复sample；不能把受影响的录像结果标为Complete而忽略实际丢失。


## 18. 质量策略与规格

默认使用已通过能力准入的 1080p60；四种正式尺寸/fps 组合均有 AVC/HEVC 实现。HEVC 与 AVC 都是本代正常能力，选择依据是当前设备实际质量与延迟表现，不把 H.264 标为慢速兼容档。

工程初始试验值如下，单位 Mbps；它们不是最终产品保证，不得未经同源画质测试直接当作发布常量：

| 配置 | AVC 初始实验值 | HEVC 初始实验值 |
|---|---:|---:|
| 1080p60 | 24 | 16 |
| 1080p30 | 16 | 10 |
| 720p60 | 10 | 7 |
| 720p30 | 6 | 4 |

1080p60 在上述点位周围继续做高低两侧质量测试，使用细纹理、头发、衣服、运动和暗光。不要认为固定 bpp 公式或“HEVC 必然减半码率”适用于每个硬件实时 encoder。

运行中不因为用户缩小预览就改变 source resolution/fps。码率可在当前配置声明的质量区间内调整，但不能突破最低质量目标还保持正常状态。持续热限制、编码来不及、网络明显偏离基线时显示原因，用户可显式重配；不自动启用软件codec或悄悄从60改30；VCam在同一输出规格下选择CpuBridge是允许且可观察的后端切换，不是源配置降级。

同尺寸验收需要同视场、有效裁剪、方向与合适的物理显示尺寸。手机预览在60fps下也必须保持合理曝光/画质，不能把本地参考降成糊画面来与电脑相同。颜色数值用离屏标准图校验；不同物理屏幕校准的差异另外说明，不把系统截图的颜色差异全部归为编解码失真。

同一源帧和同一RenderSpec下，GpuNative与CpuBridge目标像素应按相同算法生成。纯plane copy以有效像素逐字节一致为目标，padding不参与图像比较；不同平台GPU滤波/舍入允许事先声明的数值误差，不用“反正CPU路径是兼容”接受明显偏色或发糊。输出CPU像素本身不增加有损压缩，额外损失若存在应定位到格式、滤波或错误的颜色解释。


## 19. 观测、性能与验收

### 19.1 自动度量

不要求用户先手工收集记录；在开发、自动化和真机验收中自动采集。源帧、显示、VCam sample、录像各有统计，不能用sample重复数填补unique frame不足。

| 类别 | 字段 |
|---|---|
| 源与实际后端 | requested/accepted codec/profile/size/rational fps/color；硬件证据；device/adapter/driver与代际 |
| 输出路由 | sink_id、GpuNative/CpuBridge、delivery backing(system-memory/native-upload/native-surface)、选择/切换原因、backend generation |
| CPU输出 | demand_count、export_requests、unique_exports、reused_exports、bytes_readback、bytes_cpu_copy、bytes_ipc_copy、bytes_target_upload |
| GPU与缓冲 | 各pool的slots/bytes/in-flight/current/peak；readback等待与Map/lock耗时；cancelled/stale completion |
| 节拍 | capture/encoded/decoded/presented unique；VCam total samples/repeats/unique；source60→output30的采样结果 |
| 时间线 | 接收/重组/Decoder/GPU submit-done、export submit-done、IPC ready、sample delivery、present反馈、不确定度 |
| 错误 | codec配置拒绝、RAP候选失败、AU abort、Busy/超时、权限拒绝、CPU bridge不可用、device lost、命令拒绝 |
| 录像 | accepted/written AU、encoded/written frame、queue age、gaps、overrun、segments/finalize结果 |

同进程共享一次CPU物化与每个IPC/sample的copy分开计数，不把“导出一次”误报成全链零复制。只在诊断中展示后端，不将每次正常重复sample显示成用户警报。输入图像不默认落日志，截帧必须显式启动并有独立隐私/容量限制。

没有真实present feedback就只报告提交时间；跨时钟不可比较时端到端为unavailable。平台system内部的copy不可观察时标为unknown，不凭自有代码计数宣称全系统零回读。

### 19.2 继承的功能与性能目标

以下是工程验收提案，不是既有测量。条件为固定场景/照明、良好LAN、一部手机一台电脑、实际系统/driver明确。高帧率验收使用能显示相应刷新率的屏幕。启动、主动重配置和设备故障单独分桶，不能无说明地从一般卡顿数据中删去。

| 场景或指标 | 目标 |
|---|---|
| Preview-only | 自有稳态视频路径完整CPU物化/readback为0，exporter无持续任务 |
| Preview + GpuNative VCam | 自有路径无完整CPU中转；native-aware harness验证系统样本backing |
| Preview + CpuBridge VCam | 允许且明确统计CPU图像；不把预览/Decoder改CPU；同规格不过度重复导出 |
| CPU request停用 | 无新持续readback，已有任务在有界寿命内释放；静态占位初始化单独计数 |
| 可用完整源60fps→请求60fps | 输出/显示有效unique率参考≥99%，稳定60时≥59.4fps；CPU/GPU都遵守 |
| 可用完整源30fps→请求30fps | 参考≥29.7fps；不能以重复sample充数 |
| 源60→请求30 | 按约定媒体时间抽取30个不同图像；不存在每帧都回读后再丢一半 |
| 呈现间隔 | p99目标不超过约2个源帧周期，源本身异常另记但不隐瞒 |
| Receiver入口→呈现 | 有实际反馈时p95目标≤50ms；无反馈只验对应submit目标 |
| Capture→呈现 | 有可比较时钟时p95目标≤150ms；相对手机本地显示额外延迟另外验收 |
| CPU出口额外处理 | 从相应GPU输出ready到CPU sample可交付的p95初始预算：60fps≤16.7ms，30fps≤33.3ms；不含系统下游不可观测时间，必须实测 |
| 全部并用 | 预览＋VCam＋一个RenderedRecorder，可叠EncodedRecorder；CPU/GPU两条VCam各测，不静默降源配置 |
| 持续使用 | 60分钟暖机后资源高水位有上限，不按帧数/切换次数持续累积 |
| 录像可靠性 | 正常条件无未报告缺口；异常与manifest一致 |

“CPU出口一帧周期”是初始开发预算，不是当前硬件保证；实测未达标必须改进、标记配置暂不可用或明确征得配置调整，不能在报告中把请求60改成实出30后仍称通过。

预览独立使用、GPU摄像头使用、CPU摄像头使用分别出A/B结果。CPU路径不可用全局“readback=0”验收，GpuNative也不能拿CpuBridge的测试成功代替。吞吐、延迟、画质、资源消耗同时报告；不承诺整机固定倍数。

### 19.3 平台与消费矩阵

Android→Windows、Android→Mac、iOS→Windows、iOS→Mac，分别测AVC/HEVC与四个正式配置。小米15→Windows优先日常观感验收，小米15→Mac mini M4持续自动化和实际CMIO验证，iOS独立真机测试。

Windows包含Intel核显、AMD核显、AMD独显无核显、NVIDIA、双GPU。除了成功native共享，还要故障注入同adapter不支持格式、跨adapter、缺manager、system-memory allocator、native-only allocator和CPU上传分支。不是只测一台强显卡能走native就完成。

Mac对GPU和CPU填充PixelBuffer的真实sink/source路径分别强制测试；检查CPU锁的执行线程、池、已完成GPU写入和授权。无显示器自动化可以覆盖离屏处理与文件、IPC，但不声称代替实际窗口呈现。

第三方消费端至少包含用户日常使用的会议软件与OBS，记录实际版本/系统/使用方式。尚未验证的组合标为未验证，不写“所有软件可用”。Linux CI仅测纯核心/FakeNativeLease，不增加软件生产Decoder。


## 20. 失败、安全与生命周期

### 20.1 局部回退规则

| 条件 | 本版处理 |
|---|---|
| `PROTOCOL_MAJOR_MISMATCH` | 拒绝；CPU桥接不接旧PCP/IPC |
| `HW_CODEC_UNAVAILABLE` / `CODEC_CONFIG_UNSUPPORTED` | 该codec配置失败；不启用软件编解码 |
| `NATIVE_IMPORT_UNSUPPORTED` / `VCAM_NATIVE_ALLOCATOR_UNAVAILABLE` | 已授权且可导出时测试并选择CpuBridge，只影响当前sink |
| `GPU_ADAPTER_MISMATCH` | 尽量保留源GPU/录像，使用CpuBridge到合法目标；无法填充目标才拒绝 |
| `GPU_RESOURCE_BUSY` | 正常有界等待/缓存/过期丢弃，不把单次Busy当成GPU不可用或切换理由 |
| `GPU_DEVICE_LOST` | 停旧资源、推进device代际、有限重建；CPU无新源可用时仅静态占位，不假装视频恢复 |
| `CPU_READBACK_UNAVAILABLE` / `CPU_LAYOUT_UNSUPPORTED` | 当前CPU路径不可用；原生仍可用则保留，双路径都失败才关闭该输出 |
| `OUTPUT_BUDGET_EXCEEDED` | 局部报告超期/容量不足；不无限排队，不改变全局源配置 |
| `UNAUTHORIZED_LOCAL_PRODUCER` / 用户撤销权限 | 拒绝所有后端，停止真实内容输出，不借CPU绕过 |
| `RECORDING_OVERRUN` / `STORAGE_FULL` / `MUX_FINALIZE_FAILED` | 保留已完成段，Failed/HasGaps；不改为CPU录像继续冒充成功 |
| `AU_ABORTED` / `AU_EXPIRED` / `REFERENCE_LOST` | codec-aware恢复；录像独立记录缺口，与VCam后端选择无关 |

后端自动选择只限同代合法配置，不是软件全链回退。新硬件资源初始化失败与暂时的不可用要分类；不每帧重建codec/device、不来回抖动。因系统合同必须restart的切换明确报告，不承诺无条件零中断。

### 20.2 安全与隐私

PCP配对、签名、TLS/QUIC、channel binding继续保留。所有dimensions、NAL、AU、plane stride/offset/length、IPC slot和分配上限使用checked arithmetic，不相信远端/IPC对方声称的数据长度。

GPU handle与CPU共享内存采用同等级授权。新版本CPU ring只有明确受信的producer/consumer能打开，名字随机或难猜不能代替ACL与身份校验。Windows正确覆盖Frame Server实际服务身份及用户隔离；Mac继续遵守CMIO扩展权限，不能以文件共享绕开身份检查。

CPU内存增加一份可读图像，需计入隐私生命周期。Stop/撤权后不再产生新真实帧；旧画面只按显式hold保留（建议默认不超过500ms，作为待确认工程上限），随后必须发布占位。缓存/IPC/目标sample都检查相应generation；没有新的camera图像也要推进占位，不能等待“下一帧再覆盖”。已交给外部系统且由其持有的历史sample无法追回，不声称可清除第三方已有副本。

共享区域重用遵守有效长度和plane范围，不把padding/上次较大帧尾部暴露给新权限主体。generation切换、关闭和跨实例复用按预算清理敏感区域，清理不在UI执行。升级只清除本项目旧注册/IPC，不删除用户录像。

### 20.3 取消和资源回收

每项异步工作带适用的connection、stream、decoder、device、output、backend revision。最终校验与提交是同一受保护操作。普通deadline过期只撤销“结果可发布”资格，不意味着GPU命令或系统sample已经结束；实际资源等其真实完成后归池。

CPU mapped指针不能跨Unmap、PixelBuffer锁借用不能跨Unlock，旧IPC视图不能逃出lease。GPU/CPU缓存均不可在消费者读取时覆写。共享ring的崩溃恢复必须区分活读者和已死进程，不得通过仅清零计数掩盖竞争。

所有UI/CLI有副作用命令明确报告接收/拒绝/完成。Stop/Disconnect/Shutdown有可达控制路径；句柄销毁不在UI无界join。CPU exporter隔离阻塞能力，不代表可以无限累积挂住线程；故障后禁止不停新建worker绕过尚未退出的资源。


## 21. 逐文件修改与删除清单

新路径为设计提议，实施时核对当前主干引用。复用经过验证的算法允许，保留旧版本运行分支不允许。

| 位置 | 动作 |
|---|---|
| `Cargo.toml`、`Cargo.lock` | 最多新增bitstream/gpu/recording三个crate；统一GPUI patch；删除生产软件codec链接 |
| `proto/picoo_camera.proto` | typed AVC/HEVC、参数集、完整offers、rational fps、clock、abort；新主协议 |
| `crates/picoo-bitstream/`（新增） | 双codec NAL/参数集/RAP/配置记录；独立fuzz |
| `picoo-protocol`、`picoo-packet` | 新header和长度边界；移出仅AVC helper，保留FEC/重组 |
| `picoo-sender/src/session/*`、`picoo-ffi/src/*` | 完整编码事务和新ABI，Android/iOS同语义，无旧入口shim |
| Android `MediaCodecH264Encoder.kt` | 替换双codec adapter；actual格式、硬件、低延迟和RAP验证 |
| Android `Camera2DeviceSession.kt`、`CameraEncodingCompositor.kt` | 真实capture组合、统一几何、唯一PTS、时钟和surface生命周期 |
| iOS `VideoEncoder*.swift`、`CameraCapture.swift` | 双codec实际能力，AVC/HEVC低延迟分别配置与验证 |
| `picoo-frame-hub/src/frame.rs/color.rs/storage.rs/native/*`（新增/重构） | 唯一native帧和时间/lease类型；不隐式返回CPU图像 |
| `picoo-frame-hub/src/output/cpu_frame.rs`（新增） | CpuOutputFrame、plane layout、受限不可变buffer lease；仅输出/诊断使用 |
| `latest_frame_store.rs`与FrameBus | native发布事件；preview保留latest，录像不轮询latest |
| `picoo-media-decode/lib.rs`、`mf.rs`、`videotoolbox.rs` | 双硬件codec、tokenized native输出，无软件fallback |
| `picoo-receiver/session/media_publish.rs` | 删除owner内CPU transform与into_cpu_nv12；绑定原配置并发布 |
| `media_scheduler.rs`、`recovery.rs`、`decoder_worker.rs` | 保留准入/在途候选/代际，扩展双codec与native output |
| `picoo-receiver/src/output/coordinator.rs`（新增） | demand、OutputPlan、GpuNative/CpuBridge状态机及切换事务 |
| `picoo-gpu/src/*`（新增） | 设备/RenderSpec/表面池；`export/windows.rs`、`export/apple.rs`负责输出专用CPU访问 |
| GPUI patched core/windows/apple | 原生video primitive、颜色/clip/完成lease；退出静态视频atlas |
| `apps/desktop/video_surface.rs`、`preview_pipeline*` | native Presenter取代CPU预览缩放/转色/图片上传，不恢复CPU预览backend |
| `apps/desktop/gpui/lifecycle.rs` | 视频事件与状态刷新分离；显式visibility，不视频16ms轮询 |
| `picoo-frame-hub/src/shared_ring/*` | 重构为新版CPU输出IPC（建议模块名`output/cpu_ipc`）；保留安全lease/Busy/崩溃逻辑与测试，删除旧ABI和全局必经依赖 |
| Windows `frame_provider*`、`windows_source/*` | 共用source生命周期和SampleClock，增加两backend；SetD3DManager真实处理，CPU sample按实际stride填充 |
| Windows `sample_copy.rs` | 不再全删；重写为CpuBridge专用、平面/边界正确、可测量的最终copy |
| Windows旧`ring-reader`工具 | 改成本代CPU/GPU capture harness；不继续读取旧版本ring |
| Mac `SharedRingReader.swift`、`SharedRingAtomic.c/.h` | 从新Mac产品移除旧像素文件通道；复用需要的copy校验测试到新CPU adapter |
| Mac `PicooCameraProvider.swift`及新sink/bridge资源模块 | 同一CMIO sink/source下native/CPU-filled PixelBuffer，统一需求与授权 |
| `picoo-recording/`（新增） | 两Recorder、native硬编/mux、gap/segment/manifest；不依赖CpuBridge |
| `metrics/diagnostics/sim/testkit` | 按backend计数、强制CPU/native测试、回退边界和生产等价时序 |
| `xtask`、安装、CI | 同版本bundle、CPU IPC权限和清理、完整四平台验证入口 |
| `docs/design-specs/*` | 用本版替代GPU-only硬约束；需求009/013/024修订，新增029—040追溯 |

明确删除：旧PCP/FFI/IPC兼容分支、生产软件codec fallback、全局CpuNv12源帧、Receiver CPU变换、GPUI逐帧CPU图片路径。明确保留/重构：输出专用CPU exporter、plane copy、新版CPU IPC、占位、buffer pool和安全回归。不能用“删CPU”把这次保留的正式输出后端再次删掉。


## 22. 实施批次与完成门槛

不把用户手工记录作为前置条件。各批次带自动度量和最小真实链路验证，采用同输入/规格/场景做比较。

### 22.1 先验证高风险接口

开发探针同时覆盖Windows MF→原生图像→native/CPU系统sample，Mac VT→native/CPU-filled PixelBuffer→真实CMIO，AVC/HEVC文件封装，以及GPUI原生合成图显示。

CpuBridge成立不能用来跳过GpuNative开发；GpuNative成立也不能跳过forced-CPU测试。若某个平台CPU填充目标仍不能满足系统合同，必须记录真实限制，在发布矩阵标为不可用，不能只实现一个Vec生成函数就宣布fallback可用。

### 22.2 分批提交

| 批次 | 内容 | 完成门槛 |
|---|---|---|
| P0 | 修订40条需求、平台探针、双codec容器/新协议约定 | CPU/native输出边界都可执行验证，识别各平台合同限制 |
| P1 | bitstream/native帧/token/typed protocol/FFI，CpuOutputFrame类型边界 | 解析fuzz、生命周期、无隐式readback、混版本拒绝 |
| P2A/P2I | Android/iOS双硬件编码 | actual配置、RAP、时钟、本地参考不降质 |
| P3W/P3M | 硬解、GPU池、GPUI video surface | preview-only零CPU整图；正确颜色/资源释放 |
| P4 | FrameBus、Presenter、OutputDemand/Plan、独立SampleClock、路由状态机 | hidden/resize与输出互不混淆；后端切换代际正确 |
| P5W/P5M | 两平台GpuNative VCam | 实际跨进程、manager/allocator/身份/stop，原生sample验证 |
| P6W/P6M | CpuFrameExporter、Windows新版CPU IPC、Mac CPU-filled sink sample、自动切换 | 强制CPU完整输出、跨adapter、stride、无需求不导出、Busy占位重试 |
| P7 | 原码流与处理后Recorder、段恢复 | 不读latest、硬件编码、双codec文件、gap/failure状态正确 |
| P8 | 质量/60fps、CPU与GPU路径并用、热稳态矩阵 | 同规格验收，无静默降档；CPU开销归因、非目标sink不被拖入CPU |
| P9 | 旧ABI/依赖/工具清理、规范、发布 | 新CPU后端保留，旧回退退出；四平台交付证据齐全 |

P5与P6在共同接口明确后可以并行，先用CPU完成某环境的系统摄像头回归也可以，但不能把GPU功能延期为可选。Windows优先体验验收不缩小Mac/iOS范围。阶段分支可以破坏旧接口，正式发布不能保留dummy frame/panic占位实现冒充可用。


## 23. 需求追溯与回归清单

| 需求组 | 主要实现 | 最少回归 |
|---|---|---|
| 001—005 双平台/codec/规格 | bitstream/proto/mobile/decoder | 四连接组合×两codec×四规格；硬件不足明确；offers非笛卡尔积 |
| 006—008 观感与并用 | RenderSpec/Presenter/quality | 同源/几何/颜色；GPU/CPU VCam各自并用；不降源规格 |
| 009—011 主链与身份 | native lease/Decoder/Exporter边界 | preview不readback；旧token不发布；AMD独显无需核显 |
| 012—017 输出时钟/资源 | FrameBus/Presenter/VCam backend | hidden、同帧resize、slow consumer、device lost、池峰值 |
| 018—022 录像 | Recorder/mux/manifest | RAP启动、缺AU、盘满、切codec、缩窗口/CPU切换不改录像 |
| 023—025 恢复和有界 | scheduler/recovery/AU sender/runtime | 候选RAP参考链、CRA/RASL、cancel、命令拒绝、期限不重置 |
| 026—028 版本/权限/观测 | auth/install/IPC/diagnostics | 混版本拒绝、恶意尺寸、真实路径与unique统计 |
| 029—032 CPU隔离/选择/等价/需求 | OutputCoordinator/Exporter/RenderSpec | 无需求零导出；CPU只影响相应sink；同图像无新增压缩 |
| 033—035 执行器/资源/复用 | staging/CPU pool/IPC/sample | Map未就绪、CPU锁慢、全槽忙、source60→output30、相同spec导出复用 |
| 036—038 切换/跨adapter/安全 | backend transaction/CPU IPC/CMIO | stop竞态、旧completion、正在录像时跨GPU新sink、权限撤销/崩溃 |
| 039—040 CPU规格和不可修复错误 | 性能harness/capability faults | CPU30/60强制验收；缺codec仍失败；不绕过授权 |

固定重现：

1. 候选RAP100在途、P101到达、确认后P102，不能形成RAP100→P102的参考缺口。
2. 大于旧256KiB的合法AU在期限内完成，或明确Abort并可恢复，不把部分AU交Decoder。
3. 同一源60fps，只打开CPU720p30输出，export次数不得随source60fps持续翻倍；后续关闭后计数停止增长。
4. 已有GPU预览与RenderedRecorder，新增跨adapter VCam：只为新sink选CPU，原录像不中断且源device不升代。
5. GPU输出候选切CPU的间隙Stop/变更格式，旧GPU和CPU完成都不能覆盖新状态，sample时间不倒退。
6. 旧预览/源frame被慢消费者持有，exporter依然有严格资源上限，不无限占用Decoder DPB。
7. CPU IPC三槽被占满时提交断线占位：没有虚假Published；释放槽且不再输入新视频后占位能落地。
8. 1920×1080有效图像、1920×1088分配、目标pitch不同：Y/UV不串行、不越界、不出现绿屏；有效像素与参考匹配。
9. Mac目标CPU-filled IOSurface-backed PixelBuffer由CMIO真实消费，retention/Lock/Unlock正确，不把仅同进程测试当验收。
10. 源GPU被移除：没有新视频时CPU仅发布合法占位，不伪报CPU已修复解码；权限拒绝不能换路径访问。
11. native/cpu-only测试与并用暖机60分钟，资源不按切换次数增长，真实帧率与重复sample分别报告。
12. 最后一段录像失败，已完成段保留；CPU摄像头失败不会把录像状态错误改成成功或失败。


## 24. 风险与明确决策

| 风险 | 本版选择 | 验证与剩余限制 |
|---|---|---|
| “保留CPU输出”膨胀成CPU全链 | 独立CpuOutputFrame/Exporter，不进入源/预览/录像 | 构建依赖与readback归因 |
| 显卡能HEVC但OS后端不可用 | actual codec创建与硬件证据，不software fallback | CPU桥接不修复codec安装 |
| GPU共享/allocator不匹配 | 局部CpuBridge；仍满足目标sample合同 | 有些合同两条路都不行，明确不可用 |
| 双GPU冲突 | 保持源GPU，CPU在输出边界过桥 | 目标仅GPU样本时可能增加一次upload，必须计量 |
| CPU回读同步拖慢整机 | 按需/目标尺寸/有界staging、显式deadline | 不能保证零影响，需CPU/GPU并用测量 |
| CPU后端和native颜色不一致 | 同一RenderSpec；CPU只布局与copy | 不同硬件舍入做声明过的误差验收 |
| 强机自动只测native | CI/harness强制CpuBridge | Windows/Mac完整系统路径不能略过 |
| Mac误把CPU=无IOSurface | CPU填入合法PixelBuffer继续CMIO | sink缺失/权限失败不会被CPU绕过 |
| ring复用旧漏洞/旧ABI | 复用安全算法与测试，重建新版layout和授权 | 不以sequence校验替代安全同步 |
| 回退期间格式/PTS混乱 | OutputPlan事务、backend generation、独立SampleClock | 系统不允许原地切换时需合法restart |
| GPU不足导致CPU无限试探 | 源device lost先恢复，CPU只保占位 | 不是无GPU通用软件摄像头 |
| 录像缺口被latest语义吞掉 | 独立有序队列与manifest | 不等同手机本地母版补传 |

不承诺全系统零copy、任意第三方软件可用或整机固定倍数。CpuBridge是正式支持后端，仍以清楚/连续/跟手与同配置表现验收，不被降格为“能亮就算完成”。


## 25. 完成定义

新版完成需要同时满足：

**跨平台主链：**Android/iOS发送、Windows/macOS接收具备AVC/HEVC正式原生路径，原生帧有安全寿命与时间身份；预览不经CPU整图/静态图片视频资源。

**两种摄像头输出：**Windows与macOS都有经过真实管线验证的GpuNative和CpuBridge。自动选择只影响对应sink，保持协议、输出规格、SampleClock、授权与隐私；CPU不接旧ABI，不要求所有平台使用同一种IPC。

**独立录像：**EncodedRecorder保存收到的原码流，RenderedRecorder用GPU和硬件编码；两者不依赖preview cadence或CPU摄像头缓存。视频轨范围、缺口与文件状态真实。

**可用性和性能：**同尺寸与大画面对齐目标通过；CPU/native摄像头分别通过30/60规格与并用测试；只有活跃需求才持续readback；资源、超期、回退原因可见，不能静默降规格制造通过。

**旧实现退出：**旧PCP/FFI/IPC兼容层、软件codec回退、全局CpuNv12帧与CPU预览退出正式产物；新版CPU输出Exporter、copy、IPC、安全测试明确保留。权限/错误恢复不因破坏性修改而删除。

推荐主线：**双codec和原生帧合同 → 原生预览与输出计划 → GPU/CPU两种摄像头路径 → 独立录像 → 质量、60fps和全平台验收。**

背景调研仍可用于理解采集、编码、传输、解码、虚拟摄像头五层组合边界。本文没有沿用其成熟度星级、简化工作量或“旧帧永远不等”；本方案的期限与恢复契约以第9—10节为准。


## 26. 参考依据

R和U为上一轮检查固定提交及依赖时保留的依据，本次未重新核查主干；P为官方平台资料。本次重新查阅P01、P06、P15，并新增/查阅P25—P28；D01为用户提供的原始方案，以核对CPU输出相关API。其余平台说明沿用原文引用，实施仍需按锁定SDK与依赖验证。所有需求、接口、预算、目录、系统基线均为设计，不是API文档保证的性能或已验证兼容列表。

### 26.1 固定仓库与依赖
- [R01] 工作区与 GPUI Kit pin。
- [R02] 运行时、媒体所有权与性能边界。
- [R03] Decoder 公共接口。
- [R04] VideoFrame 与 LatestFrameStore。
- [R05] 桌面 VideoSurface。
- [R06] 预览准备与 cadence。
- [R07] Receiver 解码结果发布。
- [R08] Windows Media Foundation Decoder。
- [R09] GPUI 预览轮询入口。
- [R10] Android OES/EGL compositor。
- [R11] Android MediaCodec 编码器。
- [R12] iOS VideoEncoderPipeline。
- [R13] 码率与分辨率控制。
- [R14] QUIC 后端缓冲配置。
- [R15] 完整 AU 准入判断。
- [R16] 实际依赖锁。
- [R17] GPUI Kit 锁定版本的 Cargo.toml。
- [R18] Decoder crate 依赖边界。
- [R19] macOS VideoToolbox Decoder。
- [R20] Jitter 时序与容量。
- [R21] Windows VCam 格式与帧率。
- [R22] Android Camera2 session 与 preview targets。
- [R23] PCP 控制协议。
- [R24] Windows 虚拟摄像头 MediaSource / SetD3DManager。
- [R25] Windows 虚拟摄像头 sample 创建与 CPU copy。
- [R26] macOS 虚拟摄像头 PixelBuffer 和 Shared Ring 交接。
- [R27] 上一轮核对的代码基线提交。

### 26.2 上游与平台官方资料

- [U01] 对应上游快照：Surface element。
- [U02] 对应上游快照：Windows DirectX Renderer。
- [U03] 对应上游快照：Apple Metal Renderer。
- [U04] 发布元数据：gpui-pre-http-client 0.3.3 / zed@5b055fa。
- [P01] Microsoft：Frame Server custom media source / GPU surface 与系统合同。
- [P02] Microsoft：H.265/HEVC Decoder、输入输出及硬件协作。
- [P03] Android：MediaFormat / KEY_LATENCY 与实际配置。
- [P04] Apple WWDC21：H.264 低延迟视频编码。
- [P05] IETF RFC 7798：HEVC NAL、IDR/CRA、RASL/RADL 语义；引用其编码语义，不采用 RTP 传输。
- [P06] Apple WWDC22：Camera Extension 与 sink/source 数据交接。
- [P07] Microsoft：系统分配器 SetDefaultAllocator。
- [P08] Microsoft：D3D11 视频解码、device manager 与资源。
- [P09] Microsoft：DXGI buffer subresource index。
- [P10] Apple：CVMetalTextureCacheCreateTextureFromImage / 图像纹理生命周期。
- [P11] Microsoft：AcquireSync 返回值与同步。
- [P12] Microsoft：共享资源的 adapter LUID。
- [P13] Apple：压缩 sample 不重编码写入。
- [P14] Microsoft：允许硬件转换，不等于硬件必需。
- [P15] Microsoft：硬件必需属性、前置条件与 Windows 11 25H2 门槛。
- [P16] Microsoft：MP4 sample description、AVC 输入与其他格式约束。
- [P17] Microsoft：fragmented MPEG-4 media sink。
- [P18] Apple：movieFragmentInterval。
- [P19] Quinn 官方 Rust API：Datagram 发送、等待与缓冲语义；实施时核对锁定版本。
- [P20] Microsoft：虚拟摄像头创建、访问权限与生命周期。
- [P21] Apple：CMIOExtensionClient.signingID。
- [P22] Apple：CMIOExtensionClient.pid。
- [P23] Apple：EnableLowLatencyRateControl。
- [P24] Android：SENSOR_TIMESTAMP 与来源时钟。

- [P25] Microsoft：D3D11 Map，非阻塞flag和WAS_STILL_DRAWING返回值。
- [P26] Microsoft：IMF2DBuffer2 Lock2DSize，实际pitch/访问范围/锁接口回退顺序。
- [P27] Apple：CPU访问PixelBuffer必须锁定；单纯GPU访问无需CPU锁。
- [P28] Microsoft：CopySubresourceRegion为异步GPU复制，不执行任意缩放和颜色转换。
- [D01] 本次修订所依据的上一版完整方案（用户附件，未重新审查仓库）。

[P01]: https://learn.microsoft.com/en-us/windows-hardware/drivers/stream/frame-server-custom-media-source "Microsoft：Frame Server custom media source / GPU surface 与系统合同"
[P02]: https://learn.microsoft.com/en-us/windows/win32/medfound/h-265---hevc-video-decoder "Microsoft：H.265/HEVC Decoder、输入输出及硬件协作"
[P03]: https://developer.android.com/reference/android/media/MediaFormat "Android：MediaFormat / KEY_LATENCY 与实际配置"
[P04]: https://developer.apple.com/videos/play/wwdc2021/10158/ "Apple WWDC21：H.264 低延迟视频编码"
[P05]: https://www.rfc-editor.org/rfc/rfc7798.html "IETF RFC 7798：HEVC NAL、IDR/CRA、RASL/RADL 语义；引用其编码语义，不采用 RTP 传输"
[P06]: https://developer.apple.com/videos/play/wwdc2022/10022/ "Apple WWDC22：Camera Extension 与 sink/source 数据交接"
[P07]: https://learn.microsoft.com/en-us/windows/win32/api/mfidl/nf-mfidl-imfsampleallocatorcontrol-setdefaultallocator "Microsoft：系统分配器 SetDefaultAllocator"
[P08]: https://learn.microsoft.com/en-us/windows/win32/medfound/supporting-direct3d-11-video-decoding-in-media-foundation "Microsoft：D3D11 视频解码、device manager 与资源"
[P09]: https://learn.microsoft.com/en-us/windows/win32/api/mfobjects/nf-mfobjects-imfdxgibuffer-getsubresourceindex "Microsoft：DXGI buffer subresource index"
[P10]: https://developer.apple.com/documentation/corevideo/cvmetaltexturecachecreatetexturefromimage%28_%3A_%3A_%3A_%3A_%3A_%3A_%3A_%3A_%3A%29 "Apple：CVMetalTextureCacheCreateTextureFromImage / 图像纹理生命周期"
[P11]: https://learn.microsoft.com/en-us/windows/win32/api/dxgi/nf-dxgi-idxgikeyedmutex-acquiresync "Microsoft：AcquireSync 返回值与同步"
[P12]: https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_2/nf-dxgi1_2-idxgifactory2-getsharedresourceadapterluid "Microsoft：共享资源的 adapter LUID"
[P13]: https://developer.apple.com/documentation/avfoundation/avassetwriterinput/outputsettings "Apple：压缩 sample 不重编码写入"
[P14]: https://learn.microsoft.com/en-us/windows/win32/medfound/mf-readwrite-enable-hardware-transforms "Microsoft：允许硬件转换，不等于硬件必需"
[P15]: https://learn.microsoft.com/en-us/windows/win32/medfound/mf-readwrite-use-only-hardware-transforms "Microsoft：硬件必需属性、前置条件与 Windows 11 25H2 门槛"
[P16]: https://learn.microsoft.com/en-us/windows/win32/medfound/mpeg-4-file-sink "Microsoft：MP4 sample description、AVC 输入与其他格式约束"
[P17]: https://learn.microsoft.com/en-us/windows/win32/api/mfidl/nf-mfidl-mfcreatefmpeg4mediasink "Microsoft：fragmented MPEG-4 media sink"
[P18]: https://developer.apple.com/documentation/avfoundation/avassetwriter/moviefragmentinterval "Apple：movieFragmentInterval"
[P19]: https://docs.rs/quinn/latest/quinn/struct.Connection.html "Quinn 官方 Rust API：Datagram 发送、等待与缓冲语义；实施时核对锁定版本"
[P20]: https://learn.microsoft.com/en-us/windows/win32/api/mfvirtualcamera/nf-mfvirtualcamera-mfcreatevirtualcamera "Microsoft：虚拟摄像头创建、访问权限与生命周期"
[P21]: https://developer.apple.com/documentation/coremediaio/cmioextensionclient/signingid "Apple：CMIOExtensionClient.signingID"
[P22]: https://developer.apple.com/documentation/coremediaio/cmioextensionclient/pid "Apple：CMIOExtensionClient.pid"
[P23]: https://developer.apple.com/documentation/videotoolbox/kvtvideoencoderspecification_enablelowlatencyratecontrol "Apple：EnableLowLatencyRateControl"
[P24]: https://developer.android.com/reference/android/hardware/camera2/CaptureResult "Android：SENSOR_TIMESTAMP 与来源时钟"
[R01]: https://github.com/Haoxincode/picoo-camera/blob/6a214b34df619f407faf3b1fa7760a3123e4049e/Cargo.toml "工作区与 GPUI Kit pin"
[R02]: https://github.com/Haoxincode/picoo-camera/blob/6a214b34df619f407faf3b1fa7760a3123e4049e/docs/design-specs/architecture/0011-runtime-state-and-performance-boundary.md "运行时、媒体所有权与性能边界"
[R03]: https://github.com/Haoxincode/picoo-camera/blob/6a214b34df619f407faf3b1fa7760a3123e4049e/crates/picoo-media-decode/src/lib.rs "Decoder 公共接口"
[R04]: https://github.com/Haoxincode/picoo-camera/blob/6a214b34df619f407faf3b1fa7760a3123e4049e/crates/picoo-frame-hub/src/latest_frame_store.rs "VideoFrame 与 LatestFrameStore"
[R05]: https://github.com/Haoxincode/picoo-camera/blob/6a214b34df619f407faf3b1fa7760a3123e4049e/apps/desktop/src/video_surface.rs "桌面 VideoSurface"
[R06]: https://github.com/Haoxincode/picoo-camera/blob/6a214b34df619f407faf3b1fa7760a3123e4049e/apps/desktop/src/preview_pipeline.rs "预览准备与 cadence"
[R07]: https://github.com/Haoxincode/picoo-camera/blob/6a214b34df619f407faf3b1fa7760a3123e4049e/crates/picoo-receiver/src/session/media_publish.rs "Receiver 解码结果发布"
[R08]: https://github.com/Haoxincode/picoo-camera/blob/6a214b34df619f407faf3b1fa7760a3123e4049e/crates/picoo-media-decode/src/mf.rs "Windows Media Foundation Decoder"
[R09]: https://github.com/Haoxincode/picoo-camera/blob/6a214b34df619f407faf3b1fa7760a3123e4049e/apps/desktop/src/gpui/lifecycle.rs "GPUI 预览轮询入口"
[R10]: https://github.com/Haoxincode/picoo-camera/blob/6a214b34df619f407faf3b1fa7760a3123e4049e/apps/android/app/src/main/kotlin/com/picoo/camera/media/CameraEncodingCompositor.kt "Android OES/EGL compositor"
[R11]: https://github.com/Haoxincode/picoo-camera/blob/6a214b34df619f407faf3b1fa7760a3123e4049e/apps/android/app/src/main/kotlin/com/picoo/camera/media/MediaCodecH264Encoder.kt "Android MediaCodec 编码器"
[R12]: https://github.com/Haoxincode/picoo-camera/blob/6a214b34df619f407faf3b1fa7760a3123e4049e/apps/ios/PicooCamera/VideoEncoderPipeline.swift "iOS VideoEncoderPipeline"
[R13]: https://github.com/Haoxincode/picoo-camera/blob/6a214b34df619f407faf3b1fa7760a3123e4049e/crates/picoo-rate-control/src/lib.rs "码率与分辨率控制"
[R14]: https://github.com/Haoxincode/picoo-camera/blob/6a214b34df619f407faf3b1fa7760a3123e4049e/crates/picoo-transport/src/quinn_backend.rs "QUIC 后端缓冲配置"
[R15]: https://github.com/Haoxincode/picoo-camera/blob/6a214b34df619f407faf3b1fa7760a3123e4049e/crates/picoo-transport/src/quinn_backend/stats.rs "完整 AU 准入判断"
[R16]: https://github.com/Haoxincode/picoo-camera/blob/6a214b34df619f407faf3b1fa7760a3123e4049e/Cargo.lock "实际依赖锁"
[R17]: https://github.com/longbridge/gpui-kit/blob/39c9b7b0fb0bdf3c4d07e26d7fdf74381474646e/Cargo.toml "GPUI Kit 锁定版本的 Cargo.toml"
[R18]: https://github.com/Haoxincode/picoo-camera/blob/6a214b34df619f407faf3b1fa7760a3123e4049e/crates/picoo-media-decode/Cargo.toml "Decoder crate 依赖边界"
[R19]: https://github.com/Haoxincode/picoo-camera/blob/6a214b34df619f407faf3b1fa7760a3123e4049e/crates/picoo-media-decode/src/videotoolbox.rs "macOS VideoToolbox Decoder"
[R20]: https://github.com/Haoxincode/picoo-camera/blob/6a214b34df619f407faf3b1fa7760a3123e4049e/crates/picoo-jitter/src/lib.rs "Jitter 时序与容量"
[R21]: https://github.com/Haoxincode/picoo-camera/blob/6a214b34df619f407faf3b1fa7760a3123e4049e/extensions/windows-virtual-camera/mf-source/src/format.rs "Windows VCam 格式与帧率"
[R22]: https://github.com/Haoxincode/picoo-camera/blob/6a214b34df619f407faf3b1fa7760a3123e4049e/apps/android/app/src/main/kotlin/com/picoo/camera/media/Camera2DeviceSession.kt "Android Camera2 session 与 preview targets"
[R23]: https://github.com/Haoxincode/picoo-camera/blob/6a214b34df619f407faf3b1fa7760a3123e4049e/proto/picoo_camera.proto "PCP 控制协议"
[R24]: https://github.com/Haoxincode/picoo-camera/blob/6a214b34df619f407faf3b1fa7760a3123e4049e/extensions/windows-virtual-camera/mf-source/src/windows_source/media_source.rs "Windows 虚拟摄像头 MediaSource / SetD3DManager"
[R25]: https://github.com/Haoxincode/picoo-camera/blob/6a214b34df619f407faf3b1fa7760a3123e4049e/extensions/windows-virtual-camera/mf-source/src/windows_source/media_stream.rs "Windows 虚拟摄像头 sample 创建与 CPU copy"
[R26]: https://github.com/Haoxincode/picoo-camera/blob/6a214b34df619f407faf3b1fa7760a3123e4049e/extensions/macos-camera-extension/PicooCameraProvider.swift "macOS 虚拟摄像头 PixelBuffer 和 Shared Ring 交接"
[R27]: https://github.com/Haoxincode/picoo-camera/commit/6a214b34df619f407faf3b1fa7760a3123e4049e "上一轮核对的代码基线提交"
[U01]: https://github.com/zed-industries/zed/blob/5b055fa/crates/gpui/src/elements/surface.rs "对应上游快照：Surface element"
[U02]: https://github.com/zed-industries/zed/blob/5b055fa/crates/gpui_windows/src/directx_renderer.rs "对应上游快照：Windows DirectX Renderer"
[U03]: https://github.com/zed-industries/zed/blob/5b055fa/crates/gpui_apple/src/metal_renderer.rs "对应上游快照：Apple Metal Renderer"
[U04]: https://docs.rs/crate/gpui-pre-http-client/0.3.3 "发布元数据：gpui-pre-http-client 0.3.3 / zed@5b055fa"
[P25]: https://learn.microsoft.com/en-us/windows/win32/api/d3d11/nf-d3d11-id3d11devicecontext-map "D3D11 Map / DO_NOT_WAIT"
[P26]: https://learn.microsoft.com/en-us/windows/win32/api/mfobjects/nf-mfobjects-imf2dbuffer2-lock2dsize "MF2DBuffer2 / pitch and buffer bounds"
[P27]: https://developer.apple.com/documentation/corevideo/cvpixelbufferunlockbaseaddress%28_%3A_%3A%29 "PixelBuffer CPU lock / GPU access distinction"
[P28]: https://learn.microsoft.com/en-us/windows/win32/api/d3d11/nf-d3d11-id3d11devicecontext-copysubresourceregion "GPU copy semantics"
[D01]: #26-参考依据 "上一版用户附件（未提供）"

