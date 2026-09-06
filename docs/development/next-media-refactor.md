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
