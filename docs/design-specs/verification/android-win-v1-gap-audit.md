# Android Sender + Windows Receiver V1 差距审计

> 分支：`cursor/android-win-v1-gates-dbe3` · 审计基准 commit `2b22237` · 末次全绿 CI run [33131999904](https://github.com/Haoxincode/picoo-camera/actions/runs/33131999904)（`3ae2569`，6/6 green）；`64bd7c4` Compose + `2b22237` MSI 待本轮 CI 确认
>
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
| **PUC-001** 首次安装与配对 | ✅ | ✅ pairing/FFI/VCam scaffold | ⏳ | MSI 真机安装 + 首次配对录屏；VCam-001 系统枚举 |
| **PUC-002** 自动发现连接 | ✅ | ✅ NSD/mDNS/3s 连接测试 | ⏳ | 真 mDNS P50（`--ignored`）；多路由器环境 |
| **PUC-003** 二维码兜底 | ✅ | ✅ QR JSON + TTL | ⏳ | 企业 Wi‑Fi 隔离场景扫码 E2E |
| **PUC-004** 会议软件 VCam | ✅ | ✅ placeholder/ring CI | ❌ | **REQ-VCAM-005 `proposed`**；5 款会议/采集软件 |
| **PUC-005** 直播中调整摄像头 | ✅ | ✅ epoch/ABR/EV 单测 | ⏳ | 真机前后摄/720·1080/EV/镜像；MF 硬解 |
| **PUC-006** 断网自动恢复 | ✅ | ✅ churn/5% loss/5s 恢复 | ⏳ | 真机 Wi‑Fi 开关/漫游；camera→VCam 延迟 |
| **PUC-007** 管理已配对设备 | ✅ | ✅ clear/remove 测试 | ⏳ | 双端删除后重配对 E2E |

**BUC-001**（手机作无线会议摄像头）：实现链完整，**关闭依赖 PUC-004 会议软件证据 + 全链路真机 E2E**。

## PRD §21 验收对照

| PRD §21 条目 | 实现 | 自动化 | 真机 | 备注 |
| --- | --- | --- | --- | --- |
| 四种平台组合可连接 | 部分 | loopback 四端 crate | ❌ | V1 仅验 Android→Windows；iOS/macOS 未交付 |
| 自动发现 | ✅ | ✅ | ⏳ | |
| 二维码兜底 | ✅ | ✅ | ⏳ | |
| 配对与撤销 | ✅ | ✅ | ⏳ | 配对 flaky 修复待真机确认（`PairingConfirm` 误解析） |
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
| `rust-and-docs` | ✅ | 无 artifact |
| `android` | ✅ | `android-apk-debug`, `android-release` |
| `windows` | ✅ | `windows-bundle`, `windows-msi` |

下载步骤见 [ci-artifacts.md](ci-artifacts.md)。

## Compose UI vs HTML 原型（`64bd7c4` 已对齐）

[`64bd7c4`](https://github.com/Haoxincode/picoo-camera/commit/64bd7c4) 重写 Devices / QrScan / Pairing / Wait / Streaming / Settings 六屏，对齐 [HTML 原型](../prototypes/picoo-camera-ui-prototype.html) 流程与中文文案（REQ-PICOO-UI-003、REQ-PICOO-UI-005）。JNI / Rust 会话逻辑未改。

| 区域 | HTML 原型 | Compose（`64bd7c4` 后） | 剩余 delta |
| --- | --- | --- | --- |
| 视觉 | Bricolage/Figtree、品牌色 | 深色 graphite + coral `PicooColors`、卡片组件 | 自定义字体未嵌入；圆角/间距微调 |
| 发现空态 | 空态 + 主色「扫描二维码连接」 | `DevicesScreen` 空态清单 + 主色 CTA | 真机视觉验收 |
| 发现非空 | 列表 + ghost 扫码 | 列表 + ⋮ 菜单 + ghost 扫码 | — |
| 扫码 | 独立全屏取景 + 手动 IP | `QrScanScreen` 全屏 + 权限被拒兜底 | — |
| 配对 | 大号六位码 + 确认/取消 | `PairingScreen` | — |
| 等待 | 手机确认后等电脑端 | `WaitScreen` 新增 | — |
| 传输 | 原生相机 HUD + 快门 | `StreamingScreen` 原生布局 | 真机 FGS / 防锁屏 UX（UI-005） |
| 设置 | 手机端设置 | `SettingsScreen` 新增 | — |
| 权限 | 操作时 inline 提示 | 按需请求保留 | 文案/样式微调 |

功能路径（NSD / QR / 配对 / 流控）已在 Compose 接线；**Android 侧 UX 缺口已收窄至字体/真机 UX 验收**。

## Windows MSI / VCam 安装（`2b22237`）

[`2b22237`](https://github.com/Haoxincode/picoo-camera/commit/2b22237) 修正 WiX deferred `regsvr32` 自定义动作：`SystemFolder` 作工作目录、`[#PicooVcamDll]` 解析已安装 DLL 路径（REQ-PICOO-VCAM-004）。仍须 Win11 真机验证 MSI 安装/卸载与 COM 枚举。

## 本会话建议的关闭顺序（真机）

1. [device-e2e-android-win11.md](device-e2e-android-win11.md) — 安装→配对→Streaming→桌面预览
2. [vcam-meeting-apps.md](vcam-meeting-apps.md) — 5 款会议/采集软件（关闭 VCAM-005）
3. SESSION-005 / SESSION-007 — 2h 长稳 + 延迟抽样
4. 配对 flaky — 重复配对/取消 10 轮确认 `PairingConfirm` 修复

## 相关文档

- [ci-artifacts.md](ci-artifacts.md) — CI 产物下载
- [device-e2e-android-win11.md](device-e2e-android-win11.md) — 真机 E2E 清单
- [vcam-meeting-apps.md](vcam-meeting-apps.md) — 会议软件清单
- [session-005-soak-loopback.md](session-005-soak-loopback.md) — loopback 2h 中间证据
