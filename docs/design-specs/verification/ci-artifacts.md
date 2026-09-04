# CI 产物下载与目录结构

> 映射 **REQ-PICOO-STACK-005 / REQ-PICOO-STACK-007 / REQ-PICOO-STACK-008**。普通构建 Workflow：`.github/workflows/ci.yml`；正式发布 Workflow：`release-android.yml`、`release-windows.yml`、`release-apple.yml`。

## 最近绿 run（四平台构建基线）

| 字段 | 值 |
| --- | --- |
| Run ID | [33276518983](https://github.com/Haoxincode/picoo-camera/actions/runs/33276518983) |
| 分支 | `main` |
| Commit | `21b32d2` — iOS App/Simulator CI validation |
| 结论 | 5/5 jobs success（`rust-and-docs` + `android` + `windows` + `macos` + `ios`） |

该 run 已远端验证 `ios-app-unsigned`、XCFramework、Swift/C ABI Simulator XCTest 和 macOS GPUI ARM64 Receiver，因此作为 `REQ-PICOO-STACK-003 / REQ-PICOO-STACK-007` 的 `verified` 证据。

## 方式一：GitHub Web UI

1. 打开 https://github.com/Haoxincode/picoo-camera/actions
2. 筛选分支 `main`
3. 点选 **绿色** 的 `CI` workflow run（commit 与 tip 一致）
4. 滚动到 **Artifacts**，下载所需 zip（保留约 90 天）

## 方式二：GitHub CLI

需已 `gh auth login` 且对仓库有 read 权限。

```bash
# 列出某 run 的 artifact 名称
gh api repos/Haoxincode/picoo-camera/actions/runs/33276518983/artifacts \
  --jq '.artifacts[] | "\(.name)\t\(.size_in_bytes) bytes"'

# 下载全部 artifact 到当前目录
gh run download 33276518983 -R Haoxincode/picoo-camera

# 只下载 Windows MSI
gh run download 33276518983 -R Haoxincode/picoo-camera -n windows-msi
```

替换上方 run ID 时，可用以下命令查询最新绿 run：

```bash
gh run list --branch main --limit 1 --json databaseId,conclusion,headSha \
  --jq '.[] | select(.conclusion=="success") | .databaseId'
```

## Artifact 清单

| Artifact 名 | 约大小 | zip 内路径 | 用途 |
| --- | --- | --- | --- |
| `android-apk-debug` | ~10 MB | `app-debug.apk` | 普通 CI 快速迭代，application ID 为 `com.picoo.camera.debug` |
| `android-signed-release` | ~18 MB | `app-release.apk` | 受保护 Android Release workflow 产物，**真机发行/覆盖升级验证首选** |
| | | `app-release.aab` | 同一稳定 signer 的 Play 分发形态（sideload 用 APK） |
| | | `android-release.spdx.json` | Anchore Syft 生成的 SPDX SBOM；APK/AAB 另附 GitHub provenance attestation |
| `windows-msi` | ~8 MB | `PicooCamera.msi` | **Win11 安装首选** |
| `windows-bundle` | ~18 MB | 见下表 | 开发态 / 免安装验证 |
| `windows-signed-release` | 待首次发布记录 | `bundle/PicooCamera.msi` + 三个 Authenticode PE + `windows-release-identity.json` + SPDX SBOM | 受保护 Windows Release workflow 正式发行；真机安装首选 |
| `macos-app-unsigned` | 待 CI 记录 | `PicooCamera-macOS-unsigned.zip` + `PicooCamera-macOS.entitlements` | macOS 15+ ARM64 Host `.app` 与已展开签名输入 scaffold；未签名、未公证、不可激活 |
| `ios-rust-core-xcframework` | ~30 MB | `PicooCore.xcframework.zip` | iOS 18+ ARM64 device/simulator Rust C ABI；解压后保留 `.xcframework` 外层目录 |
| `ios-app-unsigned` | ~2.5 MB | `PicooCamera.app.zip` | iOS 18+ ARM64 Simulator SwiftUI/C ABI 编译基线；解压后保留 `.app` 与执行权限，不可安装到真机 |
| `macos-signed-notarized-release` | 待首次发布记录 | `PicooCamera-macOS.zip` + `macos-release.spdx.json` | Developer ID 签名、Hardened Runtime、公证并 staple 的正式 macOS 产物；zip 另附 GitHub provenance |
| `ios-signed-app-store-release` | 待首次发布记录 | `PicooCamera-iOS.ipa` + `ios-release.spdx.json` | Apple Distribution 签名并绑定 App Store profile 的 device ARM64 IPA；IPA 另附 GitHub provenance |

普通 CI 的 Apple artifact 只证明原生链接、SwiftUI App、macOS Host/Camera Extension 结构和边界打包成功。受保护 Release artifact 增加签名、身份与公证门禁，但系统扩展激活、覆盖安装、App Store Connect 和真机媒体链路仍必须由对应 Requirement 验收。

### `macos-app-unsigned` 解压后布局

```text
Picoo Camera.app/
└── Contents/
    ├── Info.plist
    ├── MacOS/picoo-desktop
    ├── Resources/PicooCamera.icns
    └── Library/SystemExtensions/
        └── com.haoxincode.picoo-camera.camera-extension.systemextension/
```

该 bundle 已通过 `package macos` 的 Bundle ID、App Group、Host 签名输入、ARM64 slice
与扩展禁止依赖门禁。它不能替代 Developer ID 签名、公证或 `/Applications` 中的用户
批准与激活验证。

### `windows-bundle` 解压后布局

```text
windows-bundle/
├── picoo-desktop.exe              # GPUI Receiver；松散运行仅用于 smoke
├── PicooCamera.ico                 # WiX 已安装应用图标输入
├── picoo-vcam-ring-reader.exe     # Shared Frame Ring 诊断
├── PicooVirtualCameraSource.dll   # MF IMFMediaSource（UTF-16「Picoo Camera」已嵌入）
└── msi/
    ├── PicooCamera.msi            # 与 windows-msi artifact 相同
    └── PicooCamera.version        # CI 单调递增的 MSI ProductVersion
```

CI 在打包后运行 `scripts/verify_windows_bundle.ps1`：校验 exe/dll/msi 与产品 `.ico` 存在、
EXE 可提取应用图标、DLL 含 UTF-16 `Picoo Camera`（REQ-VCAM-001），并扫描 MSI 含
`PicooProductIcon`、CLSID、`InprocServer32` 与 `--register-vcam --no-wait`，同时禁止自注册
CustomAction 与 `DllRegisterServer`（REQ-VCAM-004 / REQ-PICOO-UI-013）；同时读取 MSI
Property 表，确保 `ProductVersion` 与 `PicooCamera.version` 一致。CI 使用 workspace
Major/Minor 加 `github.run_number` 生成三字段版本，保证后生成的安装包可以替换早期产物。
松散 bundle 仅用于
编译、导出与加载 smoke，不能写系统 COM 注册；**不**在 CI 上执行 `msiexec /i`（perMachine
需 Win11 管理员真机验收）。

### `android-signed-release` 解压后布局

```text
android-signed-release/
├── app-release.apk                # com.picoo.camera，版本来自 workspace/build number
├── app-release.aab
└── android-release.spdx.json
```

安装 APK（启用未知来源后）：

```bash
adb install -r app-release.apk
```

普通 `ci.yml` 不再生成 debug-key 签名的伪 Release。正式产物由 `release-android.yml` 的受保护
Environment 注入稳定 keystore，并在上传前核对 APK/AAB certificate SHA-256、application ID、
versionName 与 versionCode；缺少任一签名输入时 Gradle Release task 直接失败。

## 真机最小组合

| 平台 | 文件 | 安装 |
| --- | --- | --- |
| Windows 11 | `PicooCamera.msi` | **管理员**双击安装（perMachine；WiX 写入 COM CLSID + 防火墙 + 安装时自动 MF 注册）。失败时见 [vcam-meeting-apps.md](vcam-meeting-apps.md) §0；不要用松散 bundle 替代安装。 |
| Android | `app-release.apk` | adb 或文件管理器安装 |

安装完成后按 [device-e2e-android-win11.md](device-e2e-android-win11.md) 走通配对与 Streaming。

## 签名说明

普通 `ci.yml` 只产出 Debug Android 与未签名 Windows/Apple 工程验证包。稳定发行身份仅由三个受保护 Release workflow 注入：Android 固定 keystore、Windows Authenticode certificate、macOS Developer ID + Notary、iOS Apple Distribution + App Store profile。所有正式产物附 SPDX SBOM 与 GitHub provenance；保护凭据首次绿测和真实覆盖升级仍需另行记录（见 [ci-and-build.md](../../development/ci-and-build.md) §Secrets）。
