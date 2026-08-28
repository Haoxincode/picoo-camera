# Android Sender + Windows Receiver V1 差距审计

> 分支：`cursor/android-win-v1-gates-dbe3` · tip `fcb9900` · 全绿 CI [33147645233](https://github.com/Haoxincode/picoo-camera/actions/runs/33147645233)（PUC-007 指纹 + 占位三选一 + ABR 480）
>
> 后续修复见 PR #14 `cursor/vcam-repair-feedback-dbe3`（VCam 反馈、Live CameraCommand、Settings Modal、pairing TTL、公钥 Toast）。
> 范围：**Android→Windows** 组合下的 BUC-001 + 全部 7 个 PUC + PRD §21 验收。iOS/macOS 四端组合不在本 V1 关闭范围。

## 图例

| 列 | 含义 |
| --- | --- |
| **实现** | 代码/CI 已落地（REQ 状态多为 `implemented`） |
| **自动化测试** | Linux loopback / JVM 单测 / CI 门禁 |
| **真机验证** | Win11 + Android 物理设备人工证据 |
| **缺口** | 关闭 V1 仍缺什么 |

## PUC 级摘要

| PUC | 实现 | 自动化测试 | 真机验证 | 主要缺口 |
| --- | --- | --- | --- | --- |
| **PUC-001** 首次安装与配对 | ✅ | ✅ pairing TTL 60s + FFI/VCam scaffold + MSI | ⏳ | MSI 真机安装 + 首次配对录屏；VCam-001 系统枚举 |
| **PUC-002** 自动发现连接 | ✅ | ✅ NSD/mDNS/3s 连接测试 | ⏳ | 真 mDNS P50（`--ignored`）；多路由器环境 |
| **PUC-003** 二维码兜底 | ✅ | ✅ QR JSON + TTL | ⏳ | 企业 Wi‑Fi 隔离场景扫码 E2E |
| **PUC-004** 会议软件 VCam | ✅ | ✅ placeholder/ring CI | ❌ | **REQ-VCAM-005 `proposed`**；5 款会议/采集软件 |
| **PUC-005** 直播中调整摄像头 | ✅ | ✅ epoch/ABR 1080→720→480/EV + Live CameraCommand | ⏳ | 真机前后摄/720·1080/EV/镜像；MF 硬解 |
| **PUC-006** 断网自动恢复 | ✅ | ✅ churn/5% loss/5s 恢复 + **退避阶梯 UI** | ⏳ | 真机 Wi‑Fi 开关/漫游；camera→VCam 延迟 |
| **PUC-007** 管理已配对设备 | ✅ | ✅ clear/remove + PUBLIC_KEY_CHANGED 环路/Android Toast | ⏳ | 双端删除后重配对 E2E；真机公钥变化 |

**BUC-001**（手机作无线会议摄像头）：实现链完整，**关闭依赖 PUC-004 会议软件证据 + 全链路真机 E2E**。

## PRD §21 验收对照

| PRD §21 条目 | 实现 | 自动化 | 真机 | 备注 |
| --- | --- | --- | --- | --- |
| 四种平台组合可连接 | 部分 | loopback 四端 crate | ❌ | V1 仅验 Android→Windows；iOS/macOS 未交付 |
| 自动发现 | ✅ | ✅ | ⏳ | |
| 二维码兜底 | ✅ | ✅ | ⏳ | |
| 配对与撤销 | ✅ | ✅ + **PairingConfirm 误解析回归测试** | ⏳ | 真机 10 轮取消/重配确认 |
| 前后摄切换 | ✅ | ✅ epoch | ⏳ | |
| 720p30 / 1080p30 | ✅ | ✅ caps/ABR | ⏳ | |
| Windows 注册 Picoo Camera | ✅ | ✅ CI UTF-16/DLL/MSI | ⏳ | 系统相机枚举仍人工 |
| 会议软件可选用 | ✅ DLL/MSI | CI bundle smoke | ❌ | VCAM-005 |
| 无手机时占位画面 | ✅ | ✅ placeholder 测试 | ⏳ | |
| 断网自动恢复 | ✅ | ✅ | ⏳ | |
| P95 E2E &lt;250ms | ✅ 传输路径 | ✅ loopback P50 | ❌ | **camera→VCam 真机仍待**（SESSION-007） |
| 1080p30 连续 2h | ✅ harness | ✅ loopback 7200s PASS | ❌ | **Win11+Android 2h 仍待**（SESSION-005） |
| 内存不增长 | ✅ | ✅ loopback ΔRSS=4KB | ❌ | 真机 Task Manager / Android profiler |
| 5% 丢包可用 | ✅ | ✅ loss loopback | ⏳ | 真 Wi‑Fi 拥塞可选 |
| 恢复后 frame_age &lt;1s | ✅ | ✅ | ⏳ | |
| 未配对无视频 | ✅ | ✅ | ⏳ | |
| 公钥变化拒绝 | ✅ | ✅ | ⏳ | |
| 删除配对须重确认 | ✅ | ✅ | ⏳ | |
| 断外网 LAN 仍可用 | ✅ | ✅ 架构+loopback | ⏳ | |
| QUIC/TLS 无明文 | ✅ | ✅ ALPN | ⏳ | 可选 Wireshark 抽检 |
| 默认不写视频文件 | ✅ | ✅ | — | |
| 日志不含视频帧 | ✅ | ✅ diagnostics | — | |

## REQ 级：`proposed` 与「仍待真机」

### 唯一 `proposed`

| ID | 描述 | 阻塞关闭 V1 |
| --- | --- | --- |
| **REQ-PICOO-VCAM-005** | Zoom/Teams/腾讯会议/OBS/浏览器可选用 | **是** |

### `implemented` 但验收含「真机仍待」

| ID | 真机待验项 |
| --- | --- |
| REQ-PICOO-VCAM-001 | 系统相机/会议软件枚举名称 `Picoo Camera` |
| REQ-PICOO-VCAM-004 | MSI 安装/卸载 + COM 清理 |
| REQ-PICOO-MEDIA-001 | Camera2 预览/硬编 |
| REQ-PICOO-MEDIA-005 | Windows MF + D3D11 硬解 |
| REQ-PICOO-MEDIA-008 | 曝光补偿 EV± |
| REQ-PICOO-SESSION-005 | Win11+Android 1080p30 连续 2h |
| REQ-PICOO-SESSION-007 | camera→VCam E2E P95 |
| REQ-PICOO-UI-005 | FGS/防锁屏/过热降档 UX |

## CI 与产物（已绿）

| Job | 状态 | 产物 |
| --- | --- | --- |
| `rust-and-docs` | ✅ run [33147645233](https://github.com/Haoxincode/picoo-camera/actions/runs/33147645233) | 无 artifact |
| `android` | ✅ | `android-apk-debug`, `android-release` |
| `windows` | ✅ | `windows-bundle`, `windows-msi` |

下载步骤见 [ci-artifacts.md](ci-artifacts.md)。真机请优先用该绿 run 的 `windows-msi` + `android-release`。

## Compose UI vs HTML 原型（REQ-PICOO-UI-0001）

[`63c9799`](https://github.com/Haoxincode/picoo-camera/commit/63c9799) + [`e57e1a6`](https://github.com/Haoxincode/picoo-camera/commit/e57e1a6) 对齐六屏 + P0 验收项。tip `71bc304` 追加 P1：**480p 三档轮换**、**翻转 180° 动画**、**轻触对焦环/双击沉浸**、桌面 **idle 摄像机 logo**。详见 [req-picoo-ui-0001-gap-audit.md](req-picoo-ui-0001-gap-audit.md)。

## Windows MSI / VCam 安装

[`83ca647`](https://github.com/Haoxincode/picoo-camera/commit/83ca647) 修复真机 `0x80040154`（COM 未注册）：桌面启动前 `ensure_com_server_registered()`（缺失则 `regsvr32 /s`）；MSI 在 `InstallFiles` 后以 `RegisterVcamComDll`（`regsvr32.exe` + `Return=ignore`）兜底 COM，再 `RegisterVcamOnInstall`（`--register-vcam --no-wait`）注册 MF。WiX 仍保留声明式 CLSID 注册表。仍须 Win11 **管理员** 真机验证：新 MSI 安装/卸载、系统相机枚举 `Picoo Camera`、会议软件选用（VCAM-005）。

## 本会话建议的关闭顺序（真机）

1. [device-e2e-android-win11.md](device-e2e-android-win11.md) — 安装→配对→Streaming→桌面预览
2. [vcam-meeting-apps.md](vcam-meeting-apps.md) — 5 款会议/采集软件（关闭 VCAM-005）
3. SESSION-005 / SESSION-007 — 2h 长稳 + 延迟抽样
4. 配对 flaky — 重复配对/取消 10 轮（自动化已有 `pairing_confirm_false_positive_*` 回归测试）

## 相关文档

- [ci-artifacts.md](ci-artifacts.md) — CI 产物下载
- [device-e2e-android-win11.md](device-e2e-android-win11.md) — 真机 E2E 清单
- [vcam-meeting-apps.md](vcam-meeting-apps.md) — 会议软件清单
- [session-005-soak-loopback.md](session-005-soak-loopback.md) — loopback 2h 中间证据
