# ARCH-PICOO-MEDIA-002：原生媒体与独立多输出边界

状态：planned · 决策日期：2026-09-06

## 场景与意义

用户以手机作为电脑摄像头，同时预览和保存视频。各输出共享源内容与时间身份，但不能因会议客户端、窗口或磁盘变慢而相互阻塞。架构优先长期维护简单，不维护旧协议、FFI、配置或 IPC 的兼容路径。ALPN 固定为 `picoocam`；协议、FFI、IPC 不引入版本号或版本协商，直接修改当前契约。流/资源 generation 仅用于生命周期安全，不是软件版本。

来源：[已采纳的产品方案](../../product/picoo-camera-next-v2-gpu-cpu-output-2026-09-06.md)；关联 PUC-004、PUC-005；追溯：[Next 需求](../requirements/next-media.md)。本文是目标契约，planned 不表示平台实现完成。

## 范围与职责

- `picoo-bitstream` 拥有 AVC/HEVC 位流解释、参数集、随机访问与平台格式适配；不依赖 packet、协议、GPU、UI 或软件解码器。`picoo-packet` 只拥有分片/FEC/重组，不再导出 AVC helper。消费者直接依赖 bitstream，不设旧 API 转发。
- Decoder 能力以完整 codec/profile、图像布局、帧率、色彩组合表达；每组合独立记录标准 level 上限和 AU 预算，不能跨条目拼接能力。源码流实际 level 可低于同组合的 Decoder 上限，原生适配器负责提供实际准入证据。
- Sender/Receiver owner 拥有配置与恢复事务。codec 工作者只报告携带原始 token 和提交时不可变配置快照的事实；完成帧的方向、镜像和色彩不得从 owner 当前配置重建。每 AU 在同一 Decoder generation 最多提交一次。
- `picoo-frame-hub` 拥有不可变原生源帧、身份、描述及 FrameBus。native image 不含 CPU 像素变体；资源引用释放与 GPU 完成分别管理。
- `picoo-gpu` 拥有平台 context、资源池、统一 RenderSpec 与输出专用 exporter。FrameHub 不反向依赖它。
- Preview latest-only；虚拟摄像头使用独立 SampleClock；处理后录像订阅有界有序原生帧；原码流录像在 live scheduler/Decoder 之前接收完整 AU。
- 每个摄像头 sink 独立验证 GpuNative/CpuBridge。CPU 输出只做目标 GPU 图像的物化、布局复制与必要上传，不拥有 codec/连接事务，不反向发布到原生帧总线。

## 约束

AVC/HEVC 使用真实硬件能力准入，正式配置为 720p/1080p × 30/60fps，默认准入后的 1080p60；不静默降低源配置。软件 codec 与 CPU 预览退出产品路径。

输出切换只推进该 sink 的 backend generation，保留源配置、正常 Decoder、其他输出与采样时钟。权限失败、非法契约、硬件 codec 缺失和源 GPU 丢失不能通过 CpuBridge 绕过。无 CPU demand 不持续导出；所有池、任务和缓存有容量、期限及隐私清理边界。

Windows 的共享句柄和 CPU 输出 ring 是不同的交接后端；macOS 两种准备方式都使用合法 CMIO sink/source。必须分别验收两后端真实系统 sample，不能用 CPU 成功代替 native 完成。

## 对旧规范的替代范围

本契约替代旧规范中 H.264-only/30fps、自动降低源尺寸、公共 CPU NV12、CPU 预览、跨平台统一 CPU ring 和不含录像的目标约束；旧验证记录仅说明旧实现。未冲突的配对、安全、会话所有权与有界恢复继续成立。删除旧实现随对应端到端替换进行，不建立并行兼容产品。

## 复用与依赖判断

首个边界调整直接移动仓库已使用且有测试的 AVC helper，不新增解析算法或第三方依赖，保留现有行为测试；其运行依赖只有 Rust std，继承 workspace edition/license，四平台均可编译，无新增二进制依赖体积。现有 packet 同时依赖协议与 FEC，Decoder 为了位流转换依赖它属于错误边界，因此拆出。

完整 HEVC/配置记录解析实施前必须独立核对成熟包与平台 SDK 的当前 API、维护、许可证、MSRV、平台和体积；本次未作选型，也不以已有 AVC helper 作为自研 HEVC 的理由。GPU/codec/mux 使用方案列出的 D3D11/MF、Metal/VideoToolbox/AVFoundation/CMIO 候选，接入前按锁定 SDK 验证。
