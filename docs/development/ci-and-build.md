# CI 与跨平台构建

本文档说明 Picoo Camera 如何在 **Cloud Agent 开发环境** 与 **GitHub Actions** 之间分工，以产出各平台可用二进制。它与 [ARCH-PICOO-STACK-001](../design-specs/architecture/0001-rust-core-monorepo-boundary.md) 中的 monorepo / xtask 边界一致，并补充 PRD §19 的构建与发布约定。

## 背景

Picoo Camera 目标四端（Android、iOS、Windows、macOS），但各平台依赖不同的原生 SDK 与工具链：

| 平台 | 关键原生依赖 | 能否在 Linux Cloud Agent 上完成最终产物 |
| --- | --- | --- |
| Rust Core（共享） | Cargo、quiche/BoringSSL | ✅ 开发与测试 |
| Android Sender | NDK、Gradle、Camera2/MediaCodec | ✅ 完整 APK/AAB |
| Windows Receiver | GPUI、Media Foundation、D3D11、COM 虚拟摄像头 | ❌ 需 Windows 原生环境 |
| macOS Receiver | GPUI、VideoToolbox、Camera Extension、codesign | ❌ 需 macOS 原生环境 |
| iOS Sender | Xcode、VideoToolbox、codesign | ❌ 需 macOS + Xcode |

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
└── macos-latest    → GPUI 桌面、Camera Extension、iOS App、公证（后续阶段）
```

## GitHub Actions Runner 矩阵

与 PRD §19.2、`cargo xtask` 命令及当前实现优先级（**Android + Windows 先行**）对齐：

| Job | Runner | 职责 | xtask 命令（实现后启用） |
| --- | --- | --- | --- |
| `rust-and-docs` | `ubuntu-latest` | workspace 测试、clippy、文档链接校验 | `cargo test --workspace`、`scripts/check-docs.sh` |
| `android` | `ubuntu-latest` | Android Sender APK/AAB | `cargo xtask build android` |
| `windows` | `windows-latest` | 桌面 exe、VCam DLL、安装包 | `cargo xtask build windows`、`cargo xtask package windows` |
| `macos` | `macos-latest` | 桌面 app、Camera Extension（后续） | `cargo xtask build macos`、`cargo xtask package macos` |
| `ios` | `macos-latest` | iOS Sender（后续） | `cargo xtask build ios` |

### 依赖关系

- `windows` job 应依赖 `rust-and-docs` 通过，避免在 Rust Core 已失败时浪费 Windows runner 时间。
- Android 与 Windows job 可并行；macOS/iOS 在实现就绪前可 `if: false` 或单独 workflow 延迟启用。
- 各 job 通过 `actions/upload-artifact` 上传产物（APK、MSI、DLL 等），供人工验证或后续 release workflow 消费。

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
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo test --workspace
      - run: cargo clippy --workspace -- -D warnings
      - run: bash scripts/check-docs.sh

  android:
    runs-on: ubuntu-latest
    needs: rust-and-docs
    steps:
      - uses: actions/checkout@v4
      # 固定 NDK / Java 版本，与 xtask 和 rust-toolchain.toml 一致
      - run: cargo xtask build android
      - uses: actions/upload-artifact@v4
        with:
          name: android-apk
          path: apps/android/app/build/outputs/

  windows:
    runs-on: windows-latest
    needs: rust-and-docs
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo xtask build windows
      - run: cargo xtask package windows
      - uses: actions/upload-artifact@v4
        with:
          name: windows-installer
          path: target/release/bundle/
```

macOS 与 iOS job 在对应 `apps/`、`extensions/` 代码就绪后按同样模式加入 `macos-latest` runner。

## 为何 Windows 不在 Linux 上交叉编译

按 [ARCH-PICOO-UI-001](../design-specs/architecture/0009-desktop-gpui-mobile-native-ui-boundary.md)、[ARCH-PICOO-MEDIA-001](../design-specs/architecture/0004-cross-platform-media-pipeline-boundary.md) 与 [ARCH-PICOO-VCAM-001](../design-specs/architecture/0007-virtual-camera-platform-boundary.md)：

- **GPUI 桌面程序** 绑定 Windows 窗口系统与 DirectX/wgpu，交叉编译成功率低且无法在 Linux 上验证 UI。
- **Media Foundation 解码** 与 **D3D11** 仅存在于 Windows SDK。
- **虚拟摄像头** 为独立 COM `IMFMediaSource` DLL，须 MSVC 在 Windows 上编译并由 Frame Server 加载。

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
| 会议软件（Zoom/Teams 等）兼容性 | 不在 CI 内自动化；依赖人工 checklist |

## 相关文档

- [ARCH-PICOO-STACK-001](../design-specs/architecture/0001-rust-core-monorepo-boundary.md) — monorepo 与 xtask 边界
- [产品 PRD §19 构建与发布](../product/picoo-camera-prd-v1.0-2026-08-27.md)
- [AGENTS.md](../../AGENTS.md) — Cloud Agent 跨平台构建指令
