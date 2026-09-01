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
| Windows | `%ProgramData%\Picoo Camera` 中的 per-machine mmap Shared File |
| macOS | App Group Container 中的 mmap Shared File |

Windows 与 macOS 都在原子租约外为每个槽增加独立的内核文件锁：Windows mmap 使用 `LockFileEx` byte-range sidecar，macOS mmap 使用 `flock` sidecar。锁在进程退出时由内核释放，下一方取得锁后才能安全清理由异常退出遗留的原子租约；每槽独立设计保留槽间并行能力。

Windows Media Foundation Frame Server 以 Local Service 身份运行在 Session 0，不能把交互用户的 `%TEMP%`、用户 Profile 或 session-local object 当作 Receiver 与 Media Source 的共同身份。因此生产环使用 MSI 预创建的 `%ProgramData%\Picoo Camera` 目录和固定编码文件名；目录 ACL 允许交互用户 Receiver 与 Local Service 读写映射及其锁文件，并由文件继承。普通应用进程不创建或修改目录 ACL。路径通过 Windows Known Folder API 解析，不信任进程可覆盖的 `PROGRAMDATA` 环境变量。Producer 在整个生命周期持有独占文件锁，保证单 Writer；Receiver 重启时复用同一个文件 identity 并重新发布占位/直播帧。布局无效时 Producer 将旧文件移到独立代际再创建新文件，Consumer 比较 volume/file index 后释放旧 mapping 并重新附着，禁止对仍被 Frame Server 映射的文件原地 resize/reinitialize。自定义 ring name 的 Named Shared Memory 只保留给同会话测试与诊断，不是 Windows 生产 VCam 数据面。

Windows 文件环是瞬态缓存，创建时使用 `FILE_ATTRIBUTE_TEMPORARY`，提示 Cache Manager 在内存允许时避免把高频脏页持续写回磁盘。当前 V1 把同一台 Windows 主机上的 Builtin Users 视为本地信任边界；由于普通用户与 Local Service 都必须访问固定 per-machine 环，本机其他登录用户理论上可以读取或干扰原始帧文件。面向不受信任多用户主机前，必须改为受控 broker/service 创建 pagefile-backed Global mapping，并按活动用户 SID 下发最小 DACL；不得把当前 ACL 宣称为跨本地账户隔离。

macOS 使用 Apple 推荐的显式 App Group `group.com.haoxincode.picoo-camera`，主应用与扩展的 Info.plist、签名 entitlement 与 Developer ID provisioning profile 必须一致授权该值。Rust Receiver 在 mmap 文件旁持有独占 Producer 生命周期 `flock`，拒绝第二个 Writer；进程异常退出时由内核释放。Rust Receiver 与 Swift Camera Extension 共享 ABI v2：64-byte RingMeta、三个 64-byte SlotMeta，以及固定容量 NV12 payload。Swift 通过小型 C17 原子边界获取/释放槽租约，不在 Swift 中模拟跨进程原子操作。

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
- Windows 生产共享环必须可由交互用户 Receiver 与 Local Service Frame Server 从不同 Session 打开；安装器必须验证 ProgramData 目录与继承 ACL，不能回退到用户临时目录。
- Windows 真机性能验收必须观察 720p/1080p30 时的磁盘写入；temporary mmap 不得形成与帧率等比例的持续磁盘写放大。
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
