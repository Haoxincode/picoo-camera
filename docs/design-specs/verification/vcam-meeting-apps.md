# REQ-PICOO-VCAM-005/009：会议软件与 MSI 升级验收清单（Win11）

本清单用于在 **Windows 11 x86_64** 真机上验证 PUC-004 / PRD §21：安装后可在目标会议软件中选用「Picoo Camera」。

> 状态：`proposed/planned` → 对应部分全部勾选并附日志/截图后更新 [vcam.md](../requirements/vcam.md) 中 REQ-PICOO-VCAM-005/009。
>
> **CI 不能替代本清单。** `windows-latest` 验证 MSI/DLL 产物、PE/MSI FileVersion、安装动作表与友好名称字符串嵌入（`scripts/verify_windows_bundle.ps1`）；真实 major upgrade、会议软件枚举与画面必须在真机完成。

**前置 E2E**：请先完成 [device-e2e-android-win11.md](device-e2e-android-win11.md) 的 A–H，确保 Android→Windows Streaming 与桌面预览正常，再测会议软件。

## 0. 安装产物

从 CI 下载（见 [ci-artifacts.md](ci-artifacts.md)）：

| 文件 | 说明 |
| --- | --- |
| `PicooCamera.msi` | 推荐：安装文件 + COM 注册（WiX 注册表）+ 防火墙规则 + **安装结束时自动 MF 注册**（`--register-vcam --no-wait`） |

**MSI 安装要求**：Windows 11 x64、**以管理员身份**运行安装程序（perMachine 包）。COM CLSID 与 `InprocServer32` 由 WiX 声明式写入；`InstallFiles` 后调用 `picoo-desktop --register-vcam --no-wait` 注册 MF（system lifetime）。若仍见 `0x80040154`（类未注册），桌面端“虚拟摄像头”页的“安装或修复…”操作会通过 UAC 检查并修复同一组 COM 注册表值；也可手动：

```powershell
# 若 MSI 自动 MF 注册失败时的补救（管理员 PowerShell，路径按实际安装目录）
cd "C:\Program Files\Picoo Camera"
.\picoo-desktop.exe --register-vcam --no-wait
```

`windows-bundle` 的松散 exe/DLL 仅供 CI 编译、导出与加载 smoke，不能替代 MSI，也不允许从用户可写目录注册系统 COM。

**MSI 诊断日志**（安装失败时）：

```powershell
msiexec /i PicooCamera.msi /l*v "$env:TEMP\picoo-camera-install.log"
# 在日志中搜索 Return value 3、Error 1722、UnregisterVcamOnRemove、
# 0xc0000005、RemoveExistingProducts、RegisterVcamOnInstall
```

若日志含 `MsiSystemRebootPending = 1`，先重启 Windows，再执行安装/升级验收；该状态本身不是已确认的 0.1.491 `1603` 根因，但会污染复测环境。

## 0.1 从 0.1.490 原位升级（REQ-PICOO-VCAM-009，必做）

此项必须从已安装的 `0.1.490` ProductCode `{2EA538DD-3324-4768-8367-FB78632D0E72}` 起步，不能用全新安装替代。

```powershell
$msi = (Resolve-Path .\PicooCamera.msi).Path
$log = "$env:USERPROFILE\Desktop\PicooCamera-upgrade.log"
$p = Start-Process msiexec.exe -ArgumentList @("/i", "`"$msi`"", "/L*V", "`"$log`"") -Wait -PassThru
$p.ExitCode
```

- [ ] 升级退出码为 `0`；日志没有 `Return value 3`、`0xc0000005` 或 rollback
- [ ] 日志顺序为新版 `InstallFiles` / `InstallExecute` → `RemoveExistingProducts` → `RegisterVcamOnInstall`
- [ ] ARP 只保留新版本；旧 ProductCode 查询为空
- [ ] `picoo-desktop.exe`、`PicooVirtualCameraSource.dll`、`picoo-vcam-ring-reader.exe` 的 PE FileVersion 与新 MSI build 一致，哈希均来自新包
- [ ] 64 位 COM `InprocServer32` 指向当前安装目录中的 DLL
- [ ] Media Foundation 只枚举一个持久化 exact symbolic link，桌面状态为 Active，会议软件可选用
- [ ] 再运行同版 repair 后 identity 不消失、不重复
- [ ] 管理员终端连续执行三次 `picoo-desktop.exe --unregister-vcam`，均不崩溃且 exit `0`；随后执行一次 `--register-vcam --no-wait` 恢复设备
- [ ] 显式卸载新版本 exit `0`，设备、COM、文件与 ARP 项均清理

升级失败路径需保留完整 verbose log；应能看到 `RestoreVcamOnUpgradeRollback` 在新版文件回滚前尝试恢复旧产品 identity。该动作是 best-effort，若底层 MF 故障仍然存在，MSI 必须恢复旧产品文件与注册，设备再由显式修复收敛。

## 1. 系统级预检（必做）

在打开任何会议软件之前：

1. [ ] 启动 **picoo-desktop**，确认托盘图标存在
2. [ ] **设置 → 蓝牙和设备 → 摄像头**（或 **Windows 相机**）→ 下拉选择名称中包含 **Picoo Camera** 的设备；Windows 可能显示为 `Picoo Camera (Windows Virtual Camera)`
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
   - [ ] 用 DebugView/调试器记录 `Picoo VCam metrics` 至少 10 秒；声明 30 FPS 时
         `requests_per_sec` 不应持续达到数百或上千，并保存 fresh/cached/placeholder
         与 delivery 耗时
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
- [ ] **卸载 MSI**：重启会议软件后列表**不再**出现 Picoo Camera
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
2. 在本清单记录各应用的版本、枚举结果与证据路径

## CI 可自动完成的前置（非本 REQ 关闭条件）

- [x] MSI 由 `PICOO_REQUIRE_MSI=1` 强制产出
- [x] DLL UTF-16 嵌入 `Picoo Camera`（`verify_windows_bundle.ps1`）
- [ ] 真机安装后系统相机枚举（§1 仍属人工）

## 故障排查

| 现象 | 处理 |
| --- | --- |
| MSI 报 setup program did not finish | 确认管理员安装；查看 `%TEMP%\picoo-camera-install.log` 搜索 `RegisterVcamOnInstall` / `WixQuietExec`；若文件已复制到 Program Files，按 §0 手动运行安装目录中的 `picoo-desktop --register-vcam --no-wait` |
| 列表有 Picoo Camera 但黑屏 | 确认 desktop Streaming；运行 `picoo-vcam-ring-reader.exe` |
| 只有 Integrated Camera | 重启应用；检查 MSI 安装；系统相机是否可见 Picoo |
| Zoom 报摄像头被占用 | 关闭 Windows 相机 App 与其他占用 VCam 的程序 |
| Teams 缓存旧设备 | 退出 Teams → `%appdata%\Microsoft\Teams` 清缓存（可选）→ 重登 |
| 腾讯会议无 1080p | 选 1080p 后看 PC 预览是否正常；可能是 App 自身限制 |
| OBS 帧率不稳 | 输出 30fps；关闭「激活停用源时重新启动」 |
