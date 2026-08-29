# Picoo Camera macOS Camera Extension

状态：`planned`。当前 `xtask build macos` 只验证共享 GPUI Receiver；尚未生成或签名 Camera Extension。

产品基线为 macOS 15+ ARM64（Apple Silicon），不构建或发布 Intel slice。

Camera Extension 使用 Core Media I/O 系统扩展机制注册统一设备名 `Picoo Camera`，并通过 App Group mmap 只读消费 NV12 Shared Frame Ring。扩展不得运行 QUIC、配对、Receiver Session 或视频解码器。

实现必须保持以下边界：

```text
Picoo Camera Desktop.app
  -> VideoToolbox decode once
  -> FrameHub
  -> App Group mmap Shared Frame Ring
  -> Picoo Camera Extension.systemextension
```

无签名 CI 只负责可编译性。系统扩展激活、用户批准、Developer ID、Hardened Runtime、公证、卸载清理和会议软件枚举必须在 macOS 真机单独验收。

追溯：`REQ-PICOO-FRAME-006`、`REQ-PICOO-VCAM-006`、`REQ-PICOO-VCAM-007`。
