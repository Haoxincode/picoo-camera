# REQ-PICOO-UI-0001 验收对照审计

> 基准 commit：`ui-gap-p0-round2`（`cursor/ui-prototype-fidelity-dbe3`）  
> 规范：[req-picoo-ui-0001-native-camera-and-desktop-gpui-acceptance.md](../requirements/req-picoo-ui-0001-native-camera-and-desktop-gpui-acceptance.md)  
> 原型：[picoo-camera-ui-prototype.html](../prototypes/picoo-camera-ui-prototype.html)

图例：**✅** 通过 · **⏳** 部分 · **❌** 未实现 · **—** 不适用

## 移动端 Sender

### 3.1 发现（Screen 1）

| AC | 状态 | 说明 |
| --- | --- | --- |
| AC-M-DISC-01 | ⏳ | 顶栏 Wi‑Fi pill；`normalizeSsid`/`fallbackLabel` 可测；无权限时回退「局域网」/「Wi‑Fi」 |
| AC-M-DISC-02 | ✅ | 发现中橙色脉冲；发现完成绿点常驻（无动画） |
| AC-M-DISC-03 | ✅ | 已配对 badge + `MoreHoriz` 分栏；**ModalBottomSheet** 撤销配对（非 Dropdown） |
| AC-M-DISC-04 | ✅ | 离线卡片置灰；点击 Toast 提示不在线 |
| AC-M-DISC-05 | ✅ | 空态 checklist + 空态时 primary 大按钮扫码 |

### 3.2 扫码（Screen 2）

| AC | 状态 | 说明 |
| --- | --- | --- |
| AC-M-SCAN-01 | ✅ | `PicooIconButton` + Chevron；右侧 warn pill |
| AC-M-SCAN-02 | ✅ | 识别后自动连接并跳转 Pairing（无「继续配对」二次确认） |
| AC-M-SCAN-03 | ✅ | 权限被拒 + 手动 `IP:Port` + 授予相机入口 |

### 3.3 配对 / 等待（Screen 3–4）

| AC | 状态 | 说明 |
| --- | --- | --- |
| AC-M-PAIR-01 | ✅ | JetBrains Mono + `482 917` 间距 + 60s 倒计时 |
| AC-M-PAIR-02 | ✅ | 过期灰化 + 红字「短码已过期」+ `重新生成短码` |
| AC-M-PAIR-03 | ⏳ | Wait 拒绝态已接线（desktop disconnect → Rejected）；超时态已接线 |

### 3.4 推流控制台（Screen 5）

| AC | 状态 | 说明 |
| --- | --- | --- |
| AC-M-LIVE-01 | ⏳ | HUD + 工具条 + 锁定；**480p 三档点按轮换** ✅；无抽屉 |
| AC-M-LIVE-02 | ⏳ | 过热横幅 + Toast；**ResPill「热降档」warn 态** ✅；无 emoji 🔥 |
| AC-M-LIVE-03 | ⏳ | 16:9 虚线框 ✅；**轻触对焦环** ✅；**双击沉浸** ✅ |
| AC-M-LIVE-04 | ⏳ | EV −/＋ 独立步进 + **↺ 重置自动测光** ✅；无 brightness 滤镜 |
| AC-M-LIVE-05 | ✅ | 码率 + 丢包 pill；已移除底栏「前置/后置」文字 |
| AC-M-LIVE-06 | ⏳ | 64px 快门 + armed 态 + **3s 自动复位** ✅；**armed 呼吸脉冲** ✅ |
| AC-M-LIVE-07 | ⏳ | 46px 翻转钮 ✅；**180° 旋转动画** ✅；无模糊转场 |
| AC-M-LIVE-08 | ✅ | 重连遮罩 + 退避阶梯文案（`第 N 次 (500ms→5s)`，对齐原型 `m-reconnect-msg`） |

### 3.5 设置（Screen 6）

| AC | 状态 | 说明 |
| --- | --- | --- |
| AC-M-SET-01 | ✅ | Chevron 返回 |
| AC-M-SET-02 | ⏳ | 自动直连 + 默认画质；配对列表展示 **公钥短指纹** ✅；无独立 sheet |

## 桌面 Receiver（GPUI）

| AC | 状态 | 说明 |
| --- | --- | --- |
| AC-D-TECH-01 | ✅ | 纯 GPUI，无 Electron/WebView |
| AC-D-TECH-02 | ✅ | `VideoSurface` + FrameHub |
| AC-D-IDLE-01 | ⏳ | 标题/副标题对齐；**摄像机品牌圆标**（非 ▶ 占位） |
| AC-D-IDLE-02 | ⏳ | VCam pill 绿/红态；**就绪 (Ready) 文案**；未就绪时 **修复按钮 + 诊断反馈** |
| AC-D-IDLE-03 | ⏳ | QR 卡片 + endpoint ✅；**Nonce 展示** ✅ |
| AC-D-PAIR-01/02 | ⏳ | Modal 结构 + **code-box** + 首次强调 ✅；无 blur backdrop |
| AC-D-LIVE-01 | ⏳ | HUD pill 8px 圆角 ✅ |
| AC-D-LIVE-02 | ⏳ | 遥测 **单行 flex** ✅；真实 jitter ✅；双语网络质量 ✅ |
| AC-D-LIVE-03 | ⏳ | IDR + **danger 红断开钮** ✅；**远程 CameraCommand**（前/后/480/720/1080/远端镜像）✅ |
| AC-D-SET-01 | ✅ | **Settings Modal** 覆盖层（非全页）；占位画面三选一（Logo/黑/彩条）✅ |
| AC-D-SET-02 | ✅ | 诊断导出已实现 |

## Anti-Patterns（§5）

| 项 | 状态 | 说明 |
| --- | --- | --- |
| UI 侵入底层 | ✅ | `ReceiverRuntime` / JNI 边界保持 |
| 移动端表单化 | ⏳ | flip 文字已移除；**480p 轮换** ✅；设置抽屉等待 |
| Emoji 替代图标 | ✅ | EV 面板改用 `BrightnessHigh` 矢量图标 |
| 断开误触 / 假死 | ⏳ | 快门二次确认 + 3s 复位 ✅；重连遮罩 ✅ |

## 汇总

| 类别 | ✅ | ⏳ | ❌ |
| --- | --- | --- | --- |
| 移动端 AC | 10 | 11 | 0 |
| 桌面 AC | 3 | 9 | 0 |
| Anti-Pattern | 2 | 2 | 0 |

**REQ-PICOO-UI-0001 不可宣称 `verified`**。下一轮 P1：Settings Modal、桌面 logo 渐变/阴影精修、Wi‑Fi SSID 权限兜底精修。
