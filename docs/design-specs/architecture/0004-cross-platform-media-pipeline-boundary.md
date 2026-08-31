# ARCH-PICOO-MEDIA-001: 跨平台媒体采集、编码与解码边界

Status: planned
Source: product PRD V1.0 / PUC-004 / PUC-005

## 背景

Picoo Camera 第一版固定 H.264 480p30 / 720p30 / 1080p30。媒体路径涉及四套平台原生 API，但语义必须一致：硬件优先、低延迟、动态码率、正确方向、镜像分离，以及 Receiver 侧单次解码、多路消费。

## 架构决策

### Sender：Android

```text
Camera2 Capture Session
  ├── Local Preview Surface
  └── SurfaceTexture (OES)
        ↓ EGL/GLES 旋转 + 中央 cover 裁切
        ↓ 固定横向 16:9 MediaCodec Input Surface (H.264)
        ↓
Rust Sender Core (Packetizer / Session / Rate Control)
        ↓
QUIC Datagram
```

- 优先硬件编码；使用 `MediaCodec.createInputSurface()`，由平台 EGL/GLES 合成器把 Camera2 OES
  纹理写入编码器，不让原始像素跨 FFI。合成器必须与 Camera2/MediaCodec generation 一起创建、
  失效和释放，不得在旧编码器 Surface 上继续交换缓冲。
- Android 在编码前按传感器方向、显示方向和前后摄像头计算唯一变换，输出始终为已经直立的横向
  480p/720p/1080p 16:9。横持尽量使用完整画面；竖持在直立空间中取中央 cover 区域。编码后的
  `StreamConfig.rotation` 固定为 `0`，Receiver 不为旧 Android Sender 保留二次裁切兼容。
- 竖持裁切时，Camera2 源的短边必须优先覆盖编码输出宽度，并受目标 FPS 的最小帧时长约束。
  本机 TextureView 使用全屏 center-cover，避免因手机屏幕与相机源比例不同产生黑边；不叠加容易
  误解的小型构图参考框。电脑端仍由 EGL 合成器独立执行中央横向 16:9 输出，因此竖屏本机预览
  是相机控制取景器，不声称与电脑端逐像素同构。
- MediaCodec 的释放、创建、配置和启动必须在同一 codec 线程串行执行；旧实例完全释放后才能创建新实例，以兼容只支持单个硬件 H.264 encoder 的设备。
- 第一版不使用 CameraX Recorder 作为实时传输核心。

### Sender：iOS

```text
AVCaptureSession
  ↓ AVCaptureVideoDataOutput
  ↓ VTCompressionSession (H.264)
  ↓ Rust Sender Core
  ↓ QUIC Datagram
```

- VideoToolbox 提供硬件编码与低延迟 H.264 能力。

### Receiver：Windows

```text
H.264 Access Units
  ↓ Media Foundation Decoder
  ↓ D3D11 / NV12
  ↓ FrameHub
```

- 优先硬件解码。
- Media Foundation 输出的可见高度与底层 allocation height 不等价；例如 1080 行可由 1088 行
  宏块对齐分配承载。进入 FrameHub 前必须按实际 row pitch 和 allocation height 分别复制可见 Y/UV
  行为紧凑 NV12，不得仅由总字节数反推一个横向 stride。

### Receiver：macOS

```text
H.264 Access Units
  ↓ VideoToolbox
  ↓ CVPixelBuffer / NV12
  ↓ Metal（如需）
  ↓ FrameHub
```

- 使用 `objc2-video-toolbox`、`objc2-core-media`、`objc2-core-video` 的生成式 Rust 系统框架绑定，不增加 Swift/C 胶水层、CMake 或软件解码器。
- Receiver 将 Annex-B / AVCC Access Unit 统一封装为四字节长度前缀的 `CMSampleBuffer`；SPS/PPS 改变或 `stream_epoch` 切换时销毁并重建 `VTDecompressionSession`。
- VideoToolbox 必须创建硬件解码器并明确请求 `420v` 双平面输出；进入 FrameHub 前按 CoreVideo plane stride 复制为紧凑 NV12。目标 macOS 仅支持 Apple Silicon，不保留软件解码兼容路径。
- macOS 正式解码失败必须作为媒体错误暴露，不得回退 OpenH264 或用占位帧掩盖真实 H.264 错误。

### 编码参数

- Codec：H.264/AVC，8-bit 4:2:0 SDR Progressive，无 B 帧。
- V1 SDR 色彩语义为 BT.709 limited range；Sender 显式配置 color standard/range/transfer，
  Receiver 的 HD NV12 预览使用同一矩阵。内存布局与色彩矩阵是两个独立契约：Receiver 必须先按
  实际 row pitch / allocation height 找到正确 UV 平面，再应用色彩转换。
- 默认 Profile Main Level 4.0；不支持时回退 Baseline。
- Keyframe interval：2 秒。
- 动态码率范围：

| 模式 | 初始 | 最低 | 最高 |
| --- | --- | --- | --- |
| 480p30 | 1.8 Mbps | 0.9 Mbps | 2.5 Mbps |
| 720p30 | 3 Mbps | 1.5 Mbps | 5 Mbps |
| 1080p30 | 6 Mbps | 3 Mbps | 10 Mbps |

### Rust 与原生编码器的状态契约

Rust Core 是码率阶梯、ABR 分辨率意图和 `stream_epoch` 的唯一事实源。Android/iOS
只负责把 Rust 的目标配置应用到 Camera2/MediaCodec 或 AVFoundation/VideoToolbox，
不得在原生层复制码率阶梯或自行分配 epoch。

ABR 分辨率变化采用显式的 `directive -> apply -> ack/nack` 契约：

1. Rust 产生带单调且不回绕的 `directive_id`、目标高度、目标码率和候选 `stream_epoch` 的编码指令；候选 epoch 此时只被分配，尚未成为 committed epoch；
2. 原生层保持该指令 pending、暂停发送媒体并重建编码器；每次原生编码器重建还要递增仅在进程内使用的 generation，并同时按 `stream_epoch + generation` 丢弃迟到回调。generation 不进入协议、不替代 Rust 分配的 epoch，专门隔离失败重建与同一 committed epoch 上的回滚重建；
3. 原生层只有在实际收到候选 epoch、目标高度的首个 IDR 后才能 `ack`；本地相机/分辨率调整以同样条件 `report`；
4. Rust 仅在匹配的 `ack/report` 后原子提交 epoch、推进活动码率阶梯；旧 epoch 的 `StreamConfig` 不得被改写成新 epoch，新 epoch 的匹配 `StreamConfig` 在可靠控制流排队前必须继续阻止 AU 入队；
5. 失败、超时、断连或取消通过 `nack/cancel` 丢弃 pending；原生层随后必须重建最后 committed 配置，并等待其首个匹配 IDR 后恢复发送；恢复仍失败则停止编码并明确断开连接；
6. 读取指令不得隐式改变 Rust 的活动编码状态，也不得覆盖另一个 pending 调整。

无 pending 时，平台对当前 epoch 的高度回报只允许首次同步匹配的 `StreamConfig` 或
幂等回报已经 committed 的高度；分辨率变化不能绕过 `begin -> apply -> report`。接收端
能力上限同时约束 Rust 的 preferred height 和 ABR 目标，平台仍须对已排队但超出能力的
旧指令显式 `nack`。Rust 分别保存用户请求高度与当前接收端下的有效高度；接收端能力
扩大或会话结束后必须从用户请求恢复，不得把临时 720p 上限永久写回偏好。

V1 编码高度只接受精确的 `480/720/1080`。提交后的媒体闸门只允许由高度与 committed
编码器完全一致的 `StreamConfig` 打开；错误配置必须返回显式错误并继续阻止 AU。

用户切换摄像头、手动修改分辨率或连接恢复同样必须先从 Rust 获取新的
`stream_epoch`，再重建编码器。允许失败的重建消耗一个 epoch；epoch 只要求单调隔离，
不要求连续。Android JNI 使用正 `Int` 表达 epoch，因此 V1 在 `Int32.max` 处 fail-fast，
不得回绕或跨入负值。

Android NSD 和 Apple 原生媒体 API 仍保留在平台层；协议字段校验、码率/分辨率业务
策略与会话状态仍归 Rust Core。

平台轮询会话时必须通过单次 Rust 锁读取原子 `SenderSnapshot`（状态、码率、活动高度、
接收端能力上限、epoch 和重连信息），不得组合多个独立 getter 推导同一时刻的状态。
Kotlin/Swift 的稳定状态码由 `SenderStatus` 生成，CI 以 `cargo xtask generate sender-status --check`
拒绝过期绑定；平台代码不得手写数值副本。

### 镜像与方向

系统区分 **本机预览镜像** 与 **远端输出镜像**：

- 默认：前置摄像头本机预览镜像；传输到会议软件的画面不镜像。
- 用户可手动开启远端镜像。
- Android Sender 在编码前将方向和横向 16:9 比例规范化；其视频帧声明 `rotation = 0`。
- Receiver 仍消费协议方向字段以支持其他 Sender 平台，但不得为旧 Android Sender 推断方向或执行
  中央裁切；会议软件接收的 Android 画面几何以 Sender 编码结果为准。

### 解码消费规则

一条视频流 **只解码一次**。解码后的帧同时提供给：

- GPUI 预览；
- Shared Frame Ring / 虚拟摄像头；
- 运行指标采集。

不允许为预览和虚拟摄像头分别运行两个解码器。

## 不采用的方案

### 原始帧跨 FFI 进入 Rust 再软件编码

不采用。编码器与摄像头生命周期留在原生层，Rust 只接收 Access Unit。

### CameraX Recorder 作为 Android 实时核心

不采用。该 API 偏向录像文件，容器与编码格式控制不足以支撑低延迟 QUIC Datagram 路径。

### Receiver 对 Android 竖屏帧二次裁切

不采用。Receiver 无法与手机构图界面共享同一变换，而且会在编码并传输无效区域后再次缩放，
浪费码率并让所有桌面平台承担 Sender 的输出契约。方向与构图应在 Android 编码前一次完成。

### Receiver 在 GPUI 或虚拟摄像头进程内重复解码

不采用。见 [ARCH-PICOO-FRAME-001](0006-framehub-shared-frame-ring-boundary.md)。

## 约束

- 设备不支持目标规格时必须通过 Capabilities 协商回退。
- 切换摄像头或分辨率时允许短暂重建编码器，并递增 `stream_epoch`、请求 IDR。
- 原生编码器应用失败不得提前推进 Rust 的活动码率档位；必须显式回报失败。
- Sender 传输期间必须保持前台；提供防锁屏与过热/低电提示。
- 音频继续使用电脑麦克风；Sender 不传输手机麦克风。

## 相关 Use Case

- [PUC-004](../use-cases/product/puc-004-use-virtual-camera-in-meeting-apps.md)
- [PUC-005](../use-cases/product/puc-005-adjust-camera-during-streaming.md)

## 相关 Architecture

- [ARCH-PICOO-PROTOCOL-001](0003-picoo-camera-protocol-boundary.md)
- [ARCH-PICOO-FRAME-001](0006-framehub-shared-frame-ring-boundary.md)
- [ARCH-PICOO-STACK-001](0001-rust-core-monorepo-boundary.md)

## 相关 Requirements

- 待分解：`REQ-PICOO-MEDIA-*`
