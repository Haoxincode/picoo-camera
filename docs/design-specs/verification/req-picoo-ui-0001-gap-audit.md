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
| AC-M-DISC-03 | ✅ | 已配对 badge + `MoreHoriz` 分栏；管理菜单为原型 `m-sheet`（`PicooSheet` bottom sheet，非 Dropdown） |
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
| AC-M-LIVE-02 | ✅ | 过热横幅 + 药丸火焰图标 + Toast「1080P 暂不可选」+ ResPill「热降档」文案；无 emoji 🔥 |
| AC-M-LIVE-03 | ✅ | 16:9 虚线框；轻触对焦环（向内微缩）；双击沉浸 |
| AC-M-LIVE-04 | ✅ | EV −/＋/↺ + 取景器 brightness 滤镜（`ExposurePreview`） |
| AC-M-LIVE-05 | ✅ | 码率 + 丢包 pill；已移除底栏「前置/后置」文字 |
| AC-M-LIVE-06 | ✅ | 64px 快门 + armed 态 + 3s 自动复位 + armed 呼吸脉冲 |
| AC-M-LIVE-07 | ✅ | 46px 翻转钮；180° 旋转；翻转瞬间模糊转场 |
| AC-M-LIVE-08 | ✅ | 重连遮罩 + 退避阶梯文案（`第 N 次 (500ms→5s)`，对齐原型 `m-reconnect-msg`） |

### 3.5 设置（Screen 6）

| AC | 状态 | 说明 |
| --- | --- | --- |
| AC-M-SET-01 | ✅ | Chevron Left 矢量返回（扫码页同步） |
| AC-M-SET-02 | ✅ | 自动直连 + 默认画质三档 sheet；已配对指纹 sheet 可撤销 |

## 桌面 Receiver（GPUI）

| AC | 状态 | 说明 |
| --- | --- | --- |
| AC-D-TECH-01 | ✅ | 纯 GPUI，无 Electron/WebView |
| AC-D-TECH-02 | ✅ | `VideoSurface` + FrameHub |
| AC-D-IDLE-01 | ⏳ | 标题/副标题对齐；**摄像机品牌圆标**（非 ▶ 占位） |
| AC-D-IDLE-02 | ⏳ | VCam pill 绿/红态；**就绪 (Ready) 文案**；未就绪时 **修复按钮 + 诊断反馈** |
| AC-D-IDLE-03 | ⏳ | QR 卡片 + endpoint ✅；**Nonce 展示** ✅ |
| AC-D-PAIR-01/02 | ⏳ | Modal + code-box + 首次强调 +「两端一致，确认配对」；暗色遮罩（GPUI 无系统 blur） |
| AC-D-LIVE-01 | ⏳ | HUD pill 8px 圆角 ✅ |
| AC-D-LIVE-02 | ⏳ | 遥测 **单行 flex** ✅；真实 jitter ✅；双语网络质量 ✅ |
| AC-D-LIVE-03 | ⏳ | IDR + **danger 红断开钮** ✅；**远程 CameraCommand**（前/后/480/720/1080/远端镜像）✅ |
| AC-D-SET-01 | ✅ | **Settings Modal** 覆盖层（非全页）+ `overflow_y_scrollbar`；占位画面三选一（Logo/黑/彩条）✅ |
| AC-D-SET-02 | ✅ | 诊断导出已实现 |

## Anti-Patterns（§5）

| 项 | 状态 | 说明 |
| --- | --- | --- |
| UI 侵入底层 | ✅ | `ReceiverRuntime` / JNI 边界保持 |
| 移动端表单化 | ✅ | 无前后摄文字钮；画质点按轮换；设置/配对管理为 bottom sheet |
| Emoji 替代图标 | ✅ | EV 面板改用 `BrightnessHigh` 矢量图标 |
| 断开误触 / 假死 | ⏳ | 快门二次确认 + 3s 复位 ✅；重连遮罩 ✅ |

## 汇总

| 类别 | ✅ | ⏳ | ❌ |
| --- | --- | --- | --- |
| 移动端 AC | 17 | 4 | 0 |
| 桌面 AC | 4 | 8 | 0 |
| Anti-Pattern | 3 | 1 | 0 |

**REQ-PICOO-UI-0001 仍不可宣称 `verified`。** 桌面验证面已落到 Linux 同一套 GPUI 壳（`REQ-PICOO-UI-010`）：可启动、可对照 `#d-view-idle` / `#d-modal-settings` 截图像素。字体已嵌入 Bricolage Grotesque / Figtree / JetBrains Mono；汉字回退 Noto Sans SC。剩余：logo/遮罩精修、Live 真流、Windows 真 VCam。不要只因 Linux 窗口能打开就标 verified。
