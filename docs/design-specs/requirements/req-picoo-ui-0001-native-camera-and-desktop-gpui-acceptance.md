# REQ-PICOO-UI-0001：全端 UI 交互设计与细化验收规范

- **Requirement ID**: `REQ-PICOO-UI-0001`
- **Area**: `PICOO-UI`
- **Status**: `proposed`
- **Design Prototype**: [`picoo-camera-ui-prototype.html`](../prototypes/picoo-camera-ui-prototype.html)
- **Supported Use Cases**:
  - [BUC-001](../../use-cases/business/buc-001-phone-as-wireless-meeting-camera.md)
  - [PUC-001](../../use-cases/product/puc-001-first-install-and-pairing.md)
  - [PUC-002](../../use-cases/product/puc-002-discover-and-connect-paired-receiver.md)
  - [PUC-003](../../use-cases/product/puc-003-qr-code-fallback-connection.md)
  - [PUC-004](../../use-cases/product/puc-004-use-virtual-camera-in-meeting-apps.md)
  - [PUC-005](../../use-cases/product/puc-005-adjust-camera-during-streaming.md)
  - [PUC-006](../../use-cases/product/puc-006-auto-reconnect-after-network-interruption.md)
  - [PUC-007](../../use-cases/product/puc-007-manage-paired-devices.md)
- **Architectural Boundary**: [ARCH-PICOO-UI-001 (0009-desktop-gpui-mobile-native-ui-boundary.md)](../architecture/0009-desktop-gpui-mobile-native-ui-boundary.md)

---

## 1. 范围与目标

本规范定义 Picoo Camera 在移动端（Android Jetpack Compose / iOS SwiftUI）与桌面端（Windows / macOS GPUI）的用户界面、交互心智、视觉令牌、状态映射以及可量化验收标准（Acceptance Criteria）。

### 核心设计原则
1. **移动端坚定靠拢专业原生相机心智**：Sender 是一个取景与推流相机，不是后台管理表单或通用 Web 列表。交互必须具备沉浸取景、即时手势与微动效。
2. **桌面端契合现代高质感原生视窗**：Receiver 是基于 GPUI 的高性能暗色桌面监视器，不是内嵌浏览器的 Electron 网页。
3. **全端设计语言一致性**：共享统一的深色科技色板（Dark Industrial Slate）、等宽数字排版与统一的 24×24 网格矢量图标（禁止 Emoji）。
4. **单向数据流防腐边界**：UI 视图仅观察只读状态快照（`DesktopAppState` / Mobile State）并派发 Action，严禁 UI 层持有底层 Socket、编解码器或共享帧内存。

---

## 2. 全局设计系统与视觉令牌规范

### 2.1 色板与表面层级（Dark Slate Industrial）
- `Surface-Base`（主底色）：`#0b0d11` ~ `#06070a`，禁止使用灰白或发灰的默认背景。
- `Surface-Panel-1`（一级容器）：`#14171f`，边框 `rgba(255, 255, 255, 0.08)`。
- `Surface-Panel-2`（二级卡片）：`#1b202c`，边框 `rgba(255, 255, 255, 0.12)`。
- `Surface-Overlay`（弹窗/浮层）：`#242b3b`，阴影 `box-shadow: 0 16px 40px rgba(0,0,0,0.6)`。
- `Brand-Accent`（品牌强调色）：活力橙渐变 `linear-gradient(135deg, #ff6a3d 0%, #ff8c42 100%)`。
- `Status-Ready`（活跃/在线/良好）：翠绿 `#3ecf8e`，胶囊背景 `rgba(62, 207, 142, 0.12)`。
- `Status-Warn`（警告/过热/扫码）：暖黄 `#f0c14a`，胶囊背景 `rgba(240, 193, 74, 0.14)`。
- `Status-Danger`（断开/拒绝/错误）：柔红 `#ff5c6c` / `#ff4757`，胶囊背景 `rgba(255, 92, 108, 0.14)`。

### 2.2 字体与排版
- 标题/品牌：几何现代无衬线（`Bricolage Grotesque` / `SF Pro Display`）。
- 正文/标签：清晰易读无衬线（`Figtree` / `SF Pro Text` / `Roboto`）。
- **遥测数据与短码**：**必须强制使用等宽字体（`JetBrains Mono` / `SF Mono`）**，包含码率（`5.2 Mbps`）、延迟（`18 ms`）、丢包率（`0.0%`）、帧率（`30.0 FPS`）、6 位短码（`482 917`，`letter-spacing: 0.25em`）与 IP:Port。

### 2.3 矢量图标矩阵（统一映射，严禁使用 Emoji）

| 语义角色 | 规范矢量图标（Lucide 24×24 基准） | 移动端原生映射 (SF Symbols / Compose Icons) | 桌面 GPUI 映射 |
| :--- | :--- | :--- | :--- |
| **设置** | `Settings` | `gearshape` / `Icons.Default.Settings` | `lucide::settings` |
| **前后摄翻转** | `Switch Camera` | `camera.rotate` / `Icons.Default.FlipCameraAndroid` | N/A |
| **画面亮度** | `Sun` | `sun.max` / `Icons.Default.WbSunny` | N/A |
| **水平镜像** | `Flip Horizontal` | `arrow.left.and.right.righttriangle.left.righttriangle.right` | `lucide::flip_horizontal` |
| **防误触锁定** | `Lock` / `Unlock` | `lock` / `lock.open` | N/A |
| **恢复自动测光** | `Rotate CCW` | `arrow.counterclockwise` | N/A |
| **停止录像/断开** | `Square` (圆角) | `stop.fill` | `lucide::square` |
| **二维码扫码** | `QR Code` | `qrcode.viewfinder` | `lucide::qr_code` |
| **服务发现/雷达** | `Antenna` / `Radio` | `antenna.radiowaves.left.and.right` | `lucide::radio` |
| **过热降档** | `Flame` | `flame.fill` | `lucide::flame` |
| **更多操作** | `More Horizontal` | `ellipsis` | `lucide::more_horizontal` |
| **导航返回** | `Chevron Left` | `chevron.left` | `lucide::chevron_left` |

---

## 3. 📱 移动端 Sender 细化验收标准（Jetpack Compose & SwiftUI）

### 3.1 发现与主页（Screen 1）
- [ ] **AC-M-DISC-01**：顶栏左侧展示产品名，右侧胶囊实时显示当前 Wi-Fi 名称（如 `Wi‑Fi · Office‑5G`），最右侧为 `Settings` 矢量图标按钮。
- [ ] **AC-M-DISC-02**：顶部常驻呼吸脉冲点指示发现中（橙色波纹）或监听中（绿色常驻）。
- [ ] **AC-M-DISC-03**：已配对设备（Paired）卡片带有 `已配对` 橙黄徽标，右侧独立收纳 `More Horizontal` 按钮呼出管理菜单，主体点按直接发起直连。
- [ ] **AC-M-DISC-04**：未配对新电脑卡片显示 `在线 · 首次连接需配对短码`；离线电脑置灰显示 `上次连接 X 天前`，点击给出友好 Toast。
- [ ] **AC-M-DISC-05**：mDNS 屏蔽无设备时展示空状态排查卡片，并将底部的「扫描电脑端二维码连接」按钮提升为主视觉大按钮，且带有 `QR Code` 矢量图标。

### 3.2 扫码直连与权限兜底（Screen 2）
- [ ] **AC-M-SCAN-01**：顶栏左侧为 `Chevron Left` 矢量返回图标（**严禁文本 `← 返回`**），右侧为 `扫码直连` 警告色 Badge。
- [ ] **AC-M-SCAN-02**：取景扫描框覆盖动态激光扫描线（2.2s 循环），扫码成功后**直接建立连接，无需用户二次确认**。
- [ ] **AC-M-SCAN-03**：相机权限被拒时展示禁止面板，**必须提供手动输入框**支持输入电脑端显示的 `IP:Port` 直连。

### 3.3 配对确认与等待态（Screen 3 & Screen 4）
- [ ] **AC-M-PAIR-01**：大字号等宽字体展示 6 位短码（`482 917`，`letter-spacing: 0.25em`），配合 60s 倒计时。
- [ ] **AC-M-PAIR-02**：60s 超时后短码变灰不可用，文案红字提示 `短码已过期`，主按钮自动变为 `重新生成短码`。
- [ ] **AC-M-PAIR-03**：等待电脑确认过程中，若对端点击拒绝，即时切换为红色 `X` 图标并提示 `电脑端拒绝了连接`。

### 3.4 实时推流原生相机控制台（Screen 5 - 核心重点）
- [ ] **AC-M-LIVE-01（顶部 HUD）**：
  - 左侧胶囊显示绿点 + 电脑设备名 + 实时 RTT（如 `● Studio PC 18ms`）；
  - 中间工具栏：`☀️ 调光`（非自动时高亮）、`⇋ 镜像`（开启时高亮）、`🔓 锁定`（锁定后图标切换为 `🔒`）；
  - 右上角画质药丸：文本为 `1080P · 30` / `720P · 30` / `480P · 30`，**点按直接一键秒级轮换切换，严禁弹出全屏抽屉/菜单**。
- [ ] **AC-M-LIVE-02（设备过热）**：
  - 设备过热时画质药丸变黄并附带火苗图标 `720P · 30 🔥`；
  - 强制降档至 720P，点击弹出 Toast 提示 `设备偏热保护中，1080P 暂不可选`。
- [ ] **AC-M-LIVE-03（取景器手势与安全框）**：
  - 取景画面正中常驻 `16:9` 电脑端裁切虚线框；
  - 点击取景器任意坐标，生成黄色方框对焦环（Focus Ring），伴随向内微缩与 800ms 渐隐淡出动效；
  - 双击取景器空白区域进入全屏沉浸模式（淡出所有浮层）。
- [ ] **AC-M-LIVE-04（极简调光条）**：
  - 调光条为紧凑单行：`[ − ]` + `☀️ 自动 / 提亮 +1` + `[ ＋ ]` + `[ ↺ ]`（重置图标）；
  - `−` 与 `＋` 为 32×30px 方钮，档位严格映射为 `☀️ 自动` / `☀️ 提亮 +1` / `☀️ 提亮 +2` / `☀️ 压暗 -1` / `☀️ 压暗 -2`；
  - 取景画面实时叠加明暗滤镜（`brightness`）反馈。
- [ ] **AC-M-LIVE-05（底栏监控与镜头文字移除）**：
  - 左侧监控仅展示当前码率（`5.2 Mbps`）与网络质量（`30 FPS · 0% 丢包`）；
  - **彻底移除“前置镜头/后置镜头”等多余文字注释**（用户看取景画面即可直观确认）。
- [ ] **AC-M-LIVE-06（停止录像大圆钮）**：
  - 外层为 64px 正圆半透明白环（`border: 3.5px solid rgba(255,255,255,0.85); border-radius: 50%`）；
  - 内层正中悬浮 22×22px 圆角红色停止方块（`border-radius: 5px; background: #ff4757`），内外比例协调，无多层圆角冲突；
  - 轻触一次外环变红呼吸脉冲并提示 `再次点击确认断开`，3 秒内二次点击安全挂断退出，3 秒超时自动复位。
- [ ] **AC-M-LIVE-07（原生翻转镜头按钮）**：
  - 右下角 46px 圆形磨砂玻璃按钮，内嵌 `Switch Camera` 矢量图标；
  - 点击时**图标自身带有 180° 平滑旋转动画**，取景器伴随瞬间重聚模糊滤镜，底层递增 `stream_epoch` 并请求 IDR 关键帧。
- [ ] **AC-M-LIVE-08（网络中断重连遮罩）**：
  - 网络突发中断时严禁直接闪退回主页，必须弹出半透明模糊重连遮罩；
  - 显示退避阶梯（`500ms` ➔ `1s` ➔ `2s` ➔ `5s`）并提供 `停止重连并退出` 按钮。

### 3.5 设置中心（Screen 6）
- [ ] **AC-M-SET-01**：顶栏左侧使用 `Chevron Left` 矢量返回图标。
- [ ] **AC-M-SET-02**：提供开机自动直连开关、默认画质选择、已配对电脑管理（查看公钥指纹与撤销信任）与权限状态就绪清单。

---

## 4. 🖥️ 桌面端 Receiver 细化验收标准（GPUI Windows & macOS）

### 4.1 技术选型与组件基线
- [ ] **AC-D-TECH-01**：Windows 与 macOS 共用同一套 Rust GPUI 代码，**严禁引入 Electron、Tauri、WebView 或内嵌浏览器运行环境**。
- [ ] **AC-D-TECH-02**：视频监视器核心自定义组件必须封装为 **`VideoSurface`**，绑定 FrameHub 解码环形缓冲，保持 16:9 画幅与断流占位画面平滑切换。

### 4.2 待机与等待连接主页（Idle View）
- [ ] **AC-D-IDLE-01**：顶部展示 Pico Camera 品牌 Logo、标题 `等待手机连接…` 与局域网广播状态。
- [ ] **AC-D-IDLE-02**：展示虚拟摄像头状态胶囊（就绪为绿色 `Virtual Camera: Ready`，未就绪为黄色/红色警示且支持一键修复）。
- [ ] **AC-D-IDLE-03**：独立卡片展示 `Show QR Code` 二维码矩阵，标注局域网直连 Endpoint（如 `192.168.1.108:4433`）与动态 Nonce。

### 4.3 配对确认对话框（Pairing Modal）
- [ ] **AC-D-PAIR-01**：手机首次发起连接时弹出 GPUI 原生对话框，背景带微模糊。
- [ ] **AC-D-PAIR-02**：清晰展示对端手机型号（如 `Xiaomi 14 Ultra`）与等宽 6 位短码（`482 917`），提供 `拒绝` 与 `两端一致，确认配对` 按钮。

### 4.4 直播接收监视器（Live Monitor）
- [ ] **AC-D-LIVE-01**：顶部 HUD 显示手机设备名（`Xiaomi 14 Ultra (Sender)`）与虚拟摄像头输出状态（`Virtual Camera: ACTIVE`）。
- [ ] **AC-D-LIVE-02**：底部遥测栏紧凑展示 6 项指标：
  1. 画质规格（`1080p30` / `720p30` / `480p30`）；
  2. 实时帧率（`30.0 FPS`，等宽数字动态刷新）；
  3. 接收码率（`5.2 Mbps`）；
  4. RTT 延迟（绿色 `18 ms`）；
  5. 丢包/抖动（`0.0% · 1.2ms`）；
  6. 网络健康等级（`极佳 (Good)` / `一般 (Fair)` / `较差 (Poor)`）。
- [ ] **AC-D-LIVE-03**：底部右侧提供 `请求关键帧 (IDR)` 与 `断开会话`（红色危险按钮）快捷操作。

### 4.5 桌面设置中心（Settings Modal）
- [ ] **AC-D-SET-01**：支持配置设备显示名、自动接受已配对设备开关、最小化到托盘开关、未推流占位图选择器（Logo/纯黑/彩条）与已信任设备列表管理。
- [ ] **AC-D-SET-02**：提供脱敏诊断日志一键导出功能（`.json` 格式，默认脱敏 IP、设备名与公钥）。

---

## 5. 架构防腐与不可接受清单（Anti-Patterns / Non-Acceptance Criteria）

在代码评审（Code Review）与实现验收中，出现以下任何一项即判定为**不合格（Fail）**：

1. ❌ **UI 侵入底层网络或媒体**：
   - 严禁 UI 视图直接持有 QUIC Connection、UDP Socket、H.264 编码器/解码器、抖动缓冲或 Shared Frame Ring 内存指针；
   - 必须严格通过只读状态快照驱动，通过轻量 Action 发送用户意图。
2. ❌ **移动端表单化退化**：
   - 严禁把前后摄做成冷冰冰的文字按钮；
   - 严禁把画质切换做成全屏抽屉/菜单；
   - 严禁在推流底栏堆砌多余的“前置镜头/后置镜头”文字标签。
3. ❌ **图标 Emoji 化与样式分裂**：
   - 严禁在代码中直接使用操作系统 Emoji 字符（如 `⚙`, `🔄`, `☀️`, `🎥` 等）替代矢量图标；
   - 严禁使用带有 `← 返回` 文字的旧式导航条。
4. ❌ **断开误触与假死**：
   - 严禁将断开连接做成无确认的一触即断；
   - 严禁在弱网断开时界面假死冻结，必须明确展示重连退避遮罩。

---

## 6. 验证与追溯映射

| 验收项分类 | 对应验证方式 | 关联 Use Case | 原型验证入口 |
| :--- | :--- | :--- | :--- |
| **配对短码与核对** | 单元测试 + 双端联动模拟 | PUC-001 | 剧本① (首次配对) |
| **已配对快速直连** | 单元测试 + 状态快照检查 | PUC-002 | 剧本② (已配对直连) |
| **二维码扫码兜底** | 协议解析测试 + 扫码 UI 交互 | PUC-003 | 剧本③ (二维码扫码) |
| **虚拟摄像头就绪** | 跨进程共享内存测试 + 驱动状态检查 | PUC-004 | 剧本④ (虚拟摄像头) |
| **画质/镜头/调光交互** | UI 自动化测试 + 手势微动效检查 | PUC-005 | 剧本⑤ ~ ⑧ (相机操作) |
| **弱网中断退避重连** | 模拟丢包/网络切断断言 | PUC-006 | 剧本⑨ (网络中断重连) |
| **信任设备管理与删除** | 状态持久化断言 | PUC-007 | 剧本⑩ (删除配对设备) |
