# Architecture

这个目录维护 Picoo Camera 长期可评审的架构选择、抽象边界和设计品味判断。

建议 ID 格式：

```text
ARCH-PICOO-<AREA>-NNN
```

## 文档索引

- [ARCH-PICOO-STACK-001: Rust Core 与 Monorepo 边界](0001-rust-core-monorepo-boundary.md)
- [ARCH-PICOO-TRANSPORT-001: QUIC 传输与 picoo-transport 封装边界](0002-quic-transport-encapsulation-boundary.md)
- [ARCH-PICOO-PROTOCOL-001: Picoo Camera Protocol 边界](0003-picoo-camera-protocol-boundary.md)
- [ARCH-PICOO-MEDIA-001: 跨平台媒体采集、编码与解码边界](0004-cross-platform-media-pipeline-boundary.md)
- [ARCH-PICOO-SESSION-001: 会话状态、重连、抖动缓冲与码率控制边界](0005-session-reconnect-jitter-bitrate-boundary.md)
- [ARCH-PICOO-FRAME-001: FrameHub 与 Shared Frame Ring 边界](0006-framehub-shared-frame-ring-boundary.md)
- [ARCH-PICOO-VCAM-001: 虚拟摄像头平台边界](0007-virtual-camera-platform-boundary.md)
- [ARCH-PICOO-DISCOVERY-001: 设备发现、配对与安全边界](0008-discovery-and-pairing-security-boundary.md)
- [ARCH-PICOO-UI-001: 桌面 GPUI 与手机原生 UI 边界](0009-desktop-gpui-mobile-native-ui-boundary.md)
- [ARCH-PICOO-UI-002: 跨端视觉语义、原生适配与 Icon 边界](0010-cross-platform-design-system-boundary.md)
- [ARCH-PICOO-RUNTIME-001: 显式状态、媒体所有权与性能边界](0011-runtime-state-and-performance-boundary.md)
