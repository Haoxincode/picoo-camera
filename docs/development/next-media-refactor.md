# Next 媒体重构实施记录

日期：2026-09-06。用户授权开始重构；不兼容旧接口，以长期维护简单、架构清晰为准。

产品原文：[Next v2](../product/picoo-camera-next-v2-gpu-cpu-output-2026-09-06.md)；目标：[ARCH-PICOO-MEDIA-002](../design-specs/architecture/0012-native-media-multi-output-boundary.md)；[稳定需求](../design-specs/requirements/next-media.md)。

## 已落实

- 原附件移入 product，保留提案内容和原始证据成熟度说明；旧 PRD/context 明确新旧目标优先级。
- 40 条 Next 提案需求映射为独立稳定 ID，未冒称完成。
- AVC helper 与原 7 个行为测试从 packet 移到 bitstream::avc；所有调用方直接更换依赖，旧导出删除。bitstream 不增加第三方依赖。

## 尚未完成

P0 平台高风险接口探针尚未执行；本次边界调整只是 P1 中可独立验证的一部分，不代表 P0/P1 完成。HEVC、typed protocol/FFI、native frame/lease、GPU Presenter、两类 VCam、Recorder、旧产品实现清理和四平台验收均待实施。

下一项工作先核对锁定平台 SDK 与可复用位流库，验证双 codec 配置记录及 MF/VT 原生输出、GPUI surface 和 CMIO sink 的实际合同，再落地共同帧/codec 契约。不要直接加入空 NativeImage、假 GPU frame 或未接入生产的回退状态机宣称完成。当前 AVC、CPU 源帧、软件测试后端仍是待替换实现，不是允许长期保留的兼容层。

平台验收应按原方案分别验证 GpuNative/CpuBridge；本机 Rust 回归不能代替 Windows/Android/iOS 构建、真实 GPU/CMIO 与四组合真机验证。

## 本次验证

macOS 本机执行 `cargo test -p picoo-bitstream -p picoo-packet -p picoo-sender -p picoo-receiver -p picoo-media-decode -p picoo-ffi --quiet`：207 passed、0 failed、2 ignored，退出码 0；忽略项不计为通过。`cargo fmt --all -- --check`、`git diff --check` 通过。`cargo tree -p picoo-bitstream --edges normal` 确认无第三方运行依赖；40 项稳定需求映射完整且唯一。未运行其他平台构建或新版真机验收。

## P0：本机硬件合同探针

- 首次提交：`0ff9aa4`（规范归档、位流拆分）。
- `cargo xtask test apple-native-media`：M4 / macOS 26.6.2 / Xcode 26.6，AVC High、HEVC Main × 720p/1080p × 30/60 请求配置，各 3 帧硬件编码/解码；核对 UsingHardware 属性、IOSurface、双平面 Metal 纹理、GPU blit 实际完成；CPU 逐行复制到独立 IOSurface-backed PixelBuffer 并创建合法 CMSampleBuffer。Swift 6 warnings-as-errors 通过。
- 小米 15 / API 36：`NativeCodecContractTest` 真机通过，8 组配置分别得到 3 个不同 PTS 的 AU；检查 hardware 标志、实际 MIME/尺寸/CSD。实际组件为 `c2.qti.avc.encoder` / `c2.qti.hevc.encoder`。驱动输出 latency=4；未请求 KEY_LATENCY=0 的本探针不能作为低延迟验收。
- 手机原 debug 包因异签名卸载后重装；用户已允许应用与 instrumentation 安装。构建 `:app:assembleDebugAndroidTest :app:assembleDebug -x cargoBuildFfi` 使用已有本机 JNI，本探针不调用 FFI，不算新版 JNI 构建证据。
- 证据：[M4](../design-specs/verification/artifacts/next-media/apple-m4-native-contract.json)、[小米 15](../design-specs/verification/artifacts/next-media/xiaomi15-native-contract.json)。只含合成图与硬件元数据，不采集用户图像。
- 本次未测真实 Camera2 60fps、持续吞吐、颜色/构图、GPUI 呈现、CMIO sink/source、Windows 或 iPhone。P0 仍未全部完成，不提升 NEXT-003/004/009 为 verified。

Android 重跑：构建并安装 debug 与 debugAndroidTest APK，然后运行 `adb -s <serial> shell am instrument -w -e class com.picoo.camera.media.NativeCodecContractTest com.picoo.camera.debug.test/androidx.test.runner.AndroidJUnitRunner`。测试结果与 `PicooNativeProbe` tag 一起检查；am instrument 的 shell 退出码不替代 JUnit 成功。

## P1：双 codec 位流边界

硬件探针提交：`66a870c`。新增显式 Codec/NalFormat、借用 AU、保守 picture/RAP 分类及有界 CodecConfiguration；Scuffle 负责 avcC/hvcC 标准记录，Picoo 在调用前限制输入 64 KiB/64 个参数集，AU 限制 2 MiB/256 NAL，拒绝不支持层和未知 IRAP。没有把 HEVC CRA、RASL、RADL 当成 AVC IDR。

原 7 个 AVC 测试和 5 个新合同测试通过；Clippy 无警告；Android/iOS/Windows 纯库目标 check 通过。空 corpus 的 fuzz 6,463,806 次/31 秒，再加入 4 个 M4 硬件样本运行 3,943,129 次/21 秒，未崩溃；这只是有界 campaign，不声称完成全部 parser 安全证明。新增 fuzz 目标已接入 xtask。

该接口仍待 typed protocol/native encoder/decoder 正式调用。参数集完整语义与录像配置记录生成尚未完成；旧 AVC helper 的平台入口在对应调用方重建时移除，不新建旧协议兼容器。依赖核对见 [研究记录](../research/next-bitstream-dependencies.md)。

位流依赖调整后的 Sender/Receiver/Decoder/FFI 回归：180 passed、0 failed、2 ignored（macOS 本机）；仅此批受影响 crate。

## 实施修正：固定无版本协议

用户明确要求简单、无版本号：撤销未提交的 picoocam/2 和 protocol_major TXT。ALPN 保持 picoocam，协议/FFI/IPC 直接修改当前契约，无旧接口协商、迁移器或兼容分支；资源与流 generation 不受此修正影响。配对摘要仍显式绑定固定协议身份和 Ed25519 算法，发现字段拒绝重复。

共享帧 IPC 同步删除 Rust/C 的版本常量、header 字段与版本检查，直接采用当前布局；64-byte header 内 latest_sequence 偏移变为 16。保留 magic、槽数、容量与原子租约校验，不提供旧布局转换。关联 REQ-PICOO-FRAME-006 / REQ-PICOO-NEXT-027。

无版本契约验证：discovery/pairing/protocol/transport 78 passed、2 ignored；配对启停和相机命令 roundtrip 通过；frame-hub 41 passed、2 ignored；Rust→生产 Swift/C 跨进程环测试 10 passed（含异常进程恢复）；相关六个 crate all-targets clippy -D warnings 通过。以上不替代 Windows runner 或已签名 CMIO 真机消费验收。

生产 Decoder 工厂（REQ-PICOO-MEDIA-024）删除 Stub/OpenH264 回退：仅 macOS VideoToolbox / Windows MF 可创建平台后端，其余明确 unavailable。软件 codec 由 test-codecs 显式启用，生产工厂即使启用该 feature 也不调用测试工厂。四目标 normal/build feature 图均不含 OpenH264；Linux musl 默认 cargo check 通过。Android 单独检查 decoder 被 frame-hub 现有 shared_memory 不支持 Android 阻断，此项不记录为通过；Sender 正式依赖图不应包含 Receiver Decoder，后续原生帧模块调整时拆清边界。

共享帧运行时名称同步去掉旧 v1 后缀：Receiver/Windows source 使用 picoo-camera，macOS 扩展使用 picoo-camera.ring；不读取旧名称，不增加迁移分支。

该批本机回归：Decoder 6 passed，Receiver 97 passed / 2 ignored。无后端且 test-codecs 已启用仍报错的测试只在非 macOS 无原生后端目标执行，待对应 CI 验证，不以 Mac 测试替代。

REQ-PICOO-MEDIA-025：DecoderJob 的 Arc<StreamConfig> 随完成事件返回，方向/镜像以该快照发布；waiting/reconnecting placeholder 显式不镜像。15 项媒体 gate 回归通过，含同 epoch 下先提交再改变 owner 配置的双向旋转/镜像断言、迟到旧 generation 拒绝和 placeholder 隔离。Receiver all-targets clippy -D warnings 通过。这是当前同步平台 Decoder 工作者的快照修正，尚不代表原生异步多输出 token 契约完成。

CI 34016190367 的 rust-and-docs/Android job 在同一 Linux Decoder 单测失败：错误地要求 unavailable.reset 返回错误。reset 的契约是清理参考状态，无后端时可空操作成功；已改为验证 reset 前后 decode 都拒绝，并在 macOS 直接覆盖同一 unavailable 实现（1 passed）。跨目标生产工厂断言仍保留，继续等待 CI 与本机 Linux 容器回归。

REQ-PICOO-PROTOCOL-015：复用现有 Protobuf/prost 生成与传输边界，能力消息直接替换成 DecoderOffer[]，删除 codecs/resolutions/fps 独立列表和 Receiver 不拥有的 front/back camera 声明。VideoFormat 显式携带 codec/profile、8-bit 420、coded size、visible rect、有理帧率与 BT.709 SDR 色彩；DecoderOffer 单独记录该组合的标准 level_idc 上限和 AU 字节上限，不能把 level 上限误作必须精确相等的源码流 level。

当前域边界只准入 AVC High / HEVC Main、720/1080 × 30/60 和明确的 limited/full range；未知枚举、缺字段、重复组合、越界/非偶数 crop、过大 AU 预算直接拒绝。规格的 level 下限按 H.264/H.265 Annex A 对 HD picture/sample rate 的限制编码为 Picoo 产品约束（不自行实现参数集解析）；标准 record 解析仍归 Scuffle 位流边界。复用现有依赖，无新包、许可证或平台工具链要求。

Receiver 目前仅宣告已接入的 AVC/30 组合，不能因协议可以表达 HEVC/60 就宣告平台实现完成。Sender 当前 AVC adapter 的高度计算只使用相同 codec/fps 的条目，不能从 HEVC/1080 或 AVC/60 推导 AVC/30 的 1080 支持；无可匹配条目明确记录 NO_MATCHING_DECODER_OFFER。完整相机/Encoder 运行时 offer、StreamConfig 和 FFI 的 typed 替换仍未完成，本条不升级 NEXT-003/004 的平台验收状态。

完整 offer 验证：Linux 容器（Rust 1.98.1、独立 target）protocol 21 passed、Sender 68 passed、Receiver 107 passed / 2 ignored；本机跨 codec/fps 高度隔离回归通过；相关 all-targets clippy -D warnings 与文档链接检查通过。ControlEnvelope fuzz 接入 offer 验证，初始 6,343,760 次 / 21 秒，加入八种完整组合 corpus 后再运行 6,028,957 次 / 21 秒，无崩溃。

REQ-PICOO-MEDIA-026：Android 生产 Encoder 与真机 probe 共用 NativeVideoEncoder。直接使用官方 MediaCodecList/MediaCodec（现有 compile SDK、minSdk 29；isHardwareAccelerated/isAlias、Surface/profile/size-rate/bitrate 查询均在当前最低 API 内），按硬件名称创建而非 createEncoderByType。候选只有系统硬件 AVC High / HEVC Main，无新增依赖/二进制体积；厂商私有库和软件 codec 不符合平台统一边界，未引入。官方接口依据：https://developer.android.com/reference/android/media/MediaCodecInfo 与 https://developer.android.com/reference/android/media/MediaCodec 。

创建请求是当前 generation 的不可变尺寸/fps/码率快照。输出格式必须匹配请求 codec、High/Constrained High 或 Main、尺寸、fps、BT.709 limited SDR，并提供 CSD，才能开始放行 AU。HEVC 工厂路径已在 probe 中验证，生产 Sender 选择与 wire/config 仍待接入；工厂不接受已废弃的 480p，旧 ABR/手机格式入口将在下一步删除，不能作为正式发布验收。

iOS 使用现有官方 VideoToolbox；删除 Main→Baseline 属性回退，直接请求 High AutoLevel；准备完成后读取 UsingHardwareAcceleratedVideoEncoder 并拒绝 false。Xcode 26.6 下 cargo xtask build ios 成功（包括当前 Rust XCFramework 和未签名模拟器 App），cargo xtask test ios 的 19 项 Swift/Core ABI 测试通过；没有 iPhone 硬件编码验收证据。

Android Gradle 完整 assembleDebug/assembleDebugAndroidTest 已重建当前 aarch64 Rust JNI，安装到已授权 Xiaomi 15。最终 NativeCodecContractTest 通过（1.151 秒，八种组合、每种三张合成 Surface 输入）；生产实际格式校验通过、驱动 latency 字段均为 0。证据为 verification/artifacts/next-media/xiaomi15-production-encoder-contract.json；这不是持续帧率、真实相机或全链路 HEVC 验收。

先前无版本/工厂/配置快照批次 69b794a 的 GitHub Actions 34016538452 五个 job（Rust、Windows、macOS、Android、iOS）全部成功。

AVC Decoder offer 的 level 上限设为 4.2，避免把 HD 尺寸所需的最低 level 当作 Decoder 上限。实际硬件 Encoder 可为同一 720p/30 图像报告 4.1（本次 Qualcomm probe 即如此）；4.2 上限不增加已宣告的尺寸或帧率组合。后续运行时能力探测仍需按具体设备收窄。

REQ-PICOO-MEDIA-027：复用现有 Rust BitrateController 的拥塞观测与码率上下界，直接删除 DownshiftResolution/UpshiftResolution、自动配置 directive、升降档重试状态与优先级抢占分支，没有引入替代库或新的通用控制算法。网络恢复只能提高当前配置内的码率；只有显式原生配置 commit 才更新 active height 和对应码率边界。热状态只抑制码率增长，仍允许拥塞降码率；重复报告相同热状态不重置健康窗口。

Android ViewModel 保持热状态与提示副作用所有权，plain StreamingScreenContent 只接收 thermalLimited 和显式操作回调；删除过热自动 setResolution、分辨率点击拦截及其 composition toast 状态。一次性热提示标记移到 ViewModel 内，不占用渲染状态。UI 提示“设备温度较高，请注意散热或停止推流”，不再声称已自动降 720p。热策略不影响用户主动选择配置；剩余 480p 静态入口另行移除，不将本批当作所有源配置重构已完成。

验证：Linux 容器 rate-control 7 passed、Sender 68 passed、Receiver 106 passed / 2 ignored；补充热状态重复报告回归后本机 rate-control 8 passed。显式 1080→720 的真实 OpenH264 测试码流仍可跨 QUIC/配置事务解码到 FrameStore，但 1000 个拥塞窗口和 1000 个恢复窗口均不能自行改变源尺寸或 epoch。相关四个 crate all-targets clippy -D warnings 通过，Android unit test / debug / androidTest（含当前 JNI）构建通过。

该批最终验证：Android 再次完整构建通过（含删除无用协调器方法），cargo fmt、四个 crate clippy 和文档链接检查通过。Xiaomi 15 StreamingScreenSemanticsTest 六项全部通过，包含过热提示下显式分辨率按钮仍可操作。HyperOS 阻止测试 Activity 后台启动并要求 Picoo 打开自身测试包；通过 ADB 前台启动和系统“本次允许”完成测试，未更改全局后台启动策略。总运行 648.455 秒包含系统对话框等待，不作为 UI 性能证据。

REQ-PICOO-MEDIA-028：直接删除 480p 源入口、码率档与 normalize_height，Rust 码率查询返回 Option；非法 source transaction/preference 不推进事务或 epoch。C/JNI 码率查询返回 0、preference 返回 -1，不跨 FFI panic。Android/iOS 的高度解析只做精确匹配，远端宽高必须一起吻合；两端删除 Receiver 最大高度触发的静默重配，桌面菜单删除 480p。沿用已有 enum/Option、JNI/C ABI、GPUI PopupMenu 和原生 UI，不新增包或自制组件。完整 codec/fps/format offer 准入及 60fps 菜单仍未完成，不把本条当作 NEXT-004/005 完整验收。

Core 回归 rate-control 9、Sender 68、FFI 10 通过；Android unit test 与完整 APK/JNI 构建通过，Xiaomi 15 SourceHeightContractTest 1 项通过；iOS 当前 XCFramework/未签名模拟器 App 构建与 19 项 Swift/C ABI 测试通过。

ba6c8a8 的 macOS CI 34019052133 失败于旧专项测试仍期待 ABR 自动降档。已将该测试和 xtask 命令替换为显式 720→1080 配置事务，两个 1000 拥塞窗口不能自行切换。使用新增 M4 VideoToolbox 1080p AVC High 合成 IDR fixture，本机真实 VideoToolbox + QUIC + FrameStore 回归 1 passed（24.75 秒）。这是解码事务证据，不替代真实相机、GPU 输出或 Windows 验收。此前 f60f977 的五平台 CI 34017881353 已全部成功。

最终 Linux 容器回归 rate-control 9、Sender 68、Receiver 106 passed / 2 ignored。旧 camera-epoch 测试同样移除 480p 输入，使用 720p 保留三秒恢复阈值。相关六个 crate all-targets clippy 与文档链接检查通过；macOS Receiver/未签名 Camera Extension build 和 package 成功。桌面只删除一个 PopupMenu 选项及对应 Action，复用原有选中状态、键盘和焦点契约；不将编译作为完整桌面交互或已签名虚拟摄像头验收。

按正常 GUI 语义退出旧测试实例后，以 open -n 启动当前 Mac 测试包；进程已出现，但检查时尚无可见窗口或 UDP 4433 监听，未记录为发现/配对/桌面交互通过。继续以实际运行事实追踪启动条件。

REQ-PICOO-PROTOCOL-016：StreamConfig codec/profile/range 复用 DecoderOffer 枚举，level 直接为标准 level_idc；删除字符串 wire 类型、Baseline/3.1 默认猜测以及 Sender 中参数集头的自定义字符串映射。当前 Receiver AVC adapter 在改变已提交配置和等待 epoch 前拒绝 unspecified/未知/未接入 HEVC codec。此门禁不代替完整 VideoFormat、参数记录与实际 SPS 一致性准入；当前硬件 Sender 仍为 AVC，HEVC/60fps 未完成。

REQ-PICOO-BITSTREAM-003：复用已核对的 Scuffle H.264 0.2.2 AVCDecoderConfigurationRecord::build/parse（现有 MIT/Apache-2.0 Rust 依赖，无新增平台包），从 raw SPS/PPS 构建 bounded avcC；长度边界在分配与 u16 序列化前验证。记录头 profile/compatibility/level 与 SPS 前四字节一致性校验是 Picoo 配置约束，不自行解释完整 SPS。Sender 通过该记录取得 profile/level；不支持或缺参数时明确 unspecified/0，不伪造可支持的格式。完整 SPS 几何、颜色和 no-B-frame 证据仍缺失，未按它们宣称准入完成。

REQ-PICOO-MEDIA-029：VideoToolbox/MF 以及显式测试 OpenH264 adapter 共用 configured_avc gate。已声明配置中的 SPS/PPS 必须是有界 raw NAL，逐个比对 AU 内每个参数集（含孤立 SPS/PPS），任何冲突在 native session 状态变化前拒绝。VideoToolbox 删除“带内优先于配置”与嵌套格式兼容解释；无配置的独立原生 Decoder fixture 测试仍可自描述，生产 Receiver 继续提交配置快照。当前 Annex-B/四字节长度的 adapter 检测尚未被显式 NalFormat wire 字段替换。

用户完成 Keychain 系统授权后，c810636 Mac 测试包窗口已存在且 UDP 4433 监听恢复；此前启动卡住的线程栈确认等待 SecKeychainFindGenericPassword。本记录不把该旧测试包当作当前 typed StreamConfig wire 的端到端证据。

本批最终 Linux 容器：bitstream 13、Decoder 7、protocol 23、Sender 69、Receiver 107 / 2 ignored、sim 20 全部通过。Mac Decoder 9 项（含真实 VideoToolbox 冲突后 session 不变且可继续解码）、Receiver 100 / 2 ignored 通过；最后增加 avcC builder 后 bitstream 13、Sender 69 再次通过。相关六个 crate all-targets clippy、文档链接及格式检查通过。codec-bitstream fuzz 同时覆盖 builder，4,107,645 次 / 21 秒无崩溃。Android 当前 JNI + APK/unit test 构建通过，iOS 当前 XCFramework/未签名 App 构建及 19 项 Swift/C ABI 测试通过。Windows 原生调用仍等待当前变更的 CI，不以 Mac 替代。

原生 GPU 链路开始建立平台所有权边界：frame-hub 的 Apple NativeImage 只接收完成且不可变的 IOSurface NV12 limited，保留原 CVPixelBuffer，不映射或复制像素；unsafe 入口明确要求 producer/alias 不写，GPU consumer 留存引用直到完成。当前仅落地资源类型，尚未替换 Decoder/FrameBus/Preview，NEXT-009/016/029 继续 planned。实际 CoreVideo 测试证明回调所有者释放后跨线程 clone 仍保留同一原生对象，以及 CPU-backed/错误格式拒绝；它不证明 GPU fence 或池复用。

GPU 转换复用判断与 M4 可重复 probe 见 docs/research/next-apple-gpu.md 和 verification/native-media/core-image-color-probe.swift。Core Image 显式 Metal 渲染的 BT.601/709 target matrix 数值通过；当前 GPUI 的固定颜色契约已核实，不将 CPU 转换删掉后直接提交错误颜色的 source surface。d5cddc4 CI 的 Rust/Android/macOS/iOS job 已通过，Windows 仍在执行。

本步 Mac frame-hub 单元测试 43 passed / 2 ignored（原有忽略项），all-targets clippy -D warnings、cargo fmt、文档链接检查通过；Core Image probe 加入目标 YUV ±1 断言并复跑通过。未运行当前 wire 的 Mac/Xiaomi 相机端到端测试，不将资源单元测试视为该证据。

REQ-PICOO-GPU-001：新增架构指定的 picoo-gpu crate，依赖方向 GPU→FrameHub。AppleRenderer 显式 Metal/Core Image，无 CPU 像素访问或软件 fallback；旋转后镜像、contain 黑边、BT.709 limited 与 GPUI BT.601 full 输出分开。固定布局 CVPixelBufferPool 三槽阈值，GPU 完成写入后才交付 RenderedImage；下游 GPU 读取仍须保留 clone 至自己的完成，不能复用这个写入完成证据。source attachment 不匹配拒绝，源对象不变。每次渲染 autorelease pool，禁用中间图像缓存。

首次红色 fixture 测试通过后，灰阶发现 Core Image 默认输入颜色推断将 Y=40 变为 52；改为显式 kCIImageColorSpace BT.709 后，实际 M4 七项测试全部通过（46.66 秒），覆盖两输出、灰阶、八种方向/镜像、contain、池耗尽、保留 clone、跨线程及非法尺寸/缺失 source color。Linux 仅公共契约 1 项通过，不宣称 Linux GPU 支持。cargo xtask test macos 已包含此测试，以真实 Metal 可用性验收，不增加无 device 时的跳过或软件替代。Decoder/FrameBus/Preview 仍未使用新 renderer，四十项总目标继续未完成。

该 GPU 批次 picoo-gpu/xtask all-targets Clippy -D warnings、cargo fmt、文档链接检查通过。前一批 CI 34021277005 的 Windows/macOS 仍在执行，先保留本地阶段提交，避免频繁 push 取消原生构建；待下一次推送一并提交后续集成。

REQ-PICOO-GPU-002：为原生输出接入补齐目的端 CPU 物化边界。CpuExporter 只接受已完成 RenderedImage 与完全一致 RenderSpec，逐平面复制可见行，保持目标颜色/方向，不接收 NativeImage、不向帧总线发布、不自行订阅或启动计时器。布局拒绝与三槽耗尽都发生在 mapping 前；只在显式 export 时懒分配，Arc 仍被消费者持有时不能覆盖，弱引用同样不能绕过写入独占。正常/错误路径都释放 CoreVideo read lock，正常 unlock 失败会明确返回错误。

选型核对：仓库 FrameBufferPool 只限制 idle retained storage，checkout 在慢消费者占用时仍可继续分配，不满足新 CPU staging 总量契约。此处直接采用 Rust std::sync::Arc 的 get_mut 独占语义和固定三槽，不引入通用对象池框架、不自行维护引用计数，不复用旧软上限语义。CoreVideo 官方 lock/plane API 复用现有锁定 0.3.2 绑定，无新包。

M4 完整 GPU 套件九项通过（26.36 秒），新增实际 GPU→CPU 像素、紧密行布局、槽满拒绝、clone 寿命及错误 color contract 零导出测试。最终 unlock 错误传播调整后再跑对应 CPU exporter 测试；all-targets clippy -D warnings 与文档检查通过。当前仍在替换前的原生组件接线准备，Decoder/公共 CPU VideoFrame/旧预览尚未删除，不据此将 NEXT-029/033/034 总需求标为完成。输出协调器负责的 demand、唯一源去重和跨 sink 共享仍待实现。

CPU exporter 最终两项定向测试通过（25.28 秒，包含最终 unlock 错误传播实现）；格式及 diff 检查通过。bc3a30d 与本批保持本地阶段提交，等待前一批 Windows 原生构建结束后推送，避免取消其证据。

REQ-PICOO-FRAME-012：建立 NativeVideoFrame / FrameDescription / FrameIdentity / FrameBus。身份携带 connection、stream epoch、Decoder generation、frame ID，描述携带提交配置 revision、coded geometry、原生坐标中的剩余 visible crop、pixel aspect、明确 NV12 BT.709 limited/chroma siting 与剩余旋转/镜像；不含 CPU stride 或 Bytes。NativeImage 现在为平台所有权枚举，ApplePixelBufferLease 保留原已验证的 CVPixelBuffer 所有权；未接入平台没有 CPU 占位变体。Fake 仅 cfg(test) 编译，不提供生产 feature，不作为软件 codec。GPU 使用明确 Apple lease 入口，Rotation 的唯一领域定义移到 FrameHub。

FrameBus 独立保存 latest 和唯一有序订阅，订阅只接收未来发布。std bounded sync_channel 容量八，try_send 不等待消费；满队列终止订阅并携带被拒绝帧身份，latest 继续。取出帧超过 150ms 时明确 TooOld，清空/发布者退出/取消分别有终止原因，不能成为普通录像成功。取消后的消费者下次 poll 丢弃队列；已经持有引用的 deadline 回收和 commit gate 仍由输出协调器负责，未宣称单靠 FrameBus 可以撤销外部 Arc 或停止卡住的 GPU。

复用判断：使用 Rust std 的 Arc、sync_channel/try_send、OnceLock，不自制并发队列、引用计数或 callback 框架；当前同步平台工作者不需要 Tokio runtime，crossbeam 的额外多消费者功能在唯一录像订阅中没有收益，因此不新增包。现有 latest CPU store 不能提供有序事件且携带 Bytes，不能当作原生 bus 的目标模型。旧公共 VideoFrame/Receiver/Preview 仍待端到端替换，本步不作兼容重导出。

Mac frame-hub 全套 49 passed / 2 ignored，Linux 六项新纯契约测试通过；Linux production cargo check 同时验证没有 Fake variant 时的未实现平台可编译。Linux 1.98.1 容器未安装 Clippy component，未将该失败记作通过；本机两个 crate all-targets Clippy -D warnings 通过。NativeImage 类型变化后，实际 M4 双输出 Metal 颜色回归 1 passed（25.88 秒）。最后取消重查后的六项纯契约再验证见相应日志；四十项总目标继续未完成。

FrameBus 最终六项定向测试、本机两 crate Clippy 及文档/格式检查通过。前一批 07df65a 的 GitHub Actions 34021277005 五个 job 全部成功（包括 Windows）；现将 bc3a30d、2885016 与原生帧总线提交一起推送，当前 GPU/FrameBus 变更的 CI 结果另行验收。
