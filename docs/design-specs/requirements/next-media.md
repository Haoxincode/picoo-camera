# Next 原生媒体与多输出需求

来源：[产品方案](../../product/picoo-camera-next-v2-gpu-cpu-output-2026-09-06.md) §3；架构：ARCH-PICOO-MEDIA-002。提案的 NEXT-REQ 编号只作为来源别名，稳定 ID 使用 REQ-PICOO-NEXT，不覆盖旧 ID。所有 40 项目前为 planned，旧平台测试不能作为新版验收证据。

| ID | 状态 | 提案来源 | 契约 | 验收 |
| --- | --- | --- | --- | --- |
| REQ-PICOO-NEXT-001 | planned | NEXT-REQ-001 | Android/iOS→Windows/macOS 四种组合完整实现 | 四组合发布矩阵，不能互相代替 |
| REQ-PICOO-NEXT-002 | planned | NEXT-REQ-002 | 不新增用途性能模式；默认配置无需反复调参 | UI 及用户流程测试 |
| REQ-PICOO-NEXT-003 | planned | NEXT-REQ-003 | H.264 High/Constrained High 与 HEVC Main，均支持 8-bit 4:2:0 SDR | 两种真实码流、参数集与输出检查 |
| REQ-PICOO-NEXT-004 | planned | NEXT-REQ-004 | 1080p60 是完整功能目标；同时提供 1080p30、720p60、720p30 正式配置 | 每个 codec、配置组合的真机测试 |
| REQ-PICOO-NEXT-005 | planned | NEXT-REQ-005 | 默认选择通过准入的 1080p60 原生配置；不在运行中静默改帧率/分辨率/codec | 配置事务、异常日志和 UI 实际配置 |
| REQ-PICOO-NEXT-006 | planned | NEXT-REQ-006 | 同尺寸对比时保留手机端细节、颜色和构图，不以降画质/减帧换取指标 | 同源、同视场、同物理显示尺寸人工对照 |
| REQ-PICOO-NEXT-007 | planned | NEXT-REQ-007 | 大窗口和全屏不改变源配置，视频连贯且跟手 | DPI/resize/fullscreen 回归 |
| REQ-PICOO-NEXT-008 | planned | NEXT-REQ-008 | 预览、虚拟摄像头、一个处理后录像任务可并用；同时允许一个原码流录像，不承诺多路并发重编码 | 同时工作与热稳态测试 |
| REQ-PICOO-NEXT-009 | planned | NEXT-REQ-009 | 生产主链路硬件编解码、原生帧、GPU 预览/处理后录像；整图 CPU 访问只允许在显式 CpuBridge 或诊断 | 按 sink/backend 归因的 map/readback 计数，生产工厂无软件 codec |
| REQ-PICOO-NEXT-010 | planned | NEXT-REQ-010 | Intel/AMD/NVIDIA 显卡走统一平台接口；不要求核显 | 包含 AMD 独显且无核显、单核显、双 GPU 的矩阵 |
| REQ-PICOO-NEXT-011 | planned | NEXT-REQ-011 | 每个 AU 在同一 Decoder generation 最多提交一次；输出携带原始 token | 延迟/多输出/reset 回归 |
| REQ-PICOO-NEXT-012 | planned | NEXT-REQ-012 | 预览 latest-only，窗口隐藏停止预览工作，不停止其他输出 | 非可见时预览提交计数为零 |
| REQ-PICOO-NEXT-013 | planned | NEXT-REQ-013 | Windows/macOS VCam 都实现 GPU 原生与 CPU 桥接；两者提供相同协商内容和时间语义 | 强制两后端的 capture harness 及实际客户端验证 |
| REQ-PICOO-NEXT-014 | planned | NEXT-REQ-014 | VCam 按协商格式与独立时钟输出；支持 30/60fps，不把重复 sample 算新画面 | sample PTS/duration 与源帧 ID 分别检查 |
| REQ-PICOO-NEXT-015 | planned | NEXT-REQ-015 | 任意慢消费者不能阻塞其他输出；无无界 surface 或任务队列 | 饱和/悬挂消费者/显存压力测试 |
| REQ-PICOO-NEXT-016 | planned | NEXT-REQ-016 | CPU 引用释放与 GPU 完成分离；未完成 GPU 工作不能复用表面 | 平台调试层、lease 状态机测试 |
| REQ-PICOO-NEXT-017 | planned | NEXT-REQ-017 | GPU 丢失/重建推进资源代际，旧帧不能进入新资源 | device-lost、睡眠恢复、adapter 改变测试 |
| REQ-PICOO-NEXT-018 | planned | NEXT-REQ-018 | 原码流录像保存接收到的 AVC/HEVC，不重编码，不从预览取帧 | AU 归一化摘要对照；两种文件回放 |
| REQ-PICOO-NEXT-019 | planned | NEXT-REQ-019 | 处理后录像由 GPU 图像和硬件编码器生成，目标尺寸独立于窗口 | 隐藏/缩小预览期间录像内容与尺寸不变 |
| REQ-PICOO-NEXT-020 | planned | NEXT-REQ-020 | 录像使用独立有界队列与源时间线；丢帧/写入失败必须明确 | 慢磁盘、缺 AU、编码器拒绝、低存储测试 |
| REQ-PICOO-NEXT-021 | planned | NEXT-REQ-021 | 录制结果区分完整、含缺口和失败；普通成功不能掩盖中断 | manifest 与 UI 完成状态一致 |
| REQ-PICOO-NEXT-022 | planned | NEXT-REQ-022 | codec、分辨率、配置变化时安全分段，拒绝把不兼容 sample description 硬塞同段 | 切配置后每段可独立解码 |
| REQ-PICOO-NEXT-023 | planned | NEXT-REQ-023 | 重组与恢复 codec-aware；IDR/CRA 不能混同 | AVC/HEVC 各类随机访问样本与缺片注入 |
| REQ-PICOO-NEXT-024 | planned | NEXT-REQ-024 | 只有摄像头输出边界可自动选择 CpuBridge；硬件 codec 或 GPU 主链路不足明确失败 | 能力组合故障注入、路由原因和依赖检查 |
| REQ-PICOO-NEXT-025 | planned | NEXT-REQ-025 | 网络、命令、媒体、输出全部有容量和时限；控制命令有明确结果 | 过载状态机与预算断言 |
| REQ-PICOO-NEXT-026 | planned | NEXT-REQ-026 | 旧协议、旧 IPC、旧配置、旧 FFI 不被新产品接受 | 非法契约输入拒绝测试 |
| REQ-PICOO-NEXT-027 | planned | NEXT-REQ-027 | 配对认证、加密、资源权限、输入边界、隐私行为继续有效 | 未授权访问与恶意输入测试 |
| REQ-PICOO-NEXT-028 | planned | NEXT-REQ-028 | 度量自动采集并区分请求值/实际值、提交/呈现、新帧/重复帧 | 结构化诊断完整性检查 |
| REQ-PICOO-NEXT-029 | planned | NEXT-REQ-029 | CPU 整图仅由 Output Exporter 产生；不进入 Decoder API 或 Preview | 构建依赖审查、preview-only 导出数为零 |
| REQ-PICOO-NEXT-030 | planned | NEXT-REQ-030 | 各 VCam sink 独立选择 GpuNative/CpuBridge，默认自动，不增加用户模式 | 同时两 sink 不同后端；原因可见 |
| REQ-PICOO-NEXT-031 | planned | NEXT-REQ-031 | CPU 路径不降低既定分辨率、帧率、色彩或重复执行镜像 | 同源同 RenderSpec 对照、两后端格式一致 |
| REQ-PICOO-NEXT-032 | planned | NEXT-REQ-032 | 无 CPU 输出需求时无持续 readback；重复源帧不反复导出 | demand/unique-source/export 计数与缓存测试 |
| REQ-PICOO-NEXT-033 | planned | NEXT-REQ-033 | readback、PixelBuffer 锁定、CPU copy 不在 UI/Receiver owner/实时 RequestSample 回调等待 | 执行器约束、延迟注入 |
| REQ-PICOO-NEXT-034 | planned | NEXT-REQ-034 | CPU staging、工作帧、IPC、系统 sample 均有资源上限和明确寿命 | 槽位占满、慢消费、内存高水位测试 |
| REQ-PICOO-NEXT-035 | planned | NEXT-REQ-035 | 相同输出内容可共享一次 CPU 物化，不同规格独立准备；低频 sink 不增加高频无用导出 | 一源两消费同/异规格、source60→output30 |
| REQ-PICOO-NEXT-036 | planned | NEXT-REQ-036 | 后端切换为事务，推进 backend generation；配置和 SampleClock 连续，旧完成不可提交 | 切换中 stop/格式改变/旧任务完成 |
| REQ-PICOO-NEXT-037 | planned | NEXT-REQ-037 | 跨 GPU 原生交接不可用时可用 CPU bridge，不为新增 sink 重置正在录像的源 device | 双 adapter GPU→CPU→合法目标 sample |
| REQ-PICOO-NEXT-038 | planned | NEXT-REQ-038 | CPU IPC 使用当前无版本 ABI、最小权限和崩溃可恢复 lease；隐私时限适用所有缓存 | 非法布局/未授权/崩溃/断开占位测试 |
| REQ-PICOO-NEXT-039 | planned | NEXT-REQ-039 | CPU 输出按所选30/60fps规格验收，并验证其对 GPU 预览与录像的影响 | 同配置 A/B、p95/p99、热稳态并用 |
| REQ-PICOO-NEXT-040 | planned | NEXT-REQ-040 | CPU bridge 不能绕过身份/权限失败，也不能冒充硬件 codec 或 GPU 故障的修复 | 拒绝后无回退访问；缺 decoder 仍失败 |

## 位流边界分解

| ID | 状态 | 来源 | 契约 | 验收 |
| --- | --- | --- | --- | --- |
| REQ-PICOO-BITSTREAM-001 | verified | ARCH-PICOO-MEDIA-002 / REQ-PICOO-NEXT-003、023 | AVC 位流 helper 从 packet 移入独立 bitstream crate，所有调用方直接依赖；无 packet 重导出、无网络/GPU/软件 codec 依赖 | 原参数集、IDR 与格式转换测试；packet、Sender、Receiver、Decoder、FFI 回归；不代表 HEVC 实现 |

位流拆分验证结果见 [本次实施记录](../../development/next-media-refactor.md)：本机相关 207 个测试通过、2 个忽略；verified 仅限本条模块边界与既有行为，不覆盖 Next 双 codec 或四平台产品验收。

| ID | 状态 | 来源 | 契约 | 验收 |
| --- | --- | --- | --- | --- |
| REQ-PICOO-BITSTREAM-002 | implemented | ARCH-PICOO-MEDIA-002 / REQ-PICOO-NEXT-003、023、025 | 双 codec 明确 NAL 格式、有界配置解释与保守 RAP 分类；区分 HEVC IDR/CRA/RASL/RADL，拒绝不支持结构 | 硬件生成 AVC/HEVC fixtures、截断/溢出/多图像/恶意数量回归；跨平台编译；原生解码合同另验 |

| REQ-PICOO-BITSTREAM-003 | implemented | ARCH-PICOO-MEDIA-002 / REQ-PICOO-NEXT-003、025 | 原生 AVC raw SPS/PPS 通过标准 avcC builder 进入有界配置记录；record profile/compatibility/level 必须与全部 SPS 头一致，禁止缺参数时猜测 | 实际硬件参数集 roundtrip、头冲突/长度边界、fuzz；不等同完整 SPS 几何/色彩/no-B-frame 准入 |

| REQ-PICOO-BITSTREAM-004 | implemented | ARCH-PICOO-MEDIA-002 / REQ-PICOO-NEXT-003、025 | 有界提取 AVC 编码尺寸、可见 crop、PAR 和 VUI 色彩事实；未知值保持未知，拒绝溢出和不支持的像素结构 | 原生 1080p 的 1088 编码高度回归、恶意尺寸/crop/截断/上限；不替代完整 SPS/HRD/no-B-frame 或平台准入 |

## GPU 边界分解

| ID | 状态 | 来源 | 契约 | 验收 |
| --- | --- | --- | --- | --- |
| REQ-PICOO-GPU-001 | implemented | ARCH-PICOO-MEDIA-002 / REQ-PICOO-NEXT-009、016、029 | Apple 原生 NV12 源经显式 Metal/Core Image 转为独立目标图像；源/目标色彩明确，旋转后镜像，contain 不变形；固定布局三槽池，完成 GPU 写入后交付不可变输出 | M4 实际 GPU 色彩/灰阶/八种方向组合、黑边、池耗尽与保留引用、跨线程寿命；不代表 Decoder/Preview 接入或跨输出总预算完成 |
| REQ-PICOO-GPU-002 | implemented | ARCH-PICOO-MEDIA-002 / REQ-PICOO-NEXT-029、033、034 | Apple CPU exporter 只物化匹配 RenderSpec 的 GPU 完成输出，逐 plane 复制可见行并移除 padding；固定三槽，慢消费者不能触发扩池；无调用不 readback | 实际 GPU→CPU 像素比对、池耗尽/clone 寿命、错误输出拒绝；执行器接入、唯一源去重及完整 CpuBridge 验收另验 |
| REQ-PICOO-GPU-005 | implemented | ARCH-PICOO-MEDIA-002 / REQ-PICOO-NEXT-016、025、034 | Windows Flush1 完成事件保留命令 owner，取消/失败/panic 不提前释放；device removed 只报告失败；每 context 三个、全进程十二个完成对象，未取结果仍占容量；MF 输出在专用 worker 等待完成 | 真实 threadpool 取消、容量、panic、跨 context 上限与 GPU copy 回归；Windows 原生 CI 执行，MFT 硬件端到端与图像字节预算另验 |
| REQ-PICOO-GPU-006 | implemented | ARCH-PICOO-MEDIA-002 / REQ-PICOO-NEXT-009、016、029、031 | Windows 原生 Video Processor 按独立 format/color 输出 NV12 BT.709 limited 或 BGRA8 RGB full G22/BT.709；检查驱动颜色/旋转/镜像与无历史帧依赖能力；crop 按 rotation→mirror→clip 变换；输出池三槽且完成事件保留源/目标 | 八种几何组合、1088 补齐区裁剪、真实 D3D11 输出池保留与扩池拒绝；Windows 原生 CI、真实显卡像素对照；主链路与 Exporter/Preview 接入另验 |
| REQ-PICOO-GPU-007 | implemented | ARCH-PICOO-MEDIA-002 / REQ-PICOO-NEXT-029、031、033、034 | Windows 仅导出匹配 spec/device 的 GPU 完成目标；单 staging 加共享三槽 CPU 池，先占槽再 readback；完成后 DO_NOT_WAIT 映射并去行 padding，CPU copy 不持有 context 锁 | 原生 NV12 上传/复制/导出行布局、三槽/Weak/复用、规格与 device 拒绝；Mac 既有 GPU/导出回归；实际 VCam 接线及性能另验 |
| REQ-PICOO-GPU-008 | implemented | ARCH-PICOO-MEDIA-002 / REQ-PICOO-NEXT-016、017、031 | BGRA 目标池提供 NT shared/keyed-mutex 交接；同 adapter 导入、exact S_OK 准入、GPU 完成后释放 key 0；Consumer 持有原图像到读取/锁释放，handle/COM 引用不替代池 lease | Windows 两 device 的实际 BGRA clear→共享导入→GPU copy→诊断 readback、写锁期间超时拒绝、持有时三槽满与完成后复用；新测试待 CI；GPUI draw 与硬件显卡验收另验 |

## 原生帧分发分解

| ID | 状态 | 来源 | 契约 | 验收 |
| --- | --- | --- | --- | --- |
| REQ-PICOO-FRAME-012 | implemented | ARCH-PICOO-MEDIA-002 / REQ-PICOO-NEXT-011、012、015、020 | 原生帧身份与配置描述不可变，无 CPU 像素或 stride；latest 与未来发布的有序订阅分开；唯一录像订阅八帧上限，取出时拒绝超过 150ms 的工作，终止原因显式 | 共享原对象、身份快照、订阅顺序/容量/年龄、取消/清空/停止、几何边界；Decoder/输出接线和隐私期限对持有引用的回收另验 |

## Mac 原生主链路分解

| ID | 状态 | 来源 | 契约 | 验收 |
| --- | --- | --- | --- | --- |
| REQ-PICOO-MEDIA-030 | implemented | ARCH-PICOO-MEDIA-002 / REQ-PICOO-NEXT-009、011、016、029 | Mac Decoder 直接交付已完成原生 NV12；FrameBus 持有原始身份、编码尺寸和剩余可见区域；预览 GPU 处理，CPU 输出仅在独立 sink 工作者物化；placeholder 不作为原生源 | 实际 VideoToolbox 颜色/几何拒绝、1088/1080 元数据、Receiver 原生快照、GPU→CPU 像素、占位输出；完整配置 wire、sink demand、SampleClock 与全局预算另验 |
| REQ-PICOO-GPU-003 | implemented | ARCH-PICOO-MEDIA-002 / REQ-PICOO-NEXT-016 | GPUI surface 的 CVPixelBuffer 与 CVMetalTexture 由 Metal 完成回调持有至读取结束，Scene 释放不能提前让池复用 allocation | M4 实际阻塞 Metal 队列：读取未完成时单槽池拒绝分配，完成后复用；移除修补的负向版本必须失败 |

| REQ-PICOO-FRAME-015 | implemented | ARCH-PICOO-MEDIA-002 / REQ-PICOO-NEXT-032、035 | 每个 CPU ring 的实际读取推进有界请求序号；Owner 每次新准备消费一个最新请求，多次请求合并不积压，租期内无新请求也不连续导出；同源仍去重 | 一次请求后多个新源只物化一次；新请求取最新保留源；C/Rust 请求序号一致与耗尽不回绕；多 sink 不同规格协调另验 |

| REQ-PICOO-FRAME-016 | implemented | ARCH-PICOO-MEDIA-002 / REQ-PICOO-NEXT-009、016、029 | Windows 原生源 owner 保留已完成 MF sample、NV12 D3D11 texture 与合法 subresource；安全 API 无 CPU 像素或可变平台对象，保留 sample 防止 allocator 提前复用 | Windows 原生资源/COM 保留回归，非法存储及不同 device 身份拒绝；GPU fence、MFT 工厂和预览接入另验 |

| REQ-PICOO-GPU-004 | implemented | ARCH-PICOO-MEDIA-002 / REQ-PICOO-NEXT-009、010、017、024 | Windows GPU context 固定硬件 D3D11 device 并开启原生保护；输出采用原生图像现有 device，不重建源设备，不创建 MF manager；创建/采用入口均拒绝 software/SINGLETHREADED，MF manager 归 Decoder | Windows SDK 类型检查、WARP/SINGLETHREADED 拒绝、native device identity 与 panic 后锁释放；Decoder manager 身份回归；真实硬件及 adapter 矩阵另验 |

| ID | 状态 | 来源 | 契约 | 验收 |
| --- | --- | --- | --- | --- |
| REQ-PICOO-BITSTREAM-005 | implemented | ARCH-PICOO-MEDIA-002 / REQ-PICOO-NEXT-003、022、027 | bitstream 按已提交配置的 codec 逐个校验 AU 中 VPS/SPS/PPS 的字节身份；孤立参数更新同样不能覆盖配置，拒绝不修改已提交记录 | 硬件 AVC/HEVC fixture 的匹配、逐参数冲突、跨 codec 拒绝及 AVC Decoder 既有配置回归；不替代完整 slice/SPS 准入与 HEVC 原生解码 |

| ID | 状态 | 来源 | 契约 | 验收 |
| --- | --- | --- | --- | --- |
| REQ-PICOO-BITSTREAM-006 | implemented | ARCH-PICOO-MEDIA-002 / REQ-PICOO-NEXT-003、025 | AVC/HEVC 共享 VideoSpsFacts/VideoColorFacts 源描述；HEVC 复用有界 Scuffle SPS 解析，准入单层 progressive Main 8-bit 4:2:0 与零重排，保留 coded/visible/PAR/色彩/chroma 事实；缺失不猜测，算术/分配前校验，拒绝尾部垃圾 | 硬件 HEVC fixture、逐字节截断、逐位变异、裁剪/块尺寸/预测 scaling matrix/palette/重排异常回归；fuzz 包使用同一补丁；平台 Decoder 与完整配置事务另验 |
