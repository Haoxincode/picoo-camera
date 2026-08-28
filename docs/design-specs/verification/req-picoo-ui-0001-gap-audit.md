# REQ-PICOO-UI-0001 验收对照审计

> 基准 commit：`e57e1a6`（`cursor/ui-prototype-fidelity-dbe3`）  
> 规范：[req-picoo-ui-0001-native-camera-and-desktop-gpui-acceptance.md](../requirements/req-picoo-ui-0001-native-camera-and-desktop-gpui-acceptance.md)（远端 `f9572e5`）  
> 原型：[picoo-camera-ui-prototype.html](../prototypes/picoo-camera-ui-prototype.html)

图例：**✅** 通过 · **⏳** 部分 · **❌** 未实现 · **—** 不适用

## 移动端 Sender

### 3.1 发现（Screen 1）

| AC | 状态 | 说明 |
| --- | --- | --- |
| AC-M-DISC-01 | ⏳ | 顶栏结构对齐；Wi‑Fi pill 为「局域网」非实时 SSID |
| AC-M-DISC-02 | ⏳ | `DiscoveryPulseDot` 有呼吸动画；无「发现完成→绿点常驻」分支 |
| AC-M-DISC-03 | ⏳ | 已配对 badge + `MoreHoriz` 分栏；菜单为 Dropdown 非 bottom sheet |
| AC-M-DISC-04 | ⏳ | 离线卡片置灰；点击无 Toast |
| AC-M-DISC-05 | ⏳ | 空态清单对齐；空态时扫码为 ghost 非 primary 大按钮 |

### 3.2 扫码（Screen 2）

| AC | 状态 | 说明 |
| --- | --- | --- |
| AC-M-SCAN-01 | ✅ | `PicooIconButton` + Chevron；右侧 warn pill |
| AC-M-SCAN-02 | ❌ | 识别成功仍经「继续配对」确认，非自动直连 |
| AC-M-SCAN-03 | ✅ | 权限被拒 + 手动 `IP:Port` + 授予相机入口 |

### 3.3 配对 / 等待（Screen 3–4）

| AC | 状态 | 说明 |
| --- | --- | --- |
| AC-M-PAIR-01 | ⏳ | 六位码大号展示；**无** JetBrains Mono、**无** 60s 倒计时 |
| AC-M-PAIR-02 | ❌ | 无过期灰化 / `重新生成短码` 流 |
| AC-M-PAIR-03 | ⏳ | `WaitScreen(WaitOutcome.Rejected)` 已有 UI；**MainActivity 未接线** sender 拒绝态 |

### 3.4 推流控制台（Screen 5）

| AC | 状态 | 说明 |
| --- | --- | --- |
| AC-M-LIVE-01 | ⏳ | HUD + 工具条 + 锁定；仅 720↔1080，**无 480p**；EV/镜像高亮部分 |
| AC-M-LIVE-02 | ⏳ | 过热横幅文案对齐；药丸无 🔥 态；无 Toast |
| AC-M-LIVE-03 | ⏳ | 16:9 虚线框 ✅；**无**对焦环、**无**双击沉浸 |
| AC-M-LIVE-04 | ⏳ | EV 面板单行；用 `☀️` 字符（违反 §5.3）；无 brightness 滤镜反馈 |
| AC-M-LIVE-05 | ⏳ | 码率 + 丢包 pill ✅； flip 下仍显示「前置/后置」文字 |
| AC-M-LIVE-06 | ⏳ | 64px 快门 + armed 态 ✅；**无** 3s 自动复位、无呼吸脉冲动画 |
| AC-M-LIVE-07 | ⏳ | 46px 翻转钮 ✅；**无** 180° 旋转动画 / 模糊转场 |
| AC-M-LIVE-08 | ⏳ | 重连遮罩 ✅；**无** 退避阶梯文案（500ms→5s） |

### 3.5 设置（Screen 6）

| AC | 状态 | 说明 |
| --- | --- | --- |
| AC-M-SET-01 | ✅ | Chevron 返回 |
| AC-M-SET-02 | ⏳ | 自动直连开关 + 默认画质 toggle；配对管理跳回 Devices；无公钥指纹 sheet |

## 桌面 Receiver（GPUI）

| AC | 状态 | 说明 |
| --- | --- | --- |
| AC-D-TECH-01 | ✅ | 纯 GPUI，无 Electron/WebView |
| AC-D-TECH-02 | ✅ | `VideoSurface` + FrameHub |
| AC-D-IDLE-01 | ⏳ | 标题/副标题对齐；品牌为简化圆标非完整 logo |
| AC-D-IDLE-02 | ⏳ | VCam pill 绿/红态；文案与规范略有差异 |
| AC-D-IDLE-03 | ⏳ | QR 卡片 + `advertise_host:4433` ✅；**无 Nonce 展示** |
| AC-D-PAIR-01/02 | ⏳ | Modal 结构对齐；短码无等宽/code-box；缺「首次」强调 |
| AC-D-LIVE-01 | ⏳ | HUD pill 有；形状为 rounded 非 8px 规范 |
| AC-D-LIVE-02 | ⏳ | 6 项遥测有；grid 3×2 非单行；抖动硬编码 `—` |
| AC-D-LIVE-03 | ⏳ | IDR + 断开有；断开非 danger 红钮 |
| AC-D-SET-01 | ⏳ | 功能在 **全页 Settings** 非 Modal；占位图为 bool 非三选一 |
| AC-D-SET-02 | ✅ | 诊断导出已实现 |

## Anti-Patterns（§5）

| 项 | 状态 | 说明 |
| --- | --- | --- |
| UI 侵入底层 | ✅ | `ReceiverRuntime` / JNI 边界保持 |
| 移动端表单化 | ⏳ | 480p/抽屉未犯；flip 下仍有镜头文字 |
| Emoji 替代图标 | ❌ | EV/过热文案含 `☀️`；规范 §2.3 与 §5.3 冲突处待澄清 |
| 断开误触 / 假死 | ⏳ | 快门二次确认 ✅；重连遮罩 ✅；退避细节不足 |

## 汇总

| 类别 | ✅ | ⏳ | ❌ |
| --- | --- | --- | --- |
| 移动端 AC | 3 | 18 | 2 |
| 桌面 AC | 3 | 9 | 0 |
| Anti-Pattern | 1 | 2 | 1 |

**REQ-PICOO-UI-0001 不可宣称 `verified`**。下一轮 P0：AC-M-PAIR-01/02/03（TTL + 拒绝接线）、AC-M-SCAN-02（扫码自动继续）、AC-M-DISC-01（SSID pill）、字体 mono、去除 Emoji 违规项。
