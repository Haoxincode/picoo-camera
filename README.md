# picoo-camera

仓库：`picoo-camera` · 产品名：**Pico Camera**

Pico Camera 是一套局域网无线摄像头系统。用户在 Android 或 iPhone 上运行 Sender 应用，通过同一 Wi-Fi 将实时画面传输到 Windows 或 macOS 电脑，并注册为系统虚拟摄像头，供腾讯会议、Zoom、Microsoft Teams、OBS 等软件使用。

## 文档

### 原始需求文档

- [无线手机摄像头系统：产品需求与技术设计文档 V1.0（2026-08-27）](docs/product/pico-camera-prd-v1.0-2026-08-27.md) — 立项原文，完整保留

### Design Specs

由原文拆分整理的产品设计与架构契约位于 [docs/design-specs/](docs/design-specs/)。

- [Context 与追溯规则](docs/design-specs/context.md)
- [Use Cases](docs/design-specs/use-cases/)
- [Architecture](docs/design-specs/architecture/)

## 平台支持

| 平台 | 最低版本 | 角色 |
| --- | --- | --- |
| Android | Android 10 ARM64 | Sender |
| iOS | iOS 15 ARM64 | Sender |
| Windows | Windows 11 Build 22000 x86_64 | Receiver |
| macOS | macOS 12.3 Apple Silicon / Intel | Receiver |
