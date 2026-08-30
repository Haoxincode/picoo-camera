# Picoo Camera macOS Camera Extension

状态：原生可编译、跨进程共享环和 Host bundle 基线已实现。`cargo xtask test macos` 会直接编译生产 Swift/C Reader，验证 Rust Writer 并发读写和 Reader/Producer 异常退出恢复；`cargo xtask package macos` 构建共享 GPUI Receiver 与 ARM64 Camera Extension，并将扩展嵌入未签名 `Picoo Camera.app` 的标准 System Extensions 目录。App Group 签名读写、激活、用户批准和公证仍待真机验收。

产品基线为 macOS 15+ ARM64（Apple Silicon），不构建或发布 Intel slice。

Camera Extension 使用 Core Media I/O 系统扩展机制注册统一设备名 `Picoo Camera`，并通过 App Group mmap 消费 NV12 Shared Frame Ring。扩展只修改每槽的原子读取租约，帧元数据和像素只读；不得运行 QUIC、配对、Receiver Session 或视频解码器。

扩展 bundle 与 Bundle ID 同名：`com.haoxincode.picoo-camera.camera-extension.systemextension`，并嵌入 `Picoo Camera.app/Contents/Library/SystemExtensions/`。这与 System Extensions 的安装约束一致，不保留旧的通用文件名。

App Group 后缀为 `com.haoxincode.picoo-camera`，Xcode 签名时统一展开 Team Identifier 前缀；代码从 Info.plist 读取最终值，不写死开发团队 ID。

原生边界采用 Swift 6 严格并发检查和 C17 原子操作。共享环固定为三个槽；Producer 取得独占写租约后才覆盖槽，Extension 在复制 NV12 到 `CVPixelBuffer` 期间持有读取租约，从而避免 torn frame。macOS file mapping 为每个槽使用独立 advisory lock，既允许不同槽并行读写，也能在进程异常退出后由内核释放锁并安全回收遗留的原子租约。扩展提供 480p、720p、1080p 的 30 fps NV12 格式，环中尺寸与客户端当前选择不一致时输出黑帧，等待 Receiver 完成格式切换。

实现必须保持以下边界：

```text
Picoo Camera Desktop.app
  -> VideoToolbox decode once
  -> FrameHub
  -> App Group mmap Shared Frame Ring
  -> Picoo Camera Extension.systemextension
```

无签名 CI 只负责主程序与扩展的可编译性、ARM64 slice、CMIO 身份和禁止依赖边界。系统扩展激活、用户批准、Developer ID、Hardened Runtime、公证、卸载清理和会议软件枚举必须在 macOS 真机单独验收。

追溯：`REQ-PICOO-FRAME-006`、`REQ-PICOO-VCAM-006`、`REQ-PICOO-VCAM-007`。
