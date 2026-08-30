# CI 与跨平台构建

本文档说明 Picoo Camera 如何在 **Cloud Agent 开发环境** 与 **GitHub Actions** 之间分工，以产出各平台可用二进制。它与 [ARCH-PICOO-STACK-001](../design-specs/architecture/0001-rust-core-monorepo-boundary.md) 中的 monorepo / xtask 边界一致，并补充 PRD §19 的构建与发布约定。

## 背景

Picoo Camera 目标四端（Android、iOS、Windows、macOS），但各平台依赖不同的原生 SDK 与工具链：

| 平台 | 关键原生依赖 | 能否在 Linux Cloud Agent 上完成最终产物 |
| --- | --- | --- |
| Rust Core（共享） | Cargo、Quinn/Rustls；vendored `protoc` | ✅ 开发与测试 |
| Android Sender | NDK、Gradle、Camera2/MediaCodec | ✅ 完整 APK/AAB |
| Windows Receiver | GPUI、Media Foundation、D3D11、COM 虚拟摄像头 | ❌ 需 Windows 原生环境 |
| macOS Receiver | GPUI、VideoToolbox、Camera Extension、codesign | ❌ 需 macOS 原生环境；当前 CI 已覆盖 GPUI 编译与 VideoToolbox→NV12 解码基线 |
| iOS Sender | Xcode、VideoToolbox、codesign | ❌ 需 macOS + Xcode；远端已验证 Rust XCFramework、SwiftUI 壳与 Simulator C ABI 测试基线 |

**结论：** Cloud Agent（Linux）负责 Rust Core 实现、协议测试、Android 构建与 CI 维护；**各平台最终安装包与原生组件由 GitHub Actions 在对应 runner 上编译**。不要试图在 Linux 上交叉编译 GPUI 桌面程序、MF 虚拟摄像头 DLL 或 macOS/iOS 签名产物。

## 构建分工

```text
Cloud Agent（Linux）
├── Rust Core crate 开发与 cargo test
├── picoo-testkit 协议模拟（Sender ↔ Receiver）
├── Android APK/AAB（NDK + Gradle）
├── .github/workflows/ 维护与 CI 修复
└── push 后订阅 CI 结果并迭代

GitHub Actions
├── ubuntu-latest   → Rust 测试、Android 构建、文档校验
├── windows-latest  → GPUI 桌面、MF 解码、Virtual Camera DLL、安装包
└── macos-26 ARM64 → GPUI 桌面、CMIO Camera Extension、iOS Rust XCFramework 与 SwiftUI App；签名、公证和 Apple 真机链路另行验收
```

## GitHub Actions Runner 矩阵

与 PRD §19.2、`cargo xtask` 命令及当前四端实现边界对齐：

| Job | Runner | 职责 | xtask 命令 |
| --- | --- | --- | --- |
| `rust-and-docs` | `ubuntu-latest` | workspace 测试、clippy、文档链接校验 | `cargo test --workspace`、`scripts/check-docs.sh` |
| `android` | `ubuntu-latest` | Android Sender APK/AAB | `cargo xtask build android` |
| `windows` | `windows-latest` | 桌面 exe、VCam DLL、安装包 | `cargo xtask build windows`、`cargo xtask package windows` |
| `macos` | `macos-26` ARM64 + Xcode 26.6 | 共享 GPUI Receiver、VideoToolbox→NV12 原生解码、Swift 6 CMIO Camera Extension 无签名编译 | `cargo clippy -p picoo-desktop --all-targets --features gpui-ui -- -D warnings`；`cargo xtask test macos`；`cargo xtask build macos`；`.app` 嵌入、签名与公证仍待 `package macos` |
| `ios` | `macos-26` ARM64 + Xcode 26.6 | Rust Core device/simulator XCFramework、SwiftUI App ARM64 编译链接、Simulator C ABI 单测 | `cargo xtask build ios`；`cargo xtask test ios` |

### 依赖关系

- `android` / `windows` / `macos` / `ios` 与 `rust-and-docs` **并行**：平台产物不等待通用测试矩阵；同一 ref 的新 push 仍由 concurrency 取消旧 run。
- 各 job 通过 `actions/upload-artifact` 上传产物（APK、MSI、DLL 等），供人工验证或后续 release workflow 消费。
- **下载最新绿 run 产物**（artifact 名、zip 内路径、`gh run download`）：见 [CI 产物下载](../design-specs/verification/ci-artifacts.md)。
- Workflow 使用 `concurrency`（按 PR 号或 `github.ref` 分组、`cancel-in-progress: true`），同分支/同 PR 的新 push 会取消仍在跑的旧 CI，避免 tip 被积压 run 饿死。

### 示例 Workflow 结构

实现 monorepo 后，`.github/workflows/ci.yml` 应近似遵循以下结构（具体步骤随 xtask 落地而调整）：

```yaml
name: CI

on:
  push:
    branches: [main, 'cursor/**']
  pull_request:

jobs:
  rust-and-docs:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v7
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo test --workspace
      - run: cargo clippy --workspace -- -D warnings
      - run: bash scripts/check-docs.sh

  android:
    runs-on: ubuntu-latest
    needs: rust-and-docs
    steps:
      - uses: actions/checkout@v7
      # 固定 NDK / Java 版本，与 xtask 和 rust-toolchain.toml 一致
      - run: cargo xtask build android
      - uses: actions/upload-artifact@v7
        with:
          name: android-apk
          path: apps/android/app/build/outputs/

  windows:
    runs-on: windows-latest
    needs: rust-and-docs
    steps:
      - uses: actions/checkout@v7
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo xtask build windows
      - run: cargo xtask package windows
      - uses: actions/upload-artifact@v7
        with:
          name: windows-installer
          path: target/release/bundle/
```

已记录的远端绿测证明共享 GPUI Receiver、Rust XCFramework、SwiftUI App、Simulator C ABI 生命周期测试和 iOS 原生媒体源码的 Apple 原生编译、链接边界。macOS VideoToolbox 解码由 `xtask test macos` 使用仓库内静态真实 H.264 IDR 验证 `CMSampleBuffer → 420v NV12`、AVCC Receiver 链路以及 720p→480p ABR/epoch/FrameHub 恢复，并检查产品依赖树不含 OpenH264/CMake；macOS 测试依赖也不编译 OpenH264。`xtask build macos` 还以 Swift 6 严格并发和 C17 编译 ARM64 CMIO Camera Extension，检查 CMIO 身份、架构 slice 以及扩展不链接 QUIC/Decoder。静态样本让该验收不依赖 CMake 或外部编码器。这些证据都不替代 `.app` 内嵌激活、签名、公证、会议软件枚举或 iPhone→macOS 真机媒体链路验收。

### Apple 无签名构建基线

Apple 基线保持三个独立 artifact：

- `macos-receiver-camera-extension-unsigned`：ARM64 `picoo-desktop` GPUI 可执行文件和 `PicooCameraExtension.systemextension.zip`；两者未组成可发布 `.app`，也未签名或公证。
- `ios-rust-core-xcframework`：`PicooCore.xcframework.zip`，包含 iOS device arm64 与 simulator arm64 slice，并携带 `picoo_camera.h` 和 `module.modulemap`。
- `ios-app-unsigned`：`PicooCamera.app.zip`，是 SwiftUI + Swift 6 编译的 ARM64 Simulator App，用于验证 Swift module 与 Rust C ABI 的最终链接，不是可安装到真机的签名包。

`xtask build ios` 使用 Cargo 编译 ARM64 `picoo-ffi` staticlib，将最终 Apple 产品稳定输出到仓库 `target/apple/`，再由 iPhone Simulator SDK 的 Clang 完整链接一次 C ABI smoke、由 `xcodebuild -create-xcframework` 组合 device/simulator 产物并编译 SwiftUI App。上传前使用 macOS `ditto` 生成保留 bundle 外层目录、权限和符号链接的 zip。`xtask test ios` 按数值版本选择 runner 上最新的可用 iPhone Simulator，执行 Swift Testing 覆盖 Rust Sender handle 生命周期、手动 Endpoint、UI 状态映射和编码策略。iOS 工程使用 Swift 6 语言模式、strict concurrency、默认 `MainActor` 与 Swift Observation；构建固定 iOS 18.0 deployment target，macOS Receiver 固定 15.0，不随 runner 的 Xcode SDK 默认值漂移。Apple 产物不包含 Intel 架构，整条路径不引入 CocoaPods、Carthage、CMake、第三方 Swift Package 或额外项目生成器。

## 为何 Windows 不在 Linux 上交叉编译

按 [ARCH-PICOO-UI-001](../design-specs/architecture/0009-desktop-gpui-mobile-native-ui-boundary.md)、[ARCH-PICOO-MEDIA-001](../design-specs/architecture/0004-cross-platform-media-pipeline-boundary.md) 与 [ARCH-PICOO-VCAM-001](../design-specs/architecture/0007-virtual-camera-platform-boundary.md)：

- **GPUI 桌面程序** 绑定 Windows 窗口系统与 DirectX/wgpu，交叉编译成功率低且无法在 Linux 上验证 UI。
- **Media Foundation 解码** 与 **D3D11** 仅存在于 Windows SDK。
- **虚拟摄像头** 为独立 Rust COM `IMFMediaSource` `cdylib`，须由 Cargo 使用 Windows 原生链接器与 SDK 完成最终链接，并由 Frame Server 加载验证。

Rust Core 静态库理论上可从 Linux 交叉编译为 Windows `.lib`，但 GPUI、MF、VCam 的最终链接与注册仍必须在 `windows-latest` 上完成。因此 CI 策略是 **Windows 原生构建**，而非 Linux 交叉编译整条 Receiver 链路。

## Cloud Agent 工作流

在 Cursor Cloud Agent 中开发时，Agent 应：

1. 在 Linux 环境完成 Rust Core 变更与 `cargo test`。
2. 更新或新增 `.github/workflows/` 中与变更相关的 job。
3. `git commit` 并 `git push` 到功能分支。
4. 使用 **cursor-subscriptions** 的 `subscribe_github_ci` 订阅该分支 CI，等待结果而非轮询。
5. CI 失败时读取 GitHub Actions 日志，修复后再次 push。
6. 不在 Cloud 环境内尝试运行 Windows/macOS 安装包或虚拟摄像头注册。

Cloud 环境 `.cursor/install.sh` 只需保证 Rust 工具链与文档校验工具；Android NDK 等可在 install 脚本或 workflow 步骤中按需补齐，**macOS SDK 与 Windows SDK 不放入 Linux install**。

## Secrets 与签名

以下项需在 GitHub 仓库 **Settings → Secrets and variables → Actions** 中配置，Agent 只在 workflow 中引用 secret 名称，不包含证书内容：

| Secret（示例名） | 用途 | 必需阶段 |
| --- | --- | --- |
| `ANDROID_KEYSTORE` / 相关 signing 配置 | Android Release 签名 | 发布 AAB 前 |
| `WINDOWS_CERTIFICATE` | Windows 安装包代码签名 | 可选，发布前建议 |
| `APPLE_CERTIFICATE` / `APPLE_NOTARIZATION` | macOS 公证与 iOS 分发 | macOS/iOS 阶段 |

未配置签名 secret 时，CI 仍应能产出 **未签名** 的 debug/CI 构建供功能验证。

## 与 xtask 的边界

- **xtask**（见 ARCH-PICOO-STACK-001）：封装各平台 build/package 命令，供本地与 CI 统一调用。
- **GitHub Actions**：编排 runner、缓存、artifact 上传与 job 依赖；不替代 xtask 中的平台构建逻辑。
- CI workflow 应调用 `cargo xtask …`，避免在 YAML 中复制各平台构建细节。

## 验证范围

| 验证类型 | 执行位置 |
| --- | --- |
| Rust 单元/集成/协议测试 | `ubuntu-latest`（Cloud Agent 本地亦可） |
| Android 安装与采集发送 | CI artifact + 真机（人工或后续设备 farm） |
| Windows 安装与虚拟摄像头枚举 | `windows-latest` 构建 + Windows 真机/VM 人工验证 |
| 会议软件（Zoom/Teams 等）兼容性 | 不在 CI 内自动化；[会议软件验收清单](../design-specs/verification/vcam-meeting-apps.md) |
| Android→Windows 真机 E2E | [真机 E2E 清单](../design-specs/verification/device-e2e-android-win11.md) |

## 相关文档

- [ARCH-PICOO-STACK-001](../design-specs/architecture/0001-rust-core-monorepo-boundary.md) — monorepo 与 xtask 边界
- [产品 PRD §19 构建与发布](../product/picoo-camera-prd-v1.0-2026-08-27.md)
- [AGENTS.md](../../AGENTS.md) — Cloud Agent 跨平台构建指令
