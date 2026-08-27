# ARCH-PICOO-STACK-001: Rust Core 与 Monorepo 边界

Status: planned
Source: product PRD V1.0 / architecture baseline

## 背景

Picoo Camera 需要在 Android、iOS、Windows 和 macOS 四端保持统一的协议、传输、会话、配对、视频分包、重连和码率控制语义。若将这些能力分散在各平台 UI 或原生胶水层中重复实现，会导致行为漂移、测试矩阵爆炸和后续演进困难。

因此产品采用 **Rust Core + 平台原生媒体 + 平台原生 UI** 的分层：共享业务逻辑统一进入 Rust crate；摄像头、编解码、虚拟摄像头和系统权限保留在各平台原生层。

## 架构决策

本仓库采用 product monorepo 边界。Rust Core 通过稳定 C ABI 向 Android 与 iOS 暴露能力；桌面 Receiver 直接在 Rust 中链接 Core 并驱动 GPUI。

推荐边界：

```text
picoo-camera/                    # 本仓库根目录
  proto/picoo_camera.proto
  crates/picoo-protocol/
  crates/picoo-transport/
  crates/picoo-quiche/
  crates/picoo-session/
  crates/picoo-pairing/
  crates/picoo-packet/
  crates/picoo-jitter/
  crates/picoo-rate-control/
  crates/picoo-metrics/
  crates/picoo-frame-hub/
  crates/picoo-ffi/
  crates/picoo-testkit/
  apps/android/
  apps/ios/
  apps/desktop/
  platform/android-media/
  platform/ios-media/
  platform/windows-media/
  platform/macos-media/
  extensions/windows-virtual-camera/
  extensions/macos-camera-extension/
  installers/windows/
  installers/macos/
  xtask/
  tests/
  docs/design-specs/
```

### Rust Core 职责

Rust Core 负责：

- PCP/1 协议类型与控制消息编解码。
- QUIC 传输封装（基于 quiche，经 `picoo-transport` 隔离）。
- 会话状态机、重连退避与能力协商。
- 配对、公钥固定与设备模型。
- 视频分包、重组、`stream_epoch` 隔离与抖动缓冲策略接口。
- 自适应码率控制与运行指标。
- FrameHub 与共享帧环的抽象与一致性规则。
- 通过 cbindgen 生成 `picoo_camera.h` 的稳定 FFI。

Rust Core 不负责：

- Android Camera2 / MediaCodec 生命周期。
- iOS AVFoundation / VideoToolbox 生命周期。
- Android/iOS 权限弹窗与前台服务 UI。
- Windows Media Source 安装器与 COM 注册。
- macOS System Extension 授权 UI。
- GPUI View 渲染与 Compose/SwiftUI 页面结构。

### FFI 边界

Android 与 iOS 通过 C ABI 调用 Rust Core：

```text
Android: Kotlin → JNI → C ABI → Rust
iOS:     Swift → Bridging Header → C ABI → Rust
```

FFI 边界只允许：

- 编码后的 H.264 Access Unit 元数据与载荷引用。
- 摄像头配置与会话命令。
- 状态快照、指标事件与错误事件。

原始摄像头帧不得跨 Rust FFI 传输。

### xtask 边界

`xtask` 是本仓库任务组合入口，不是产品引擎。适合放置构建 Android/iOS/Windows/macOS、协议测试、打包和 cbindgen 编排；不适合放置 parser、会话状态机或码率算法。

各平台最终二进制由 GitHub Actions 在对应 runner 上调用 `cargo xtask …` 产出；Cloud Agent（Linux）负责 Rust Core 与 Android 构建，Windows/macOS/iOS 原生产物不在 Linux 上交叉编译。见 [CI 与跨平台构建](../../development/ci-and-build.md)。

## 不采用的方案

### Flutter / React Native / Electron / Tauri / WebView 作为跨端 UI

不采用。手机端 UI 很薄，但摄像头、编码器、权限与生命周期必须调用原生 API；桌面端 Windows 与 macOS 共用 GPUI 即可减少重复，无需引入额外跨端框架。

### 业务层直接调用 quiche::Connection

不采用。quiche 是低层协议状态机，应用必须提供 UDP I/O、定时器、连接表和发送节奏。所有平台只依赖 `picoo-transport` trait，不直接感知 quiche 细节。

### 在虚拟摄像头进程内持有网络会话

不采用。Windows Media Source 与 macOS Camera Extension 只消费 Shared Frame Ring，不运行 QUIC、解码器或配对逻辑。

## 约束

- 四端业务状态、协议、传输、配对、重连和码率控制尽可能统一在 Rust Core。
- 原始 YUV/RGB 摄像头帧不跨 FFI；编码发生在平台原生媒体层。
- `gpui`、`gpui_platform` 和 `gpui-component` 必须在 workspace 根目录统一锁定到相互兼容的 Git revision。
- 第一版不在本仓库引入云账号、Registry HTTP 服务或 Plugin 体系。

## 相关 Use Case

- [BUC-001](../use-cases/business/buc-001-phone-as-wireless-meeting-camera.md)
- [PUC-001](../use-cases/product/puc-001-first-install-and-pairing.md)
- [PUC-007](../use-cases/product/puc-007-manage-paired-devices.md)

## 相关 Architecture

- [ARCH-PICOO-TRANSPORT-001](0002-quic-transport-encapsulation-boundary.md)
- [ARCH-PICOO-FFI 边界见本文件 FFI 节]

## 相关 Requirements

- 待分解：`REQ-PICOO-STACK-*`
