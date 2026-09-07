# ARCH-PICOO-MEDIA-002：原生媒体与独立多输出边界

状态：planned · 决策日期：2026-09-06

## 场景与意义

用户以手机作为电脑摄像头，同时预览和保存视频。各输出共享源内容与时间身份，但不能因会议客户端、窗口或磁盘变慢而相互阻塞。架构优先长期维护简单，不维护旧协议、FFI、配置或 IPC 的兼容路径。ALPN 固定为 `picoocam`；协议、FFI、IPC 不引入版本号或版本协商，直接修改当前契约。流/资源 generation 仅用于生命周期安全，不是软件版本。

来源：[已采纳的产品方案](../../product/picoo-camera-next-v2-gpu-cpu-output-2026-09-06.md)；关联 PUC-004、PUC-005；追溯：[Next 需求](../requirements/next-media.md)。本文是目标契约，planned 不表示平台实现完成。

## 范围与职责

- `picoo-bitstream` 拥有 AVC/HEVC 位流解释、参数集、随机访问与平台格式适配；不依赖 packet、协议、GPU、UI 或软件解码器。`picoo-packet` 只拥有分片/FEC/重组，不再导出 AVC helper。消费者直接依赖 bitstream，不设旧 API 转发。
- Decoder 能力以完整 codec/profile、图像布局、帧率、色彩组合表达；每组合独立记录标准 level 上限和 AU 预算，不能跨条目拼接能力。源码流实际 level 可低于同组合的 Decoder 上限，原生适配器负责提供实际准入证据。
- Sender/Receiver owner 拥有配置与恢复事务。codec 工作者只报告携带原始 token 和提交时不可变配置快照的事实；完成帧的方向、镜像和色彩不得从 owner 当前配置重建。每 AU 在同一 Decoder generation 最多提交一次。已准入配置下的完整随机访问候选可终止旧预测链与其缺帧等待，清理必须先于候选入队；这只是恢复尝试，只有对应 Decoder completion 确认后才恢复发布，delta 或未完成候选不能获得相同权限。
- `picoo-frame-hub` 拥有不可变原生源帧、身份、描述及 FrameBus。native image 不含 CPU 像素变体；资源引用释放与 GPU 完成分别管理。
- NativeVideoFrame 将连接、stream epoch、Decoder generation、frame ID、source PTS 与提交时的配置描述绑定；描述区分 coded size、原生坐标内剩余 visible crop、pixel aspect、色彩和剩余变换，不虚构 CPU stride。FrameBus 为预览保留 latest，并为唯一处理后录像提供独立发布订阅；订阅不从 preview cache 补旧帧。满队列或超龄必须终止该订阅并明确原因，不能阻塞其他输出或静默恢复成普通成功。
- Apple 原生图像在 Decoder 完成回调返回前 retain 已完成的 IOSurface-backed CVPixelBuffer；所有别名不得继续写像素或 attachment。GPU 消费者持有图像引用直至任务完成（包括取消/失败），不能将工作提交或 CPU 引用释放解释为 GPU 完成。图像边界不提供安全的可变平台对象或 CPU mapping 接口。
- `picoo-gpu` 拥有平台 context、资源池、统一 RenderSpec 与输出专用 exporter。FrameHub 不反向依赖它。
- RenderSpec 的旋转先于输出坐标中的水平镜像，contain 保持源比例并填充不透明黑边。Apple GPU renderer 使用显式 Metal device 和 Core Image；源、目标色彩空间分别明确指定，不能以隐式推断替代已提交色彩合同。每个固定布局的输出池最多三张表面，拒绝继续分配；输出发布前必须完成 GPU 写入，后续消费者仍对自己的 GPU 读取寿命负责。
- Preview latest-only；虚拟摄像头使用独立 SampleClock；处理后录像订阅有界有序原生帧；原码流录像在 live scheduler/Decoder 之前接收完整 AU。
- 每个摄像头 sink 独立验证 GpuNative/CpuBridge。CPU 输出只做目标 GPU 图像的物化、布局复制与必要上传，不拥有 codec/连接事务，不反向发布到原生帧总线。
- CpuExporter 只接受已完成的目标 GPU 图像及完全相同的 RenderSpec；源 native image 不提供 CPU 导出入口。CPU 槽位同时约束正在使用和闲置的分配，持有输出引用时不能覆盖或额外扩池；无调用不分配像素和 readback。唯一源帧去重、需求与跨 sink 复用由输出协调器拥有，不放到 exporter 内重复维护。

## 约束

AVC/HEVC 使用真实硬件能力准入，正式配置为 720p/1080p × 30/60fps，默认准入后的 1080p60；不静默降低源配置。软件 codec 与 CPU 预览退出产品路径。网络反馈只在当前已提交格式的码率边界内调节，不创建尺寸/fps/codec 事务；热状态可以暂停码率增长并提示用户，但不能替换源格式。源格式变化只由显式配置事务在匹配原生事实后提交。

输出切换只推进该 sink 的 backend generation，保留源配置、正常 Decoder、其他输出与采样时钟。权限失败、非法契约、硬件 codec 缺失和源 GPU 丢失不能通过 CpuBridge 绕过。无 CPU demand 不持续导出；所有池、任务和缓存有容量、期限及隐私清理边界。

Windows 的共享句柄和 CPU 输出 ring 是不同的交接后端；macOS 两种准备方式都使用合法 CMIO sink/source。必须分别验收两后端真实系统 sample，不能用 CPU 成功代替 native 完成。

## 对旧规范的替代范围

本契约替代旧规范中 H.264-only/30fps、自动降低源尺寸、公共 CPU NV12、CPU 预览、跨平台统一 CPU ring 和不含录像的目标约束；旧验证记录仅说明旧实现。未冲突的配对、安全、会话所有权与有界恢复继续成立。删除旧实现随对应端到端替换进行，不建立并行兼容产品。

## 复用与依赖判断

位流标准能力复用成熟库：avcC/hvcC 记录采用 Scuffle，AVC SPS/VUI 事实采用 h264-reader；候选、API、许可证与验证边界见 [位流依赖研究](../../research/next-bitstream-dependencies.md)。packet 不拥有这些标准对象，Decoder 不为格式转换反向依赖网络分包。

Apple 图像、硬件 codec 与 GPU 分别复用 CoreVideo/IOSurface、VideoToolbox 和显式 Metal/Core Image；选型和框架颜色合同见 [Apple GPU 研究](../../research/next-apple-gpu.md)。GPUI 的 surface 资源必须由真实 Metal command buffer 完成回调释放；框架缺少该合同的地方只修补资源交接，不把 Picoo 配置、输出协调或像素处理放进 UI 框架。

编码尺寸与原生 allocation 尺寸分别来自 SPS 和平台输出；可见区域扣除 Decoder 已应用的裁剪，禁止再次裁剪同一边。未知 SPS PAR/色彩仍为未知；Apple adapter 使用 CoreVideo 实际 clean aperture、以方形像素表示的 nominal display size 和明确色彩 attachments 建立原生输出描述，并拒绝与已声明编码事实冲突的结果。平台输出缺失 BT.709 色彩依据时拒绝发布，不通过修改共享 attachment 使检查通过。

源配置入口只接受精确的正式尺寸组合（1280×720 / 1920×1080），无 480p 或任意高度归档。用户请求不按 Receiver 最大高度静默替换；不能满足的组合必须明确拒绝。码率策略查询对未知高度返回错误，C/JNI 数值接口以 0 表示不支持，不将其当作可用目标码率（REQ-PICOO-MEDIA-028）。

源配置使用与能力声明相同的 codec/profile/色彩枚举和标准 level_idc，不使用可随意拼写的字符串，也不在缺参数时猜测 profile 或 level。已提交参数集属于 Decoder job 的不可变配置快照；带内参数集只有逐个匹配时才可随 AU 进入平台 Decoder，不能绕过配置事务改写 source format。Sender 在原生编码器提供参数前没有源配置，不在会话创建时合成配置。参数解释失败直接返回错误，不能生成空记录或占位身份；完整原生回调必须先验证配置，再绑定 generation 或暂存源状态。Receiver 在替换已提交配置、推进 revision、使输出失效或释放未来 epoch 媒体之前校验记录与声明身份；校验失败必须保持原状态。StreamConfig 的 `codec_configuration` 承载标准 avcC/hvcC 记录，不再单独传输 SPS/PPS；原生 API 提供的参数集仅在平台输入适配边界保留。标准记录中的 configurationVersion 是外部标准语法，不是 Picoo 协议版本。标准 avcC/hvcC 解释与生成由位流依赖负责，领域层只执行有界准入和一致性校验。

CPU IPC 内容失效由独立原子代际表达，与文件/映射的进程世代分开。每次准备携带内容代际，槽位提交保留该原始值；Consumer 获得读 lease 后再次检查槽位值与当前值相等，不能把复制后的最新代际贴到旧内容上。Owner 只推进标量，不等待复制或 GPU 任务，也不覆盖消费者仍持有的像素。此门禁使迟到旧发布不可重新获得 lease；已交付系统 sample、缓存与持有引用的隐私期限仍由各 sink 的清理合同负责。代际耗尽永久关闭该映射，不能归零后重新启用。

CPU demand 必须来自实际消费请求，不能把已打开映射或已安装 VCam 当作持续需求。CPU ring 的消费租期使用同一主机的单调时钟，最长 250ms；有请求才续期，消费者崩溃后不永久保留需求。Camera Extension 聚合客户端生命周期，最后一个客户端停止时撤销请求。输出 owner 只保留一个最新待处理原生源，等待 demand 时不准备 GPU 目标或 readback；当前内容代际内已成功导出的同一源身份不重复物化。租期只控制 CPU 输出工作，不调整源 codec、帧率、Decoder 或其他 sink。

Camera Extension 的系统 sample 图像同样受每固定布局三槽上限约束。池对象随有限输出格式表复用，格式切换不能创建新的同布局池绕过仍在外部持有的 allocation。CoreVideo 拒绝扩池时跳过该次准备，不能覆盖已交付图像；恢复由外部 sample 引用释放决定。此边界不替代全链路总预算或系统已持有样本的隐私期限。

CPU 消费租期只证明客户端活跃，不授予连续物化权限。每个 ring 代表一个输出 sink 的聚合消费请求；实际读取推进单调请求序号，准备工作只消费尚未处理的最新序号，多请求合并为一个最新工作而不排队。即使源 60fps、消费者 30fps，也不能在两次消费请求间持续准备新图像。请求耗尽不回绕；不同输出规格的共享与完整 SampleClock 仍由输出协调器负责。

Mac VCam 的固定协商表为 720p/1080p × 30/60，初始 1080p60；同尺寸的帧率变体共享一个布局池。SampleClock 以 host uptime 为锚、以有理数频率推导每个绝对输出槽，禁止累加截断后的纳秒周期；定时器只调度下一槽，不补历史队列。格式属性索引与 duration 一起验证后原子提交，切换频率保留已发时间线；源帧重复或 latest 降采样不等于系统 sample 丢失。后端状态与 source identity 不拥有时钟重置权限。

Windows 解码原生图像的持有单位包含原始 MF sample、NV12 D3D11 texture 与 subresource index，不能只保留纹理 COM 引用而释放 sample 的 allocator lease。原生输出构造时要求已完成且所有别名不可变；各 GPU 消费者仍分别持有原 owner 到读取完成。平台依赖与 GPUI 表面能力核对见 [Windows GPU 研究](../../research/next-windows-gpu.md)。

Windows 原生输出必须验证纹理所属 device 与 Decoder generation 固定的 device 具有相同 COM identity；同一 adapter 上的不同 device 仍属于不同命令与完成域，不得以 adapter LUID 相同替代该检查。拒绝错误 device 的 sample 不消费或修改生产者资源。

Decoder 工作者接收可跨线程的 factory，在工作线程内创建、调用、重建与释放平台 Decoder。Decoder 接口不要求 Send，不用 unsafe Send 绕过 COM apartment 的同线程清理责任；仅显式测试注入的合成 Decoder 需要 Send。源图像/context 的可跨线程资源寿命与 codec runtime 的线程归属分别约束。

Windows MFT 生产工厂只接受硬件 DXGI adapter；创建固定 D3D11 video device 后检查 MF_SA_D3D11_AWARE 并绑定 manager，然后才协商媒体类型。完整源配置必须存在，正式可见尺寸/帧率、驱动 H.264 NV12 profile 与 coded geometry 的 decoder configuration 均需准入。MFT 必须自行提供 DXGI sample，实际纹理的 device identity 必须匹配；禁止为生产 MFT 分配 CPU 输出 sample 或解绑 manager 后软件重试。显式软件诊断只由 test-codecs/test 构建的诊断入口调用，不是产品工厂的候选或失败分支。

Windows GPU 完成通知使用官方 Flush1 的事件查询和一次性 threadpool wait，避免 CPU 轮询或自行维护跨提交 fence 序号。容量、系统 event、device-removed 注册和 wait 对象在新命令提交前准备；完成通知只在提交函数退出后启用，原生事件提前到达也不能释放提交函数仍在使用的 owner。提交失败或 panic 仍在同一队列后放置完成查询，取消接收者不取消资源保留。设备移除事件只能交付失败，不发布成功图像。每 context 最多三个未消费完成对象，全进程最多十二个；未消费结果仍占容量，context 重建不能绕过全局上限。该计数不替代图像字节总预算。

MFT 调用不在外部 ID3D11Multithread Enter/Leave 锁内执行，以免等待 MFT 内部线程时造成跨线程死锁；实际 GPU 命令组才使用 context 保护。Flush1 在提交函数返回后有序发出，覆盖该函数已提交命令；提交函数不得把尚未提交的 GPU 工作交给游离线程。UI/Receiver owner 不等待完成，允许专用 codec/输出工作者等待系统通知。

Windows RenderSpec 使用固定 GPU context 和原生 Video Processor；输入为已完成 NV12 BT.709 limited/left-chroma、方形像素的原生帧，输出为 NV12 BT.709 limited，几何与颜色能力均查询驱动。RenderSpec 表示调用方已合并的剩余变换，不再次叠加 FrameDescription 的变换。微软接口按 rotation→mirror→source clipping 应用，因此原生 visible crop 必须先映射到变换后的完整纹理坐标，再设置 source rect；contain 按裁剪后比例计算，黑边不透明。禁用自动画质处理，选择不依赖过去/未来帧的 progressive processor，不为独立 sink 引入隐藏时间队列。

Windows CpuExporter 只接受相同 RenderSpec、相同 device 的已完成 RenderedImage。无需求调用时不创建 staging，不映射图像；一次导出先取得三槽 CPU 池的可写槽，再将目标图像 GPU-copy 到唯一复用的 NV12 staging。完成事件保留图像与 staging，随后使用 DO_NOT_WAIT 读取映射；若 GPU 仍阻止访问则明确失败，不把 Map 当作完成等待。按平台 RowPitch 分别覆盖完整 Y/UV 有效行，丢弃 padding，不做几何或颜色转换。Map/Unmap 的 context 调用串行化，但 CPU 行复制不持有 context 锁。Mac/Windows 共用同一个 CPU 输出槽位实现，包含 Weak reader 的不可覆盖约束。

WindowsGpuContext 只拥有 D3D11 device、immediate context 保护与完成通知预算。MF DXGI manager 的创建、固定 ResetDevice 和保留属于 Decoder；创建 renderer/exporter 不初始化 MF，也不创建 manager。输出工作者可从原生图像/目标图像采用其现有 device，必须保持相同 COM identity；这种 wrapper 创建不代表新的 source device generation。已有设备同样检查 software adapter 与 SINGLETHREADED 标志，禁止通过导入入口绕过生产准入。MF/COM 的线程归属仍由 Decoder 管理，GPU wrapper 不承担其关闭责任。

MF runtime 与 codec COM apartment 分开持有。COM 初始化和反初始化属于同一个 codec worker；运行时引用随 sample lease 保留，先释放 sample/texture，再释放生产者运行时。最后一个跨线程引用仅关闭标准 channel，由应用创建的专用线程配对执行 MFStartup/MFShutdown，禁止在 MF 工作队列或任意消费者析构里直接关闭。存活与清理中的运行时 cohort 合计最多 16 个，容量不足明确拒绝新 Decoder；挂起的清理仍占容量。此数量上限不代替图像字节预算与隐私期限。

Decoder 输入由借用的压缩 AU 与不可变 DecodeToken 组成。Token 保留原 connection/stream/decoder generation、frame_id、source PTS、各提交时间、配置 revision 与配置快照，不持有压缩字节。输出是零到多张分别携带原 token 的图像；当前 AU 的 refresh acceptance 与返回图像的归属分开，暂时没有图像不等于丢帧。Receiver 必须逐图像验证原身份，再使用原配置发布，不能用触发本次输出的输入身份或当前 owner 状态替换。

MF 复用 IMFSample 的 100ns sample time 关联完整 AU 与输出。内部时间键单调、checked advance，reset 不归零；相同源 PTS 不冲突，因为源时间只保留在 token 内。只有 ProcessInput 成功才登记对应 token；最多 16 个待输出提交，容量不足明确失败，不额外扩队列。每次取得所有可用输出并消费各自登记项，未知、缺失或重复时间键拒绝；reset 清空登记和原生预测状态。GetSampleTime 与标准有界映射足够表达此适配，不用当前 AU 猜测归属，也不为补图重投原 AU。

RenderSpec 分开表达像素存储格式与颜色，不由颜色名称猜测 NV12/BGRA。Windows 的 NV12 BT.709 limited 用于 NV12 输出，BGRA8 RGB full G22/BT.709 用于原生显示交接；驱动必须明确支持所请求的格式与颜色转换。现有 Apple renderer 与 CPU exporter 只准入各自已实现的 NV12 组合；不把 RGB 目标作为 NV12 字节解释。

Windows BGRA 目标在同一个三槽池内创建 NT shared/keyed-mutex 资源，每个 allocation 只创建一次 handle，并随资源关闭。Producer 与 Consumer 都使用 key 0；只有 AcquireSync 的原始 HRESULT 等于 S_OK 才获得权限，等待超时和 abandoned 不授予访问。Producer 把写入权限和目标 owner 一起保留到 GPU 完成后释放，再交付图像；释放失败使该池槽不可再用。Consumer 必须保留原图像引用到 GPU 读取与 ReleaseSync 完成，不能只持有导入纹理或复制的 handle。共享锁只协调 native device 访问，Rust owner 决定是否可复用池槽，两者不可替代。预览和各输出的消费规格/缓存仍由相应输出 owner 管理，不能通过共享纹理改动源 Decoder。

Windows 显示资源提供者采用 UI 的实际 device，持有每图像的导入缓存与原图像 lease。GPUI 仅同步提交只读 draw，不拥有 Picoo 输出状态或另设完成调度器。相同图像的未完成重复读取共用访问权，避免递归 keyed-mutex 获取；每次读取仍分别持有到其 GPU 完成。忙碌时跳过当前图像绘制，其他控件可以重绘。绘制结束必须解绑 view，禁止后续无 owner 的命令继续读取。

MF 输出协商保留枚举类型的原生尺寸与 aperture；编码尺寸不是原生 allocation 必须相等的约束。完成图像的显示范围必须位于实际 allocation 内且与已提交源的可见尺寸一致；Decoder 已裁剪的图像只保留剩余裁剪，不能重复应用 SPS crop。

带内参数集的已提交身份检查由 bitstream 配置对象拥有，统一覆盖 AVC SPS/PPS 与 HEVC VPS/SPS/PPS。平台 Decoder 调用这一检查后才修改原生状态；原生 codec 的支持范围仍须独立准入，不能把位流记录解析成功解释为硬件解码已经支持。

VideoSpsFacts/VideoColorFacts 表达与 codec 无关的源几何和色彩事实；AVC 与 HEVC 的语法解析分别由成熟标准库承担。HEVC 源准入限制为单层 progressive Main 8-bit 4:2:0、零重排；扩展、未知非法 PAR、越界裁剪或尾部垃圾明确拒绝。解析器必须在执行算术或依据输入分配前限制取值，不能靠返回结构后的校验弥补解析时的溢出或无界分配。

Apple Decoder 以完整 CodecConfiguration（codec 与所有原始参数集）创建 CoreMedia 格式描述；同一配置复用 session，变更配置先建立可用的新 session，再替换旧 session。参数或原生创建失败不销毁仍有效的旧 session。AVC/HEVC 共用 token 与原生 NV12 输出边界，平台尚未实现的 codec 在提交前明确拒绝。

VideoToolbox 的闭合 HEVC 解码合同仅接纳 IDR 随机访问序列；CRA、RASL、RADL 在原生会话变更前明确拒绝。只有具备 leading-picture 归属与独立验收的恢复策略才能扩展此边界，不把 CRA 自动视为 IDR。

Windows Decoder 的 AVC/HEVC 配置切换通过新的同步原生 MFT 准备和提交，保留源 D3D11 设备身份；禁止在已工作的 transform 上试探另一 codec 后再回滚。HEVC 组件必须来自系统枚举并提供同设备原生 NV12 输出；组件缺失或 DXVA 准入失败使该配置不可用。同步驱动不接纳需要事件循环的异步 MFT。

Sender 的原生配置快照必须持有已验证 CodecConfiguration，codec/profile/level 由该记录派生；不得保存可独立互相矛盾的 raw SPS/PPS 与 codec 标签，也不存在空参数集的默认有效配置。平台适配器负责原生 CSD 的显式 framing 转换和失败返回，Core 不猜测输入是 raw NAL 还是 Annex B。配置快照自身的几何、帧率、方向和时间世代继续由源配置事务验证。

Android 编码器回调按自身 generation 保存完整标准配置记录，AU 入队时连同该记录快照交接；媒体工作者不从全局最新参数集反推旧 AU 的配置。Android UI 仅提出配置请求并请求关键帧，不单独写入 Core 的源配置；配置记录随匹配 AU 原子提交，不能越过队列中的旧世代媒体。MediaCodec 的 CSD→标准 record 转换在原生适配处完成，JNI 不把 record 再拆成 raw 参数返回 Kotlin。

iOS 直接从原生 CMFormatDescription 的 avcC/hvcC atom 获取每张 AU 的配置快照，不提取再拼接 raw 参数。连接和编码器准备只记录用户意图，不提交空配置；源配置只随原生 AU 进入 Core。原码流不因携带配置快照而添加重复参数 NAL。

原生编码器的 keyframe/sync 标志是待核对的提示。进入 Core 的 AU 必须经过共享位流分类，当前闭合序列合同只接纳 AVC/HEVC IDR 为关键帧；提示与实际 picture 不一致、CRA 或 leading-picture 序列在配置暂存和媒体提交前拒绝。平台层不能用系统标志绕过 codec-specific 恢复准入。

CodecConfiguration 的共同源事实由位流层解释，各 SPS 必须一致。Sender 序列化与 Receiver 配置替换前核对声明的可见尺寸与 SPS 裁剪后尺寸；coded padding 不当作显示尺寸。Decoder 复用同一检查，仍独立验证实际原生 allocation 和剩余 crop。该静态一致性不能替代相机帧率、色彩或硬件能力证据。

iOS 相机输入的尺寸与帧率必须来自同一个 AVFoundation format；配置服务在串行 owner 内共同设置 activeFormat 与 min/max frame duration。编码配置的 fps 不只是压缩器 hint；采集层不得固定另一个帧率，也不由 sessionPreset 重新选择已提交的格式。原生 8-bit 输入与 420v 输出的选择不代替实际颜色和持续吞吐验证。

iOS 编码输入在提交时登记不可变的配置事实，VideoToolbox 的 sourceFrameRefcon 只承载不解引用的单调身份。回调按原身份消费该快照，不能读取后来修改的方向或码率；未知、重复身份不得发布 AU。每个原生 session 最多十六个待完成项，失败取消幂等且身份不复用。关闭 session 完成原生回调后释放上下文；开始新配置不沿用旧压缩 session。平台丢帧明确触发恢复事实，不静默当作连续预测链。

Android 原生 AU 提交直接返回 Accepted、Rejected 或 Error 对象；Accepted 的配置提交、关键帧请求分别表达，Rejected 可独立请求关键帧但不能提交配置，Error 不承载成功副作用。JNI 不把负错误码与成功位掩码混用，不提供旧整数结果解码器。JVM 无法创建结果时保留其异常，不合成成功对象。

Encoder 请求以显式 SourceFormat 表达 codec、正式可见尺寸和 fps，无默认配置；这是固定 progressive 8-bit 4:2:0 SDR 产品子集的请求，构造成功不代表硬件支持。原生 started 可以先绑定 generation，但有效随机访问配置必须匹配全部请求字段后才能暂存/提交。C/JNI 请求与 directive 原样携带 codec/fps；恢复从旧已提交记录恢复这些字段，无旧记录不能猜测。相机/编码器/Decoder 的完整 offers 和实际颜色准入仍由各自能力边界负责。

能力消息先于源选择时仅保存已验证 offers，不猜测 AVC 或 30fps，也不捏造可用最大高度。明确请求优先于旧已提交配置，能力查询按其 codec/fps 与既定 SDR 色彩匹配。已收到 Decoder offers 的新请求必须命中单个完整条目；拒绝不消耗 epoch 或事务 ID，后续合法请求可继续执行。此准备门禁不替代原生相机/编码器 offers、实际参数集 level 和每 AU 预算的完成准入。

iOS 编码配置必须带 codec；VideoToolbox session/profile 与每输入快照均由该值派生，输出原生 atom 的 codec 必须一致。HEVC 明确关闭 Open GOP，同时关闭帧重排；原生 sync 标记仍接受位流 IDR 校验。压缩颜色属性不能只依赖输入 attachment 隐式传播：逐帧确认 420v、BT.709 primaries/transfer/matrix 后显式配置对应 VUI；未知输入或转换目标颜色拒绝，不通过篡改输入标签使其满足契约。

Decoder offer 分开表达原生编码存储与正式可见图像：1080p 可对应 1920×1080 或 1920×1088 存储，可见尺寸仍是 1920×1080。codec level 准入按编码工作量，不能因裁剪变小而降低；最终 supports 对存储和 crop 位置仍严格相等。准备请求只指定尚未取得参数集的可见尺寸，因此可选择含合法存储 padding 的同一条 offer，不从 codec 名称猜测 SPS 裁剪。

Android 每个编码 generation 从不可变 CaptureProfile 取得 codec、尺寸、fps 和镜头意图，MediaCodec 请求与 Core 配置请求使用相同字段。恢复持有完整旧 CaptureProfile，并校验 codec/fps/尺寸与 Core 恢复指令一致；不能只还原高度而保留失败候选的 codec、帧率或镜头。原生 CSD 与 AU 仍由该 generation 的回调快照携带，硬件能力准入独立执行。

移动端公开 FFI 只通过带配置快照的完整编码事件提交 AU；不提供独立 ingest、flush 或 started 入口，避免调用方绕过配置与媒体的事务边界。Core 内部状态机仍可分解事实处理，失败与恢复控制独立于媒体提交。

实际配置到协议 VideoFormat 的映射由协议层统一拥有，复用 bitstream 的有界源事实，不另写 SPS 解析器。映射保留 coded size、visible crop 和明确 VUI 色彩；缺失或不支持事实拒绝，不能从请求标签补齐。StreamConfig 的 codec/profile/level/可见尺寸/色彩声明必须与记录一致；该映射用于能力比较，不产生原生支持证据。依赖方向为 protocol → bitstream，bitstream 不依赖协议或平台。

720p HEVC 可使用 1280×736 编码存储，正式可见高度仍为 720。允许的有界存储高度包含 720/736 和 1080/1088；AVC level 按 coded macroblock 数与每秒 macroblock 数计算，HEVC 按 coded luma samples 与每秒速率计算，不能简单把补齐高度归回可见档位。最终能力成员比较仍精确区分存储/crop。

Android 相机输入必须来自同一个 Camera2 stream map，具有可满足所选固定 fps 的已知最小时长及固定 AE fps range；未知时长或空候选不猜测可用。输入在既定旋转裁剪后足以覆盖已请求图像，禁止隐式放大或把源请求改成 720p。不能满足的指定配置明确失败，由源事务处理，采集适配器不修改 CaptureProfile 来掩盖失败。

Sender 获得 Receiver offers 后，完整编码事件在绑定 generation、暂存配置或发送数据前，按实际源 VideoFormat/record level/本 AU 字节数验证同一条能力；不从准备请求匹配推导提交成功。缺少配置不能越过门禁。序列化的色彩范围来自明确 BT.709 VUI，不能用默认 limited 标签覆盖未知或 full-range 源事实。

完整 VideoFormat 显式区分 AVC、HEVC Main tier 与 HEVC High tier；缺省 tier 非法，不把 Main profile 当作 Main tier。hvcC 的 profile/tier/level 与每个 SPS 必须一致，header 声明不能降低源事实。准备请求尚无原生 tier 时可匹配任一合法条目，最终提交按实际 tier 精确比较；High tier 不存在 level4 以下的合法能力。

Decoder 能力探测在创建原生 Decoder 的工作线程内执行，同一个实例逐个提交有界的自有合成闭合 IDR 与标准记录。只有实际返回原始 token 和合法原生图像的候选才能成为 offer；每项后 reset，失败不借另一候选能力，不发布探测图像到 FrameBus。资产只是输入，不能作为运行设备支持证据；禁用、缺失或失败的原生 backend 不通过软件替代。能力结果包含真实存储/crop/tier/level，AU预算取本实现的有界准入上限；配置/吞吐/热稳态仍分别验收。探测错误导致该项不可用；reset或内部不变量失败使整份结果失败，不能混合重建前后的设备证据。

### Decoder 能力证据的生命周期

原生 Decoder 在自己的工作线程创建并探测，网络 owner 只接收完整 offers，不获取探测图像。配置按标准记录核对实际源格式后，必须命中该 Decoder 实例的完整条目。探测尚未结束时，只保留一个绑定 transport session 和 control generation 的待准入配置；断连销毁它。线程致命失败或 reset 失败即终止该实例并作废能力，不能自动换实例后继续使用旧证据；新实例须重新探测后才能接纳直播。现有 bounded worker 与平台 SDK 已覆盖此边界，无需引入任务运行时或另写媒体后端。

准备候选只从同一完整 Decoder offer 提取正式 codec/可见尺寸/fps 三元组，不组合不同条目的字段；未知能力与已知但没有匹配项分开表示。平台经同一 Core 锁的状态快照获取候选，再与所选 Camera/Encoder 能力求交集。候选可准备不等于真实输出已准入，最终原生配置仍须命中完整存储/crop/tier/颜色/level 条目。跨语言边界复用现有 cbindgen/C ABI 和官方 JNI/Swift 桥接，不新增序列化协议或版本。

已提交 SourceFormat 来自成功接纳的原生 AU，而非码率控制器的初始高度、用户请求或 prepared 配置。事务准备和失败不覆盖旧完整格式；普通已绑定 encoder generation 的描述属性可更新，但 codec/尺寸/fps 变化必须走明确事务。断连保留最后已提交格式用于恢复，该历史事实与当前传输状态分别表达。此状态复用 Core 事务和 SourceFormat，不新增平台格式推断层。
