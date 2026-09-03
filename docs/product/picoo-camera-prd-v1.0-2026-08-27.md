# 无线手机摄像头系统：产品需求与技术设计文档

暂定产品名： Picoo Camera

文档版本： V1.0

文档日期： 2026 年 8 月 27 日

最近更新： 2026 年 8 月 29 日

文档状态： 立项与第一版开发基线

## 1. 项目概述

Picoo Camera 是一套局域网无线摄像头系统。

用户在 Android 手机或 iPhone 上运行 Sender 应用，手机通过同一 Wi-Fi 局域网将实时画面传输到 Windows 或 macOS 电脑。电脑端接收、解码并将画面注册为系统虚拟摄像头，供腾讯会议、Zoom、Microsoft Teams、OBS、浏览器会议等软件使用。

产品的核心体验是：

```text
打开电脑端
    ↓
打开手机端
    ↓
自动发现并连接
    ↓
会议软件中选择“Picoo Camera”
    ↓
手机成为无线摄像头
```

第一版完全在局域网内运行，不依赖云服务器、账号系统、USB 数据线、ADB、浏览器、OBS 中转或公网穿透。

## 2. 产品目标

### 2.1 核心目标

1. 支持 Android 和 iPhone 作为无线摄像头。
2. 支持 Windows 和 macOS 作为桌面接收端。
3. 四种组合全部成立：
   - Android → Windows
   - Android → macOS
   - iPhone  → Windows
   - iPhone  → macOS
4. 手机与电脑只需连接同一个 Wi-Fi。
5. 电脑端向系统注册标准虚拟摄像头。
6. 支持 720p30 和 1080p30。
7. 支持前后摄像头切换。
8. Wi-Fi 波动时优先保持低延迟，不因重传旧画面造成持续卡顿。
9. 业务状态、协议、传输、配对、重连和码率控制尽可能统一使用 Rust。
10. Windows 与 macOS 桌面 UI 共用一套 GPUI 代码。

### 2.2 产品原则

**局域网优先**

系统默认只在本地网络内工作：

```text
手机 ←→ Wi-Fi 路由器 ←→ 电脑
```

视频不经过任何云端服务。

**实时性优先**

当网络出现丢包时，可以丢弃过期视频帧，但不能因为重传旧帧导致延迟持续累积。

**媒体能力原生化**

摄像头、硬件编解码、虚拟摄像头、系统权限和扩展安装使用各平台原生能力。

**业务核心 Rust 化**

以下能力统一放入 Rust Core：

- 协议
- QUIC 传输
- 会话状态
- 设备模型
- 配对
- 重连
- 视频分包与重组
- 抖动缓冲
- 码率控制
- 运行指标
- 错误模型
- 配置模型

**UI 保持简单**

UI 不承担视频处理和协议逻辑，只负责：

- 发现设备
- 配对
- 连接与断开
- 摄像头预览
- 摄像头切换
- 分辨率设置
- 连接质量显示
- 错误提示

## 3. 第一版范围

### 3.1 第一版包含

| 能力 | 第一版要求 |
| --- | --- |
| 连接方式 | 同一 Wi-Fi 局域网 |
| 手机平台 | Android、iOS |
| 电脑平台 | Windows 11、macOS |
| 视频编码 | H.264/AVC |
| 视频规格 | 720p30、1080p30 |
| 摄像头 | 前摄、后摄 |
| 音频 | 使用电脑麦克风 |
| 发现方式 | mDNS/DNS-SD 自动发现 |
| 发现失败兜底 | 手动输入局域网 IP:端口 |
| 首次配对授权 | 双端六位配对短码核对 |
| 传输协议 | QUIC |
| 视频数据 | QUIC Datagram |
| 控制数据 | QUIC Reliable Stream |
| 桌面输出 | 系统虚拟摄像头 |
| 桌面 UI | GPUI + gpui-component |
| Android UI | Jetpack Compose |
| iOS UI | SwiftUI |
| 云服务 | 无 |
| 账号系统 | 无 |
| 视频保存 | 默认不保存 |

### 3.2 第一版明确不做

- USB 或 ADB 连接；
- 公网远程连接；
- TURN、STUN 或云端中继；
- 手机麦克风传输；
- 本地录像；
- 4K；
- 60 FPS；
- HDR；
- HEVC/H.265；
- 多手机同时输入；
- 一台手机同时连接多台电脑；
- AI 美颜、背景替换、虚化；
- 浏览器端 Receiver；
- WebAssembly 版本；
- Linux 虚拟摄像头；
- 手机锁屏后继续传输；
- 二维码生成、扫码连接及仅服务于扫码的 SDK；
- 账号、组织、订阅和云同步。

## 4. 平台支持基线

| 平台 | 最低版本 | 架构 | 角色 |
| --- | --- | --- | --- |
| Android | Android 10 | ARM64 | Sender |
| iOS | iOS 18 | ARM64 | Sender |
| Windows | Windows 11 Build 22000 | x86_64 | Receiver |
| macOS | macOS 15 | ARM64（Apple Silicon） | Receiver |

Windows 的 Media Foundation 虚拟摄像头 API 最低要求 Windows Build 22000，可以将用户态软件组件注册为系统可发现的摄像头。

macOS 使用从 12.3 开始提供的 Core Media I/O Camera Extension，以系统扩展方式向其他应用发布虚拟摄像头；产品最低版本统一收敛到 macOS 15，且不提供 Intel 产物。

## 5. 用户角色

系统只有一个主要角色：

**会议与录制用户**

用户希望利用手机摄像头获得比普通电脑摄像头更好的画质，并在电脑上的会议或录制软件中直接使用。

用户不需要理解 IP、端口、编码器、虚拟驱动或网络协议。

## 6. 核心用户流程

### 6.1 首次使用

```text
安装桌面端
    ↓
桌面端安装或激活虚拟摄像头
    ↓
安装手机端
    ↓
授予摄像头与局域网权限
    ↓
手机自动发现电脑
    ↓
选择电脑
    ↓
手机与电脑显示同一六位配对短码
    ↓
用户在两端分别确认数字一致
    ↓
保存可信设备关系
    ↓
开始传输
```

### 6.2 后续使用

```text
打开桌面端
    ↓
打开手机端
    ↓
发现已配对电脑
    ↓
自动连接或点击连接
    ↓
会议软件选择 Picoo Camera
```

### 6.3 会议中操作

用户可以在手机端：

- 切换前置或后置摄像头；
- 切换 720p 或 1080p；
- 开关镜像输出；
- 调整曝光补偿；
- 查看连接质量；
- 停止传输。

用户可以在电脑端：

- 查看实时预览；
- 查看手机名称；
- 查看分辨率、帧率和码率；
- 查看延迟、丢包和网络质量；
- 断开连接；
- 管理已配对设备；
- 检查虚拟摄像头状态。

## 7. 功能需求

### 7.1 设备发现

**FR-DISC-001 自动发现**

桌面端启动后，应通过 mDNS/DNS-SD 广播 Receiver 服务。

建议服务类型：

```text
_picoocam._udp.local
```

手机端应自动浏览该服务并显示附近可连接电脑。

Android NSD 使用 DNS-SD 发现局域网服务；Apple 平台对应使用 Bonjour。

**FR-DISC-002 发现信息**

mDNS 记录只允许包含：

- receiver_id
- display_name
- protocol_version
- quic_port
- pairing_state
- public_key_fingerprint_prefix

不得在广播记录中包含用户身份、视频状态或敏感密钥。

**FR-DISC-003 手动 IP 直连兜底**

企业网络、访客网络或开启客户端隔离的路由器可能阻止 mDNS。

桌面端等待连接页必须显示当前可用的局域网连接地址，格式为：

```text
IP:QUIC_PORT
```

手机端允许用户手动输入该地址，绕过 mDNS 直接向 Receiver 建立 QUIC/TLS 连接。IPv4 地址与端口应使用数字分段输入，分隔符由界面固定呈现，支持自动前进和整串地址粘贴分配，避免用户在手机键盘上手动输入点号与冒号。手动地址只负责定位 Endpoint，不建立信任；未配对设备仍必须完成 FR-PAIR-001。

### 7.2 配对

**FR-PAIR-001 首次确认**

Sender 通过 mDNS 或手动 `IP:端口` 确定 Receiver Endpoint 并建立 QUIC/TLS 连接。对于未配对 Sender，Receiver 为本次连接生成随机挑战及由双方设备 ID 派生的六位配对短码，并通过加密的可靠控制 Stream 发给 Sender；手机与电脑必须显示相同数字。

用户必须在手机端和桌面端分别确认数字一致，确认顺序不影响结果。任一端拒绝、连接中断或 60 秒到期后，本次挑战与短码立即失效；短码不由用户输入，也不能跨连接复用。双向确认完成后，系统保存对方公钥并开始视频协商；在此之前 Sender 不得开始推流。

**FR-PAIR-002 密钥固定**

配对完成后，双方保存：

- device_id
- device_name
- public_key
- certificate_fingerprint
- paired_at
- last_connected_at

后续连接必须验证固定公钥，不再接受同名但公钥不同的设备。

**FR-PAIR-003 撤销配对**

手机端和桌面端都必须支持删除已配对设备。

删除后，下一次连接必须重新配对。

此前未知且同名的 Sender 新身份完成双端短码配对后，桌面端应列出同名历史身份的指纹，并允许用户仅保留当前身份。新可信身份与不可扩大的候选快照必须原子持久化，重启后继续呈现同一决策；替换只撤销用户已看到的候选快照；可信重连不得触发或重新生成清理，也不得按名称、型号或硬件标识自动继承信任。用户选择保留时，应持久化消费该决策，各身份继续独立存在。

**FR-PAIR-004 未配对隔离**

未完成配对的设备：

- 不得接收视频；
- 不得切换摄像头；
- 不得读取设备详细信息；
- 不得修改码率或分辨率；
- 不得触发虚拟摄像头输出。

### 7.3 视频采集

**FR-CAM-001 摄像头选择**

手机端支持：

- Front
- Back

切换摄像头时允许短暂重建编码器和视频会话。

**FR-CAM-002 视频规格**

第一版固定支持：

- 1280 × 720 @ 30 FPS
- 1920 × 1080 @ 30 FPS

设备不支持目标规格时，必须通过能力协商回退到可用规格。

**FR-CAM-003 镜像**

系统区分：

- 本机预览镜像
- 远端输出镜像

默认行为：

- 前置摄像头手机本机预览镜像；
- 传输到会议软件的画面不镜像；
- 用户可以手动开启远端镜像。

**FR-CAM-004 方向**

视频帧必须携带旋转和方向信息。

桌面端负责将输入转换为会议软件可以稳定消费的方向与比例，不依赖会议软件自行旋转。

**FR-CAM-005 前台要求**

手机端传输期间必须保持应用前台运行。

系统提供：

- 防止自动锁屏；
- 深色低亮度传输界面；
- 过热提示；
- 电量不足提示。

### 7.4 视频编码

**FR-ENC-001 编码格式**

第一版只使用：

- H.264 / AVC
- 8-bit 4:2:0 SDR Progressive
- 无 B 帧

推荐默认：

- Profile: Main
- Level: 4.0
- Keyframe interval: 2 seconds

不支持 Main Profile 时回退到 Baseline。

**FR-ENC-002 硬件编码**

手机端必须优先使用硬件编码器。

Android 使用 Camera2 与 MediaCodec：

```text
Camera2 Capture Session
    ├── Local Preview Surface
    └── MediaCodec Input Surface
```

Android 官方媒体 API 支持通过 MediaCodec.createInputSurface() 将摄像头输出直接送入编码器。

第一版不使用 CameraX Recorder 作为实时传输核心，因为该 API 偏向录像文件，最终编码格式与容器不能由应用完整控制。

iOS 使用：

```text
AVCaptureSession
    ↓
AVCaptureVideoDataOutput
    ↓
VTCompressionSession
```

VideoToolbox 提供硬件编码器访问，并有专门面向低延迟实时通信的 H.264 编码能力。

**FR-ENC-003 动态码率**

编码器必须支持运行时调整码率。

码率阶梯与 ABR 分辨率意图由 Rust Core 唯一维护，Android/iOS 原生媒体层只应用目标值，
不得各自复制一套阈值。ABR 分辨率变化必须采用“指令 → 原生重建 → ACK/NACK”：仅在
编码器成功应用并 ACK 后，Rust 才推进活动分辨率；失败保持原状态并允许重试。

推荐范围：

| 模式 | 初始码率 | 最低码率 | 最高码率 |
| --- | --- | --- | --- |
| 480p30 | 1.8 Mbps | 0.9 Mbps | 2.5 Mbps |
| 720p30 | 3 Mbps | 1.5 Mbps | 5 Mbps |
| 1080p30 | 6 Mbps | 3 Mbps | 10 Mbps |

**FR-ENC-004 强制关键帧**

以下情况必须请求 IDR：

- 建立新连接；
- 解码器重新初始化；
- 丢失连续关键帧；
- 分辨率改变；
- 切换摄像头；
- Receiver 明确请求；
- 连续帧无法恢复。

### 7.5 视频接收与解码

**FR-DEC-001 硬件解码**

Windows：

```text
H.264
  ↓ Media Foundation Decoder
  ↓ D3D11 / NV12
```

macOS：

```text
H.264
  ↓ VideoToolbox
  ↓ CVPixelBuffer / NV12
  ↓ Metal
```

**FR-DEC-002 单次解码、多路消费**

一条视频流只解码一次。

解码后的帧同时提供给：

- 桌面 GPUI 预览
- 虚拟摄像头
- 运行指标采集

不允许为预览和虚拟摄像头分别运行两个解码器。

**FR-DEC-003 最新帧优先**

如果消费者处理速度不足，FrameHub 必须丢弃旧帧并提供最新完整帧。

系统不能因为消费者变慢而让视频延迟持续累积。

### 7.6 虚拟摄像头

**FR-VCAM-001 统一名称**

Windows 和 macOS 中均注册为：

```text
Picoo Camera
```

**FR-VCAM-002 Windows**

Windows 使用：

- MFCreateVirtualCamera
- IMFVirtualCamera
- Custom IMFMediaSource

Media Source 作为独立组件安装并注册，由 Windows Frame Server 加载。微软官方示例也采用独立 Media Source、安装器和管理程序的结构。

Windows 组件包括：

- Picoo Camera Desktop.exe
- PicooVirtualCameraSource.dll
- Installer
- Shared Frame Ring

**FR-VCAM-003 macOS**

macOS 使用：

- Core Media I/O Camera Extension

Camera Extension 作为桌面应用随附的系统扩展，首次使用时由用户批准。

组件包括：

- Picoo Camera Desktop.app
- Picoo Camera Extension.systemextension
- App Group Container
- Shared Frame Ring

Camera Extension 是独立进程边界，桌面主应用不应直接把网络会话逻辑放入扩展。Apple 将 Camera Extension 定义为现代的系统扩展机制，用于替代旧式 DAL 插件。

**FR-VCAM-004 无画面状态**

没有手机连接时，虚拟摄像头输出：

- 纯黑背景
- Picoo Camera 标志
- Waiting for phone...

连接暂时中断时，最多短暂重复最后一帧，随后切换到重连占位画面。

### 7.7 连接恢复

**FR-CONN-001 自动重连**

已配对设备发生以下情况时，应自动重连：

- Wi-Fi 短暂中断；
- 手机应用短暂进入非活动状态；
- 电脑网络接口变化；
- 路由器切换信道；
- QUIC 会话超时。

重连退避：

- 500 ms
- 1 s
- 2 s
- 5 s
- 之后每 5 s 一次

**FR-CONN-002 会话恢复**

重连成功后：

1. 重新验证固定公钥；
2. 重新协商能力；
3. 恢复上次分辨率与镜像设置；
4. 请求新的 SPS/PPS；
5. 请求 IDR；
6. 恢复虚拟摄像头画面。

**FR-CONN-003 状态提示**

UI 必须区分：

- Discovering
- Pairing
- Connecting
- Negotiating
- Streaming
- Reconnecting
- Disconnected
- Permission Required
- Virtual Camera Unavailable
- Network Unstable

## 8. 非功能需求

### 8.1 延迟目标

在同一 5 GHz Wi-Fi 或 Wi-Fi 6 网络、无明显拥塞时：

| 指标 | 目标 |
| --- | --- |
| 端到端延迟 P50 | 小于 150 ms |
| 端到端延迟 P95 | 小于 250 ms |
| 局域网发现 P50 | 小于 2 s |
| 局域网发现 P95 | 小于 5 s |
| 已配对连接建立 | 小于 3 s |
| 短暂断网后的恢复 | 小于 5 s |

端到端延迟定义为：

```text
手机摄像头曝光完成
    ↓ 编码
    ↓ 网络
    ↓ 重组与抖动缓冲
    ↓ 解码
    ↓ 虚拟摄像头提供帧
```

### 8.2 稳定性

- 连续传输 2 小时无崩溃；
- 无持续内存增长；
- 会议软件关闭和重新打开后仍可选择虚拟摄像头；
- 手机切换前后摄像头后恢复时间小于 3 秒；
- 分辨率切换后恢复时间小于 3 秒；
- 网络丢包 5% 时保持可用；
- 网络恢复后不能保留数秒历史延迟。

### 8.3 资源目标

| 平台 | 目标 |
| --- | --- |
| 手机 | 必须使用硬件编码 |
| Windows | 优先硬件解码 |
| macOS | 使用 VideoToolbox 硬件解码 |
| 桌面内存稳态 | 低于 300 MB |
| 手机内存稳态 | 低于 250 MB |
| FrameHub | 固定容量，不随时间增长 |
| 视频队列 | 有明确上限 |

### 8.4 隐私

- 视频不默认落盘；
- 不向公网发送视频；
- 不建立云端连接；
- 不需要登录；
- 不上传设备名称、局域网信息或运行指标；
- 日志不得包含视频数据；
- 日志中的 IP、设备名和公钥指纹应支持脱敏；
- 用户可以删除全部配对关系和本地配置。

## 9. 总体技术架构

```text
┌──────────────────────────── 手机 Sender ────────────────────────────┐
│                                                                    │
│   Android                              iOS                          │
│   Compose                              SwiftUI                      │
│      │                                    │                         │
│   Camera2                            AVFoundation                   │
│      │                                    │                         │
│   MediaCodec                         VideoToolbox                   │
│      └──────────── H.264 Access Units ─────┘                        │
│                              │                                     │
│                       Rust Sender Core                              │
│               Packetizer · Session · Rate Control                  │
│                              │                                     │
│                  Quinn / QUIC Stream + Datagram                    │
└──────────────────────────────┬─────────────────────────────────────┘
                               │                            Wi-Fi LAN
┌──────────────────────────────▼─────────────────────────────────────┐
│                        Desktop Receiver                            │
│                                                                    │
│                     Quinn / QUIC Receiver                          │
│                              │                                     │
│                 Reassembly · Jitter · Session                      │
│                              │                                     │
│              ┌───────────────┴────────────────┐                    │
│              │                                │                    │
│        Windows Decode                    macOS Decode               │
│       MF + D3D11                        VideoToolbox                 │
│              │                                │                    │
│              └──────────── Decoded Frame ─────┘                    │
│                              │                                     │
│                         FrameHub                                   │
│                    ┌─────────┴─────────┐                           │
│                    │                   │                           │
│              GPUI Preview       Shared Frame Ring                  │
│                                        │                           │
│                          ┌─────────────┴─────────────┐             │
│                          │                           │             │
│                 Windows MF Source          macOS CMIO Extension    │
│                          │                           │             │
│                          └───────────┬───────────────┘             │
│                                      ▼                             │
│                    Zoom / Teams / 腾讯会议 / OBS                    │
└────────────────────────────────────────────────────────────────────┘
```

## 10. UI 技术架构

### 10.1 最终选型

| 应用 | UI 技术 |
| --- | --- |
| Android Sender | Jetpack Compose |
| iOS Sender | SwiftUI |
| Windows Receiver | GPUI + gpui-component |
| macOS Receiver | GPUI + gpui-component |

不引入：

- Flutter
- React Native
- Electron
- Tauri
- WebView
- GPUI Mobile

原因是手机端 UI 很薄，而摄像头、编码器、权限与生命周期无论如何都必须调用原生 API。桌面 Receiver 则是同一个产品，Windows 和 macOS 共用 GPUI 可以真正减少重复代码。

### 10.2 gpui-component 使用方式

桌面端直接使用完整的：

```text
gpui-component
      ↓
gpui-base
      ↓
GPUI
```

第一版不从 gpui-base 开始重做 Picoo Camera Design System。

gpui-component 已提供完整样式组件、主题和 60 多个桌面组件，并明确支持一套 Rust 代码运行于 macOS、Windows 和 Linux；gpui-base 则用于产品需要自行拥有完整设计系统时复用行为与基础设施。

桌面端的视觉与信息架构以 `picoo-camera-receiver.html` 为准。HTML 中的 Tailwind 类名与 OKLCH `@theme` 变量应映射为 GPUI 的 `rem` 比例、组件语义尺寸和 Picoo 明暗主题，首次启动默认使用亮色主题，用户仍可在 Sidebar 中切换深色主题；不引入 CSS、WebView 或浏览器运行时，也不以像素复刻为由绕过 gpui-component 已有的键盘、焦点、滚动与弹窗行为。

桌面 Receiver 默认窗口与最小窗口均为 1120×720，不记忆用户上次调整的窗口尺寸。连接页在 Sidebar 之后使用全部可用宽度，不保留 HTML 演示壳的 1160px 内容上限；左侧配对/实时预览主区域消费全部剩余宽度，网络状态在同一主卡内部底部横向摊平，不另建带圆角或外边框的长卡；右侧只保留设备与连接卡，待机态保持 21–24rem，直播态收窄为 18rem，且均不随窗口额外拉宽。实时卡与右侧设备卡的上下边框对齐，实时视频视口始终保持 16:9、在剩余空间内取最大尺寸并成为页面最大的视觉对象；预览画面内部不叠加设备名、规格、虚拟摄像头状态、等待提示或 `LIVE`，链路摘要保留在画面之外。直播状态只在右侧标题栏以绿色 `Live` 表达，虚拟摄像头状态移入右侧指标区；设备条目不重复直播文案，断开操作使用透明底红色 X。右侧待机列表与直播内容分别持有纵向溢出，「自动接受可信设备」固定在卡片底栏。“开始使用”旁的手机→无线→电脑真机 SVG 拓扑作为完整插图按可用宽度等比缩放并设置舒适上限，在最小窗口不得越过所属卡片边框。可信设备行使用中性手机机框 SVG 建立三行行高，依次完整显示设备名称、人类可读的最近连接时间（今天 / 昨天 / N 天前）和身份指纹，等待状态与透明底红色 X 操作使用稳定尾部车道；删除按钮采用标准中号组件，整框均可命中，并打开命名具体设备及后果的确认框。横向网络状态使用紧凑文案，在默认与最小窗口下完整显示图标、文字和状态符号。连接工作区自身不滚动。

第一版仅定制：

- 颜色
- 字体
- 圆角
- 间距
- 明暗主题（默认亮色）
- 状态色
- 品牌图标

需要的组件：

- Button
- Card
- Badge
- Select
- Switch
- Slider
- Dialog
- Tooltip
- Popover
- Toast
- Progress
- Separator
- Icon

自定义组件：

第一版只新增一个核心自定义组件：**VideoSurface**

职责：

- 接收平台视频纹理；
- 保持画面比例；
- 支持镜像；
- 支持旋转；
- 支持占位画面；
- 不拥有解码器或网络会话。

### 10.3 GPUI 依赖管理

gpui、gpui_platform 和 gpui-component 必须在 Workspace 根目录统一锁定到相互兼容的 Git revision。

不得让不同 crate 自行引用不同 GPUI commit，否则 Cargo 可能解析出两个互不兼容的 GPUI 类型。gpui-base 官方文档也明确提示应用与组件库必须使用相同 GPUI revision。

## 11. Rust Core 架构

### 11.1 Rust Core 职责

- picoo-protocol
- picoo-transport
- picoo-session
- picoo-pairing
- picoo-discovery-model
- picoo-packet
- picoo-jitter
- picoo-rate-control
- picoo-metrics
- picoo-frame-types
- picoo-ffi

Rust Core 不负责：

- Android Camera2
- Android MediaCodec 生命周期
- iOS AVFoundation
- iOS VideoToolbox 生命周期
- Android/iOS 权限弹窗
- Windows 安装器
- macOS System Extension 授权

### 11.2 QUIC 实现选型

四端统一使用 Quinn + Rustls（ring provider）。

选择理由：

1. Rust 实现；
2. 支持 QUIC Stream；
3. 支持 QUIC Datagram；
4. QUIC 状态机、TLS、可靠 Stream 与 Datagram 由 Cargo 统一管理；
5. 可构建到 Android、iOS、Windows 和 macOS；
6. 不引入 BoringSSL、CMake 或 NASM 构建步骤；
7. Rust API 与项目的 Rust Core 边界一致。

Quinn 提供可靠 Stream 与 QUIC DATAGRAM；Rustls 使用 ring provider。ring 可能通过 Cargo
编译少量 C/汇编，但不要求 CMake，且不把第二套原生构建系统带入仓库。

QUIC Datagram 根据 RFC 9221 提供不可靠数据报，不保证交付，适合承载允许丢弃的实时视频片段。

**Quinn 封装要求**

Quinn 的 Endpoint、Connection、SendStream 等类型仍属于传输实现细节。`picoo-transport`
以 actor 拥有异步连接、定时器和 I/O，并向业务层只暴露领域命令与事件：

- 可靠控制消息队列；
- 有界视频 Datagram 队列；
- 连接、断开与错误事件；
- 传输统计快照。

业务代码禁止直接调用 `quinn::Connection`，必须通过统一封装：

```rust
trait PicooTransport {
    fn connect(&mut self, endpoint: Endpoint) -> Result<SessionId>;
    fn send_control(&mut self, message: ControlMessage) -> Result<()>;
    fn send_video(&mut self, packet: VideoPacket) -> Result<()>;
    fn poll_event(&mut self) -> Option<TransportEvent>;
    fn close(&mut self, reason: CloseReason);
}
```

所有平台只依赖 `picoo-transport`，不直接感知 Quinn 细节。视频队列达到上限时允许
按实时媒体策略丢弃，控制消息和生命周期事件使用独立队列，不得被视频背压阻塞。

### 11.3 FFI

Android 由 `libpicoo_ffi.so` 直接导出 Rust JNI 方法，不经过 C++ shim；iOS 通过稳定
C ABI 调用 Rust Core。

生成方式：

```text
Android: Kotlin → Rust JNI exports → Rust Core
iOS:     Swift → PicooCore XCFramework Clang module → C ABI → Rust Core
```

平台封装：

```text
Android: Kotlin → JNI → C ABI → Rust
iOS:     Swift → PicooCore XCFramework Clang module → C ABI → Rust
```

媒体数据使用专门的 Buffer API，不通过 JSON、Dart、WebView 或通用对象序列化层。

FFI 边界只允许：

- 编码后的 H.264 Access Unit；
- 摄像头配置；
- 会话命令；
- 状态快照；
- 指标事件；
- 错误事件。

原始摄像头帧不得跨 Rust FFI 传输。

## 12. 协议设计

协议暂定名称：**Picoo Camera Protocol**

协议版本：**PCP/2**

QUIC ALPN：**picoocam/2**

### 12.1 连接角色

桌面 Receiver：

- QUIC Server
- mDNS Advertiser

手机 Sender：

- QUIC Client
- mDNS Browser

### 12.2 QUIC 通道

每个会话只建立一条 QUIC Connection。

```text
QUIC Connection
│
├── Bidirectional Reliable Stream
│   ├── Hello
│   ├── Capabilities
│   ├── Pairing
│   ├── Start / Stop
│   ├── Camera Control
│   ├── Stream Configuration
│   ├── SPS / PPS
│   ├── Request IDR
│   ├── Network Statistics
│   ├── Heartbeat
│   └── Error
│
└── QUIC Datagram
    └── H.264 Video Fragments
```

控制消息使用 Protobuf，并通过 Rust prost 生成协议类型。

视频包头使用固定二进制结构，避免每个视频片段执行 Protobuf 编解码。

### 12.3 控制消息

主要消息：

- ClientHello
- ServerHello
- Capabilities
- PairingChallenge
- PairingConfirm
- PairingApproval
- PairingCommit
- PairingComplete
- StartStream
- StopStream
- StreamConfig
- CameraCommand
- EncoderCommand
- ReceiverStats
- RequestKeyframe
- Heartbeat
- SessionError

**StreamConfig**

- codec
- profile
- level
- width
- height
- fps
- bitrate
- rotation
- mirrored
- color_range
- sps
- pps
- stream_epoch

`stream_epoch` 在以下情况递增：

- 摄像头切换；
- 分辨率变化；
- 编码器重建；
- 连接恢复；
- 编码参数重大变化。

`stream_epoch` 由 Rust Core 统一分配；Camera2/MediaCodec 与
AVFoundation/VideoToolbox 只能消费 Rust 返回的新 epoch，不得在平台层自行递增。

Receiver 不得将不同 epoch 的视频片段组成同一帧。

### 12.4 视频数据包

建议结构：

```text
VideoPacket {
    version: u8,
    flags: u8,
    stream_epoch: u32,
    frame_id: u64,
    pts_us: u64,
    fragment_index: u16,
    fragment_count: u16,
    payload: bytes,
}
```

Flags：

- KEYFRAME
- START_OF_ACCESS_UNIT
- END_OF_ACCESS_UNIT
- DISCARDABLE

**包大小**

单个 UDP/QUIC 数据包应控制在路径 MTU 范围内。

第一版采用：

- 最大 VideoPacket 载荷约 1150 字节

避免依赖 IP 分片。

**帧重组**

Receiver 按以下键重组：

```text
stream_epoch + frame_id
```

帧重组规则：

- 所有片段到齐后进入抖动缓冲；
- 超过截止时间仍不完整则丢弃；
- 不请求重传旧视频片段；
- 连续错误时请求新的 IDR；
- 未收到 SPS/PPS 时不得向解码器提交普通帧。

## 13. 抖动缓冲与丢包策略

### 13.1 目标

抖动缓冲解决短暂乱序和网络抖动，但不能形成不断增长的延迟。

默认：

- 目标缓冲：50 ms
- 正常范围：30–80 ms
- 最大缓冲：120 ms

### 13.2 丢包处理

```text
完整帧                 → 解码
不完整非关键帧         → 丢弃
不完整关键帧           → 丢弃并请求 IDR
解码器报错             → 清空当前 epoch 缓冲并请求 IDR
```

### 13.3 队列策略

所有视频队列必须有固定上限：

- Sender Access Unit Queue
- QUIC Datagram Queue
- Receiver Reassembly Map
- Jitter Buffer
- Decoded Frame Queue
- Shared Frame Ring

队列满时优先丢弃：

1. 最旧的非关键帧；
2. 已经过播放期限的帧；
3. 依赖已丢失参考帧的帧。

不得无限等待或无限增长。

## 14. 自适应码率

Receiver 每秒向 Sender 发送一次统计：

- RTT
- packet_loss
- jitter
- reassembly_drop
- decoder_drop
- frame_age
- receive_bitrate
- jitter_buffer_depth

建议控制策略：

```text
丢包 > 3% 或帧龄持续增加 或发送队列持续堆积
    → 码率降低 20%

丢包 < 1% 且缓冲稳定 且持续 5 秒有余量
    → 码率提高 10%
```

降级顺序：

```text
降低码率
    ↓
降低图像复杂度
    ↓
1080p 降至 720p
    ↓
720p 降至 480p
```

不能通过扩大缓冲区来掩盖带宽不足。

## 15. FrameHub 与进程间帧共享

### 15.1 FrameHub

FrameHub 是桌面端解码帧的统一出口。

```text
Decoded Frame
    ↓ FrameHub
    ├── GPUI Preview Consumer
    └── Virtual Camera Producer
```

FrameHub 采用三槽环形缓冲：

```text
Slot 0
Slot 1
Slot 2
```

每个 Slot 包含：

- sequence
- timestamp
- width
- height
- stride
- pixel_format
- rotation
- data_length
- ready_state
- pixel_data

写入流程：

1. 选择非活动槽；
2. 写入帧信息和像素；
3. 内存屏障；
4. 更新序列号；
5. 标记 Ready。

读取者总是选择最新完整序列。

### 15.2 跨进程共享

Windows 和 macOS 统一使用“共享内存环形帧区”的抽象，但平台实现不同：

- Windows：Named Shared Memory
- macOS：App Group Container 中的 mmap Shared File

第一版不依赖跨进程 IOSurface 共享。

理由：

- 降低系统扩展权限复杂度；
- 便于 Windows 与 macOS 共享同一 Frame Ring 设计；
- 虚拟摄像头扩展只需理解 NV12 帧；
- 网络、QUIC 和解码器不进入虚拟摄像头进程；
- 一次额外内存复制在 1080p30 场景中可接受。

后续性能不足时，再评估 IOSurface、共享 GPU 纹理或平台专属零复制路径。

## 16. 桌面 UI 设计

### 16.1 页面结构

**首次启动页**

显示：

```text
Picoo Camera
Use your phone as a wireless camera

Virtual Camera [ Installed / Not Installed ]
[ Install Virtual Camera ]
```

**等待连接页**

```text
Picoo Camera

Waiting for phone...
Open Picoo Camera on your phone and connect to this computer.

Connection Code: 482 917
Direct Address: 192.168.1.108:4433

Virtual Camera: Ready
```

**直播页**

```text
┌────────────────────────────────────────────┐
│ Picoo Camera                           ⚙    │
├────────────────────────────────────────────┤
│                                            │
│             Camera Preview                 │
│                                            │
├────────────────────────────────────────────┤
│ Xiaomi Phone                     ● Live    │
│ 1080p · 30 FPS · 6.2 Mbps · 18 ms          │
│                                            │
│ Network Quality                  Good      │
│ Virtual Camera                   Active    │
│                                            │
│                         [ Disconnect ]     │
└────────────────────────────────────────────┘
```

**桌面导航页**

包含：

- 连接；
- 虚拟摄像头状态、重新检测与安装/修复；
- 网络状态；
- 桌面显示名称；
- 自动接受已配对设备；
- 开机启动；
- 最小化到系统托盘（仅 Windows；macOS 关闭窗口后保留在 Dock/后台）；
- 默认占位画面；
- 日志级别；
- 已配对设备管理；
- 导出诊断信息；桌面端导出成功后可直接打开文件所在文件夹。

Windows 产品进程必须使用 GUI subsystem，从资源管理器或开机启动进入桌面 UI 时不得附带控制台窗口，也不得通过 `reg.exe` 等控制台子进程检测状态。普通启动只能以只读方式检测并启动已安装的虚拟摄像头；系统级 COM/Media Foundation 注册修复只能由安装器或用户明确触发的“虚拟摄像头修复”操作执行。
用户在“虚拟摄像头”页明确点击“安装或修复…”后，应用通过 Windows UAC 启动独立维护进程；主界面必须保持响应，并在原操作位置显示等待、成功或失败结果。若安装包组件缺失，应明确要求重新运行 `PicooCamera.msi`，不能提示前往不存在的“设置页”。

### 16.2 GPUI 状态边界

GPUI View 不直接持有 QUIC Connection、Decoder 或 Frame Buffer。

统一状态模型：

```rust
struct DesktopAppState {
    receiver_status: ReceiverStatus,
    discovered_devices: Vec<DeviceSummary>,
    active_session: Option<SessionSummary>,
    virtual_camera: VirtualCameraStatus,
    metrics: StreamMetrics,
    last_error: Option<AppError>,
}
```

后台 Rust Core 通过事件更新状态，GPUI 只观察并渲染。

## 17. 手机 UI 设计

### 17.1 设备列表页

```text
Picoo Camera

Available Computers

┌─────────────────────────────┐
│ Work PC                     │
│ Windows · Ready             │
└─────────────────────────────┘

┌─────────────────────────────┐
│ Mac mini                    │
│ macOS · Paired              │
└─────────────────────────────┘

[ Enter IP Address ]
```

### 17.2 配对页

```text
Enter Connection Code

Work PC

[ _ _ _   _ _ _ ]

Enter the 6-digit code shown on your computer.

[ Connect ]   [ Cancel ]
```

### 17.3 传输页

```text
┌──────────────────────────────┐
│                              │
│       Camera Preview         │
│                              │
├──────────────────────────────┤
│ Work PC             ● Live   │
│ 1080p · 30 FPS               │
│ Wi-Fi Good · 18 ms           │
│                              │
│ Camera      [ Front ▼ ]      │
│ Quality     [ 1080p ▼ ]      │
│ Mirror      [ Off ]          │
│                              │
│         [ Disconnect ]       │
└──────────────────────────────┘
```

### 17.4 权限

Android 需要处理：

- Camera；
- Network；
- 局域网服务发现相关权限；
- 前台运行和防锁屏。

iOS 需要处理：

- Camera；
- Local Network；
- Bonjour Service 声明。

Apple 要求访问局域网和使用 Bonjour 的应用提供本地网络用途说明，并声明浏览或广播的 Bonjour 服务类型。

权限必须在用户执行相应操作时请求，不应在启动后一次性弹出全部权限。

## 18. 代码仓库结构

```text
picoo-camera/
│
├── Cargo.toml
├── Cargo.lock
├── rust-toolchain.toml
│
├── proto/
│   └── picoo_camera.proto
│
├── crates/
│   ├── picoo-protocol/
│   ├── picoo-transport/
│   ├── picoo-session/
│   ├── picoo-pairing/
│   ├── picoo-packet/
│   ├── picoo-jitter/
│   ├── picoo-rate-control/
│   ├── picoo-metrics/
│   ├── picoo-frame-hub/
│   ├── picoo-ffi/
│   └── picoo-testkit/
│
├── apps/
│   ├── android/
│   │   ├── app/
│   │   └── native/
│   │
│   ├── ios/
│   │   ├── PicooCamera/
│   │   └── RustBridge/
│   │
│   └── desktop/
│       ├── src/
│       │   ├── app.rs
│       │   ├── model.rs
│       │   ├── views/
│       │   ├── components/
│       │   └── platform/
│       └── assets/
│
├── platform/
│   ├── android-media/
│   ├── ios-media/
│   ├── windows-media/
│   └── macos-media/
│
├── extensions/
│   ├── windows-virtual-camera/
│   └── macos-camera-extension/
│
├── installers/
│   ├── windows/
│   └── macos/
│
├── xtask/
├── tests/
│   ├── protocol/
│   ├── network/
│   ├── integration/
│   └── compatibility/
│
└── docs/
    ├── architecture.md
    ├── protocol.md
    ├── security.md
    ├── virtual-camera.md
    └── release.md
```

## 19. 构建与发布

### 19.1 构建目标

- Android ARM64 APK / AAB
- iOS ARM64 App
- Windows x86_64 Installer
- macOS ARM64 App（Apple Silicon，不提供 Intel slice）

### 19.2 构建工具

- Cargo Workspace
- Gradle
- Xcode
- cbindgen
- prost-build
- xtask

Windows Media Source DLL 使用独立 Rust `cdylib` crate，通过 `windows-rs` 实现 COM 与 Media Foundation 接口，并由 Cargo 在 Windows runner 上构建。仓库不维护等价的 C++/WRL、VCXPROJ 或 MSBuild 工程；Windows SDK 与原生链接器仍是平台构建的必要条件。

xtask 统一提供：

```text
cargo xtask build android
cargo xtask build ios
cargo xtask build windows
cargo xtask build macos
cargo xtask test protocol
cargo xtask package windows
cargo xtask package macos
```

### 19.3 Windows 发布

安装器负责：

- 安装桌面程序；
- 安装 Media Source DLL；
- 注册 COM/Media Foundation 组件；
- 创建虚拟摄像头；
- 配置卸载清理；
- 配置本地网络防火墙规则；
- 可选配置开机启动。

安装器承担系统级 COM/Media Foundation 注册与修复。桌面程序的普通启动路径不提升权限、不写入 HKLM，也不以启动失败为由隐式修复；只有安装流程或用户明确触发的修复入口可以进入该写入路径。

### 19.4 macOS 发布

macOS 包含：

- GPUI 主应用；
- Camera Extension；
- App Group；
- 签名与 Hardened Runtime；
- Developer ID；
- Notarization；
- 首次启动时的扩展激活引导。

## 20. 测试设计

### 20.1 Rust 单元测试

覆盖：

- 协议版本协商
- 视频分包
- 乱序重组
- 重复包
- 缺失包
- epoch 切换
- 关键帧恢复
- 配对状态机
- 公钥固定
- 重连状态机
- 码率控制
- FrameHub 原子一致性
- 队列上限

### 20.2 模糊测试

重点目标：

- VideoPacket Parser
- Protobuf Control Decoder
- Reassembly Map
- Pairing Message Parser
- Shared Frame Header

输入必须覆盖：

- 任意长度；
- 非法 fragment_count；
- 超大 frame_id；
- 重复片段；
- 不同 epoch 混合；
- 畸形 SPS/PPS；
- 乱序控制消息。

### 20.3 网络模拟

测试条件：

| 条件 | 范围 |
| --- | --- |
| 丢包 | 0%–10% |
| 抖动 | 0–100 ms |
| 带宽 | 1–20 Mbps |
| 乱序 | 0%–20% |
| 短暂断网 | 1–30 s |
| RTT | 1–200 ms |

必须验证：

- 不形成无限延迟；
- 丢包时主动降低码率；
- 完整帧才能进入解码器；
- 重连后请求 IDR；
- 旧 epoch 帧不会污染新会话。

### 20.4 平台组合测试

必须覆盖：

- 小米 Android → Windows
- 小米 Android → macOS
- iPhone → Windows
- iPhone → macOS

Android 设备矩阵至少包含：

- 小米/Redmi；
- Google Pixel 或接近 AOSP 的设备；
- 另一家主流国产品牌。

### 20.5 会议软件兼容测试

Windows：

- 腾讯会议；
- Zoom；
- Microsoft Teams；
- OBS；
- Chrome/Edge 浏览器会议。

macOS：

- 腾讯会议；
- Zoom；
- Microsoft Teams；
- OBS；
- Safari/Chrome 浏览器会议；
- FaceTime 可发现性检查。

### 20.6 长稳测试

每个主要平台组合执行：

- 2 小时 720p30；
- 2 小时 1080p30；
- 前后摄像头切换 50 次；
- 分辨率切换 50 次；
- 断网重连 50 次；
- 会议软件重复打开关闭；
- 电脑休眠唤醒；
- 手机来电或系统弹窗中断；
- 手机温度升高后的降级恢复。

## 21. 第一版验收标准

只有同时满足以下条件，第一版才可视为完成：

**功能验收**

- 四种平台组合都可连接；
- 自动发现可用；
- 手动 `IP:端口` 直连兜底可用；
- 六位配对短码在两端一致显示，绑定单次连接，并在成功、拒绝、中断或到期后失效；
- 配对与撤销配对可用；
- 前后摄像头切换可用；
- 720p30 和 1080p30 可用；
- Windows 和 macOS 均能注册 Picoo Camera；
- 目标会议软件可以选择并使用 Picoo Camera；
- 无手机连接时显示占位画面；
- 断网后自动恢复。

**性能验收**

- 健康局域网内端到端 P95 延迟低于 250 ms；
- 1080p30 连续运行 2 小时无崩溃；
- 内存无持续增长；
- 网络丢包 5% 时仍能保持可用；
- 恢复后延迟不会累积到 1 秒以上。

**安全与隐私验收**

- 未配对设备无法获得视频；
- 公钥变化时拒绝自动连接；
- 删除配对后必须重新确认；
- 断开互联网后完整功能仍可工作；
- 抓包不能看到明文控制消息或视频；
- 默认不写入任何视频文件；
- 日志不包含视频帧。

## 22. 开发阶段

**阶段 0：技术验证**

必须先完成四个独立验证：

- **验证 A：跨平台 QUIC** — Android、iOS、Windows、macOS 统一使用 Quinn/Rustls；完成可靠 Stream、Datagram、TLS、背压、丢包与重连测试，构建不得依赖 CMake。
- **验证 B：Windows 虚拟摄像头** — Rust/Native Producer → Shared Frame Ring → IMFMediaSource → MFCreateVirtualCamera；并在腾讯会议、Zoom、OBS 中验证。
- **验证 C：macOS Camera Extension** — Main App → App Group mmap Ring → CMIO Camera Extension；并验证签名、安装、用户批准和卸载。
- **验证 D：GPUI 视频预览** — Decoded Frame → Platform Texture → GPUI VideoSurface；验证 1080p30 预览无明显 CPU 拷贝瓶颈。

**阶段 1：Android → Windows 纵向闭环**

范围：Android Camera2、MediaCodec H.264、Quinn Datagram、Windows Decode、Windows Virtual Camera、GPUI Preview、720p30。该阶段建立完整产品骨架。

**阶段 2：扩展四端**

增加：iOS Sender、macOS Receiver、1080p30、手动 IP 直连、完整配对、自适应码率。

**阶段 3：稳定性与发布**

完成：长稳测试、会议软件兼容、安装器、签名与公证、日志与诊断、自动更新、UI 完善、错误恢复。

**阶段 4：后续能力**

可选扩展：手机麦克风、HEVC、4K、60 FPS、HDR、多手机、远程互联网模式、录制、背景虚化、人像追踪、OBS 专用输出、Linux Receiver。这些能力不得提前侵入第一版协议和 UI。

## 23. 主要风险与控制措施

| 风险 | 影响 | 控制措施 |
| --- | --- | --- |
| 异步 QUIC 生命周期复杂 | 连接状态泄漏到业务层 | `picoo-transport` actor 独占 Quinn 对象，只暴露领域命令与事件 |
| 实时视频背压 | 队列增长、延迟累积 | 控制/视频分队列；视频队列有界并允许丢弃，控制事件保持可靠 |
| GPUI 版本快速变化 | 类型冲突、构建失败 | 锁定精确 commit，提交 Cargo.lock |
| 小米等 OEM Camera2 差异 | 黑屏、规格不支持 | 能力探测、安全默认值、设备测试矩阵 |
| mDNS 被路由器屏蔽 | 手机发现不到电脑 | 桌面端展示局域网 `IP:端口`，手机端支持手动直连 |
| Windows Media Source 安装复杂 | 虚拟摄像头不可见 | 独立安装与修复工具，参考官方组件结构 |
| macOS 扩展授权复杂 | 用户无法完成安装 | 首次启动向导和状态检查 |
| Wi-Fi 丢包 | 马赛克、卡顿 | Datagram、丢旧帧、请求 IDR、自适应码率 |
| 手机发热降频、掉帧 | 画质下降 | 硬编、码率控制、温度提示、自动降至 720p |
| 队列积压 | 延迟不断增加 | 所有队列固定上限，最新帧优先 |
| 跨进程帧共享 | 花屏或数据竞争 | 三槽环形区、原子 sequence、完整性测试 |

## 24. 最终技术决策

```text
手机端 UI
├── Android: Jetpack Compose
└── iOS: SwiftUI

桌面端 UI
└── Windows + macOS:
    GPUI + gpui-component

共享业务核心
└── Rust

传输
└── Quinn + Rustls / QUIC

设备发现
├── Android: NSD
├── iOS: Bonjour
└── Desktop: Rust mDNS/DNS-SD Adapter

Android 媒体
└── Camera2 + MediaCodec

iOS 媒体
└── AVFoundation + VideoToolbox

Windows 接收
└── Media Foundation + D3D11

macOS 接收
└── VideoToolbox + Metal

Windows 虚拟摄像头
└── MFCreateVirtualCamera + IMFMediaSource

macOS 虚拟摄像头
└── Core Media I/O Camera Extension

桌面帧分发
└── FrameHub + Shared Frame Ring

视频格式
└── H.264 480p30 / 720p30 / 1080p30

音频
└── 继续使用电脑麦克风

连接范围
└── Wi-Fi LAN Only
```

最终架构边界是：

手机原生 UI 和原生媒体能力负责采集与编码，Windows 与 macOS 使用一套 GPUI 桌面应用，Rust Core 统一协议、传输、配对、状态、视频分包、重连和码率控制，平台虚拟摄像头通过共享帧环与主程序隔离。

这份架构能够在不引入 Flutter、React Native、WebView、OBS 中转和云服务的前提下，实现 Android、iOS、Windows、macOS 四个平台的完整无线摄像头组合。
