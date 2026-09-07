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
| REQ-PICOO-BITSTREAM-007 | implemented | ARCH-PICOO-MEDIA-002 / REQ-PICOO-NEXT-003、025 | 原生 HEVC Annex B CSD 只包含单组 VPS/SPS/PPS，经源 SPS 准入和 Scuffle hvcC mux 进入统一四字节长度记录；保留 profile/tier/constraint/level，未知帧率和 parallelism 不猜测 | 硬件原生 hvcC 字段对照、缺失/冲突/非参数 NAL/长度边界；移动端正式接口接入另验 |

| ID | 状态 | 来源 | 契约 | 验收 |
| --- | --- | --- | --- | --- |
| REQ-PICOO-MEDIA-035 | implemented | ARCH-PICOO-MEDIA-002 / REQ-PICOO-NEXT-003、025、026 | Sender 配置必须持有已验证 CodecConfiguration，无空参数默认配置、raw SPS/PPS 猜测或独立 codec/profile 标签；原生适配在 Core 状态变更前拒绝无效参数，非法帧率/方向不取整 | HEVC 原生记录序列化、配置事务非法属性拒绝、FFI 缺失/Annex B 冒充 raw/超长输入；完整双 codec 配置协商另验 |
| REQ-PICOO-MEDIA-036 | implemented | ARCH-PICOO-MEDIA-002 / REQ-PICOO-NEXT-003、011、025、026 | Android 每个 AU 在回调时携带原生 generation 的标准配置快照；JNI 接纳显式 codec/fps/record，不拆回 SPS/PPS；删除 UI 独立配置写入，队列预算含配置数据，分配前限制 AU/CSD | 不同 codec/帧率世代排队回归、配置预算、Kotlin/JNI 编译、真实 CSD instrumentation；真机执行和完整 HEVC 配置协商另验 |
| REQ-PICOO-MEDIA-037 | implemented | ARCH-PICOO-MEDIA-002 / REQ-PICOO-NEXT-003、011、025、026 | iOS 每张 AU 携带 CoreMedia 原生 avcC/hvcC atom；删除空配置 prime、独立 C setStreamConfig 和 raw SPS/PPS FFI；原始 AU 不追加参数；新 encoder generation 即使 record 相同也重新确认配置 | device/simulator Core、App 和 Swift/C 测试；原生 format atom codec/record 保留与缺失拒绝、C record 边界；真实 iPhone 链路另验 |
| REQ-PICOO-MEDIA-038 | implemented | ARCH-PICOO-MEDIA-002 / REQ-PICOO-NEXT-023、025 | Apple/Android FFI 使用同一原生 AU 准入：显式 framing 只解析一次，IDR/普通图像必须与原生 keyframe hint 一致；CRA/RASL/RADL 在 Core 配置暂存前拒绝，四字节长度输入保持借用 | 双 codec 实际 IDR、hint 冲突、delta 冒充 keyframe、CRA/leading-picture 拒绝及原 FFI 回归；完整 CRA 恢复另验 |
| REQ-PICOO-MEDIA-039 | implemented | ARCH-PICOO-MEDIA-002 / REQ-PICOO-NEXT-003、025 | CodecConfiguration 拥有共同 SPS 源事实；Sender/Receiver 修改配置身份前核对可见尺寸，Decoder 复用同一解释，编码 padding 不等于显示尺寸 | 双 codec 尺寸冲突拒绝、Receiver 旧快照与 revision 保留、1080 裁剪及原生 Decoder 回归；帧率/色彩/硬件完整准入另验 |
| REQ-PICOO-MEDIA-040 | implemented | ARCH-PICOO-MEDIA-002 / REQ-PICOO-NEXT-004、011 | iOS 采集服务按请求尺寸与 30/60fps 选择同一原生 format，并在同一配置内设置 min/max frame duration；删除固定 30fps 与 preset 二次选择；帧率变化不能命中旧配置缓存 | iOS 原生构建/模拟器回归；相机格式交集、实际 timestamp 帧率和热稳态须真机另验 |
| REQ-PICOO-MEDIA-041 | implemented | ARCH-PICOO-MEDIA-002 / REQ-PICOO-NEXT-011、023、028 | iOS 编码回调由原生输入 ID 关联提交时不可变配置；最多十六个待完成项，重复/未知完成拒绝，取消幂等且身份不复用；新配置关闭旧原生 session，平台丢帧报告恢复 | Swift 乱序完成、方向/码率快照、容量与取消回归及原生构建；真机异步压缩/吞吐另验 |
| REQ-PICOO-MEDIA-042 | implemented | ARCH-PICOO-MEDIA-002 / REQ-PICOO-NEXT-026 | Android JNI AU 提交直接返回 Accepted/Rejected/Error 对象，配置提交与关键帧请求独立表达；删除整数位掩码与 Kotlin fromNative；JNI 构造器由 Keep 保护 | NDK Clippy、完整 APK/JVM 构建、打包 JNI 错误和旧 generation 拒绝 instrumentation；真机成功媒体提交另验 |
| REQ-PICOO-MEDIA-043 | implemented | ARCH-PICOO-MEDIA-002 / REQ-PICOO-NEXT-003、004、026 | Encoder 事务显式请求 codec/正式可见尺寸/fps，匹配 generation 和高度仍不能提交另一 codec/fps；C/JNI directive 保留这些字段，恢复采用旧配置事实，缺失时拒绝 | 请求与实际配置冲突不改变旧快照/epoch/控制序号，正确替换可提交；Swift/C 与 Android JNI 显式参数准入；完整原生 offers/用户选择另验 |
| REQ-PICOO-MEDIA-044 | implemented | ARCH-PICOO-MEDIA-002 / REQ-PICOO-NEXT-003、004 | 未选择源时保存 Decoder offers 而不猜测 codec/fps；明确请求按单条完整格式匹配，不能跨尺寸/fps/codec/色彩借能力；拒绝保持事务编号与 epoch，合法重试清除该请求错误 | 先到 HEVC/60 能力不被 AVC 默认拒绝、组合/色彩交叉借用拒绝、无身份消耗、合法显式请求；原生 offers 与完整参数集准入另验 |
| REQ-PICOO-MEDIA-045 | implemented | ARCH-PICOO-MEDIA-002 / REQ-PICOO-NEXT-003、011、023、025 | iOS VideoToolbox 由显式 codec 请求选择硬件 AVC High/HEVC Main；HEVC 关闭 Open GOP；输入快照和原生 atom codec 一致；确认每张 420v BT.709 输入/目标后显式写入压缩 VUI，未知色彩拒绝 | M4 上同一生产 Swift 的八组原生调用、共享位流闭合 IDR/几何/色彩检查、未知输入拒绝与 iOS 构建；iPhone 相机/完整选择/持续 fps 另验 |
| REQ-PICOO-MEDIA-046 | implemented | ARCH-PICOO-MEDIA-002 / REQ-PICOO-NEXT-003、004、025 | Decoder offer 接纳 1080 可见图像的 1088 编码存储，正式可见尺寸仍受约束；level 按编码工作量，最终 supports 精确匹配存储/crop；准备请求按该条目的可见尺寸选择 | 双 codec padding/crop 区分、拒绝 padding 冒充画面、裁剪不降低 level、合法准备请求；原生探测 offers 接线另验 |
| REQ-PICOO-MEDIA-047 | implemented | ARCH-PICOO-MEDIA-002 / REQ-PICOO-NEXT-003、004、026 | Android MediaCodec generation 从 CaptureProfile 读取显式 codec，Core 请求同源；删除 H264 专用编码生命周期名称；恢复保留完整 profile 并匹配 codec/fps/尺寸，不能只恢复高度 | 完整 APK/JNI 与 JVM 构建；原有 native configuration/handoff 回归；小米实际双 codec、完整 offers/用户选择与失败恢复须另验 |
| REQ-PICOO-MEDIA-048 | implemented | ARCH-PICOO-MEDIA-002 / REQ-PICOO-NEXT-026 | 删除 C/JNI 无产品调用方的独立 ingest、flush、started 入口；公开媒体提交必须携带同一原生配置快照，无旧接口别名 | FFI 原子提交拒绝与事务身份回归、Android 完整 JNI/APK、iOS 构建与 Swift/C ABI 测试；内部状态机接口不属于公开平台契约 |
| REQ-PICOO-MEDIA-049 | implemented | ARCH-PICOO-MEDIA-002 / REQ-PICOO-NEXT-003、004、025 | 实际标准记录统一映射完整 VideoFormat，未知色彩不猜测，wire 声明必须一致；支持真实 736/1088 padding，level 下限按实际编码工作量计算 | Apple/小米真实八组合参数事实与 Mac 原生解码，缺色彩/标签冲突/预算/错误组合/736 宏块门限回归；原生 offers 广告及会话提交门禁另验 |
| REQ-PICOO-MEDIA-050 | implemented | ARCH-PICOO-MEDIA-002 / REQ-PICOO-NEXT-004、005、006 | Android 输入选择只接纳已知且满足固定 fps 的 Camera2 同表候选；旋转裁剪后像素充足，空候选/不足明确失败，不隐式降尺寸或放大 | JVM 空表/不足/旋转回归，小米后置相机八组合真实输出 PTS 短时测量；前摄、竖持、完整 offers、长时热稳态另验 |
| REQ-PICOO-MEDIA-051 | implemented | ARCH-PICOO-MEDIA-002 / REQ-PICOO-NEXT-003、004、025、026 | Sender 在媒体事件修改状态前按实际存储/crop/tier/level/色彩/AU预算匹配同一 Decoder offer；显式 tier 无旧默认，hvcC身份与SPS一致；序列化色彩来源于实际VUI | 小米High tier样本、伪造tier/level、错误存储/色彩/tier/预算保持generation和控制序号、合法重试；Core/原生平台回归；Receiver原生offers及网络全链路另验 |
| REQ-PICOO-MEDIA-052 | implemented | ARCH-PICOO-MEDIA-002 / REQ-PICOO-NEXT-003、004、009、025 | 原生 Decoder owner 内逐项 probe，只有实际原生输出与原始 token 成立才生成完整 offer；探测图像不外泄，每项 reset，无软件替代，reset/身份失败中止整份结果 | M4 实际11条完整offer、25项Decoder回归、最多三次输入的延迟输出原始身份、全拒绝不继承样本能力与reset失败停止；Receiver能力发送接线及持续吞吐另验 |
| REQ-PICOO-MEDIA-053 | implemented | ARCH-PICOO-MEDIA-002 / REQ-PICOO-NEXT-003、004、025 | Receiver 工作线程在直播前完成原生探测，owner 仅发送该实例的完整能力；配置以实际记录精确准入，探测期间最多暂存一个绑定连接的配置，断连清理；致命解码器失败作废能力，不静默重建并沿用证据 | Mac Receiver116/Sender75、Linux Receiver121/Sender75、FFI13回归；Apple/小米16组合经真实probe与QUIC解码；线程归属、早到配置/断连隔离、失败后停止；Windows和持续真机另验 |
| REQ-PICOO-MEDIA-054 | implemented | ARCH-PICOO-MEDIA-002 / REQ-PICOO-NEXT-003、004、005 | Core 保留有效能力中的所有选择，即使当前请求不匹配；从同一完整 Decoder offer 提取八种正式源格式的准备候选，未知能力与已知空交集不同；C/JNI/iOS 同锁快照传递完整 codec/height/fps，不能只传最高高度后重建笛卡尔积 | Sender76/FFI14、Android77 JVM与3项真机JNI、iOS构建与Swift/C桥接通过；混合 codec/fps 不借能力、断连未知；本地 Camera+Encoder 交集和设置UI仍待接线 |

| REQ-PICOO-MEDIA-055 | implemented | ARCH-PICOO-MEDIA-002 / REQ-PICOO-NEXT-003、004、026 | 完整已提交源格式只来自已接纳原生AU；普通世代不能静默改变codec/尺寸/fps，准备与失败事务保留旧格式；断连保留恢复事实但不表示正在传输 | Sender77项回归与Clippy通过，覆盖初次提交、明确切换、失败保留、同世代变更拒绝与描述属性更新；跨平台快照与UI另验 |

| REQ-PICOO-MEDIA-056 | implemented | ARCH-PICOO-MEDIA-002 / REQ-PICOO-NEXT-003、004、005 | Android准备查询与创建复用相同Camera2固定fps/旋转裁剪和MediaCodec硬件Surface/profile/尺寸帧率/码率规则；候选按单条完整SourceFormat与Decoder求交集，不隐式替换请求 | Android77项JVM与5项真机准备/硬件/JNI合同通过；小米后摄八组合采集重跑通过；前摄竖持排除1080p60；准备不代替实际输出记录和持续fps准入 |

| REQ-PICOO-MEDIA-057 | implemented | ARCH-PICOO-MEDIA-002 / REQ-PICOO-NEXT-003、004、005、026 | Android设置和直播选择显式完整源格式；默认请求AVC1080p60经本地与远端准入后使用，不可用明确提示；准备查询由ViewModel生命周期持有，按镜头/方向更新，界面不乐观显示请求为已提交 | Android76项JVM、模拟器12项UI/JNI、真机4项状态/JNI合同；完整手机到桌面切换、方向变化输入重准备、重连和长期fps另验 |

| REQ-PICOO-MEDIA-058 | implemented | ARCH-PICOO-MEDIA-002 / REQ-PICOO-NEXT-005、006、026 | Android方向请求通过原生输入事务重新选择足够像素且满足fps的Camera2输入；方向属于完整CaptureProfile并随已提交事实恢复，不仅修改正在使用的GPU矩阵；不可准备的方向请求不自动降低格式或无限重试 | 横竖转换的原生世代/epoch隔离、输入覆盖与fps，失败恢复保留完整profile；真机验证 |

| REQ-PICOO-MEDIA-059 | implemented | ARCH-PICOO-MEDIA-002 / REQ-PICOO-NEXT-003、005、006、026 | iOS完整SourceFormat贯穿默认请求、原生准备、设置、事务匹配与失败恢复；同一codec/尺寸/fps组合求交，不按最大高度推断，默认明确请求AVC1080p60 | Swift格式匹配、原生准备及事务合同；iOS构建与真机验收 |

| REQ-PICOO-MEDIA-060 | implemented | ARCH-PICOO-MEDIA-002 / REQ-PICOO-NEXT-005、006、026 | 手机与电脑触发Android镜头切换共用目标镜头完整准备；按用户明确授权自动选择目标镜头与Receiver交集上限，优先当前codec、其次分辨率、再fps；镜头和格式一次提交，异步准备后核对原会话/方向/源事实，无交集保留原镜头；画质菜单的明确格式请求仍不降级 | 不支持目标保持原镜头，前后摄可准备格式实际切换与fps合同；完整GUI链路验证 |

| REQ-PICOO-MEDIA-061 | implemented | ARCH-PICOO-MEDIA-002 / REQ-PICOO-NEXT-005、006、026 | iOS镜头切换按与Android一致的codec、分辨率、fps优先序选择目标完整上限；镜头与完整格式在同一次原生配置中提交，目标码率由实际目标高度派生，失败恢复旧事实 | Swift上限选择与回退合同、iOS构建；iPhone真机切换及持续fps另验 |
