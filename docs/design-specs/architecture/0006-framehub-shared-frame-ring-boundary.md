# ARCH-PICOO-FRAME-001: FrameHub 与 Shared Frame Ring 边界

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

`sequence`、`timestamp`、`width`、`height`、`stride`、`pixel_format`、`rotation`、`data_length`、`ready_state`、`reader_count`、`pixel_data`

写入流程：

1. 在未被读取的槽上原子取得独占写租约；
2. 标记 Writing，写入帧信息和像素；
3. 更新序列号并通过 Release 屏障标记 Ready；
4. 释放写租约并发布最新序列号。

读取者总是选择最新完整序列，并在读取期间持有原子租约；Writer 不覆盖仍被读取的槽。消费者处理速度不足时，**丢弃旧帧并提供最新完整帧**；三个槽都被占用时 Producer 保留上一完整帧而不阻塞。

### Shared Frame Ring

主应用与虚拟摄像头扩展/组件之间的跨进程共享：

| 平台 | 实现 |
| --- | --- |
| Windows | Named Shared Memory |
| macOS | App Group Container 中的 mmap Shared File |

Windows 与 macOS 都在原子租约外为每个槽增加独立的内核文件锁：Windows Named Shared Memory 使用 `LockFileEx` byte-range sidecar，macOS mmap 使用 `flock` sidecar。锁在进程退出时由内核释放，下一方取得锁后才能安全清理由异常退出遗留的原子租约；每槽独立设计保留槽间并行能力。

Windows flink 中的 OS mapping ID 是 Producer 代际定位器，不是长期固定的映射身份。Producer 在整个生命周期持有独占内核锁；只有取得该锁的一方可以创建、打开或修复定位器，从而保证单 Writer。Consumer 周期性比较当前定位器与自身 mapping ID；定位器缺失或变化时释放旧映射并重新附着，因此 Receiver 正常重启、异常退出或重建损坏映射后，VCam 不会永久停留在旧代际。代际切换后帧序列可以从 1 重新开始，Consumer 必须重置去重状态。

macOS 的 App Group 后缀为 `com.haoxincode.picoo-camera`，签名时由 Xcode 添加 Team Identifier 前缀，主应用与扩展从各自 Info.plist 读取同一个展开后的值。Rust Receiver 在 mmap 文件旁持有独占 Producer 生命周期 `flock`，拒绝第二个 Writer；进程异常退出时由内核释放。Rust Receiver 与 Swift Camera Extension 共享 ABI v2：64-byte RingMeta、三个 64-byte SlotMeta，以及固定容量 NV12 payload。Swift 通过小型 C17 原子边界获取/释放槽租约，不在 Swift 中模拟跨进程原子操作。

虚拟摄像头扩展只理解 NV12 帧；不持有 QUIC、解码器或网络会话。

第一版 **不依赖** 跨进程 IOSurface 共享或共享 GPU 纹理。1080p30 下的一次额外内存复制可接受；性能不足时再评估零复制路径。

### 无画面状态

没有手机连接时，FrameHub / Shared Frame Ring 输出定义的占位画面：

- 纯黑背景；
- Picoo Camera 标志；
- `Waiting for phone...`

连接暂时中断时，最多短暂重复最后一帧，随后切换到重连占位画面。

## 不采用的方案

### 预览与虚拟摄像头各用一个解码器

不采用。违反单次解码原则，见 [ARCH-PICOO-MEDIA-001](0004-cross-platform-media-pipeline-boundary.md)。

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

- [ARCH-PICOO-MEDIA-001](0004-cross-platform-media-pipeline-boundary.md)
- [ARCH-PICOO-VCAM-001](0007-virtual-camera-platform-boundary.md)
- [ARCH-PICOO-UI-001](0009-desktop-gpui-mobile-native-ui-boundary.md)

## 相关 Requirements

- [REQ-PICOO-FRAME-001..007](../requirements/frame.md)
