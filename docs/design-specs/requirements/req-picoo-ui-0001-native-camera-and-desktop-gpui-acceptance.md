# REQ-PICOO-UI-0001：全端 UI 交互设计与细化验收规范

- **Requirement ID**: `REQ-PICOO-UI-0001`
- **Area**: `PICOO-UI`
- **Status**: `implemented`
- **Design Prototype**: [`picoo-camera-receiver.html`](../../../picoo-camera-receiver.html)
- **Supported Use Cases**:
  - [BUC-001](../use-cases/business/buc-001-phone-as-wireless-meeting-camera.md)
  - [PUC-001](../use-cases/product/puc-001-first-install-and-pairing.md)
  - [PUC-002](../use-cases/product/puc-002-discover-and-connect-paired-receiver.md)
  - [PUC-004](../use-cases/product/puc-004-use-virtual-camera-in-meeting-apps.md)
  - [PUC-005](../use-cases/product/puc-005-adjust-camera-during-streaming.md)
  - [PUC-006](../use-cases/product/puc-006-auto-reconnect-after-network-interruption.md)
  - [PUC-007](../use-cases/product/puc-007-manage-paired-devices.md)
  - [PUC-008](../use-cases/product/puc-008-connect-with-code-or-ip.md)
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
- `Status-Warn`（警告/过热）：暖黄 `#f0c14a`，胶囊背景 `rgba(240, 193, 74, 0.14)`。
- `Status-Danger`（断开/拒绝/错误）：柔红 `#ff5c6c` / `#ff4757`，胶囊背景 `rgba(255, 92, 108, 0.14)`。

### 2.2 字体与排版
- 标题/品牌：几何现代无衬线（`Bricolage Grotesque` / `SF Pro Display`）。
- 正文/标签：清晰易读无衬线（`Figtree` / `SF Pro Text` / `Roboto`）。
- **遥测数据与短码**：**必须强制使用等宽字体（`JetBrains Mono` / `SF Mono`）**，包含码率（`5.2 Mbps`）、延迟（`18 ms`）、丢包率（`0.0%`）、帧率（`30.0 FPS`）、6 位短码（`482 917`，`letter-spacing: 0.25em`）与 IP:Port。

### 2.3 矢量图标体系（统一基于 Reicon 24×24 像素网格，严禁使用 Emoji）

全平台（移动端 Compose/SwiftUI、桌面端 GPUI、Web 原型）统一采用 **[Reicon](https://github.com/dqev/reicon)**（24×24 标准网格，2px 纯净描边与标准几何曲率）作为核心图标库：

| 语义角色 | Reicon 标准图标 | Android (Compose ImageVector) | iOS (SwiftUI Vector / SF Symbols) | 桌面 GPUI (Rust SVG Path) |
| :--- | :--- | :--- | :--- | :--- |
| **系统设置** | `reicon::settings` | `Reicon.Settings` | `Image("reicon_settings")` | `svg().path("reicon/settings.svg")` |
| **前后摄翻转** | `reicon::camera-rotate` | `Reicon.CameraRotate` | `Image("reicon_camera_rotate")` | N/A |
| **曝光补偿** | `reicon::sun` | `Reicon.Sun` | `Image("reicon_sun")` | N/A |
| **水平镜像** | `reicon::flip-horizontal` | `Reicon.FlipHorizontal` | `Image("reicon_flip_horizontal")` | N/A |
| **防误触锁定** | `reicon::lock` / `unlock` | `Reicon.Lock` | `Image("reicon_lock")` | N/A |
| **恢复自动测光** | `reicon::rotate-ccw` | `Reicon.RotateCcw` | `Image("reicon_rotate_ccw")` | N/A |
| **停止推流快门** | `reicon::square` (圆角) | `Reicon.Square` | `Image("reicon_square")` | `svg().path("reicon/square.svg")` |
| **画面修复/刷新** | `reicon::refresh-cw` | N/A | N/A | `svg().path("reicon/refresh_cw.svg")` |
| **断开连接** | `reicon::phone-off` | `Reicon.PhoneOff` | `Image("reicon_phone_off")` | `svg().path("reicon/phone_off.svg")` |
| **局域网广播** | `reicon::radio` | `Reicon.Radio` | `Image("reicon_radio")` | `svg().path("reicon/radio.svg")` |
| **加密直连** | `reicon::shield-check` | `Reicon.ShieldCheck` | `Image("reicon_shield_check")` | `svg().path("reicon/shield_check.svg")` |
| **服务发现/雷达** | `reicon::radio` | `Reicon.Radio` | `Image("reicon_radio")` | `svg().path("reicon/radio.svg")` |
| **过热降档** | `reicon::flame` | `Reicon.Flame` | `Image("reicon_flame")` | `svg().path("reicon/flame.svg")` |
| **更多操作** | `reicon::more-horizontal` | `Reicon.MoreHorizontal` | `Image("reicon_more_horizontal")` | `svg().path("reicon/more_horizontal.svg")` |
| **导航返回** | `reicon::chevron-left` | `Reicon.ChevronLeft` | `Image("reicon_chevron_left")` | `svg().path("reicon/chevron_left.svg")` |

---

## 3. 📱 移动端 Sender 细化验收标准（Jetpack Compose & SwiftUI）

### 3.1 发现与主页（Screen 1）
- [ ] **AC-M-DISC-01**：顶栏左侧展示产品名，右侧胶囊实时显示当前 Wi-Fi 名称（如 `Wi‑Fi · Office‑5G`），最右侧为 `Settings` 矢量图标按钮。
- [ ] **AC-M-DISC-02**：顶部常驻呼吸脉冲点指示发现中（橙色波纹）或监听中（绿色常驻）。
- [ ] **AC-M-DISC-03**：已配对设备（Paired）卡片带有 `已配对` 橙黄徽标，右侧独立收纳 `More Horizontal` 按钮呼出管理菜单，主体点按直接发起直连。
- [ ] **AC-M-DISC-05**：局域网广播未发现设备时展示空状态排查卡片，底部提供「输入 IP 直连」按钮；到达 Receiver 后再进入连接码授权页。

### 3.2 连接码授权与手动 IP 直连抽屉（Screen 2 - Manual Connect Sheet）
- [ ] **AC-M-MANUAL-01**：点击主页底部按钮升起原生半屏抽屉；已通过 mDNS 确定 Receiver 时输入桌面端展示的 6 位连接码，发现失败时先输入局域网 IP:端口（如 `192.168.1.108:4433`），连接建立后再输入连接码。
- [ ] **AC-M-MANUAL-02**：输入 IP:端口后，Sender 直接向目标 Endpoint 发起 QUIC/TLS 握手；六位连接码仅在加密控制 Stream 内提交，不得被解释为地址查询码。
- [ ] **AC-M-MANUAL-03**：首屏 0 相机权限要求，相机权限延迟至正式进入推流（Screen 5）时按需申请。
- [ ] **AC-M-MANUAL-04**：连接流程不提供二维码生成或扫码入口，不引入 ZXing、ML Kit Barcode Scanning 或其他扫码 SDK。

### 3.3 配对确认与等待态（Screen 3 & Screen 4）
- [ ] **AC-M-PAIR-01**：Sender 使用大字号等宽输入框接收桌面端展示的 6 位连接码（`482 917`，`letter-spacing: 0.25em`），并显示剩余有效时间。
- [ ] **AC-M-PAIR-02**：连接码到期后输入结果不可提交，文案红字提示 `连接码已过期，请在电脑端刷新`。
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
- [ ] **AC-D-TECH-03（gpui-component 与 Tailwind CSS 4.0 对齐）**：
  - 桌面 UI 组件库必须基于 `gpui-component` 体系；
  - 样式遵循 **Tailwind CSS 4.0** 的原子化类名与 `@theme` 变量体系（如 `bg-zinc-950`, `bg-zinc-900/80`, `border-zinc-800`, `rounded-xl`, `font-mono` 等），确保前端原型类名可 1:1 无损映射为 Rust GPUI DSL。

### 4.2 待机与连接主页（Desktop Connect View - 左右 58% : 42% 黄金分栏）
- [ ] **AC-D-LAYOUT-01（左右 58%:42% 黄金分栏）**：
  - 左侧 58% 宽度承载主机标识、连接码与开始使用指引；
  - 右侧 42% 宽度由「Box 1 设备与连接状态机」与「Box 2 极简网络状态」纵向排列组成。
- [ ] **AC-D-HOST-02（主机识别卡片）**：
  - 顶部展示纯粹设备名 `Studio PC`（彻底移除生硬的 `（本机）` 括号文字）；
  - 搭配绿色状态徽标 `<span class="w-1.5 h-1.5 rounded-full bg-emerald-500"></span> 接收端已就绪`；
  - 居中呈现大号等宽 6 位局域网无线连接码（`482 917`，支持点击重新生成）与局域网 IP 直连胶囊（`192.168.1.108:4433`，带一键复制）。
- [ ] **AC-D-ONBOARDING-03（开始使用与硬件拓扑）**：
  - 左卡片底部包含清晰的开始使用 3 步指南（1. 打开手机 App → 2. 选择自动发现的电脑或输入 IP → 3. 输入连接码并开始推流）；
  - 包含真机硬件互联拓扑图（手机 ➔ 无线波纹 ➔ 电脑）与 3 项轻量系统状态（虚拟摄像头、自动发现、连接方式）。

### 4.3 右侧卡片：设备连接状态机与极简网络状态
- [ ] **AC-D-DEVICE-01（待机多设备纵向滚动卡片）**：
  - 待机未连接时，Box 1 呈现多设备独立卡片构成的纵向滚动列表（`iPhone 16 Pro`、`iPad Pro 11"`、`Xiaomi 14 Ultra`、`MacBook Air M2`），每一台设备显示型号、镜头规格、在线状态及 `[ 连接 ]` 按钮；
  - 填满右侧纵向高度，视觉紧凑充实，彻底消除空旷感。
- [ ] **AC-D-DEVICE-02（推流状态无缝切换）**：
  - 点击任意设备连接后，多设备列表自动收起；
  - 原地切换为当前推流设备条目（显示 `实时推流中 · 1080p 60FPS` 及红色危险态 `[ 断开 ]` 按钮）；
  - 展开实时视频规格参数（1080p 60FPS/码率/电量）与 4 组镜头画面控制网格。
- [ ] **AC-D-NET-03（极简网络状态卡片）**：
  - Box 2 保持极简 4 行原生指标（网络: 局域网可用 / 发现服务: 在线 / 延迟: 低 / 安全: 已保护）。

### 4.4 直播接收监视器与镜头控制面板
- [ ] **AC-D-LIVE-01**：推流时左侧大卡片自动切换为实时大屏视频流监视器（16:9 画幅、Canvas 动态推流渲染、左上角实时规格水印、右上角虚拟摄像头输出状态）。
- [ ] **AC-D-LIVE-02（镜头与画面控制网格）**：
  - 右侧 Box 1 展开 4 组 2×2 镜头控制按钮：
    1. ⇋ **`镜像翻转`**（水平镜像翻转画面）；
    2. 📷 **`镜头切换`**（前置 / 后置主摄 / 超广角）；
    3. 🔄 **`画面修复`**（请求 IDR 关键帧消除卡顿与花屏）；
    4. 📸 **`拍照截图`**（截取当前高清帧画面保存）。

### 4.5 Sidebar 导航结构收敛
- [ ] **AC-D-NAV-01（4+2 极简导航体系）**：
  - 彻底移除一级导航中独立的「已信任设备」页面（多设备管理直接收敛于首页滚动卡片列表中）；
  - 彻底移除冗余的「系统状态」卡片；
  - Sidebar 导航项固定为：`连接`（首页）、`虚拟摄像头`、`网络`、`通用`，底部固定为 `帮助`、`关于` 与主题切换。
- [ ] **AC-D-NAV-02（100% Reicon 矢量图标标准化）**：
  - 全局所有功能图标严格采用 `dqev/reicon` 官方 24×24 像素网格矢量标准，严禁在功能交互中使用系统 emoji。

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
5. ❌ **扫码能力回流**：
   - 严禁添加二维码生成、二维码扫描、条码解析或扫码相机预览；
   - 严禁引入 ZXing、ML Kit Barcode Scanning 或其他仅服务于扫码的依赖。

---

## 6. 验证与追溯映射

| 验收项分类 | 对应验证方式 | 关联 Use Case | 原型验证入口 |
| :--- | :--- | :--- | :--- |
| **连接码授权与桌面确认** | 单元测试 + 双端联动模拟 | PUC-001 | 剧本① (首次配对) |
| **已配对快速直连** | 单元测试 + 状态快照检查 | PUC-002 | 剧本② (已配对直连) |
| **连接码授权与 IP 直连** | 协议状态测试 + 输入 UI 交互 + 失败限流检查 | PUC-008 | 剧本③ (手动连接) |
| **虚拟摄像头就绪** | 跨进程共享内存测试 + 驱动状态检查 | PUC-004 | 剧本④ (虚拟摄像头) |
| **画质/镜头/调光交互** | UI 自动化测试 + 手势微动效检查 | PUC-005 | 剧本⑤ ~ ⑧ (相机操作) |
| **弱网中断退避重连** | 模拟丢包/网络切断断言 | PUC-006 | 剧本⑨ (网络中断重连) |
| **信任设备管理与删除** | 状态持久化断言 | PUC-007 | 剧本⑩ (删除配对设备) |
