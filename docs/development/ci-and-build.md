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
├── picoo-testkit QUIC 边界测试 + picoo-sim 虚拟时钟全链路模拟
├── Android APK/AAB（NDK + Gradle）
├── .github/workflows/ 维护与 CI 修复
└── push 后订阅 CI 结果并迭代

GitHub Actions
├── ubuntu-latest   → Rust 测试、Android 构建、文档校验
├── windows-latest  → GPUI 桌面、MF 解码、Virtual Camera DLL、安装包
└── macos-26 ARM64 → GPUI 桌面、CMIO Camera Extension、iOS Rust XCFramework 与 SwiftUI App；release workflow 负责 macOS 签名/公证，Apple 真机链路另行验收
```

## GitHub Actions Runner 矩阵

与 PRD §19.2、`cargo xtask` 命令及当前四端实现边界对齐：

| Job | Runner | 职责 | xtask 命令 |
| --- | --- | --- | --- |
| `rust-and-docs` | `ubuntu-latest` | workspace 测试、clippy、文档链接校验 | `cargo test --workspace`、`scripts/check-docs.sh` |
| `nightly-validation` | `ubuntu-latest` | PCP parser/state fuzz、30 分钟 paired loopback soak、Shared Ring/FFI Miri 与原子协议 Loom model | `cargo xtask test fuzz/soak/miri/loom` |
| `android` | `ubuntu-latest` | 独立 application ID 的 Android Sender Debug APK | `cargo xtask build android` |
| `windows` | `windows-latest` | 桌面 exe、VCam DLL、安装包 | `cargo xtask build windows`、`cargo xtask package windows` |
| `Windows VCam host contract` | `self-hosted, Windows, X64, picoo-vcam`（专用管理员 Win11 client） | MSI 安装/repair/卸载、exact-link 枚举、MF Source 激活与 Start/Stop/Shutdown | `cargo xtask package windows`、`scripts/test_windows_vcam_host.ps1` |
| `macos` | `macos-26` ARM64 + Xcode 26.6 | 共享 GPUI Receiver、VideoToolbox→NV12 原生解码、Rust Writer↔Swift/C Reader 跨进程恢复、Swift 6 CMIO Camera Extension 与 Host `.app` 无签名打包 | `cargo clippy -p picoo-desktop --all-targets --features gpui-ui -- -D warnings`；`cargo xtask test macos`；`cargo xtask package macos` |
| `ios` | `macos-26` ARM64 + Xcode 26.6 | Rust Core device/simulator XCFramework、SwiftUI App ARM64 编译链接、Simulator C ABI 单测 | `cargo xtask build ios`；`cargo xtask test ios` |
| `Apple Release / macos` | `macos-26` ARM64 + Xcode 26.6 | 递增 Host/Extension 版本；Developer ID profile/授权证书/effective entitlements 校验；Hardened Runtime 签名、Notary Service 公证与 staple | `cargo xtask release macos`；首次真实凭据绿测与真机激活仍是独立验收 |

Nightly validation 使用 `nightly-2026-09-03`，并由 `xtask` 持有同一常量；更新 Miri 或
cargo-fuzz 工具链时必须同时本地复核 strict-provenance、四个 fuzz target 和 workflow。

`windows-latest` 可能是 Windows Server/headless 环境，只证明 bundle、MSI 数据库与 DLL
进程内契约，不冒充 Windows 11 Frame Server 主机。独立 `windows-vcam-host.yml` 不作为 PR
必需检查；它只调度带 `picoo-vcam` 标签、管理员权限、交互式非 Session 0 登录且启用 Media
Foundation Frame Server 的专用 Windows 11 client runner。脚本要求 runner 开始时不存在 Picoo 安装，且 finally 只以本次 MSI 做有界
卸载清理，避免接管不属于当前 run 的系统状态。首次真实 runner 绿测前，相应 Requirement 只
能标记 `implemented`。

### 依赖关系

- `android` / `windows` / `macos` / `ios` 与 `rust-and-docs` **并行**：平台产物不等待通用测试矩阵；同一 ref 的新 push 仍由 concurrency 取消旧 run。
- 各 job 通过 `actions/upload-artifact` 上传产物（APK、MSI、DLL 等），供人工验证或后续 release workflow 消费。
- 四个平台的用户版本来自 workspace SemVer；普通 CI 将同一个 `github.run_number` 注入 `PICOO_BUILD_NUMBER`：Android Debug 用作 `versionCode`，iOS/macOS 用作 `CFBundleVersion`，Windows 与 SemVer Major/Minor 组合为三字段 MSI `ProductVersion`，并同步写入 desktop、MF Source 与 ring reader 的四字段 PE `FileVersion`。Android 正式 APK/AAB 只由 `release-android.yml` 在受保护 Environment 中注入稳定 keystore 后调用 `xtask package android`；Gradle 遇到任意 Release task 且签名输入不完整时立即失败。`xtask` 负责版本边界和范围校验，WiX 不硬编码版本；Windows CI 还查询 MSI `File`、`InstallExecuteSequence` 与 `CustomAction` 表，强制 late MajorUpgrade 的受限窗口只包含 `RemoveExistingProducts`，并运行 ICE27/ICE63/ICE77。最终虚拟摄像头注册在 `InstallExecute` 前写入 commit script，于旧产品移除并成功提交后执行。因此较新的 CI 安装包会执行平台原生升级，不会保留旧二进制。macOS 签名发布仍可用经过校验的 `PICOO_RELEASE_BUILD_NUMBER` 显式覆盖普通 CI 构建号。
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

已记录的远端绿测证明共享 GPUI Receiver、Rust XCFramework、SwiftUI App、Simulator C ABI 生命周期测试和 iOS 原生媒体源码的 Apple 原生编译、链接边界。macOS VideoToolbox 解码由 `xtask test macos` 使用仓库内静态真实 H.264 IDR 验证 `CMSampleBuffer → 420v NV12`、AVCC Receiver 链路以及 720p→480p ABR/epoch/FrameHub 恢复，并检查产品依赖树不含 OpenH264/CMake；macOS 测试依赖也不编译 OpenH264。该命令还会直接编译 Camera Extension 使用的生产 Swift 6 Reader 与 C17 原子边界，在独立进程中验证 Rust Writer 并发覆盖、NV12 完整性、Reader/Producer 异常退出后的租约恢复和单 Producer 生命周期锁。`xtask package macos` 编译 ARM64 CMIO Camera Extension，将其按 Bundle ID 命名并嵌入 Host `.app` 的 `Contents/Library/SystemExtensions/`，同时检查 Host/Extension 身份、App Group、安装扩展签名输入、架构 slice 以及扩展不链接 QUIC/Decoder。静态样本与跨进程 harness 让该验收不依赖 CMake 或外部编码器。这些证据都不替代签名 App Group 读写、系统扩展激活、公证、会议软件枚举或 iPhone→macOS 真机媒体链路验收。

### Apple 无签名构建基线

Apple 基线保持三个独立 artifact：

- `macos-app-unsigned`：`PicooCamera-macOS-unsigned.zip` 包含 ARM64 `Picoo Camera.app`，Camera Extension 已嵌入标准目录，品牌 `PicooCamera.icns` 位于 Host `Contents/Resources/` 并由 `CFBundleIconFile` 引用；同一 artifact 还包含已展开的 Host 与 Extension entitlements 签名输入 scaffold。无签名构建使用 `UNSIGNED.` Team 前缀和独立 Host Info.plist marker，Shared Ring 降级到 Application Support，不能完成系统激活。
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
| `ANDROID_KEYSTORE_BASE64` / `ANDROID_KEYSTORE_PASSWORD` / `ANDROID_KEY_ALIAS` / `ANDROID_KEY_PASSWORD` | Android Release 稳定签名 | 发布 APK/AAB 前，缺一即失败 |
| `ANDROID_SIGNER_SHA256` | Android release certificate 固定指纹 | 签名后 APK/AAB 双重核验 |
| `WINDOWS_CERTIFICATE` | Windows 安装包代码签名 | 可选，发布前建议 |
| `APPLE_DEVELOPER_ID_P12_BASE64` / `APPLE_DEVELOPER_ID_P12_PASSWORD` / `APPLE_KEYCHAIN_PASSWORD` | 导入临时 Developer ID Application identity | macOS 发布 |
| `APPLE_TEAM_ID` / `APPLE_MACOS_SIGNING_IDENTITY` | 校验并选择 Host/Extension 共用签名团队与 identity | macOS 发布 |
| `APPLE_MACOS_HOST_PROFILE_BASE64` / `APPLE_MACOS_EXTENSION_PROFILE_BASE64` | 授权 Host 与 Camera Extension 的 Bundle ID、App Group 和 System Extension capability | macOS 发布 |
| `APPLE_NOTARY_KEY_BASE64` / `APPLE_NOTARY_KEY_ID` / `APPLE_NOTARY_ISSUER_ID` | `notarytool` App Store Connect API Key | macOS 发布 |

Apple Release 手动触发时还必须填写一至三段数字的 marketing version 与严格递增的正整数 build number；tag `vX.Y.Z` 触发时从 tag 取得 marketing version，并使用单调递增的 GitHub run number 作为 build number。两者分别注入 `PICOO_RELEASE_VERSION` 与 `PICOO_RELEASE_BUILD_NUMBER`，不是 secret。

未配置签名 secret 时，普通 CI 仍产出 `com.picoo.camera.debug` Debug APK；不得产出使用 debug key
冒充正式身份的 Release APK/AAB。

Android 产物签名与包元数据使用 Android SDK `apksigner` / `apkanalyzer` 和 JDK `jarsigner` /
`keytool` 验证。SBOM 采用维护活跃、Apache-2.0 的 Anchore Syft Action，provenance 采用 GitHub
官方 artifact attestation；release workflow 中所有 Action 固定到审核过的 commit SHA。

## 与 xtask 的边界

- **xtask**（见 ARCH-PICOO-STACK-001）：封装各平台 build/package 命令，供本地与 CI 统一调用。
- **GitHub Actions**：编排 runner、缓存、artifact 上传与 job 依赖；不替代 xtask 中的平台构建逻辑。
- CI workflow 应调用 `cargo xtask …`，避免在 YAML 中复制各平台构建细节。

## 验证范围

| 验证类型 | 执行位置 |
| --- | --- |
| Rust 单元/集成/协议测试 | `ubuntu-latest`（Cloud Agent 本地亦可） |
| Android 安装与采集发送 | CI artifact + 真机（人工或后续设备 farm） |
| Windows 安装与虚拟摄像头枚举 | `windows-latest` 构建/静态契约 + 专用 self-hosted Win11 Host Contract；系统相机 UI 与会议软件仍人工验证 |
| 会议软件（Zoom/Teams 等）兼容性 | 不在 CI 内自动化；[会议软件验收清单](../design-specs/verification/vcam-meeting-apps.md) |
| Android→Windows 真机 E2E | [真机 E2E 清单](../design-specs/verification/device-e2e-android-win11.md) |

## 相关文档

- [ARCH-PICOO-STACK-001](../design-specs/architecture/0001-rust-core-monorepo-boundary.md) — monorepo 与 xtask 边界
- [产品 PRD §19 构建与发布](../product/picoo-camera-prd-v1.0-2026-08-27.md)
- [AGENTS.md](../../AGENTS.md) — Cloud Agent 跨平台构建指令
