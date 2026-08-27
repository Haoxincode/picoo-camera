# ARCH-PICO-MEDIA-001: 跨平台媒体采集、编码与解码边界

Status: planned
Source: product PRD V1.0 / PUC-004 / PUC-005

## 背景

Pico Camera 第一版固定 H.264 720p30 / 1080p30。媒体路径涉及四套平台原生 API，但语义必须一致：硬件优先、低延迟、动态码率、正确方向、镜像分离，以及 Receiver 侧单次解码、多路消费。

## 架构决策

### Sender：Android

```text
Camera2 Capture Session
  ├── Local Preview Surface
  └── MediaCodec Input Surface (H.264)
        ↓
Rust Sender Core (Packetizer / Session / Rate Control)
        ↓
QUIC Datagram
```

- 优先硬件编码；使用 `MediaCodec.createInputSurface()` 直接将摄像头输出送入编码器。
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

### Receiver：macOS

```text
H.264 Access Units
  ↓ VideoToolbox
  ↓ CVPixelBuffer / NV12
  ↓ Metal（如需）
  ↓ FrameHub
```

### 编码参数

- Codec：H.264/AVC，8-bit 4:2:0 SDR Progressive，无 B 帧。
- 默认 Profile Main Level 4.0；不支持时回退 Baseline。
- Keyframe interval：2 秒。
- 动态码率范围：

| 模式 | 初始 | 最低 | 最高 |
| --- | --- | --- | --- |
| 720p30 | 3 Mbps | 1.5 Mbps | 5 Mbps |
| 1080p30 | 6 Mbps | 3 Mbps | 10 Mbps |

### 镜像与方向

系统区分 **本机预览镜像** 与 **远端输出镜像**：

- 默认：前置摄像头本机预览镜像；传输到会议软件的画面不镜像。
- 用户可手动开启远端镜像。
- 视频帧必须携带旋转和方向信息；桌面端负责输出会议软件可稳定消费的方向与比例。

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

### Receiver 在 GPUI 或虚拟摄像头进程内重复解码

不采用。见 [ARCH-PICO-FRAME-001](0006-framehub-shared-frame-ring-boundary.md)。

## 约束

- 设备不支持目标规格时必须通过 Capabilities 协商回退。
- 切换摄像头或分辨率时允许短暂重建编码器，并递增 `stream_epoch`、请求 IDR。
- Sender 传输期间必须保持前台；提供防锁屏与过热/低电提示。
- 音频继续使用电脑麦克风；Sender 不传输手机麦克风。

## 相关 Use Case

- [PUC-004](../use-cases/product/puc-004-use-virtual-camera-in-meeting-apps.md)
- [PUC-005](../use-cases/product/puc-005-adjust-camera-during-streaming.md)

## 相关 Architecture

- [ARCH-PICO-PROTOCOL-001](0003-pico-camera-protocol-boundary.md)
- [ARCH-PICO-FRAME-001](0006-framehub-shared-frame-ring-boundary.md)
- [ARCH-PICO-STACK-001](0001-rust-core-monorepo-boundary.md)

## 相关 Requirements

- 待分解：`REQ-PICO-MEDIA-*`
