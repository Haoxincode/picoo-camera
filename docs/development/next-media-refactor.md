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

## AVC SPS 事实与恶意输入边界

REQ-PICOO-BITSTREAM-004 新增 AvcSpsFacts，分离编码尺寸和可见 crop，保留未知 PAR/色彩，拒绝溢出、过大尺寸及非 progressive 8-bit 4:2:0。初选 Scuffle SPS parser 的 fuzz 触发 Exp-Golomb 下溢，已放弃该解析入口，改用 h264-reader 0.8.0；选型、依赖规模和未指定 MSRV 的限制见研究记录。真实崩溃输入作为固定回归保存。

Mac 与 Linux 分别 18 项 bitstream 测试通过；Mac Clippy all-targets 无警告。nightly-2026-09-03/libFuzzer 31 秒运行 1,361,338 次，无崩溃；这是有时限的验证，不是解析器无缺陷证明。实际 M4 AVC 1080p fixture 检查 1920×1088 编码尺寸与 1920×1080 可见尺寸。平台 Decoder 的元数据准入和完整配置 wire 接入仍另验。

此前提交 1f7b77c 的 Actions 34023015090 已全部成功。

## Mac 原生主链路与 GPU 下游 lease

REQ-PICOO-MEDIA-030 / GPU-003：VideoToolbox 完成回调直接 retain IOSurface，不再把源 plane 复制为 Bytes。Decoder 输出保留 SPS 编码尺寸与 CoreVideo 剩余 clean aperture、nominal display size、BT.709 色彩；尺寸声明冲突在 session 变化前拒绝，未知色彩不重标记。Receiver Mac 改用 NativeVideoFrame/FrameBus，原始 job 的配置 revision、身份、方向和镜像随帧发布；占位图只进入输出，不进入源 bus。源统计方法从 session owner 拆出，避免超过 800 行。

Mac Preview Worker 改用 Core Image/Metal，删除原 CPU scaling/color-copy 实现；60fps cadence 保持普通轮询相位，恢复可见后重新建立截止时间，避免密集补交。clear 使在途旧结果失效。CPU ring 在独立工作者执行 GPU 处理与输出专用 CpuExporter，不阻塞 Receiver owner 的 GPU/像素操作；启动等待有界，事件 mailbox 与 pending frame 都有固定容量。

GPUI 发布包的 CVMetalTexture/PixelBuffer 寿命问题通过可审查局部 patch 修补，renderer 源文件按职责拆为三个低于 800 行模块。实际 M4 阻塞 GPU 读取/单槽 pool 复用回归通过；移除修补的负向实验失败，恢复后依赖四项单元测试全部通过。上游来源与升级规则见 vendor/gpui-apple，未修改 UI Skills。

`cargo xtask test macos` 全部通过：系统临时 Keychain 合约、FrameHub 49 项与显式 Rust/Swift ring 跨进程合约、Decoder 11 项、GPU 9 项、Receiver 100 项、GPUI Apple 4 项、Desktop 70 项。该命令已把原生 Rust 测试复制到独立临时目录运行，消除 CoreVideo 首次 IOSurface 初始化对大型 Cargo deps 目录的 NSBundle 扫描；不扩大媒体截止时间。Mac 相关库与 xtask Clippy all-targets 通过。Linux Receiver 107 项通过、2 项忽略；这些不代替 Windows 硬件证明。

本步没有完成全部 Next 验收：完整 VideoFormat/config record/framing wire、Windows 原生图像、独立输出协调器、真实 client demand、唯一源导出复用、SampleClock、CMIO/MF GpuNative/CpuBridge、跨输出总预算/超时隔离和严格 IPC generation fence 均仍有工作。当前桌面仍在启动时附加 CPU ring，不能声称普通预览已达到 CPU readback 为零；CPU worker 的提交前 generation 检查也不等同跨进程原子失效边界。源重构与局部资源验证不能替代这些验收。


Mac 完整生产构建与本机测试包打包通过；Android 构建在 Mac 上也复用隔离测试执行目录，完整 workspace 单元测试、doc tests 与 Gradle assembleDebug 通过，新包已安装到 Xiaomi 15。手机锁屏尚未解除，本次新包的发现/视频真机验收尚未完成。旧安装包曾连接到新 Mac 后因 StreamConfig wire 类型已改变被明确拒绝；没有添加旧协议兼容。

发现状态修正 13550df：等待真实 Announce 时展示启动中，5 秒无回执进入超时；首次错误也记录。21 项发现测试通过（1 项受控 LAN 测试忽略），discovery 与 xtask Clippy 通过。Mac 正常 GUI 重启的 mDNS 日志确认在物理 Wi-Fi 地址 192.168.8.100 于约 1 秒内完成公告；原运行的 Error 日志级别过滤了诊断，因此未声称查明原持续异常根因。临时 PICOO_LOG_FILTER 不再被 GPUI 启动路径覆盖。c9da1b5 的 Actions 34025594034 五个 job 全部通过。


Xiaomi 15 解锁后，新 Mac 日志确认真实视频持续进入 VideoToolbox，当前源仍是 30fps；数分钟观测到恢复与丢片，不能把可推流记录当作 60fps/无损质量验收。随后 ADB 显式打开重现两个 MainActivity 共存：旧 ViewModel 继续编码，新页面显示离线。Android MainActivity 改用官方 singleTask 生命周期，通知/重复启动复用原实例，不增加会话全局缓存或旧状态迁移。真机 `SingleSenderActivityTest` 通过，三次系统显式启动保持 Activity 和 ViewModel 身份；ActivityScenario 的同步新实例启动不适用于此复用场景，测试通过 UiAutomation 的真实系统启动路径观察生命周期。


## 随机访问恢复候选与旧缺帧门禁

314fa68 的 macOS CI 在 5% 丢包场景失败：完整刷新帧仍被前面的未解决 gap 阻挡。定向回归先证明旧实现保留缺失 frame 2 并阻挡已完整的关键帧 3；修正后在已准入配置内，以完整随机访问候选结束旧预测链等待，先清理再入队，保留 Decoder completion 的身份和 refresh acceptance 门禁。delta、不完整候选和未来配置仍不能绕过门禁；主动放弃旧 gap 不伪造已观测网络丢失统计。

原丢包测试把序号当微秒 PTS、以约 500fps 发送并重复计数缓存帧。测试源改为真实 30fps 节奏、单调微秒 PTS，响应关键帧请求，统计不同帧并以解码进展检测停顿；350ms 检查及恢复后 1s 新鲜度检查保持不变。新的真实节奏用例在修正前稳定于约第 89 帧失败，修正后连续三次通过（同一确定性丢包种子），不将其描述为不同随机种子的覆盖。

Mac `cargo xtask test macos` 完整通过，其中 Receiver 102 passed / 2 ignored、Decoder 11、GPU 9、GPUI Apple 4、Desktop 70；Receiver all-targets Clippy 与 397 项文档链接检查通过。跨平台新提交的 CI 另验，不沿用旧绿灯。


## 标准编解码配置记录

REQ-PICOO-PROTOCOL-017 将 StreamConfig 的独立 SPS/PPS 改为 `codec_configuration`。Sender 原生参数输入通过既有 Scuffle avcC 构造，VideoToolbox、Media Foundation 与显式 OpenH264 诊断适配器通过同一有界解析器读取记录。无旧字段、迁移器或协议版本；avcC/hvcC 自带的 configurationVersion 属于标准语法。模拟器使用显式空的合成配置，不伪造参数集；Linux 真实编解码回归的编码器显式选择产品允许的 High profile。

Mac 原生套件通过（Decoder 12、GPU 9、Receiver 102 passed / 2 ignored、GPUI Apple 4、Desktop 70）；Sender 69、协议 24 项测试通过，包含标准 avcC/hvcC 字节往返、记录截断和 codec 不匹配拒绝。全 workspace 单元与集成测试通过。Windows/Linux 与 Android 产物继续由本次构建及 CI 验证，不将标准 hvcC 传输测试计为 HEVC 原生发送/接收完成。

此改动只替换配置载体：完整 VideoFormat 提交前准入、Sender 缺参阶段与合成测试边界收紧、AU 四字节长度规范化和 HEVC 原生事务仍未完成；不将当前 Decoder 的记录校验当成整条配置事务的验证。


## CI 时钟统计测试同步

dfd57e8 的 CI 中，5% 丢包恢复测试通过，macOS 失败项为统计用例固定等待后假定时钟映射已稳定。估计器会拒绝延迟异常的交换；统计窗口数不保证低延迟样本数。1dda9cf 改为在独立 5 秒测试超时内等待已发布的映射和至少两个统计窗口，保留延迟/不确定度断言，不修改生产估计器或媒体新鲜度预算。本机完整 Receiver 102 项通过。配置改造涉及的四个 crate all-targets Clippy 与 397 项文档链接检查通过。


## 跨平台配置样本与恢复节奏修正

04ab866 的 CI：iOS 通过；Rust/Android 共同失败于 AVCC 跨平台测试仍使用 Baseline 的 64×64 样本，新记录准入正确拒绝。统一采用已有真实 High/BT.709 样本，保留实际解码与尺寸断言。macOS 丢包测试主段通过，但恢复段最新帧年龄 1112ms 超过 1s；该段仍以突发帧和 50fps 发送。恢复段统一按 30fps 持续源发送并在间隔中泵送，保留 1s 新鲜度断言；需以新回归和 CI 验证，不能由节奏调整本身宣称修复生产恢复性能。

修正后 `cargo xtask test macos` 完整通过：Receiver 102 passed / 2 ignored、Decoder 12、GPU 9、GPUI Apple 4、Desktop 70，包含恢复帧年龄断言。日志 `/tmp/picoo-config-ci-fixes-tests.log`；跨平台新 CI 另验。


## 提交配置前准入

REQ-PICOO-PROTOCOL-018 在 Receiver 改变任何源状态前解析标准配置，并验证声明 profile/level 与记录一致。空记录、必需头截断、声明冲突被拒绝时，当前配置 Arc、revision 和等待中的未来 epoch 均保持不变。Sender 创建时不再预置空 StreamConfig；首份配置由原生编码参数提供。配对/统计/恢复测试与显式 loopback 诊断提供真实 High 参数，不通过关闭生产校验维持合成测试。诊断依赖 picoo-testkit 仅由 loopback-diagnostics feature 启用。

Mac 完整套件通过：Receiver 103 passed / 2 ignored、Decoder 12、GPU 9、GPUI Apple 4、Desktop 70；Sender 69 项通过。此条仍不代表完整 VideoFormat 几何/色彩准入、异步平台准备与提交事务完成；对应 Next 要求保持 planned。


## MF 输出类型重新协商

b5702aa 的 Windows CI 在真实 High/BT.709 小样本上返回 `MF_E_TRANSFORM_STREAM_CHANGE`，此前 Decoder 将其当成普通错误。按微软 [Handling Stream Changes](https://learn.microsoft.com/en-us/windows/win32/medfound/handling-stream-changes) 的原生契约，使用当前 windows 0.62.2 的 GetOutputAvailableType / SetOutputType，接受匹配几何的 NV12 输出后重新 ProcessOutput。保留 MFT 已收到的 AU，不 flush、不重送输入；32 个候选及一次输出重试保证有界。此修正仍需 Windows CI 验证，不代表 D3D11 原生帧、显卡或 GpuNative VCam 验收。


## Sender 配置失败不产生控制记录

REQ-PICOO-PROTOCOL-019 将 StreamConfigParams 的记录构造改为 Result，删除解析失败后的空记录与 Unspecified/0 占位身份。发送配置失败返回专门的 CodecConfiguration 错误；匹配 IDR 也不能提交失败配置、推进 epoch、发送控制或占用控制消息序号。有效参数替换后，同一待处理事务仍可完成。完整原生回调的参数校验发生在 generation 绑定和源配置暂存之前。

Sender 71 项测试通过；准入回归按职责位于 `session/tests/configuration_admission.rs`，与原有 epoch 事务测试分开。成功事务与重连测试使用真实 High 参数，不在产品代码保留空参数的测试例外。Android 全 workspace 测试、原生构建及 macOS 最终回归另记录。

Sender 严格准入最终验证：Mac 完整套件通过（Receiver 103 passed / 2 ignored、Decoder 12、GPU 9、GPUI Apple 4、Desktop 70），Sender 71 项通过，Sender/Receiver all-targets Clippy 与 Android 完整构建通过。


## 时钟统计回归使用实时源

01c1ac1 的 macOS CI 仍在旧首帧的 end-to-end 统计断言失败；只增加等待并不足以保证该断言。新增确定性估计器测试证明：映射已稳定时，靠近原点的旧帧仍可能映射到 Receiver 时钟零点之前，按契约应返回未知；同期新帧可正常映射。统计集成测试保留首个空闲窗口的 stale-frame/ABR 断言，在后续时钟验证阶段持续发送 30fps、单调 PTS 的新帧，并在失败时输出有界估计器诊断。生产时钟估计器及 5 秒测试超时均未修改。这解释了原测试断言为何不普遍成立，但不冒充已取得该 CI 失败时刻的估计器内部状态；新 CI 仍需验证。

用户已通知暂时拔下手机；后续暂停 ADB/手机真机操作，继续本机测试、架构与 CI 工作，真机验收保持待验证。

时钟修正本机验证：6 项 clock_sync 单元测试、Mac 完整套件通过（Receiver 103 passed / 2 ignored、Decoder 12、GPU 9、GPUI Apple 4、Desktop 70）。


## MF 部分输出类型

01c1ac1 的 Windows CI 已执行重新协商，但返回“无匹配 NV12”。按 [GetOutputAvailableType](https://learn.microsoft.com/en-us/windows/win32/api/mftransform/nf-mftransform-imftransform-getoutputavailabletype) 契约，候选可为只含 major/subtype 的部分类型；缺失 MF_MT_FRAME_SIZE 不应直接等同已知尺寸不匹配。仅在属性明确不存在时补入请求的输出尺寸，仍以 SetOutputType 原生接受为门禁；已知不匹配尺寸继续拒绝。失败信息包含最多 32 个候选的 subtype/尺寸，以定位剩余原生差异。旧 CI 未记录候选内容，不能据此断言此次失败必然由部分类型导致；后续 Windows CI 继续验证。


## 唯一 AU 线路格式

REQ-PICOO-PROTOCOL-020 统一线路图像为四字节大端 NAL 长度。显式原生适配器调用既有有界解析器，拒绝截断、无图像和错误格式，再进入 Sender 状态处理；VideoToolbox/C ABI 输入借用原字节，MediaCodec/JNI 从 Annex B 转换。Receiver 在改变配置前检查记录长度字段，真实 Decoder 不再猜测输入格式；仅 MF/OpenH264 平台 API 边界需要 Annex B。测试样本在显式边界转换，模拟传输的任意字节仍只用于 StubDecoder。

本机 bitstream 单元 15、标准原生记录契约 6、FFI 10 项通过，Decoder/FFI cargo check 通过；Mac 完整套件与跨平台 CI 继续验证。没有手机操作，不将 JNI 编译或样本测试计为 Android 多厂商硬件编码验收。

唯一 AU 格式的 Mac 完整回归通过：Receiver 103 passed / 2 ignored、Decoder 12、GPU 9、GPUI Apple 4、Desktop 70；bitstream/FFI/Decoder/Receiver all-targets Clippy 通过。Windows 与 Android 新线路格式仍须本次 CI 验证。


## 调度抖动与时钟未知的验收

c89ce6f 的 CI 34032626796 给出直接证据：Mac 实时视频正常、帧龄 54ms，但 mapper 的 12 个样本只有一个落在最低不确定度加 2ms 的筛选带内，stable=false。不是继续等待或刷新媒体就能保证可用映射。保持生产估计器的三样本和跨度门槛，新增确定性回归重放这组不确定度，确认总时间映射保持未知；新低延迟样本到达后可恢复。

网络统计集成测试验证实际收到至少三个已接受交换、完整统计窗口、本地分段指标以及总延迟与不确定度的依赖，不再把真实宿主调度当作稳定映射保证。精确 affine offset/drift 与总延迟数值仍由生产 ReceiverClockSync 的确定性时序测试验收，没有扩大生产筛选范围或延长测试超时。7 项 clock_sync 和 Mac 完整套件通过；Windows/macOS 新 CI 继续验收。


## MF 编码分配与可见区域

c89ce6f 的 Windows CI 明确列出 NV12 等所有输出候选为 192×96，而声明可见图像为 64×64。复用 picoo-bitstream 的 h264-reader SPS 事实区分 coded size 与 visible crop，提交平台配置前验证声明尺寸。MFT 输入/输出协商使用 coded size，CPU 适配器按原生行距复制正确的可见 Y/UV 行，删除从缓冲区长度推断 stride/高度的实现。

遵循微软 [Lock2DSize](https://learn.microsoft.com/en-us/windows/win32/api/mfobjects/nf-mfobjects-imf2dbuffer2-lock2dsize) 的原生内存边界与只读锁契约；核对现有 windows 0.62.2 API。优先使用 IMF2DBuffer2 的 scanline/pitch/allocation bounds；线性 IMFMediaBuffer 使用输出类型 default stride 或官方 MFGetStrideForBitmapInfoHeader，无自研对齐猜测。RAII 在所有退出路径释放一次锁；不引入新依赖。布局复制测试在本机运行，实际 MFT 解码与编译以 Windows CI 为准。这是现存 CPU Decoder 的正确性修复，不是新架构要求的硬件 D3D11 输出验收。

MF 显式布局 4 项本机测试通过，包含实际样本 SPS 的 192×96 编码分配与 64×64 可见尺寸；Windows 平台代码及真实 MFT 验收待 CI。


## 6301604 的跨平台回归结果

CI 34034219152 的 Rust/docs、Android 和 iOS 通过。Windows 成功编译并进入原生测试，没有继续出现 coded/visible 协商错误；新增单图测试却错误地假设首个 ProcessInput 必须同步给出图像，实际返回成功但 frame=None。测试按 MFT 的 END_OF_STREAM/COMMAND_DRAIN 契约取得已接受图像，不重复提交 AU；新 Windows 回归待验证。

macOS 时钟回归通过；丢包测试最后的帧龄为 1129ms。检查发现持续恢复源之后还存在 20 次停止供帧的固定 sleep/pump，实际调度时间不受 200ms 名义和约束。删除这段无媒体的等待，直接在已经跨过完整统计窗口的持续恢复阶段测量，保留 1s 断言，并在失败信息中记录实际窗口时间、已发送序号与统计。此次本机完整 Mac 套件通过；不能据删除 idle 阶段推断所有 CI 抖动原因或宣称恢复性能已经跨平台验收。


## CPU IPC 内容失效门禁

REQ-PICOO-FRAME-013 复用现有 mmap、Rust/C11 原子和内核槽锁，在 ring/slot 中增加内容代际。Producer 的准备 token 随槽位提交，消费者取得 lease 后检查当前代际；Owner 通过只暴露原子的独立 handle 立即失效，不等待像素复制。映射由引用计数保留到最后一个 handle，完整 mapping 仍不开放跨线程访问。零为永久关闭，计数耗尽不回绕；旧布局的零填充头不被接受，无版本字段与迁移器。

Mac CPU 工作者在入队时捕获代际，placeholder、配置失效与退出同步推进共享门禁。Camera Extension 让准备缓存身份包含内容代际，旧 lease 不能继续复制，复制期间失效返回失败；系统 sample 入队前再次检查，不修改已经共享的缓存像素。这不承诺撤回此前已交给系统的 sample，也不把 Windows 尚未接入的 Owner 当作完成。

3 项原子失效/迟到提交/耗尽测试和 2 项 Loom 模型通过；生产 Rust→Swift/C 跨进程门禁通过，新增持有 lease 跨进程失效后拒绝复制的验收正在运行。Mac 完整套件与 frame-hub/Receiver Clippy 已通过前一轮，新增 Extension 缓存合同需要本轮最终验证。

CPU IPC 最终验证：Mac FrameHub 52 passed / 2 ignored，生产 Rust→Swift/C 跨进程合同（包含持有 lease 的失效复制拒绝）通过；Decoder 16、GPU 9、Receiver 103 passed / 2 ignored、GPUI Apple 4、Desktop 70 通过。完整 macOS Receiver release 与 Camera Extension 构建成功；Clippy 和 400 项文档链接检查通过。未运行手机或签名扩展真实客户端验收，Windows Owner 和系统 sample 隐私期限保持待验证。


## CPU 消费需求与唯一源物化

REQ-PICOO-FRAME-014 在现有 IPC 中添加有界需求租期：实际读取才续期，打开映射不产生需求；Camera Extension 最后一个客户端停止时清零，异常退出在 250ms 内自动过期。跨进程时钟复用 Unix CLOCK_MONOTONIC（现有 libc 0.2）和 Windows GetTickCount64（现有 windows-sys 0.61.2 的 SystemInformation feature，最低平台早于项目 Windows 11 基线）；只传同机单调毫秒，不引入时钟服务、墙钟或额外消息队列。

Mac CPU 工作者无需求时只保留最新待处理 Arc，不做 GPU render/export。需求晚到时有界唤醒，即使没有新相机回调也能处理该源；无待处理工作时休眠。当前内容代际、原始帧身份和配置 revision 构成物化身份，已发布同源不重复导出或写 ring；Busy 时准备结果可复用。实际导出数进入结构化 trace，测试检查真实 GPU→CPU 路径的 0→1→重复仍 1→租期过期仍 1→新消费后 2，不通过 StubExporter 推断。

本机 Mac 完整回归与 Clippy 已通过初次需求门禁实现；增加 C 读取续期/显式停止检查和空队列休眠后运行最终验证。Windows CPU owner 尚未切到新 GPU 输出协调器，完整多 sink/格式协商仍待，不能由这条 Mac 门禁替代。

39ab1d1 的 CI 34035360507 已通过 Windows Shared Ring/MFT/Receiver AVCC 原生测试以及 macOS Clippy/原生媒体与共享区测试；两端产物构建仍在运行。MF coded/visible 和 EOS drain 至此有 Windows 原生回归证据，仍不等于 D3D11 硬件链路或实际显卡矩阵验收。

CPU demand 最终验证通过：Mac FrameHub 53 passed / 2 ignored、Rust→Swift/C 请求续期/显式停止与共享区合同、Decoder 16、GPU 9、Receiver 103 passed / 2 ignored、GPUI Apple 4、Desktop 70；Receiver release 与 Camera Extension 完整构建、Clippy 和 400 项文档链接检查通过。250ms 是异常退出的有限租期，不是摄像头样本时钟；source60→output30 的完整协商与采样节奏仍由输出协调器验收，当前实现不宣称完成此项。


## Android 原生 AVC CSD 准入

REQ-PICOO-MEDIA-033 按 Android 官方 [MediaCodec codec-specific data](https://developer.android.com/reference/android/media/MediaCodec) 的 Annex B 起始码契约，将 csd-0/csd-1 与 BUFFER_FLAG_CODEC_CONFIG 统一送到显式 Rust 适配入口。复用现有有界 NAL 拆分与 Scuffle 标准 avcC builder，无格式猜测或新依赖。仅接收完整 SPS/PPS；相同重复参数允许，冲突参数与非参数 NAL 拒绝。JNI 返回裸 NAL，避免把起始码送入严格 raw 参数构造器。

使用真实 VideoToolbox High 参数 fixture 的组合、重复、缺参、冲突与错误封装测试通过。Android 硬件测试增加对实际 AVC CSD 的生产 JNI 准入检查；构建和小米 15 实测结果待记录。


小米 15（24129PN74C，Android 16）NativeCodecContractTest 通过：QTI AVC/HEVC 硬件编码器在 720p/1080p × 30/60 的 8 组配置各产出 3 个独立 AU；4 组 AVC 实际 CSD 通过生产 JNI 并返回裸 SPS/PPS。此项不验证持续帧率或 HEVC 生产推流。测试 APK 与应用 APK 构建成功，bitstream/FFI 单元及集成测试通过。测试结束后的常规应用安装被系统拒绝 INSTALL_FAILED_USER_RESTRICTED；Mac 新包 GUI 已启动，但 4433 尚未监听且 SecurityAgent 出现，实际发现/推流验证尚未完成。


最终 `cargo xtask build android` 完整工作区原生测试、doc tests 与 Android 构建通过；bitstream/FFI Clippy、401 项文档链接检查通过。删除 JNI setStreamConfig 中缺 PPS 时猜测整段 SPS 输入格式的旧分支；配置参数只遵循当前裸 NAL 契约，原生 CSD 只在显式适配入口转换。


## Camera Extension 系统图像池上限

REQ-PICOO-VCAM-014 将生产系统 sample 图像分配集中到 OutputPixelBufferPool，采用 CoreVideo 官方 CVPixelBufferPoolCreatePixelBufferWithAuxAttributes 与 kCVPixelBufferPoolAllocationThresholdKey=3，和现有 Rust Apple GPU pool 使用相同平台契约。API 已由当前 macOS SDK/Swift 6 严格并发与 warnings-as-errors 编译验证，不引入依赖或手写引用计数。固定格式表持有固定池，切格式不重建同布局池；外部系统 sample 持有 allocation 时不能通过创建新池绕过上限。

生产 Swift 池的真实 CMSampleBuffer 回归通过：持有三张后拒绝第四张；只释放局部 CVPixelBuffer 引用不能恢复分配，释放一个系统 sample 后恢复，其他持有样本继续占槽。测试纳入 cargo xtask test macos，并随现有 Swift/C harness 在 Mac CI 执行。这是每布局系统图像池边界，不代表全链路显存总预算、CMIO 实际客户端或已交付样本隐私期限完成。

系统池最终验证：cargo xtask test macos 完整回归通过（FrameHub 53 passed/2 ignored，Swift/C 跨进程合同，Decoder 16，GPU 9，Receiver 103 passed/2 ignored，GPUI Apple 4，Desktop 70）；Receiver release/Camera Extension 构建与 Mac 打包成功，文档链接检查通过。


## CPU 准备按消费请求合并

REQ-PICOO-FRAME-015 使用现有 ring 的一个 AtomicU64 记录实际读取序号（头偏移 40，仍是 64 字节、无版本）；C11 与 Rust SeqCst 递增均在耗尽时停止而不回绕。250ms lease 仍只负责活跃性。Mac 工作者在 GPU 准备前消费最新请求，一个请求最多授权一次尝试；新请求可在准备期间独立到达，未处理请求只合并为最新值。源帧仍只有一个 latest 待处理槽，生产者不会把消费者租期当作每张源帧的导出许可。

复用现有跨进程原子、单调时钟和 Condvar 工作者，无额外协议库、计时线程或通用调度器。当前一个 ring 是一个 sink 的聚合消费面；本条不替代多种 RenderSpec 共享物化、30/60 协商 SampleClock 或 Windows 新 GPU Owner 的接入。

请求门禁最终验证：真实 GPU 导出回归证明已消费请求后连续 8 个新源不再导出，下次读取才导出最新工作；同源去重、租期过期、晚到请求和内容失效继续通过。FrameHub 54 passed/2 ignored、生产 Swift/C 序号合同、Decoder 16、GPU 9、Receiver 103 passed/2 ignored、GPUI Apple 4、Desktop 70；Clippy、文档检查与完整 Mac 构建通过。


## Mac 摄像头固定格式与独立 SampleClock

REQ-PICOO-VCAM-015 使用官方 CMIOExtensionStreamFormat 的固定 min/max frame duration 和 streamActiveFormatIndex/streamFrameDuration 属性，不引入第三方定时器。当前 Xcode SDK CMIOExtensionProperties.h 与 CMIOExtensionStream.h 已核对 API，Swift 6 严格并发编译通过；复用 Foundation DispatchSourceTimer 的单次绝对 uptime 调度和 CoreMedia 精确 duration。Picoo 自有部分只负责固定配置准入、槽选择与溢出边界，不自行实现平台线程或时钟源。

协商表改为 720p/1080p × 30/60，默认 1080p60，无旧 480p 配置。属性设置在同一锁内先验证索引和 duration 再提交，非法事务保持原配置。同尺寸两个帧率复用同一三槽系统图像池。SampleClock 按 host uptime 跳过过期槽，不累积整数纳秒周期的舍入误差；帧率改变从上次输出后重新确定边界，不能回退。源图像 latest 跳过不再误报 sampleDropped；实际时钟跳槽或准备失败才标记系统样本不连续。

生产 Swift 时钟的一小时 30/60 每槽边界、提前/重复/回退 tick、晚到跳槽、帧率切换、非法频率与耗尽测试通过。原生 CMIO device/stream 对象的四配置、默认索引、duration 改变及非法属性原子拒绝合同通过；此测试未启动已注册的系统扩展或真实摄像头客户端，不能替代 CMIO 端到端及热稳态帧率验收。

Mac SampleClock 最终验证：完整 cargo xtask test macos、Receiver release 与 Camera Extension 构建、Mac 打包及文档检查通过。61c08d8 的 CI 34037216590 已通过全部平台步骤，包含 Windows Shared Ring/MFT 原生测试、Receiver 编译、MSI 和 package smoke；新的系统池、请求门禁与 SampleClock 提交待下一轮 CI。


## Windows 原生解码图像所有权

REQ-PICOO-FRAME-016 新增 D3D11ImageLease，保留原始 MF sample 与 NV12 texture/subresource，不只保留纹理。已完成且不可变的构造契约、平台访问和 GPU 读取寿命均为显式 unsafe 边界；安全 API 只公开尺寸，Clone 共享同一个 sample owner。拒绝 CPU buffer、非 NV12/default storage、多 mip/multisample 或越界 subresource，不提供像素或 map。

Windows 目标 all-targets Clippy 已在 Mac 使用 x86_64-pc-windows-msvc 完成类型检查，包括真实 COM marker 与 WARP 资源测试代码；这不是 Windows 二进制或运行验收。测试检查原始 sample 的 COM marker 必须持续到最后一个跨线程 image clone 释放，及 CPU/BGRA 存储拒绝。现有 cargo xtask test windows 会执行 FrameHub 全套，无新增 workflow 平台逻辑。实际 WARP/MF 对象执行待 Windows CI，硬件 MFT 工厂、GPU 处理和 GPUI surface 接入尚未完成。


## Windows GPU device 与 MF manager 关联

REQ-PICOO-GPU-004 新增 WindowsGpuContext::for_adapter，显式使用同一硬件 adapter 创建 D3D11 video/BGRA device 与 immediate context，先启用并读取确认 ID3D11Multithread 保护，再初始化固定的 MF DXGI device manager。只在对象初始化时 ResetDevice，重建应创建新 context；原生对象借用和批量 immediate context 访问为受约束 unsafe API，批量调用的 Enter/Leave 在 panic 时也会释放。平台 codec 工作者持有 MF/COM runtime 责任，可跨线程 context 不承担 CoUninitialize。

生产入口按实际 DXGI descriptor 拒绝 software adapter；诊断测试通过官方 EnumWarpAdapter 获取真实 WARP 对象并检查拒绝。Windows all-targets Clippy、13 项 xtask 测试及文档检查通过；Windows GPU crate 已纳入 xtask test windows 的 Clippy 和原生测试列表。实际硬件成功路径、codec 准入、资源代际与 GPUI 互操作保持待完成，不以构建设备替代 H.264/HEVC 硬解证明。


## Windows CI 依赖缓存

连续平台原生测试后 Windows release 仍需完整重编依赖，当前 CI 未配置 Cargo cache。按 REQ-PICOO-STACK-004/005 复用已核对并固定 SHA 的 Swatinem/rust-cache v2.9.2，只为 Windows job 添加依赖缓存；workspace crate、平台测试、产物构建和打包门禁仍每轮执行。缓存不影响任务完成判断，首次冷/暖缓存结果待 CI；选型、许可证和生命周期边界见 ci-and-build.md。

39899fe 的 CI 34038336004 全部通过，包含 Windows 原生测试、release 构建、MSI 和 smoke。缓存 workflow 的 actionlint 与文档检查通过；本次新 Windows 图像/context 的运行测试将在新提交的 CI 中执行。


CI 34039825921 的 Windows 原生测试步骤已通过，包含 D3D11 NV12 sample 保留/跨线程最终释放、CPU/BGRA 拒绝及生产 GPU context 对真实 WARP adapter 的拒绝；Windows release 构建仍运行。Rust/docs、Android、iOS、macOS 已通过。FRAME-016 仅按这一资源 owner 合同标为 implemented，不代表 Windows Native Decoder 或硬件矩阵完成。

补充 GPU context 正向平台合同：复用生产内部 device/manager 绑定逻辑，在仅测试可达的 WARP 资源上检查 GetVideoService 返回同一设备，并在命令组 panic 后由另一线程取得原生锁。没有公开软件设备构造器，生产入口仍先拒绝软件 adapter。Windows all-targets Clippy 通过，新增正向合同待下一轮 Windows CI。


## Windows 输出设备身份准入

REQ-PICOO-FRAME-016 的原生图像构造现在要求 Decoder 固定 device，并使用官方 ID3D11DeviceChild::GetDevice 与 IUnknown COM identity 检查纹理来源。相同 adapter 的不同 device 不共享完成域，不能接受。复用 windows-rs 已有 API，无新增依赖。实际双 WARP device 回归同时检查错误设备拒绝与原 sample 可继续被正确设备接收；Windows 目标 all-targets Clippy 通过，运行验证待本次 CI。

CI 34039825921 已全部成功，包括 Windows release、MSI、smoke 和首次依赖缓存保存。此前 context 正向合同及本次设备身份检查将由下一轮 Windows CI 执行。当前 ADB 列表为空，无新手机验收证据。
