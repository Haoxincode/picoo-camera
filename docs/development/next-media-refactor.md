# Next 媒体重构实施记录

日期：2026-09-06。用户授权开始重构；不兼容旧接口，以长期维护简单、架构清晰为准。

产品原文：[Next v2](../product/picoo-camera-next-v2-gpu-cpu-output-2026-09-06.md)；目标：[ARCH-PICOO-MEDIA-002](../design-specs/architecture/0012-native-media-multi-output-boundary.md)；[稳定需求](../design-specs/requirements/next-media.md)。

## 当前交付状态（2026-09-07）

整体仍未过半，40 项 Next 总需求尚未逐项验收闭环。基础契约的 implemented/verified 不等于整个产品完成；下面的初次实施记录属于历史，不表示当前还未执行平台探针。

| 交付范围 | 当前状态 | 主要剩余 |
| --- | --- | --- |
| 架构、无版本协议、旧路径删除 | 主要边界已调整 | 各功能替换时继续删除剩余旧实现 |
| Windows/macOS 原生帧、GPU 预览、按需 CPU 输出 | 已接线，相关原生 CI 成功 | 真实显卡画质、全局预算、完整多 sink 验收 |
| AVC/HEVC 与四种正式配置 | 部分完成，Apple HEVC 解码已本地验证，Windows 接入待原生验收 | 移动端接线、配置事务、Windows HEVC 实测、真实 1080p60 链路 |
| 两平台 VCam 双后端 | 尚未完成 | GpuNative/CpuBridge、SampleClock、切换和真实系统 sample |
| 两种录像 | 主要工作尚未完成 | 原码流/处理后录像、分段、失败语义与独立时间线 |
| 四组合发布验收 | 尚未完成 | 真机矩阵、画质、延迟、热稳态、设备丢失及隐私期限 |

## 初次实施记录（历史）

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


## Decoder runtime 的线程归属

REQ-PICOO-MEDIA-018 移除 AccessUnitDecoder 的 Send 超 trait，以及 MF/VideoToolbox 为满足它添加的 unsafe Send。生产 Worker 已经跨线程传递 factory 并在线程内构造、重建与释放 Decoder；接口现在允许平台线程绑定类型，MF 的 CoInitializeEx/CoUninitialize 不可随 Decoder 跨线程转移。仅合成测试 Decoder 的注入入口要求 Send，不增加桥接线程或通用 runtime。

新增刻意含 Rc（不可 Send）的 Decoder 回归，实际检查 create/decode/reset/drop 的 ThreadId 一致且不同于调用方。当前本机 Receiver 104 passed/2 ignored、Decoder 16 passed；Windows windows-mf 库 Clippy 通过。包含测试依赖的 Windows 目标检查受本机 ring C 编译环境缺失限制，交由 Windows CI 验证，不记作平台通过。Mac 测试复制到临时目录执行，与已有 xtask Apple native_tests 的隔离目录方式一致。

CI 34041534379 的 Windows 原生测试已通过，包含 MF manager 正向设备身份、panic 后锁释放及双 device sample 拒绝；发布构建仍在运行。整体 Next 40 项需求未完成，Windows 原生 Decoder/预览生产替换和 GPU completion 等仍待实现。


## Windows 生产 MFT 硬件准入接入

REQ-PICOO-MEDIA-034 将 WindowsGpuContext 接入实际 MfH264Decoder 生产工厂：先建立硬件 adapter/device/manager，再检查 D3D11-aware 和发送 SET_D3D_MANAGER。正式尺寸/帧率与驱动 H.264 NV12 coded geometry configuration 在 SetInputType 前检查；配置缺失明确拒绝。输出必须由 MFT 提供并属于固定 device，不为硬件模式分配 caller CPU sample，不清空 manager 后重试。SoftwareDiagnostic 是只在 test/test-codecs 编译的枚举变体，Windows 核心回归显式使用诊断工厂，避免将 CI 无真实显卡时的软件测试冒充硬解测试。

该提交推进硬件生产入口，仍保留旧 mf/buffers.rs 读取 DXGI 输出像素的待删除实现；没有声称 FrameBus、GPU Preview、HEVC、completion、60fps 稳态或显卡矩阵完成。下一步应在完成同步和元数据合同后将其直接替换为原生帧，不为旧路径添加兼容 API。

Windows 目标 windows-mf 生产及 windows-mf,test-codecs 诊断库 Clippy 均通过；Mac Decoder/Receiver all-targets Clippy 通过。全目标 Windows 原生测试由 CI 执行，本机不具备 ring 所需 Windows C 工具链。

本提交 Mac 回归：Receiver 104 passed/2 ignored、Decoder 16 passed，文档检查通过。上一轮 CI 34041534379 现已全部通过；本次工厂接入与线程归属提交待新一轮 Windows CI，软件诊断回归不计入硬件验收。


## Windows GPU 完成通知与 MF 输出保留

REQ-PICOO-GPU-005 采用官方 ID3D11DeviceContext3::Flush1(D3D11_CONTEXT_TYPE_ALL, event)、RegisterDeviceRemovedEvent 和一次性 threadpool wait。所有 fallible 等待资源与 capacity 在提交前创建，回调只在提交退出并放置完成查询后 arm；立即完成/设备已移除不会与提交函数并发释放 owner。错误和 panic 也等待已提交工作完成；接收者取消只关闭通知，不释放在途样本。callback 注销设备事件、关闭 wait 与 event 后交付结果，析构/通知 panic 不跨 native ABI。

每 context 三个、全进程十二个 completion permit，完成但未消费的 channel 结果仍持有 permit；重建 context 不绕过全局上限。它不替代全图像字节预算，也不将 GPU hang 的系统 TDR 时间作为已验证隐私期限。

MfH264Decoder 使用 Arc 固定 GPU context，输出前预留 completion，再在同一 Decoder worker 调用 ProcessOutput；MftOutput 立即保存返回的原始 sample，包括错误路径。MFT 调用不持有外部 immediate-context lock；完成查询单独串行提交，工作者等待结果后才检查和读取源。旧 CPU 输出读取仍待原生帧/预览替换，但源样本完成与保留不再依赖 Lock2DSize 的隐式同步。

新增实际 OS wait 取消保留、未消费结果容量、panic 清理、跨 context 全局上限和 WARP GPU CopyResource→完成事件→诊断 Map 校验。GPU all-targets 与生产 MF 库的 Windows 目标 Clippy 通过；实际测试执行待下一轮 Windows CI。上一轮 CI 34042514875 已全部通过，包括硬件生产工厂 feature 编译及显式软件 MF 核心回归，不代表真实硬件解码验收。


## Windows Video Processor 原生输出

REQ-PICOO-GPU-006 新增 WindowsRenderer，显式接收源 GPU context 和固定 RenderSpec。源必须是同 device 的已完成 native NV12 BT.709 limited/left-chroma、方形像素；输出仅接受可精确声明的 BT.709 limited，不猜测 Apple Bt601Full 混合颜色合同的 DXGI 等价物。官方 VideoProcessorEnumerator1 验证 NV12 双向支持、精确色彩组合、rotation/mirror feature 与无 past/future frame 依赖的 rate capability，明确失败，无 CPU 处理分支。

三槽目标池首次 render 才分配，held Arc/Weak 阻止写入复用；原生 input/output view、原 MF sample 的 source lease 与目标 lease 均交给已有 completion，Blt 的失败/panic 同样保留到 GPU 完成。禁用自动画质处理，设置不透明黑边并执行 contain；输入 allocation/crop 根据官方 rotation→mirror→clip 顺序处理，不能在 90° 后把底部补齐行当成可见左边。

Windows 目标 GPU all-targets Clippy 通过。新增八种几何组合、1920×1088 的 1080 可见区旋转/镜像、越界拒绝及真实 NV12 输出纹理池三槽/保留/复用测试，待 Windows CI 实际执行。完整 Video Processor 颜色/缩放对照需要真实显卡，不能由几何单测或 WARP 池分配替代。此 renderer 尚未接入 Windows FrameBus/GPUI；生产 Decoder 的旧 CPU 输出读取仍待对应端到端替换。

CI 34043657175 Windows 原生步骤已通过，包括 Flush1 真实 GPU copy、threadpool 取消保留、未消费完成容量与 panic 清理；Windows release 正在构建，其余平台按运行状态继续等待。

WindowsRenderer 最终静态验证：Windows GPU all-targets Clippy 与 Mac GPU all-targets Clippy 均通过，文档/格式检查通过。CI 34043657175 现已全部通过，包含 Windows release、MSI 与 smoke；本次 renderer/pool/geometry 回归待下一轮 CI。


## Windows 目标图像 CpuExporter

REQ-PICOO-GPU-007 新增只接收 RenderedImage 的 Windows CpuExporter。相同 RenderSpec/device 的校验先于容量、staging 与 readback；首次实际导出才创建一个 staging texture，先从三槽 CPU 池取得可写输出。CopyResource 的目标图像与 staging owner 随 completion 保留，工作者收到成功后以 DO_NOT_WAIT Map 读取 RowPitch，复制 Y/UV 有效行并排除 padding。CPU 行复制不持有 immediate-context Enter/Leave，Map/Unmap 仍受原生保护。输出不再次裁剪、镜像、变换颜色，不接收 source NativeImage。

CpuImage 与三槽 pool 从已有 Apple exporter 提取为内部公共实现，保留强引用/Weak 排他写入和失败不发布语义，Mac API 不变。Windows 新增显式诊断 NV12 texture 初始数据（含 padding、不同 Y/UV 行内容）→GPU 完成→实际 exporter 的回归，检查字节、三槽占满、Weak 阻止覆盖、同 staging 复用与错误 device/spec 在 readback 前拒绝。GPU native 测试共享测试锁，避免全进程 completion 容量极限测试与其他 GPU fixture 互相干扰；产品容量不改。

Windows GPU all-targets Clippy、Mac GPU/Receiver all-targets Clippy 通过；Mac GPU/CPU export 9 项实际原生回归全部通过。Windows 新回归待下一轮 CI。前一轮 CI 34044898768 的 Windows 全部通过，包含 Renderer 几何与 NV12 target pool、release、MSI 和 smoke；这不是实际 Video Processor 画质或整个 CpuBridge 已完成的证明。

CI 34044898768 已全部通过，包含 Mac 最终打包。当前 CpuExporter 提交的 Windows 实际导出测试将由下一轮 CI 执行；整体 Next 与 Windows 主链路迁移仍未完成。


## Windows device 采用与 MF manager 归属

REQ-PICOO-GPU-004 / MEDIA-034 将 MF manager 从 GPU context 移入 DecoderDevice。Decoder 在 MFT type 协商前创建并绑定 manager，并持有它直到 transform 释放；固定 device 与不解绑软件重试不变。GPU context 的 renderer/exporter 路径不再初始化 MF，旧 device_manager getter 删除，不做转发兼容。

新增采用已有 device 的入口：GetCreationFlags 拒绝 SINGLETHREADED，IDXGIDevice adapter descriptor 拒绝 software，GetImmediateContext 复用原对象。WindowsRenderer::for_source、CpuExporter::for_image 可从原生图像创建一次对应 worker 的 context，不需要把 Decoder 的 Rust 类型塞入 FrameHub，也不会因为增加输出创建新 source device。

GPU 测试 helper 不再调用 MFStartup/MFShutdown；原 manager 正向 COM identity 回归迁到 Decoder，GPU 侧继续验证实际 device identity、panic 后锁释放，并增加已有 WARP/SINGLETHREADED 拒绝测试。真实硬件采用与主链路接入仍待后续验证，不能以私有 WARP fixture 代替生产成功路径。

静态验证：Windows GPU all-targets Clippy、MF windows-mf/test-codecs 库 Clippy 与文档检查通过，新增 manager/设备采用测试待 CI。

原生 sample 交付前还需闭合 MF runtime 寿命：MFStartup 必须持续到最后的 sample lease 释放，不能在任意消费线程析构时直接 MFShutdown。实现应让有上限的标准工作线程拥有 Startup/Shutdown，跨线程 lease 最终释放只通知关闭；COM apartment 仍留在调用 MFT 的 codec worker。此项尚未实现，不应提前将原始 MF sample 发布到 FrameBus。

CI 34045983650 已全部通过：Windows NV12 readback/Weak/容量/拒绝回归以及 Windows/Mac 最终产物门禁均成功。本次 device 所有权调整将进入新一轮 CI。

## MF 运行时保留边界

REQ-PICOO-NEXT-011 / FRAME-016 将 MFStartup/Shutdown 移到最多 16 个 cohort 的标准线程。codec worker 的 COM guard 保持线程亲和性；最后 runtime 引用仅关闭 channel，不在消费者或 MF callback 上执行 Shutdown。D3D11ImageLease 增加不透明 producer lifetime，字段释放顺序为 sample、texture、runtime；旧构造签名直接删除。尚未将 MF 输出发布到 FrameBus，完整原始 token 对应和原生描述迁移仍待完成。

新增 Windows 测试覆盖 codec apartment 退出后的 runtime 保留、异线程最后释放与 Shutdown worker 退出，以及 sample 先于 producer lifetime 销毁。Windows MF 库 Clippy 与 FrameHub all-targets Clippy 通过，原生执行待 CI；不据此宣称原生主链路已完成。

## Decoder 原始 token 与零到多输出

REQ-PICOO-NEXT-011 / MEDIA-025 直接替换仅含字节/配置的 Decoder 方法为 submit(DecodeSubmission)。不可变 token 保留提交时身份、PTS、时间及配置；压缩字节只借用，不随输出滞留。DecodeOutcome 返回分别携带原 token 的多张输出，refresh acceptance 仍指本次输入。删除未使用的 live flush API，reset 只丢弃旧状态；测试 EOS drain 保持在 MF 原生诊断内。

MF 对 ProcessInput 成功的 AU 登记单调 sample time，最多 16 个 pending token；每次取完当前可用输出，GetSampleTime 精确消费原登记。reset 不复用内部时间，未知/重复输出拒绝。Receiver 不再把当前 job 的配置贴到返回图像，逐图像检查 Decoder/连接/stream 身份，再按原配置发布。暂时无输出的成功提交不再被统计成 Decoder drop。Windows CPU 源存储仍待原生端到端替换，本提交不宣称 NEXT-011 或整个主链路完成。

Mac Decoder 16 项原生/配置回归通过；Receiver 媒体 18 项（含延迟双帧与三个旧身份门禁）、Decoder/loopback 7 项、实际 VideoToolbox→FrameBus→GPU→CPU 输出 1 项通过。新测试首次因诊断占位图不能容纳 64×32 布局而触发 panic，已改为直接提供合法 NV12 fixture 并要求 codec 无错误，避免旧输出拒绝测试错误地因 codec 崩溃而通过。Mac all-targets 与 Windows MF 库 Clippy 通过；新增两项实际 MF token/reset 测试待 Windows CI。MEDIA-025 因扩大到真实平台延迟输出，暂回到 implemented，不沿用旧较窄 verified 状态。

前一批 CI 34047409630 全部通过；MF runtime lease 提交 7cb7701 已推送，CI 34048304540 的 Windows 原生测试通过，最终产物仍在构建。该运行完成前不推送下一提交，以免取消有效验证。

CI 34049182494 的 Windows Desktop 74 项测试通过，但新增 MF token fixture 编译失败：canonical_access_unit 返回 Cow<[u8]>，fixture 声明 Vec<u8>。已在该测试数据所有权边界调用 into_owned；不是放宽 token 校验或替换原生测试。MF 两项新运行时验证尚未执行，不能把此编译失败描述成解码行为失败。

## Windows BGRA 目标与 NT 共享交接

REQ-PICOO-GPU-006/008 为 RenderSpec 增加独立 OutputFormat，Windows Video Processor 查询并选择 NV12 limited 或 BGRA8 RGB full G22/BT.709 目标，AlphaFillMode 保持 opaque。所有旧调用点明确选择 NV12；不是格式猜测或默认兼容。Apple backend 在创建原生资源前拒绝 BGRA，公共 CPU 输出池也拒绝 BGRA，防止不同字节布局进入 NV12 exporter。

BGRA 复用原三槽输出池，创建 SHARED_NTHANDLE + SHARED_KEYEDMUTEX 纹理并只创建一次 OwnedHandle。渲染前取得 key 0，SharedAccess 和原图像 owner 随 completion 保留到 GPU 完成；ReleaseSync 失败标记池槽不可复用且不发布结果。借出的共享句柄要求消费者持有原 RenderedImage 到 GPU 读取/释放 mutex 完成。AcquireSync 使用原始 HRESULT == S_OK，不能把正值超时/abandoned 当成功。

新增 Windows 原生测试使用两个独立诊断 device：BGRA render-target clear、锁持有时导入端超时拒绝、NT handle 导入、GPU copy、完成后 staging readback 验证 BGRA 红色字节；还检查持有 consumer lease 时三槽满、释放后复用，以及 CPU exporter 拒绝 BGRA。此测试尚待 Windows CI，且不是 Video Processor 硬件像素质量或 GPUI 预览端到端验收。

Windows GPU all-targets Clippy 通过；Mac GPU/Receiver/Desktop all-targets Clippy 与 10 项实际 GPU/输出格式回归通过，文档检查通过。未新增依赖库，使用已有 windows-rs、Video Processor、标准 OwnedHandle 与 GPU completion。GPUI 的共享图像描述、导入和读取完成 owner 尚待接入，源 Windows CPU Decoder/FrameBus 替换与全部 Next 项仍未完成。

CI 34049971485（a112d09）已通过 Windows 原生测试步骤：原 token/reset 两项真实 MF 诊断可以编译并运行；Windows/Mac 最终产物仍在执行。上一轮 34049182494 的其他平台已通过，Windows 因 fixture 返回类型失败跳过最终产物，不能计为完整 CI 成功。

CI 34050770806 的 Windows 在 Clippy 阶段被 Rust 1.98 新增的 chunks_exact_to_as_chunks lint 拒绝；跨 device 测试尚未运行。测试改用 as_chunks::<4> 比较 BGRA 像素，不关闭 lint。另校正本机验证范围：此前 Desktop 命令未带 gpui-ui，只能证明桌面非 UI 目标；后续 GPUI 补丁必须显式启用 gpui-ui 验证。

## Windows 显示读取 owner

REQ-PICOO-GPU-008 增加 WindowsDisplayReader：显示 owner 采用 UI 实际 device，按图像缓存 NT 共享导入与 shader view，重复重绘共用仍存活的读取锁。每次绘制复用已有 submit_owned 完成边界；原 RenderedImage 持有到实际读取完成，失败释放使池不可复用。忙碌以 false 返回且不调用绘制闭包，生产路径继续拒绝 software/SINGLETHREADED device。

新增实际双 device 回归覆盖写锁期间 busy、导入 view 的 device 身份、同图像尚有读取时重复绘制、失败图像拒绝及池拒绝复用。Windows GPU all-targets Clippy 本机通过，原生运行待 CI；这不是 GPUI 像素绘制或 Windows 完整预览验收。GPUI 框架接入仍在工作区开发。

启用 gpui-ui 的 Mac Desktop all-targets Clippy 已通过。78a4a07 已推送，CI 34051699070 正在执行，待其结束再推送下一批。本次 ADB devices 未列出设备；当前任务不依赖手机。

CI 34051699070 的 Windows 原生测试步骤已成功，BGRA 跨 device clear/共享/copy/readback 与池 lease 回归由 Windows runner 实际执行通过。完整构建、打包仍进行中；be68f37 的显示读取器测试尚未推送，不包含在该结果内。

## GPUI Windows surface 接入（验证中）

在现有 gpui-pre/gpui-pre-windows 0.3.3 增加窄 Direct3DSurfaceSource 接口、Windows PaintSurface/paint_surface 与原生 shader draw。GPUI 不持有业务事务、不导入 CPU 图像；供应者同步提供 UI device 上的 view 并保留读取完成 owner。draw_view 复用 PolychromeSprite shader 与 content mask，结束时解除 VS/PS 的图像绑定。源码包来源、校验和、许可证与升级边界写入 vendor 的 ORIGIN.json / README.picoo.md。

新增 Windows WARP shader 回归直接执行生产 draw_view：BGRA 原生纹理 clear 后绘制，左半裁剪区域输出红色、右半保持黑色；诊断 staging readback 验证像素，并检查 VS/PS 解绑、错误尺寸与错误 device 拒绝。测试已接入 xtask Windows 流程，尚未编译/原生执行通过，不宣称完整 GPUI 预览。完整应用仍待 Windows NativeVideoFrame 接线。

CI 34051699070（78a4a07）全平台已成功。be68f37 已推送，CI 34052549802 运行中；显示测试的异步回调收尾补充提交为 0c8d7d4，尚未推送。Windows GPU all-targets Clippy 对收尾补充已通过。

GPUI 默认特性本机 Windows 跨目标检查在资源嵌入 build script 因缺少 llvm-rc 失败，尚未到达框架代码验证。继续运行 standalone --tests --no-default-features 类型检查，仅排除 manifest 嵌入；Windows CI 保持产品默认特性和原生测试。与发布 tarball 比较，既有源码修改仅 core 的 surface/scene/window 接线和 Windows renderer 接线，新增实现与测试位于独立模块。

无 manifest 的框架类型检查发现并修复 ObjectFit 需要 DevicePixels（不是 Pixels）以及 GPUI glob 导入遮蔽 Rust 内置 test 属性的问题。上游 Windows PlatformWindow 的测试方法要求依赖同步启用 test-support；xtask 原生 shader 测试命令现在显式启用 gpui-pre-windows/test-support，本机继续验证该配置。没有删除原生测试方法来绕过依赖契约。

GPUI Windows 的 standalone --tests --no-default-features --features test-support 跨目标类型检查已通过，包含新 shader 测试。根 workspace 通过桌面直接依赖 gpui-kit/test-support 启用共同 GPUI 测试接口；cargo tree 已确认该测试命令选中本地 gpui-pre-windows 与桌面包。Windows 原生执行仍待提交后的 CI。

## CPU 输出共享 worker 与平台适配

REQ-PICOO-NEXT-029/033/034：将 Apple renderer/exporter 资源与 prepare 移到 output/apple.rs，需求请求、单槽最新帧、缓存、失效世代、IPC 发布和错误回报留在 CpuOutput。删除 MacCpuOutput 名称，不提供旧别名。此次仍只在 Mac 接入；Windows 替换原生源时应复用此 worker，而不是再建一套需求/失效调度。

Receiver all-targets Clippy 通过；实际 VideoToolbox→FrameBus→GPU→CPU 输出测试通过（1 passed），包含无 consumer 不导出、请求触发导出、同帧缓存及内容失效。CI 34052549802 的 Windows 原生测试步骤已通过，显示读取器 busy/重复同图像/失效池回归已实际执行；最终构建打包仍进行中。

## Windows 原生源到输出接线（工作区验证中）

REQ-PICOO-NEXT-011/016/025/029：Windows DecodedFrame 改为 NativeImage 与不可变原生描述，移除源 CPU stride/storage/pixel getter。硬件 MF 输出在已有 GPU 完成边界后保留原始 sample 和 runtime；GetOutputCurrentType 读取颜色、PAR、frame size、minimum display aperture，不 Map/Lock。协商保留 MFT advertised NV12 media type 的 aperture，缺失或冲突描述明确拒绝。原缓冲映射只在 test/test-codecs 诊断编译，诊断结果显式上传 NV12 原生图像；无软件生产回退。

Receiver Windows 切换 FrameBus/native_publish，方向作为剩余变换保留；CPU sink 复用 CpuOutput 的消费者请求/失效门禁，Windows adapter 才执行 Video Processor 与 exporter。桌面 Windows preview 生成 BGRA NT shared 目标，由 WindowsDisplayImage 实现 GPUI 资源提供者；桌面场景使用 SurfaceSource，不再把 Windows 视频上传 CPU atlas。预览和 CPU 输出按连接/Decoder 世代及规格绑定资源。

Windows Decoder 生产与 test-codecs 两种库 Clippy 已通过；完整 Windows Receiver/测试的本机交叉检查因缺少 Windows CRT assert.h 在 ring C 构建阶段失败，未到达产品检查，不算通过。Mac Receiver/Desktop（启用 gpui-ui）all-targets Clippy 在第一轮接线后通过，后续测试修正仍需重新检查。新增 MF metadata 测试与 Windows 整条原生源验收待执行；此批尚未提交，不能视为 native end-to-end 已验证。

诊断 NV12 上传同样改为复用 picoo-gpu 已有 submit_owned，完成资源在 CreateTexture2D 前保留，texture/sample/runtime 覆盖失败与取消。WARP factory 仅由显式 test-support 提供；生产 factory/adoption 继续拒绝软件 adapter。避免另建诊断事件调度器，也不把 CreateTexture2D 消费 initial data 误当成 GPU 初始化完成。

框架 CI 34053537489 的 Windows 因仓库精确锁定 proptest 1.6 缺少 GPUI test-support 的 RngSeed API 失败，shader 测试未执行。修复 6fa3372 已单独提交并推送，统一五个测试调用方的 proptest 版本；116 项现有回归通过、2 项忽略。CI 34054474529 运行中。原生源接线仍未提交；不要将上述 CI 归到此工作区变更。

CI 34054474529（6fa3372）全部成功；Windows 日志确认 native_surface_shader_draws_bgra_and_respects_clip 实际执行通过，覆盖原生 shader 像素与裁剪。该运行不包含尚未提交的 Windows Decoder→FrameBus→Preview 接线。

桌面预览删除旧 CPU BGRA 转换、缩放和 RenderImage 路径，以及直接 image/yuv/fast_image_resize/smallvec 依赖。Windows/macOS 共用仅持有 SurfaceSource 的 VideoSurface；GUI 模块只在这两个正式桌面平台编译。原生预览接线的 macOS gpui-ui all-targets Clippy 通过，预览测试继续验证。本机 ADB 当前未列出手机。

原生预览最终验证：macOS 独立目录运行的 preview_pipeline 5 项和 VideoSurface 1 项全部通过；Windows MF/test-codecs 库跨目标 Clippy 通过；文档链接检查零错误。完整 Windows Receiver/桌面及新增 MF 元数据测试仍待本批 CI，不把跨目标库检查作为 Windows 硬件执行证据。

## MF 原生裁剪尺寸协商

REQ-PICOO-MEDIA-031 / NEXT-025：移除 stream-change 枚举中要求原生 frame size 与 SPS coded size 完全相等的旧判断。协商保留 MF 类型，完成图像仍由 native_output 校验 actual allocation、可见尺寸、aperture、色彩和 PAR；软件诊断的 CPU 布局另由 buffers 严格检查。新增原生 64×64 allocation 对应 192×96 coded size 的回归，并验证超出 allocation 的媒体尺寸被拒绝。Windows 库 Clippy 通过，新回归原生执行待 CI。

## 已提交 AVC/HEVC 参数集统一校验

REQ-PICOO-BITSTREAM-005：CodecConfiguration 对已解析 AU 校验 codec 和逐个 VPS/SPS/PPS 的完整字节身份。AVC Decoder 删除自己的参数遍历并调用共同实现；它仍显式拒绝 HEVC 配置，直到该平台原生适配完成。此处不新增依赖，不解析完整 slice，也不允许带内孤立更新绕过配置事务。

硬件 AVC/HEVC fixture 新增两项回归，逐个参数覆盖原值接受、改变 payload 后拒绝、跨 codec 拒绝和拒绝后配置保持不变。Mac Decoder 16 项测试全部通过，包含真实 VideoToolbox 及拒绝冲突后旧 session 保持有效。bitstream/Decoder all-targets Clippy、Windows MF 库 Clippy、文档检查通过。完整 HEVC 原生解码与正式配置事务仍未完成。

28e2adf 的 CI 34055695761 已通过 Windows 原生测试步骤与 macOS 原生测试；Android、iOS、Rust/docs job 成功，Windows/macOS 最终产物仍在构建。该运行不包含随后提交的裁剪协商修复及共同参数校验。

CI 34055695761（28e2adf）全部成功。Windows Decoder 19 项、桌面 70 项、GPUI shader 1 项和 Receiver AVCC 1 项均执行通过；日志明确包含 native aperture 与缺失裁剪/色彩拒绝两项。该证据不覆盖真实显卡 Video Processor 画质、VCam 系统交接或四组合端到端。

HEVC SPS 候选与 CoreMedia API 的实证见 next-bitstream-dependencies 研究：新增 raw parameter description probe 从原始 VPS/SPS/PPS 重建平台格式，不补充尺寸或颜色，M4 八种 codec/size/fps 组合执行成功。CoreMedia 返回的尺寸不能替代原始 coded size，继续保留共享位流事实要求；尚未将未经有界审查的 SPS 解析器接入产品。

## HEVC 依赖整数边界

REQ-PICOO-BITSTREAM-002 / NEXT-025：在准确发布包副本上复现 scuffle-expgolomb 0.1.5 读取边界值时发生减法溢出 panic，两项负向回归均失败。修补保留上游读取 API，对超出 u64 的前导零/尾值及超出 i64 的正值返回 InvalidData；合法 u64::MAX 可读。来源、MIT/Apache 双许可、校验和与补丁范围保存在 vendor/scuffle-expgolomb，根 workspace patch 替换既有传递依赖，没有新增生产依赖。

修补后 bitstream 26 项、Mac Decoder 16 项共 42 项全部通过，包括真实 VideoToolbox 与 AVC/HEVC fixture；bitstream/Decoder all-targets Clippy 通过，文档检查零错误。该修补不等同 HEVC SPS 解析准入完成，SPS 块尺寸和扩展分配边界仍待处理。

位流库 aarch64 Android、aarch64 iOS、x86_64 Windows MSVC 三目标 cargo check 通过；不是这些平台的完整产品二进制或硬件解码验收。


## HEVC 源事实与 SPS 边界

REQ-PICOO-BITSTREAM-006：AVC/HEVC 共用 VideoSpsFacts/VideoColorFacts，删除旧 AVC 专用类型名和调用。HEVC 通过 Scuffle SPS 获取 coded/visible/PAR/色彩/chroma，准入单层 progressive Main 8-bit 4:2:0、零重排；拒绝扩展与尾部垃圾。原生 Decoder 暂仍显式走 parse_avc，不把这次事实解析当作 HEVC 产品链路完成。

Scuffle 发布包补丁在算术/分配前限制块尺寸、PCM、scaling matrix 引用/系数、SCC palette，使用 checked crop 并验证 RBSP 对齐零位。上游 SPS 测试按概念移入独立模块，源码文件均低于 800 行。46 项位流/Decoder 回归及上游 15 项回归全部通过；包含真实硬件 HEVC SPS、1920×1088→1080 crop、BT.709/PAR、截断、逐位变异和异常字段。相关 Mac all-targets Clippy、Android/iOS/Windows 位流库 check 通过；跨目标库检查不替代完整 Windows 产品 CI。

第一次 60 秒 fuzz 在 512 MiB RSS 限额下退出；报告显示主要存活分配是 libFuzzer 覆盖率/语料统计。保存输入回放 1,000 次成功，36ms，无超限。随后保持 sanitizer 默认行为、将测试进程限额设为 1024 MiB，完成 6,466,923 次/61 秒，峰值 RSS 554 MiB，未崩溃。不能把首次运行记为通过，也不能把有限 fuzz 解释为全部解析安全证明。fuzz 独立 workspace 明确引用同一 Scuffle 补丁。

### 2026-09-07：macOS 原生 HEVC Decoder 接入

- REQ-PICOO-NEXT-003/011/025：配置解析按实际 codec，VideoToolbox 使用原生 HEVC 参数集 API；完整 record 决定会话复用。新原生格式及会话创建成功后才释放旧会话。MF/OpenH264 仍显式拒绝非 AVC，避免公共解析扩展隐式开放未实现后端。
- M4 本地 Decoder 20 项测试通过：真实 HEVC Main→原生 IOSurface NV12、AVC/HEVC 往返切换及旧图像存活、相同配置复用、冲突配置不改变会话、原始 token 保留。CRA/RASL/RADL 明确拒绝，尚未开放相关恢复合同。
- 这仅完成 Apple 解码适配器的闭合 IDR 子集。端到端 offers/config 事务、Windows HEVC、移动编码配置、双 VCam、两类录像及全平台验收仍未完成。

### 2026-09-07：Windows HEVC 原生配置准备

- REQ-PICOO-NEXT-003/009/024：MF Decoder 按 codec 创建系统同步 MFT，HEVC 使用官方枚举和 Main profile；D3D11 准入按 AVC/HEVC profile，公共源事实验证覆盖全部 SPS。配置变更先在同设备新 transform 完成协商，成功后才替换。类型改名 MfVideoDecoder，无旧名称别名；原生类型协商、工厂和测试按职责分离。
- macOS 上 Windows Decoder library 的 GNU target check/Clippy 通过；未跨编译完整 Receiver。尝试 all-targets 检查被测试依赖 ring 所需的 MinGW C 编译器缺失阻断，不能记录为通过，Windows 测试交由原生 CI。
- 补充配置几何不匹配在 HEVC 原生激活前拒绝的常规测试。真实系统 HEVC 解码诊断测试明确标记 requires installed Windows system HEVC decoder，默认忽略；安装该组件的 Windows 主机必须显式执行，不以跳过代表成功。尚无 Windows HEVC 原生解码/硬件性能验收证据。

### 2026-09-07：HEVC CSD 配置适配与原生替换失败回归

- REQ-PICOO-BITSTREAM-007：新增显式 Annex B HEVC codec-config→标准 hvcC，复用已准入 SPS 和 Scuffle mux；参数集和 profile/tier/constraint/level 与 M4 原生 hvcC 对照通过。非参数 NAL、缺失集合、冲突集合和超长输入拒绝；重复同一参数集合可归一化。
- Bitstream/Decoder 53 项本地测试通过，包含新增 VideoToolbox 原生候选创建失败后旧 HEVC 会话和配置仍可用的测试；两 crate all-targets Clippy 通过。
- codec-bitstream sanitizer fuzz 加入原生 CSD 种子和配置 roundtrip：6,469,219 次 / 61 秒，峰值 RSS 529 MiB，无崩溃。仍是有限 campaign，不表示任意输入安全的完整证明。
- 移动端正式接口、Sender 配置和 Receiver 事务尚未接入这个 CSD 转换；该边界的通过不提升完整 NEXT-003/004 验收状态。

### 2026-09-07：Sender 配置记录所有权

- REQ-PICOO-MEDIA-035：StreamConfigParams 必须持有已验证 CodecConfiguration，不再有 Default 或 raw SPS/PPS 字段。codec/profile/level 由记录派生；Core 中 raw/Annex B 猜测删除。现有 AVC 平台适配在调用 Core 前建立记录；C 入口在读取/分配前检查参数长度和缺失指针，不将缺 PPS 当作另一个输入格式。
- 协议序列化仅接纳 30/60fps 和合法 quarter-turn，删除容忍性方向取整。无效源属性在匹配 IDR 到达时也不能提交事务；合法替换仍可继续提交。
- 本地 Sender 68、Receiver 106、FFI 11 项测试通过（合计 185）；Receiver 另有 2 项忽略，不计通过。此批保留 AVC 原生 FFI 签名，正式 HEVC 原生回调和 Receiver 配置准入仍待接入；没有兼容旧配置记录或协议版本的路径。

该批 Sender/Receiver/FFI all-targets Clippy 通过；Android aarch64 JNI library 使用本机 NDK 28.2 的官方 Clang/AR 编译检查通过。首次直接 cargo check 未指定 NDK 编译器失败，随后配置工具路径重跑成功。配置快照使用 Arc 共享不可变已解析记录，避免在事务枚举和每次快照克隆中复制参数集合。

原生 Decoder 提交 fdfda46 的 CI 34067849022 全平台成功；Windows 的系统 HEVC 诊断仍为显式忽略项，不能把该 CI 结果当作 Windows HEVC 实测通过。配置适配提交 01a560a 已推送，CI 34068821610 执行中。

### 2026-09-07：Android 配置随原生 AU 交接

- REQ-PICOO-MEDIA-036：MediaCodec callback 按自身 generation 持有 codec/fps/尺寸/标准记录，AU 在回调时携带该快照进入有界队列。删除全局 raw 参数集和单独的 Android setStreamConfig JNI；UI 配置请求等待原生关键帧提交，不能抢先覆盖排队媒体的历史配置。
- JNI 的 parseCodecConfiguration 接纳显式 AVC/HEVC Annex B CSD，返回 avcC/hvcC；submit 接受显式 codec/fps/record。传入 JNI 和从 MediaCodec 复制前限制 AU 2 MiB、CSD 64 KiB；队列 byte budget 包含配置记录。每个 generation 的首次配置提交不依赖 UI dirty flag 是否已被旧 AU 消耗。
- Android 80 项 JVM 测试通过；instrumentation 编译通过，真实硬件 CSD 测试已扩展到两 codec，但 ADB 当前无设备，未运行真机。NDK 28.2 JNI all-library Clippy 通过。assembleDebug / assembleDebugAndroidTest 包含重新构建的 release JNI，完整构建成功；符号检查确认新 parseCodecConfiguration 和 submit 存在，旧 parseAvcCodecConfig / setStreamConfig JNI 不再导出。
- 生产 MediaCodec 选择仍为当前 AVC 请求；这批完成配置载体和所有权，不宣称 HEVC/60fps 全链路已启用。iOS 原生回调、Receiver 双 codec 配置准入和 offer 选择继续待办。

同批 macOS FFI all-targets Clippy 通过；01a560a 的 CI 34068821610 全平台成功。

### 2026-09-07：iOS 原生记录与空配置删除

- REQ-PICOO-MEDIA-037：直接携带 CoreMedia 的 avcC/hvcC atom，原 AU 不再重复追加 SPS/PPS；C 回调接纳显式 codec/record。删除连接前的空配置、prime 空参数、独立配置 setter，以及无产品调用方的 raw 参数提取 C ABI。
- SenderMediaPipeline 只保存已成功提交的配置和 encoder generation；镜像是待提交意图，新 generation 即使 record 字节相同仍需要配置提交。Core 状态变更前校验原生记录和 AU 的字节容量。
- cargo xtask build ios 成功：device/simulator Rust staticlib、C ABI smoke 链接、XCFramework、ARM64 Simulator App。cargo xtask test ios 成功：Swift/C handle、Keychain 与现有场景回归，以及 CoreMedia format atom codec/record 配对与缺失配置拒绝。新增 Swift 断言首次触发冗余 require 编译错误，修正后通过。
- 这些是模拟器与原生 API 边界证据，不是 iPhone 摄像头 HEVC、60fps 或热稳态验收；VideoToolbox 生产编码器仍请求当前 AVC，双 codec 选择继续待办。

最终 iOS device/simulator/App 重建成功；生成的 C 头已移除旧 setter/extractor。Swift result bundle 报告 15 passed / 0 failed / 0 skipped；删除已废弃 extractor 测试后，Rust FFI 10 项通过。

### 2026-09-07：原生关键帧提示准入

- REQ-PICOO-MEDIA-038：Android JNI 与 Apple C AU 入口先解析实际图像类型，再核对原生 keyframe hint；AVC/HEVC IDR 可提交，delta 不可冒充随机访问点，CRA/RASL/RADL 在闭合 IDR 合同内明确拒绝。检查在 Core 状态修改前完成；四字节长度输入保留借用，Annex B 仅转换一次。
- 本地 FFI 13 项测试全部通过，包含真实两 codec IDR、相反提示、伪造 delta 标记和 HEVC leading-picture 拒绝。两目标 FFI Clippy 已通过；不等同原生摄像头完整配置选择或端到端验收。当前 adb devices 为空。

### 2026-09-07：提交前源几何一致性

- REQ-PICOO-MEDIA-039：共同 SPS 解释从 Decoder 移入 CodecConfiguration，复用已有有界 AVC/HEVC 解析；Sender 序列化与 Receiver 状态替换前验证可见尺寸，Decoder 不再维护重复的参数集合遍历。
- Sender 69、Receiver 106、原生 Mac Decoder 21、bitstream 33 项通过，合计 229 项；Receiver 另 2 项忽略。最初 Receiver 两项镜像/旋转测试使用 720p 参数集配 4×2 声明，被新检查拒绝；改为尺寸匹配的真实 64×64 配置后重跑通过，仍保留实际镜像像素和原生变换描述断言。
- 相关 all-targets Clippy 和文档检查通过。该检查只证明参数集与声明几何一致，不替代相机实际 fps、颜色与完整硬件 offers 准入。

### 2026-09-07：iOS 请求帧率进入真实相机配置

- REQ-PICOO-MEDIA-040：采集服务从相机 format 表选择匹配尺寸、8-bit 输入与请求 30/60fps 的同一条目，显式设置 activeFormat 和相同 min/max duration，并关闭受支持格式的自动降帧率。旧 preset 与固定 30fps helper 删除；相同尺寸不同帧率不复用旧配置。
- CameraCapture.swift 原为 798 行，按 AVFoundation 串行服务与 MainActor 界面模型拆分，未更改外部 UI skill。构建第一次因新 Xcode 文件标识与既有配置标识冲突失败，改为独立标识后 cargo xtask build ios 完整成功；cargo xtask test ios 模拟器回归成功。
- 当前没有真 iPhone 采集验证，不将格式表和属性设置视为实际 60fps、颜色或 encoder/decoder offers 交集已经验收。

7a2eaa2 的 CI 34070420985 全平台成功；51d191f、74e9fcf 已推送，CI 34071472188 执行中。

### 2026-09-07：iOS 编码完成保留原输入事实

- REQ-PICOO-MEDIA-041：VideoToolbox 的 sourceFrameRefcon 关联原输入快照，删除回调读取可变最新方向/码率的路径。十六项有界登记、单调不复用 ID 和幂等取消覆盖异步/同步回调；原生丢帧发出恢复事件，新配置 start 关闭旧 session。
- cargo xtask build ios 原生链路成功，cargo xtask test ios 成功，新增乱序完成保留旧方向/码率，以及容量、取消、重复/未知 ID 不消费其他输入两项回归。模拟器共 17 项；不替代真实 VideoToolbox 硬件并发与持续帧率验证。

### 2026-09-07：Android JNI 显式提交结果

- REQ-PICOO-MEDIA-042：JNI 返回 Accepted/Rejected/Error 对象，删除整数成功位掩码、负数分支解码器及其三项旧 JVM 测试；ViewModel 按结果分支处理配置与关键帧事实。原生构造器使用 AndroidX Keep，JVM 构造失败保留异常，不伪装接受。
- NDK JNI Clippy 通过；完整 assembleDebug/assembleDebugAndroidTest 含新 release JNI，77 项 JVM 测试全部通过。只读启动本机 API 36 ARM64 AVD，安装这批 APK 后执行打包 JNI 错误对象、旧 generation 拒绝对象及已有尺寸门禁，共 3 项 instrumentation 全通过；测试后关闭模拟器。
- 当前没有物理 ADB 手机；上述证明 JNI 对象交接，不替代小米硬件编码/配对后的成功媒体链路或 R8 正式发布验收。

### 2026-09-07：源请求加入 codec 与 fps

- REQ-PICOO-MEDIA-043：Core 事务使用无 Default 的 SourceFormat，C/JNI 请求和 directive 带显式 codec/fps；配置在暂存及提交前匹配所有请求字段。恢复从旧配置记录取 codec/fps，缺少记录时拒绝恢复。移动端当前执行器仍明确请求已有 AVC/30，不宣称完整 offers 或 HEVC/60 用户选择已启用。
- Rust Sender 70、Receiver 106、FFI 13 项通过，Receiver 另 2 项忽略。新增相同高度/generation 下 codec、fps 冲突保持旧快照/epoch/控制序号，以及正确 HEVC/60 配置替换可提交的 Core 回归。旧非法 fps 回归因更早的请求匹配而返回 Protocol，更新错误类别断言后通过。
- iOS 完整 build 成功；Swift 模拟器 18 项通过。新增测试最初漏写 throwing 调用的内层 try，随后又错误地期待本地 apply 出现在仅暴露 recovery effect 的 getter；修正测试保留既有 owner 语义后通过。
- Android 完整 APK/JNI/JVM 构建成功，77 项 JVM 通过。只读 API 36 ARM64 AVD 的提交结果、显式源参数准入和尺寸合同共 4 项 instrumentation 通过；首次新增测试同样误期待本地 apply 作为新 effect 返回，修正后通过。测试后关闭 AVD。macOS 与 NDK FFI all-target/library Clippy 通过，文档检查通过。

74e9fcf 的 CI 34071472188 全平台成功；799705a、57b9f62、ba609a8 已推送，新 CI 执行中。

### 2026-09-07：能力匹配不再猜测 AVC/30

- REQ-PICOO-MEDIA-044：能力可先于源请求到达；没有源格式时保留已验证 offers，最大高度为未知 0。显式待处理请求优先于旧源快照，按同一完整条目匹配 codec、尺寸、fps、profile、8-bit 420 与既定 BT.709 limited；新请求不匹配时不消耗身份，合法重试清除对应错误。
- 能力测试按职责从 epoch 测试文件移出。Sender 72、Receiver 106 项通过，Receiver 另 2 项忽略；相关 all-targets Clippy 与文档检查通过。覆盖仅 HEVC/60 的先到能力、跨尺寸/帧率/色彩借用拒绝和合法显式请求。
- 本批完成准备请求的 Decoder offer 匹配，不宣称相机/编码器原生 offers、实际 SPS level/颜色/每 AU 预算的完整提交准入已完成。

### 2026-09-07：iOS 原生双 codec 与可验证 VUI

- REQ-PICOO-MEDIA-045：VideoEncoderConfiguration 显式包含 codec，复制配置保留该值；VideoToolbox 按 AVC High/HEVC Main 创建硬件 session，HEVC 关闭 Open GOP。原输入快照的 codec 与回调原生 atom 一致，submit 明确在 callbackQueue 上执行。
- 同一生产 Swift 在 M4 上执行八组 codec/尺寸/fps 配置，初次全部为 IDR 但 SPS 无颜色信息；仅给输入设置 attachment 不够。增加输入/目标的 420v BT.709 元数据验证及 VT 压缩颜色属性后，共享 bitstream 检查八组的闭合 IDR、参数集身份、可见尺寸、BT.709 limited 全部通过。未知颜色的合成输入明确拒绝。两 codec 的 1080 实际编码高度均为 1088。
- 新 harness/检查器及命令保存在 verification/native-media 和 bitstream examples。首次提取 submit 时遗漏 nonisolated，被 iOS 的 MainActor 默认隔离构建拒绝；补充显式隔离与队列断言后，完整 cargo xtask build ios 与 18 项 Swift 模拟器测试通过，bitstream all-targets Clippy 与文档检查通过。
- 这些是 M4 原生硬件与模拟器证据；不等于 iPhone 相机、持续 60fps 或完整 offers/用户选择。CameraCaptureModel 仍明确选择 AVC/30，正式 UI 选择待完整能力交集接线。

### 2026-09-07：Decoder offers 的存储与可见尺寸

- REQ-PICOO-MEDIA-046：根据同一生产 Swift 的实际 AVC/HEVC 1080p 输出，修正 VideoFormat 对 1920×1088 存储的拒绝。可见图像继续限定正式尺寸；最低 codec level 由编码工作量决定，不能用更小 crop 降低。
- 请求准备可匹配同一条含 padding 的 offer，最终 Capabilities::supports 仍精确区分存储尺寸与 crop 原点，未放宽完整配置相等规则。Protocol 26、Sender 73 项回归全部通过，all-targets Clippy 与文档检查通过。
- 这批修正能力模型，不宣称 Receiver 原生 offers 探测或每个实际 SPS 的最终能力准入已接线。

ba609a8 的 CI 34072658310 全平台成功；35f40a0、f514b5a、475b5b3 已推送，新 CI 执行中。

### 2026-09-07：Android 编码请求与恢复共用完整 profile

- REQ-PICOO-MEDIA-047：MediaCodecVideoEncoder 从该 generation 的 CaptureProfile 获取 codec/尺寸/fps，Core 源请求使用同一 codec/fps；删除旧 MediaCodecH264Encoder/h264Encoder 名称，无别名。
- 恢复保存并还原完整 CaptureProfile，含镜头与 codec/fps；与 Core 的恢复指令不一致时拒绝。初始界面仍选择 AVC/30，完整原生 offers 和 HEVC/60 用户选择未接线。
- 完整 assembleDebug/assembleDebugAndroidTest 和新 JNI 构建成功，77 项 JVM 测试全部通过。当前无物理 ADB 设备，不宣称小米原生双 codec/恢复验收完成。

### 2026-09-07：统一移动端公开媒体提交入口

- REQ-PICOO-MEDIA-048：删除没有产品调用方的 C/JNI 独立 ingest、flush、started 及 Kotlin 声明，无兼容别名。保留完整配置快照与 AU 的原子提交、失败上报和恢复控制；Core 内部状态机仍拥有分解事实处理。
- FFI 13 项回归通过；macOS all-targets 与 Android NDK library Clippy 通过。Android 完整 APK/JNI 构建及 77 项 JVM 测试通过；iOS 完整构建和 Swift 模拟器测试成功。当前三个 Apple 生成头均不再声明被删除 C 入口。
- 475b5b3 的 CI 34074184001 全平台成功。本批接口清理不代表完整能力探测或真机媒体成功验收。

### 2026-09-07：实际配置映射与小米原生跨设备验证

- REQ-PICOO-MEDIA-049：协议层复用现有 bitstream 有界解析器，把真实记录转换为完整 VideoFormat；StreamConfig 声明与实际记录逐项核对，缺失颜色拒绝。protocol → bitstream 是单向依赖，不引入平台或新的 codec 库。
- 28 项初始协议回归中，“所有 avcC 截断都非法”的测试假设不成立：尾部可选扩展省略后仍是合法记录。改成真实缺失颜色样本的拒绝断言。新样本验证尺寸/crop、颜色、标签冲突、精确 offer 与 AU 预算。
- 小米 15（24129PN74C / Android 16）ADB 真机重装调试包后，NativeCodecContractTest 和 FFI/source 合同共 5 项通过；测试包新增仅保存 Canvas 合成样本，独立再跑硬件合同通过。AVC/HEVC × 720/1080 × 30/60 共八组，实际组件 c2.qti.avc.encoder / c2.qti.hevc.encoder，三张不同 PTS 输出，原生 profile/尺寸/fps/BT.709 limited 与 JNI 标准记录检查通过。
- 八组小米码流全部通过共享位流 IDR/颜色/几何核对；首次 Mac 验证在 HEVC 720p 遇到实际 1280×736 存储，被原格式表误拒绝。支持有界 736 存储后，将最低 level 从可见档位表改成 H.264 Annex A macroblock / H.265 Annex A luma 工作量计算，保留 AVC 736 超出 level 3.1 的回归。协议最终 30 项通过。
- Mac 生产 Decoder 成功解码 Apple 和小米各八组样本；每张确认原始 token、可见图像与原生 NV12 输出，HEVC 736 和 1080 的 1088 padding 均正确。样本含来源记录，保存在 picoo-testkit；不作为其他运行设备能力证据。
- 完整 Android APK/JNI/JVM 构建成功；Protocol/Bitstream/Decoder all-targets Clippy、文档与格式检查通过。仍不宣称实际 Camera2 采集、持续 60fps、热稳态、网络全链路或产品 offers 广告已完成。

### Camera2 真机验证基础设施

按 testing-setup 检查现有测试栈：JUnit 4.13.2、AndroidX runner 1.6.2/ext-junit 1.2.1、Compose instrumented tests 已存在；生产对象采用构造器注入 EncodedFrameListener，无需新增 DI/Mock 框架。相机测试需真实硬件与前台 UID，因此在 debug source set 添加受系统 DUMP 权限保护的空 Activity，使用既有 AndroidX lifecycle monitor 与 shell 启动；release 不包含该入口。测试只保留有界时间戳和配置元数据，不保存真实相机图像。结果继续维护本实施记录，避免新增重复测试策略文档。

### 2026-09-07：Camera2 删除静默降级并验证实际帧率

- REQ-PICOO-MEDIA-050：删除 CaptureSizeSelector 的 1080→720 回退、空表猜目标及不足输入的隐式放大；按旋转后几何选择最小足够输入。Camera2 只使用相同 stream map 中已知且满足请求 fps 的 min duration，并要求所选固定 AE fps range 实际存在；删除“筛空后恢复全部候选”和修改 CaptureProfile 的路径。
- 核对 Android 16 AOSP 的 StreamConfigurationMap / CaptureRequest：min duration 为0表示未知；所选 AE range 应来自 characteristics；多 stream 最低时长取各自最大值。这些是静态候选证据，不替代实际 Camera2 session 或持续帧率。复用平台 API 与已有自有 geometry policy，不添加库。
- 原六项尺寸测试改为三项明确请求准入回归，最终 74 项 JVM 全通过；完整 Android APK/JNI/test APK 构建成功。
- 新前台测试首次被小米后台启动策略拦截，主动结束挂起 instrumentation；改为 shell 启动受 DUMP 权限保护的 debug Activity 后，重装造成相机授权失效，补授权后又发现 harness 漏掉 JNI ensureLoaded。修正测试环境后正式相机测试成功（1 项覆盖八组合，36.971秒）。这些失败不被记作硬件编码失败。
- 小米15 后置 camera0 横向输入：AVC720/1080 的30fps分别30.040/30.040，60fps分别60.106/60.133；HEVC720/1080 的30fps分别30.048/30.040，60fps分别60.130/60.154。测量原生编码输出的唯一 PTS，先忽略1秒启动，再取约3秒；profile 与请求一致，无静默变化。未保存相机图像。
- 用户指出测试入口白屏，已添加中文测试提示和保持亮屏，仅 debug source set；测试结束自动关闭，并已恢复手机正式 MainActivity。最后构建覆盖提示修改；无需因文字修改重测硬件。前摄/竖持、长时间热稳态、网络以及完整产品 offers 仍待验。

9832582 的 CI 34075347404 全平台成功；b5cf956 已推送，CI 34076167154 执行中。

### 2026-09-07：实际编码事件完整准入与 HEVC tier

- REQ-PICOO-MEDIA-051：Sender 获取 Decoder offers 后，在绑定 generation、暂存配置或发包前，按实际 VideoFormat 和 record level/本 AU 大小匹配同一条目；准备请求命中不等于实际提交准入。已有 CodecConfiguration 直接映射格式，不重新复制/解析 hvcC。色彩序列化明确来自 BT.709 VUI，不能把 unknown/full 源一律标成 limited。
- 小米 HEVC 720p 的实际 profile 是 Main，但 tier 为 High。给 VideoFormat 增加显式 AVC/HEVC Main tier/HEVC High tier，缺省非法；SourceFormat 准备时允许同一候选的合法 tier，提交时严格相等。High tier 的 level 下限为4。bitstream 复用既有有界 Scuffle SPS parser，逐个核对 hvcC profile/tier/level，伪造 header 被拒绝。
- Bitstream 33、Protocol 31、Sender 74、Receiver 106、Mac Decoder 21、FFI 13 项通过，Receiver另2项忽略。新增实际存储/色彩/tier/AU预算拒绝保持 generation、配置和控制序号，合法精确重试成功；SPS与header tier/level冲突回归通过。首次正向重试测试漏建 MemoryTransport 连接，返回 NotConnected；补上真实内存连接后通过，未改变产品逻辑绕过连接。
- 全部相关 Clippy、文档/格式检查及最终 Android完整APK/JNI、iOS完整构建成功。当前 Receiver 仍使用待替换的保守能力声明，尚未以完整原生探测结果广告，也不宣称产品 HEVC/60 网络入口已开放。

b5cf956 的 CI 34076167154 全平台成功。

本批最终 `cargo xtask test ios` 成功；Swift/C ABI 模拟器回归继续通过。

### 2026-09-07：原生 Decoder 完整格式探测

- REQ-PICOO-MEDIA-052：复用当前平台 Decoder 工厂及正式提交 API，最多16个自有闭合IDR候选；标准记录经共享协议/位流校验，原生返回正确token和图像才生成offer。每项释放输出并reset，不把探测图像交给FrameBus；reset或身份不变量失败中止，不重建另一设备后混合证据。
- 无新媒体依赖；输入资产由原生Decoder模块的probes目录拥有，迁出testkit，生产无需依赖测试crate。样本来源和合成属性保留。相同完整格式的多条成功证据仅合并level上限，不借其他存储或tier字段。
- M4正式factory的实际probe返回11条有效offer，覆盖八种codec/正式尺寸/fps及小米736/High tier差异；24项Decoder测试通过，Clippy通过。全拒绝backend不继承样本能力，reset失败不继续；这些探测不表示持续fps或热稳态。当前仅模块与诊断程序，Receiver worker/能力发送接线继续处理。

### CI 补充：Linux 测试码流必须明确颜色

860cd08 的 CI 34077474767：Windows、macOS、iOS 成功；Rust与Android job在共享Linux Receiver测试中失败，9项旧OpenH264样本没有VUI色彩，严格Sender门禁报 missing source color。读取两个失败job的原始日志后，使用已锁定OpenH264 0.9官方 EncoderConfig::vui(VuiConfig::bt709()) 给7处测试编码器显式配置颜色；不放宽生产校验。修复单独提交922bfc4并在上一轮终态后推送，等待新CI。

本机已有Docker Linux服务，已启动隔离ARM64 Rust容器复核Receiver Linux测试；仓库只读挂载，构建缓存使用本任务专有volume，不替代GitHub Actions。


### 2026-09-07：Receiver 原生能力接线与双 codec 网络准入

- REQ-PICOO-MEDIA-053：原生 worker 创建实例后先 probe，再处理直播队列；owner 只收完整能力，探测不增加直播解码计数或发布源帧。Pending/Ready/Unavailable 明确区分；早到配置经实际记录验证后最多保留一个，绑定 transport session/control generation，旧 epoch 不覆盖新等待项，断连清除。Ready 才发送真实 offers，删除硬编码 AVC/30 与未探测的尺寸声明。
- StreamConfig 复用完整实际格式校验，AVC/HEVC、coded padding、crop、tier/level、色彩/fps 必须匹配同一条 offer；只在准入成功后修改 revision、clock/recovery 与源配置。每 AU 另受该条目的字节预算限制。reset/线程 panic 后停止该实例并作废证据，清理连接与输出，不静默换 Decoder 沿用旧能力；自动重新探测/设备恢复仍属独立工作。
- Sender 每次连接等待新能力后发送预存配置；不支持的预存格式保留为显式请求，不能先发给 Receiver 再被断连。完整原生事件在证据未到达前不能绑定 generation、暂存配置或发送媒体。重连回归明确覆盖新证据，不添加兼容路径。
- 测试注入的 Decoder 使用独立完整测试 offers，只在 test/显式 loopback diagnostic 下编译；生产调用原生 factory。新增 Mac 完整网络合同使用真实 factory/probe，Apple/小米各八组合经过 PCP offers、完整 NativeEncoderEvent、QUIC、原生 VideoToolbox 与 FrameBus，16组合全部通过；不把探测帧当网络帧。
- macOS Receiver116项（另2忽略）、Sender75项、FFI13项通过；Linux隔离ARM64容器Receiver121项（另2忽略）、Sender75项通过，Mac/Linux相关Clippy通过。旧160×120/64×64与480p网络fixture改为正式720p/1080p；合成空间复杂度固定为有限色块，镜像测试只传四个标记字节，保留原像素语义与延迟/丢包阈值。零填充的伪filler会在Annex-B规范化时消失，改成真实filler RBSP并断言规范化后仍足以超出FEC恢复能力。旧HEVC level3.1样本不能冒称60fps，事务测试改用实际720p60原生记录。
- macOS Receiver/Camera Extension构建与打包、Android完整APK/JNI/JVM、iOS完整构建与Swift/C ABI测试成功。GUI启动后本机采样显示正在等待系统Keychain授权，已请用户在系统界面允许；本批不据此声称新包手机到Mac直播已验收。未完成Windows原生HEVC设备、持续fps、热稳态、完整手机配置选择或全部NEXT验收。

922bfc4 的 CI 34078499965、2d8d9ab 的 CI 34079517343 全平台成功。本批等待提交后的CI；不以本机结果替代对应runner。

本批最终小米15安装新APK后，NativeCodecContractTest/EncoderSubmitContractTest/SourceHeightContractTest共5项通过（硬件编码合同含8组合），结束恢复MainActivity。第一次JNI合同发现“等待能力”被误报为Error；改为Core正常未接受结果，既不绑定/发送，也保持JNI Rejected与真实Error的区分。Sender75项回归和Clippy继续通过。构造器panic另有终态与队列关闭回归，最终Mac Receiver116、Linux Receiver121项通过。
