# Architecture

这个目录维护 Pico Camera 长期可评审的架构选择、抽象边界和设计品味判断。

建议 ID 格式：

```text
ARCH-PICO-<AREA>-NNN
```

## 文档索引

- [ARCH-PICO-STACK-001: Rust Core 与 Monorepo 边界](0001-rust-core-monorepo-boundary.md)
- [ARCH-PICO-TRANSPORT-001: QUIC 传输与 pico-transport 封装边界](0002-quic-transport-encapsulation-boundary.md)
- [ARCH-PICO-PROTOCOL-001: Pico Camera Protocol (PCP/1) 边界](0003-pico-camera-protocol-boundary.md)
- [ARCH-PICO-MEDIA-001: 跨平台媒体采集、编码与解码边界](0004-cross-platform-media-pipeline-boundary.md)
- [ARCH-PICO-SESSION-001: 会话状态、重连、抖动缓冲与码率控制边界](0005-session-reconnect-jitter-bitrate-boundary.md)
- [ARCH-PICO-FRAME-001: FrameHub 与 Shared Frame Ring 边界](0006-framehub-shared-frame-ring-boundary.md)
- [ARCH-PICO-VCAM-001: 虚拟摄像头平台边界](0007-virtual-camera-platform-boundary.md)
- [ARCH-PICO-DISCOVERY-001: 设备发现、配对与安全边界](0008-discovery-and-pairing-security-boundary.md)
- [ARCH-PICO-UI-001: 桌面 GPUI 与手机原生 UI 边界](0009-desktop-gpui-mobile-native-ui-boundary.md)
