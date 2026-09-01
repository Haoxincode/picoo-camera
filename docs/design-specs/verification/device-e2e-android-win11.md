# Android Sender + Windows Receiver 真机 E2E 清单

> 关闭 BUC-001 / PRD §21 **功能验收**（Android→Windows）所需的人工步骤。
> CI loopback 不能替代本清单。

## 硬件与环境

- [ ] **PC**：Windows 11 x86_64，与手机同一 Wi‑Fi（非访客隔离）
- [ ] **手机**：Android 13+（建议含小米/16KB 页设备一台），Android 10+ 亦可
- [ ] **产物**：CI [ci-artifacts.md](ci-artifacts.md) 中的 `PicooCamera.msi` + `app-release.apk`（commit 与绿 CI 一致）
- [ ] **防火墙**：安装 MSI 时应添加 QUIC **4433** 例外；若发现失败检查 Defender 防火墙

## A. Windows 安装与 VCam

1. [ ] 安装 `PicooCamera.msi`（perMachine；`windows-bundle` 的松散文件不能替代系统安装）
2. [ ] 启动 `picoo-desktop.exe`（开始菜单 **Picoo Camera**）
3. [ ] 启动时不出现命令行窗口；贴边单层工作区、完整侧栏和连接页主内容均可见，顶部工具行最右侧显示最小化/最大化/关闭按钮，普通启动不要求管理员权限
4. [ ] 开启 Windows 辅助功能后分别观察空闲和直播状态，日志不持续出现 `gpui::window: RefCell already borrowed`
5. [ ] “虚拟摄像头”页状态：**就绪 (Ready)** / **Active**（非 `0x80040154`）
6. [ ] 打开 **Windows 相机** 或 **设置 → 蓝牙和设备 → 摄像头**，确认列表有 **Picoo Camera**
7. [ ] 未连接手机时，相机预览为占位（黑底 + Waiting for phone…）

## B. Android 安装与权限

1. [ ] 安装 `app-release.apk`（`com.picoo.camera`）
2. [ ] 首次打开 Sender，授予 **附近 Wi‑Fi 设备**（API 33+）、**通知**（API 33+）
3. [ ] 发现、手动 IP 直连和核对配对短码时不请求相机权限；仅在正式开始视频采集时按需授予 **相机**

## C. 首次配对（PUC-001）

1. [ ] 桌面 Receiver 等待页显示局域网 `IP:端口`，尚无未配对请求时不预生成短码
2. [ ] 手机 **Devices** 页通过 NSD 发现 PC
3. [ ] 点击尚未配对的电脑，手机与桌面显示相同的六位配对短码
4. [ ] 用户在两端分别确认数字一致后才建立信任并允许推流
5. [ ] 两端可按任意顺序确认；`PairingApproval → PairingCommit → PairingComplete` 完成双方持久化后才进入 Streaming / Live
6. [ ] 桌面直播页有预览；Windows 相机 / Picoo Camera 有画面（非占位）
7. [ ] **重复 10 次**「取消配对 → 重新配对」无卡死（验证 `PairingConfirm` 修复）

## D. 已配对自动连接（PUC-002）

1. [ ] 杀掉 Sender 进程，重开 → 自动连接已配对 PC（&lt;3s 量级目检）
2. [ ] 桌面关闭窗口留托盘，手机重连 → 恢复 Streaming

## E. 手动 IP 直连兜底（PUC-008）

1. [ ] 关闭或屏蔽 mDNS，记录桌面等待页显示的局域网 `IP:端口`
2. [ ] 手机空态选择 **输入 IP 地址直连**，输入该地址后到达双端短码核对页
3. [ ] 两端短码一致并分别确认，后续 Pairing/Streaming 与 mDNS 路径一致
4. [ ] 地址错误时只提示连接失败，不建立信任；连接流程全程不出现扫码入口或提前请求相机权限

## F. 直播中控制（PUC-005）

| 操作 | 通过 |
| --- | --- |
| 手机切前置/后置（翻转钮 180° 动画） | [ ] 3s 内恢复，画面方向正确 |
| 预览区单击 / 双击 | [ ] 对焦环闪现；双击进入/退出沉浸（HUD 隐藏） |
| 手机或桌面切 1080p → 720p（画质药丸点按轮换） | [ ] 分辨率变化，弱网时可降至 720p |
| 手机或桌面切 720p ↔ 1080p | [ ] 分辨率变化，无明显崩溃 |
| 远端镜像开关 | [ ] 会议/预览与预期一致 |
| EV ± | [ ] 亮度变化可见 |
| 手机过热（或模拟低电量） | [ ] 提示 + 强制 720p（thermal downgrade） |

## G. 断网恢复（PUC-006）

1. [ ] Streaming 中关闭手机 Wi‑Fi 5–10s → 状态 Network unstable / Reconnecting
2. [ ] 重连遮罩显示退避阶梯文案（`QUIC 会话重试中 · 第 N 次 (500ms/1s/2s/5s)`）
3. [ ] 重新打开 Wi‑Fi → **&lt;5s** 恢复画面，无需重新核对配对短码
4. [ ] 桌面 Live 预览与 VCam 同步恢复

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
| 两端短码不一致或已过期 | 是否连错 PC、是否跨连接保留了旧挑战、两端是否显示同一次握手的短码 |
| VCam 列表无 Picoo Camera | 重装 MSI（管理员）；日志无 `0x80040154`；或“虚拟摄像头”页 → **安装或修复…**，由桌面端通过 UAC 修复 COM 注册并执行 MF 注册 |
| 日志 `0x80040154` / 类未注册 | 确认 MSI tip ≥ `83ca647`；管理员安装；DLL 与 exe 同目录；见 [vcam-meeting-apps.md](vcam-meeting-apps.md) §0 |
| VCam 有设备但仍是 Disconnected 占位 | 桌面是否在 Streaming；`picoo-vcam-ring-reader.exe` 是否附着 `%ProgramData%\Picoo Camera\frame-ring-*.bin` 且 `seq` 递增；`icacls "$env:ProgramData\Picoo Camera"` 是否包含 `LOCAL SERVICE` / `Users` 继承读写 ACL |
| Android 安装后闪退 | 16KB 页设备需 CI 绿 run 的 APK（含 16KB 对齐 `.so`） |
