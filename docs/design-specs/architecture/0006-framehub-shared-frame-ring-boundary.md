# ARCH-PICO-FRAME-001: FrameHub 与 Shared Frame Ring 边界

Status: planned
Source: product PRD V1.0 / PUC-004

## 背景

桌面 Receiver 需要将同一条解码视频流同时提供给 GPUI 预览、运行指标和虚拟摄像头。若各自解码或各自维护无界队列，会造成 CPU/GPU 浪费、延迟不一致和内存增长。

虚拟摄像头在 Windows 与 macOS 上运行在与主应用不同的进程或组件边界内，因此需要跨进程帧共享机制。

## 架构决策

### FrameHub

FrameHub 是解码帧的统一出口，位于桌面主进程内：

```text
Decoded Frame (NV12)
  ↓ FrameHub（三槽环形缓冲）
  ├── GPUI Preview Consumer
  └── Virtual Camera Producer → Shared Frame Ring
```

三槽环形缓冲每个 Slot 包含：

`sequence`、`timestamp`、`width`、`height`、`stride`、`pixel_format`、`rotation`、`data_length`、`ready_state`、`pixel_data`

写入流程：

1. 选择非活动槽；
2. 写入帧信息和像素；
3. 内存屏障；
4. 更新序列号；
5. 标记 Ready。

读取者总是选择最新完整序列。消费者处理速度不足时，**丢弃旧帧并提供最新完整帧**。

### Shared Frame Ring

主应用与虚拟摄像头扩展/组件之间的跨进程共享：

| 平台 | 实现 |
| --- | --- |
| Windows | Named Shared Memory |
| macOS | App Group Container 中的 mmap Shared File |

虚拟摄像头扩展只理解 NV12 帧；不持有 QUIC、解码器或网络会话。

第一版 **不依赖** 跨进程 IOSurface 共享或共享 GPU 纹理。1080p30 下的一次额外内存复制可接受；性能不足时再评估零复制路径。

### 无画面状态

没有手机连接时，FrameHub / Shared Frame Ring 输出定义的占位画面：

- 纯黑背景；
- Pico Camera 标志；
- `Waiting for phone...`

连接暂时中断时，最多短暂重复最后一帧，随后切换到重连占位画面。

## 不采用的方案

### 预览与虚拟摄像头各用一个解码器

不采用。违反单次解码原则，见 [ARCH-PICO-MEDIA-001](0004-cross-platform-media-pipeline-boundary.md)。

### 第一版使用 IOSurface 跨进程共享

不采用。降低系统扩展权限复杂度，且 Windows 无对等抽象；Shared Frame Ring 设计可跨平台统一。

### 无界帧队列

不采用。FrameHub 与 Shared Frame Ring 必须固定容量。

## 约束

- 桌面内存稳态低于 300 MB；FrameHub 不随时间增长。
- 跨进程读写必须通过 sequence 与 ready_state 保证一致性；需有原子一致性测试。
- GPUI `VideoSurface` 只接收平台视频纹理，不拥有解码器或网络会话。

## 相关 Use Case

- [PUC-004](../use-cases/product/puc-004-use-virtual-camera-in-meeting-apps.md)
- [PUC-005](../use-cases/product/puc-005-adjust-camera-during-streaming.md)

## 相关 Architecture

- [ARCH-PICO-MEDIA-001](0004-cross-platform-media-pipeline-boundary.md)
- [ARCH-PICO-VCAM-001](0007-virtual-camera-platform-boundary.md)
- [ARCH-PICO-UI-001](0009-desktop-gpui-mobile-native-ui-boundary.md)

## 相关 Requirements

- 待分解：`REQ-PICO-FRAME-*`
