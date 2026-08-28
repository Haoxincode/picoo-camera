# Android Sender + Windows Receiver 真机 E2E 清单

> 关闭 BUC-001 / PRD §21 **功能验收**（Android→Windows）所需的人工步骤。  
> CI loopback 不能替代本清单。

## 硬件与环境

- [ ] **PC**：Windows 11 x86_64，与手机同一 Wi‑Fi（非访客隔离）
- [ ] **手机**：Android 13+（建议含小米/16KB 页设备一台），Android 10+ 亦可
- [ ] **产物**：CI [ci-artifacts.md](ci-artifacts.md) 中的 `PicooCamera.msi` + `app-release.apk`（commit 与绿 CI 一致）
- [ ] **防火墙**：安装 MSI 时应添加 QUIC **4433** 例外；若发现失败检查 Defender 防火墙

## A. Windows 安装与 VCam

1. [ ] 安装 `PicooCamera.msi`（或解压 `windows-bundle` 后以管理员运行 `register-vcam.ps1`）
2. [ ] 启动 `picoo-desktop.exe`（开始菜单 **Picoo Camera**）
3. [ ] 设置页 VCam 状态：**Installed / Active**
4. [ ] 打开 **Windows 相机** 或 **设置 → 蓝牙和设备 → 摄像头**，确认列表有 **Picoo Camera**
5. [ ] 未连接手机时，相机预览为占位（黑底 + Waiting for phone…）

## B. Android 安装与权限

1. [ ] 安装 `app-release.apk`（`com.picoo.camera`）
2. [ ] 首次打开 Sender，授予 **附近 Wi‑Fi 设备**（API 33+）、**通知**（API 33+）
3. [ ] 点击 Connect / Scan QR / Enable preview 时按需授予 **相机**

## C. 首次配对（PUC-001）

1. [ ] 桌面 Receiver 显示等待连接 / QR 入口
2. [ ] 手机 **Devices** 页 NSD 发现 PC（或空态走 **Scan QR**）
3. [ ] 点击连接 → 双方显示 **相同六位短码**
4. [ ] 手机确认、桌面确认 → 进入 Streaming / Live
5. [ ] 桌面直播页有预览；Windows 相机 / Picoo Camera 有画面（非占位）
6. [ ] **重复 10 次**「取消配对 → 重新配对」无卡死（验证 `PairingConfirm` 修复）

## D. 已配对自动连接（PUC-002）

1. [ ] 杀掉 Sender 进程，重开 → 自动连接已配对 PC（&lt;3s 量级目检）
2. [ ] 桌面关闭窗口留托盘，手机重连 → 恢复 Streaming

## E. 二维码兜底（PUC-003）

1. [ ] 桌面 **Show QR Code**
2. [ ] 手机 **Scan QR**（或粘贴 JSON）→ 配对/Streaming 与 mDNS 路径一致
3. [ ] （可选）等 QR 过期后重扫，应失败或桌面自动刷新

## F. 直播中控制（PUC-005）

| 操作 | 通过 |
| --- | --- |
| 手机切前置/后置 | [ ] 3s 内恢复，画面方向正确 |
| 手机或桌面切 720p ↔ 1080p | [ ] 分辨率变化，无明显崩溃 |
| 远端镜像开关 | [ ] 会议/预览与预期一致 |
| EV ± | [ ] 亮度变化可见 |
| 手机过热（或模拟低电量） | [ ] 提示 + 强制 720p（thermal downgrade） |

## G. 断网恢复（PUC-006）

1. [ ] Streaming 中关闭手机 Wi‑Fi 5–10s → 状态 Network unstable / Reconnecting
2. [ ] 重新打开 Wi‑Fi → **&lt;5s** 恢复画面，无需重新输入配对码
3. [ ] 桌面 Live 预览与 VCam 同步恢复

## H. 配对管理（PUC-007）

1. [ ] 手机删除单条已配对 PC → 下次连接需重新六位码
2. [ ] 手机 Remove all / 桌面清除信任 → 双端一致

## I. 性能抽样（PRD §21，非完整 2h）

| 项 | 方法 | 通过 |
| --- | --- | --- |
| E2E 延迟 | 手机镜头前摆钟/秒表，对比 PC 预览 | [ ] 健康 Wi‑Fi 主观 &lt;250ms |
| 2h 长稳 | 1080p30 连续 2h | [ ] 无崩溃；任务管理器内存无明显斜坡 |
| 5% 丢包 | （可选）路由器限速/干扰 | [ ] 仍可用，恢复后无秒级延迟累积 |

完整 2h loopback 中间证据见 [session-005-soak-loopback.md](session-005-soak-loopback.md)；**真机 2h 通过后方可把 SESSION-005 升为 `verified`**。

## J. 会议软件（PUC-004 / VCAM-005）

在 C–G 通过后，继续 [vcam-meeting-apps.md](vcam-meeting-apps.md)。**全部勾选方可关闭 V1。**

## 证据存放

- 截图/录屏 → `docs/design-specs/verification/artifacts/`（本地，**勿提交含人脸素材**）
- 或 attach 到 PR #10 / 发布说明
- 文件名建议：`{date}-{device}-{scenario}.mp4`（例：`20260828-mi15-pairing.mp4`）

## 故障排查

| 现象 | 检查 |
| --- | --- |
| 手机发现不到 PC | 同网段、4433 防火墙、PC 托盘运行中、Android 13+ Nearby Wi‑Fi |
| 配对码不一致 | 两端时钟、是否连错 PC、QR nonce 是否过期 |
| VCam 列表无 Picoo Camera | 重装 MSI；管理员 `register-vcam.ps1`；重启 Frame Server 服务 |
| VCam 有设备但黑屏 | Shared Frame Ring；桌面是否在 Streaming；`picoo-vcam-ring-reader.exe` |
| Android 安装后闪退 | 16KB 页设备需 CI 绿 run 的 APK（含 16KB 对齐 `.so`） |
