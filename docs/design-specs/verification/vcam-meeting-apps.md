# REQ-PICOO-VCAM-005：会议软件兼容验收清单（Win11）

本清单用于在 **Windows 11 x86_64** 真机上验证 PUC-004 / PRD §21：安装后可在目标会议软件中选用「Picoo Camera」。

> 状态：`proposed` → 全部勾选并附截图/录屏后改为 `verified`，并更新 [vcam.md](../requirements/vcam.md) 中 REQ-PICOO-VCAM-005。
>
> **CI 不能替代本清单。** `windows-latest` 仅验证 MSI/DLL 产物与友好名称字符串嵌入（`scripts/verify_windows_bundle.ps1`）；会议软件枚举与画面必须在真机完成。

**前置 E2E**：请先完成 [device-e2e-android-win11.md](device-e2e-android-win11.md) 的 A–H，确保 Android→Windows Streaming 与桌面预览正常，再测会议软件。

## 0. 安装产物

从 CI 下载（见 [ci-artifacts.md](ci-artifacts.md)）：

| 文件 | 说明 |
| --- | --- |
| `PicooCamera.msi` | 推荐：安装文件 + COM 注册（WiX 注册表）+ 防火墙规则 + **安装结束时自动 MF 注册**（`--register-vcam --no-wait`） |
| 或 `windows-bundle` 解压 | 开发态：`register-vcam.ps1`（**管理员** PowerShell） |

**MSI 安装要求**：Windows 11 x64、**以管理员身份**运行安装程序（perMachine 包）。COM CLSID 由 WiX 声明式注册表写入；`InstallFiles` 后 MSI 还会以 `RegisterVcamComDll`（`regsvr32.exe /s`，`Return=ignore`）兜底 COM，再调用 `picoo-desktop --register-vcam --no-wait` 注册 MF（system lifetime）。若仍见 `0x80040154`（类未注册），桌面启动时会自动尝试 `regsvr32`；亦可手动：

```powershell
# 开发态示例（在解压后的 bundle 目录）
Set-ExecutionPolicy -Scope Process Bypass
.\register-vcam.ps1
# 卸载：.\register-vcam.ps1 -Unregister

# 若 MSI 自动 MF 注册失败时的补救（管理员 PowerShell，路径按实际安装目录）
cd "C:\Program Files\Picoo Camera"
.\picoo-desktop.exe --register-vcam --no-wait
# 开发态或需重写 COM 键时仍可用 regsvr32：
# regsvr32 /s PicooVirtualCameraSource.dll
```

**MSI 诊断日志**（安装失败时）：

```powershell
msiexec /i PicooCamera.msi /l*v "$env:TEMP\picoo-camera-install.log"
# 在日志中搜索 Return value 3、Error 1722、FirewallQuic
```

## 1. 系统级预检（必做）

在打开任何会议软件之前：

1. [ ] 启动 **picoo-desktop**，确认托盘图标存在
2. [ ] **设置 → 蓝牙和设备 → 摄像头**（或 **Windows 相机**）→ 下拉选择 **Picoo Camera**
3. [ ] **未开 Streaming**：预览为占位（Waiting for phone…）
4. [ ] **Android 已 Streaming**：预览为手机画面，方向直立、无明显撕裂
5. [ ] 若列表无 Picoo Camera：重装 MSI → 重启「Windows Camera Frame Server」服务或重启 PC

记录预检截图：`vcam-win11-system-camera.png`

## 2. 通用测试步骤（每个应用重复）

1. 完全退出该应用（含托盘），再重新打开
2. 进入 **设置 → 视频/摄像头**（或加入会议前设备选择）
3. 摄像头列表中选择 **Picoo Camera**（勿选「Integrated Camera」）
4. 确认预览：
   - [ ] 画面来自手机（非笔记本内置头）
   - [ ] 直立、无 90° 错误旋转
   - [ ] 无明显卡顿（主观流畅）
5. **分辨率**：若应用可选 720p / 1080p，分别试一次
6. **断线恢复**：Streaming 中关手机 Wi‑Fi 10s → 开回 → 会议内预览应恢复或短暂占位后恢复
7. **占位**：停止 Streaming（手机 Disconnect）→ 会议内应显示品牌占位，**不崩溃**

## 3. 会议 / 采集软件矩阵

对每一项勾选下表；备注栏记录版本号与异常。

| 应用 | 版本 | 枚举到 Picoo Camera | 720p 可用 | 1080p 可用 | 占位画面 | 断线恢复 | 备注 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Zoom | | [ ] | [ ] | [ ] | [ ] | [ ] | 设置 → 视频 → 摄像头 |
| Microsoft Teams | | [ ] | [ ] | [ ] | [ ] | [ ] | 设置 → 设备 → 摄像头 |
| 腾讯会议 | | [ ] | [ ] | [ ] | [ ] | [ ] | 设置 → 视频 → 选择摄像头 |
| OBS Studio | | [ ] | [ ] | [ ] | [ ] | [ ] | 来源 → 视频采集设备 |
| 浏览器 | | [ ] | [ ] | [ ] | [ ] | [ ] | 见 §4 |

**OBS 提示**：添加「视频采集设备」→ 设备选 Picoo Camera；分辨率与 FPS 选 1280×720 或 1920×1080 @ 30。

## 4. 浏览器子项

任选一种即可，建议两种都测：

### A. Google Meet（meet.google.com）

1. [ ] Chrome/Edge 打开 Meet → 设置（齿轮）→ 视频 → 摄像头 **Picoo Camera**
2. [ ] 预览正常后开一场即时会议自测

### B. 本地 getUserMedia 页

在 PC 上保存并打开：

```html
<!DOCTYPE html>
<meta charset="utf-8">
<video id="v" autoplay playsinline muted style="width:100%;max-width:640px"></video>
<script>
navigator.mediaDevices.enumerateDevices().then(ds => {
  console.log(ds.filter(d => d.kind === 'videoinput'));
  return navigator.mediaDevices.getUserMedia({
    video: { deviceId: { exact: ds.find(d => d.label.includes('Picoo'))?.deviceId } }
  });
}).then(s => { v.srcObject = s; }).catch(e => alert(e));
</script>
```

- [ ] 控制台列出含 **Picoo Camera** 的 `videoinput`
- [ ] `<video>` 显示手机画面

## 5. 负面路径

- [ ] **未配对 / 无 Sender**：会议软件仍能打开 Picoo Camera，显示占位（Waiting for phone…）
- [ ] **卸载 MSI**（或 `register-vcam.ps1 -Unregister`）：重启会议软件后列表**不再**出现 Picoo Camera
- [ ] **公钥变化**（清除 PC 配对数据后重配）：旧信任不自动出画（需重新配对）

## 6. 证据与关闭条件

| 条件 | 要求 |
| --- | --- |
| 矩阵 | 上表 **5 应用 × 6 列** 全部 ✅ |
| 负面 | §5 全部 ✅ |
| 证据 | 每应用至少 1 张设备选择器截图 + 1 段 10s 预览录屏（可打码人脸） |
| 存放 | `docs/design-specs/verification/artifacts/` 或 PR #10 附件 |

全部完成后：

1. 将 [vcam.md](../requirements/vcam.md) 中 **REQ-PICOO-VCAM-005** 状态改为 `verified`
2. 在 [android-win-v1-gap-audit.md](android-win-v1-gap-audit.md) 更新 PUC-004 真机列

## CI 可自动完成的前置（非本 REQ 关闭条件）

- [x] MSI 由 `PICOO_REQUIRE_MSI=1` 强制产出
- [x] DLL UTF-16 嵌入 `Picoo Camera`（`verify_windows_bundle.ps1`）
- [ ] 真机安装后系统相机枚举（§1 仍属人工）

## 故障排查

| 现象 | 处理 |
| --- | --- |
| MSI 报 setup program did not finish | 确认管理员安装；查看 `%TEMP%\picoo-camera-install.log` 搜索 `RegisterVcamOnInstall` / `WixQuietExec`；若文件已复制成功，手动运行 `picoo-desktop --register-vcam --no-wait`；开发态可用 `register-vcam.ps1`（见 §0） |
| 列表有 Picoo Camera 但黑屏 | 确认 desktop Streaming；运行 `picoo-vcam-ring-reader.exe` |
| 只有 Integrated Camera | 重启应用；检查 MSI 安装；系统相机是否可见 Picoo |
| Zoom 报摄像头被占用 | 关闭 Windows 相机 App 与其他占用 VCam 的程序 |
| Teams 缓存旧设备 | 退出 Teams → `%appdata%\Microsoft\Teams` 清缓存（可选）→ 重登 |
| 腾讯会议无 1080p | 选 1080p 后看 PC 预览是否正常；可能是 App 自身限制 |
| OBS 帧率不稳 | 输出 30fps；关闭「激活停用源时重新启动」 |
