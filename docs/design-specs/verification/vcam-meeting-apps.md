# REQ-PICOO-VCAM-005：会议软件兼容验收清单（Win11）

本清单用于在 **Windows 11 x86_64** 真机上验证 PUC-004 / PRD §21：安装后可在目标会议软件中选用「Picoo Camera」。

> 状态：`proposed` → 全部勾选并附截图/录屏后改为 `verified`。

## 前置

- [ ] 安装最新 MSI（含 `picoo-desktop.exe` + `PicooVirtualCameraSource.dll`）
- [ ] 首次启动桌面 Receiver，确认托盘/窗口正常，设置中 VCam 状态为 Active/Installed
- [ ] Android Sender 与 PC 同一局域网，完成配对并进入 Streaming
- [ ] 桌面直播页可见预览；Shared Frame Ring 有帧（可选：ring-reader 工具）

## 会议 / 采集软件

对每一项：打开相机选择器 → 选择 **Picoo Camera** → 确认画面方向直立、无明显卡顿、断连后占位、重连后恢复。

| 应用 | 枚举到 Picoo Camera | 720p 可用 | 1080p 可用 | 占位画面 | 断线恢复 | 备注 |
| --- | --- | --- | --- | --- | --- | --- |
| Zoom | [ ] | [ ] | [ ] | [ ] | [ ] | |
| Microsoft Teams | [ ] | [ ] | [ ] | [ ] | [ ] | |
| 腾讯会议 | [ ] | [ ] | [ ] | [ ] | [ ] | |
| OBS Studio（视频采集设备） | [ ] | [ ] | [ ] | [ ] | [ ] | |
| 浏览器（meet.google.com / 本地 `getUserMedia`） | [ ] | [ ] | [ ] | [ ] | [ ] | |

## 负面路径

- [ ] 未配对 / 无 Sender：会议软件仍能打开 Picoo Camera，显示品牌占位（Waiting for phone…）
- [ ] 卸载 MSI 后：会议软件列表中不再出现 Picoo Camera；`regsvr32 /u` 清理成功

## 证据

将截图/录屏放入发布说明或本仓库 `docs/design-specs/verification/artifacts/`（勿提交含人脸的敏感素材）。
