# Picoo Camera

仓库：`picoo-camera` · 产品名：**Picoo Camera**

Picoo Camera 是一套局域网无线摄像头系统。用户在 Android 或 iPhone 上运行 Sender 应用，通过同一 Wi-Fi 将实时画面传输到 Windows 或 macOS 电脑，并注册为系统虚拟摄像头，供腾讯会议、Zoom、Microsoft Teams、OBS 等软件使用。

产品行为、架构边界与验收以 [Design Specs](docs/design-specs/context.md) 为准。

## 文档

- [Context 与追溯规则](docs/design-specs/context.md)
- [Use Cases](docs/design-specs/use-cases/)
- [Architecture](docs/design-specs/architecture/)
- [Requirements](docs/design-specs/requirements/)
- [产品基线](docs/product/README.md)
- [CI 与跨平台构建](docs/development/ci-and-build.md)

## 开发

```bash
cargo test --workspace
cargo xtask test protocol
cargo xtask build android
cargo xtask build windows
cargo xtask build macos
cargo xtask build ios
```

## 平台支持

| 平台 | 最低版本 | 角色 |
| --- | --- | --- |
| Android | Android 10 ARM64 | Sender |
| iOS | iOS 18 ARM64 | Sender |
| Windows | Windows 11 Build 22000 x86_64 | Receiver |
| macOS | macOS 15 ARM64 (Apple Silicon) | Receiver |
