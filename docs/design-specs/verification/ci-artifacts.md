# CI 产物下载与目录结构

> 映射 **REQ-PICOO-STACK-005**。Workflow：`.github/workflows/ci.yml`

## 最新绿 run（示例）

| 字段 | 值 |
| --- | --- |
| Run ID | [33131999904](https://github.com/Haoxincode/picoo-camera/actions/runs/33131999904) |
| 分支 | `cursor/android-win-v1-gates-dbe3` |
| Commit | `3ae2569` — `fix(vcam): embed UTF-16 Picoo Camera friendly name in DLL image` |
| 结论 | 6/6 jobs success（`rust-and-docs` + `android` + `windows` × push/PR） |

后续 tip 绿 run 用法相同：在 Actions 页打开对应 run → 页面底部 **Artifacts** 区域下载。

## 方式一：GitHub Web UI

1. 打开 https://github.com/Haoxincode/picoo-camera/actions
2. 筛选分支 `cursor/android-win-v1-gates-dbe3` 或 PR #10
3. 点选 **绿色** 的 `CI` workflow run（commit 与 tip 一致）
4. 滚动到 **Artifacts**，下载所需 zip（保留约 90 天）

## 方式二：GitHub CLI

需已 `gh auth login` 且对仓库有 read 权限。

```bash
# 列出某 run 的 artifact 名称
gh api repos/Haoxincode/picoo-camera/actions/runs/33131999904/artifacts \
  --jq '.artifacts[] | "\(.name)\t\(.size_in_bytes) bytes"'

# 下载全部 artifact 到当前目录
gh run download 33131999904 -R Haoxincode/picoo-camera

# 只下载 Windows MSI
gh run download 33131999904 -R Haoxincode/picoo-camera -n windows-msi
```

替换 `33131999904` 为最新绿 run ID：

```bash
gh run list --branch cursor/android-win-v1-gates-dbe3 --limit 1 --json databaseId,conclusion,headSha \
  --jq '.[] | select(.conclusion=="success") | .databaseId'
```

## Artifact 清单

| Artifact 名 | 约大小 | zip 内路径 | 用途 |
| --- | --- | --- | --- |
| `android-apk-debug` | ~15 MB | `app-debug.apk` | 快速迭代调试 |
| `android-release` | ~27 MB | `app-release.apk` | **真机 V1 验证首选** |
| | | `app-release.aab` | Play 分发形态（ sideload 用 APK 即可） |
| `windows-msi` | ~8 MB | `PicooCamera.msi` | **Win11 安装首选** |
| `windows-bundle` | ~18 MB | 见下表 | 开发态 / 免安装验证 |

### `windows-bundle` 解压后布局

```text
windows-bundle/
├── picoo-desktop.exe              # GPUI Receiver + VCam 注册 CLI
├── picoo-vcam-ring-reader.exe     # Shared Frame Ring 诊断
├── PicooVirtualCameraSource.dll   # MF IMFMediaSource（UTF-16「Picoo Camera」已嵌入）
├── register-vcam.ps1              # 开发态 COM+MF 注册（MSI 已含安装步骤）
└── msi/
    └── PicooCamera.msi            # 与 windows-msi artifact 相同
```

CI 在打包后运行 `scripts/verify_windows_bundle.ps1`：校验 exe/dll/msi 存在、DLL 含 UTF-16 `Picoo Camera`（REQ-VCAM-001），并扫描 MSI 不含 `regsvr32.exe`/`RegisterVcamDll` 且含 CLSID（REQ-VCAM-004）。**不**在 CI 上执行 `msiexec /i`（perMachine 需 Win11 管理员真机验收）。

### `android-release` 解压后布局

```text
android-release/
├── app-release.apk                # com.picoo.camera v0.1.0, arm64-v8a
└── app-release.aab
```

安装 APK（启用未知来源后）：

```bash
adb install -r app-release.apk
```

## 真机最小组合

| 平台 | 文件 | 安装 |
| --- | --- | --- |
| Windows 11 | `PicooCamera.msi` | **管理员**双击安装（perMachine；WiX 写入 COM CLSID + 防火墙）。首次启动 `picoo-desktop` 完成 MF 注册。失败时见 [vcam-meeting-apps.md](vcam-meeting-apps.md) §0；或解压 bundle 后 **管理员**运行 `.\register-vcam.ps1` |
| Android | `app-release.apk` | adb 或文件管理器安装 |

安装完成后按 [device-e2e-android-win11.md](device-e2e-android-win11.md) 走通配对与 Streaming。

## 签名说明

当前 CI 产出为 **debug/未商店签名**（Android release 使用 debug keystore；Windows MSI 无 Authenticode）。功能验证足够；对外发布前需配置 GitHub Secrets 签名（见 [ci-and-build.md](../../development/ci-and-build.md) §Secrets）。
