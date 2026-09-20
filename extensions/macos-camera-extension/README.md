# Picoo Camera macOS Camera Extension

状态：原生 CMIO source/sink 与 Host bundle 基线已实现。产品基线为 macOS 15+ ARM64（Apple Silicon），不构建或发布 Intel slice。

Camera Extension 在同一 `Picoo Camera` 设备注册两个流：

- 公开 `.source`：会议软件读取 720p/1080p × 30/60 NV12。
- 受限 `.sink`：只允许 signing ID 为 `com.haoxincode.picoo-camera` 的 Host 写入同一四格式表。

Host 作为 CMIO Hardware output client，通过 legacy device UID
`com.haoxincode.picoo-camera.virtual-camera` 找到 sink 并向系统三槽队列提交
IOSurface-backed `CVPixelBuffer`。扩展只在 source 客户端实际取流时调用
`consumeSampleBuffer`，缓存最后一张格式兼容图像，并用独立有理数 SampleClock 产生对外
timestamp；尚无 Host 图像时输出黑帧。扩展不运行 QUIC、配对、Receiver Session、视频解码、
缩放或效果处理。

```text
Picoo Camera Desktop.app
  -> VideoToolbox decode once
  -> FrameBus / latest-only VCam worker
  -> Apple renderer (GpuNative) or explicit CpuBridge
  -> CMIO output queue -> Extension .sink
  -> cached image + source SampleClock -> Extension .source
```

旧 App Group mmap `SharedRingReader`、C17 原子桥、文件锁与跨语言 ring ABI 已删除。
`group.com.haoxincode.picoo-camera` 仍由 Host/Extension entitlement 和 Developer ID
provisioning profile 共同声明，用于现有发布身份与 System Extension capability；它不承载像素。

扩展 Bundle ID 为 `com.haoxincode.picoo-camera.camera-extension`，bundle 文件名按 Apple
System Extensions 规则为
`com.haoxincode.picoo-camera.camera-extension.systemextension`，嵌入
`Picoo Camera.app/Contents/Library/SystemExtensions/`。

`cargo xtask test macos` 以 Swift 6 strict concurrency 和 warnings-as-errors 编译生产
source/sink，验证共享四格式选择、非法格式拒绝、source SampleClock 与三槽 CoreVideo pool；
`cargo xtask package macos` 构建 Host 与 ARM64 Camera Extension。无签名 CI 不替代系统扩展
激活、用户批准、Developer ID、Hardened Runtime、公证、真实 sink 消费、会议软件枚举与持续
吞吐验收。

追溯：`REQ-PICOO-VCAM-006`、`REQ-PICOO-VCAM-007`、`REQ-PICOO-VCAM-015`、
`REQ-PICOO-VCAM-017`。
