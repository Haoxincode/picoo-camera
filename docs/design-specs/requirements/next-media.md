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
| REQ-PICOO-NEXT-026 | planned | NEXT-REQ-026 | 旧协议、旧 IPC、旧配置、旧 FFI 不被新产品接受 | 混版本拒绝测试 |
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
| REQ-PICOO-NEXT-038 | planned | NEXT-REQ-038 | CPU IPC 使用新版 ABI、最小权限和崩溃可恢复 lease；隐私时限适用所有缓存 | 混版本/未授权/崩溃/断开占位测试 |
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
