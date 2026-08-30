# REQ-PICOO-UI-0001：全端 UI 交互设计与细化验收规范

- **Requirement ID**: `REQ-PICOO-UI-0001`
- **Area**: `PICOO-UI`
- **Status**: `planned`
- **Design Visualizations**:
  - Desktop Receiver: [`picoo-camera-receiver.html`](../../../picoo-camera-receiver.html)
  - Mobile Sender: [`picoo-camera-sender.html`](../../../picoo-camera-sender.html)（旧版深色视觉；按本规范刷新前不得作为新移动端实现依据）
- **Supported Use Cases**:
  - [BUC-001](../use-cases/business/buc-001-phone-as-wireless-meeting-camera.md)
  - [PUC-001](../use-cases/product/puc-001-first-install-and-pairing.md)
  - [PUC-002](../use-cases/product/puc-002-discover-and-connect-paired-receiver.md)
  - [PUC-004](../use-cases/product/puc-004-use-virtual-camera-in-meeting-apps.md)
  - [PUC-005](../use-cases/product/puc-005-adjust-camera-during-streaming.md)
  - [PUC-006](../use-cases/product/puc-006-auto-reconnect-after-network-interruption.md)
  - [PUC-007](../use-cases/product/puc-007-manage-paired-devices.md)
  - [PUC-008](../use-cases/product/puc-008-connect-with-code-or-ip.md)
- **Architectural Boundaries**:
  - [ARCH-PICOO-UI-001 桌面 GPUI 与手机原生 UI 边界](../architecture/0009-desktop-gpui-mobile-native-ui-boundary.md)
  - [ARCH-PICOO-UI-002 跨端视觉语义、原生适配与 Icon 边界](../architecture/0010-cross-platform-design-system-boundary.md)

---

## 1. 范围与目标

本规范定义 Picoo Camera 在移动端（Android Jetpack Compose / iOS SwiftUI）与桌面端（Windows / macOS GPUI）的用户界面、交互心智、视觉令牌、状态映射以及可量化验收标准（Acceptance Criteria）。

### 核心设计原则
1. **同一产品语义，不复制同一布局**：全端共享 token、状态、Icon 与信息层级；Compose、SwiftUI 和 GPUI 分别保留原生导航、焦点、手势和窗口行为。
2. **移动端坚定靠拢专业原生相机心智**：Sender 是取景与推流相机，不是后台管理表单；Control context 保持安静精确，Camera context 保持沉浸取景、即时手势与克制动效。
3. **桌面端契合现代高质感原生视窗**：Receiver 是基于 GPUI 的高性能原生桌面监视器，首次启动默认亮色并支持切换深色主题，不是内嵌浏览器的 Electron 网页。
4. **统一功能 Icon，区分品牌与插图**：功能 Icon 使用固定版本 Reicon 和统一语义映射；App Icon、平台窗口装饰与硬件示意图不混入功能 Icon 体系；禁止 Emoji 代替 Icon。
5. **单向数据流防腐边界**：UI 视图仅观察只读状态快照（`DesktopAppState` / Mobile State）并派发 Action，严禁 UI 层持有底层 Socket、编解码器或共享帧内存。

---

## 2. 全局设计系统与视觉令牌规范

### 2.1 三层 token 与单一主题事实源

- [ ] **AC-DSYS-01（三层 token）**：全端必须具有 Primitive → Semantic → Component 三层设计契约。Feature 页面只能消费 Semantic 或 Component token；原始色值、字号、间距、圆角、阴影和动效时长只允许出现在集中 token 定义或经审计的数据/媒体内容中。
- [ ] **AC-DSYS-02（平台适配）**：Android 通过 `MaterialTheme` 与 Picoo `CompositionLocal`，iOS 通过 Asset Catalog Any/Dark 与 SwiftUI token 扩展，桌面通过 `gpui-component::Theme` 与 Picoo 语义扩展消费同一契约。不得在单个 Screen/View 中建立私有 Palette 或另一套组件常量。
- [ ] **AC-DSYS-03（状态完整）**：组件覆盖适用的 Rest、Pressed、Focus、Selected、Disabled、Loading、Error 与 Open 状态；桌面另外覆盖 Hover。状态不得只依赖颜色，破坏性操作必须使用 Destructive 意图而不是 Primary。

### 2.2 Control context 与 Camera context

- [ ] **AC-DSYS-04（主题上下文）**：设备发现、手动连接、配对、等待和设置属于 Control context。Android/iOS 跟随系统浅色/深色；桌面首次启动默认 Picoo Light 并可切换 Picoo Dark。Sender 实时取景属于 Camera context，固定为深色沉浸界面，但不得通过应用级强制深色影响其他页面。

Control context 的 Primitive 基线与当前 Picoo Desktop 主题对齐：

| Primitive | Light | Dark | 用途边界 |
| --- | --- | --- | --- |
| `neutral.canvas` | `#ffffff` | `#0a0a0a` | 页面基面 |
| `neutral.chrome` | `#fafafa` | `#171717` | 顶栏、Sidebar、分组表面 |
| `neutral.subtle` | `#f5f5f5` | `#262626` | 次级填充、按下态 |
| `neutral.border` | `#e5e5e5` | `rgba(255,255,255,0.10)` | 分隔线、普通边界 |
| `neutral.foreground` | `#0a0a0a` | `#fafafa` | 主内容 |
| `neutral.muted` | `#737373` | `#a1a1a1` | 次要说明 |
| `brand.primary` | `#1447e6` | `#193cb8` | 默认承诺、选中与品牌重点 |
| `brand.highlight` | `#155dfc` | `#2b7fff` | Focus、交互高亮 |
| `status.success` | `#00bc7d` | `#00bc7d` | 已连接、就绪 |
| `status.warning` | `#b77900` | `#f0c14a` | 过热、权限待处理 |
| `status.danger` | `#e7000b` | `#ff6467` | 拒绝、断开、不可恢复错误 |

组件不直接引用表中 Primitive，而是引用 `surface.page`、`surface.group`、`surface.subtle`、
`content.primary`、`content.muted`、`border.default`、`action.primary`、`action.destructive`、
`status.*` 与 `focus.ring` 等 Semantic token。Camera context 额外定义 `camera.surface`、
`camera.overlay`、`camera.content`、`camera.contentMuted` 和 `camera.control`，不复用 Control
context 的普通 Card 颜色模拟取景 HUD。

### 2.3 字体、间距、圆角与动效

- [ ] **AC-DSYS-05（排版）**：UI 正文使用平台系统 UI 字体并支持系统文字缩放；只在短码、IP:Port、延迟、丢包、帧率和码率使用平台等宽字体与表格数字。排版角色保持 `caption / body / label / section / title / code / codeHero` 的稳定层级，不以页面私有字号表达层级。
- 间距关系使用 `2 / 4 / 8 / 12 / 16 / 24 / 32` 语义比例；Control context 通过共享 leading/trailing spine、分隔线和分组间距建立层级，禁止重复叠加容器 padding。
- 圆角使用 `radius.control = 7`、`radius.surface = 15`、`radius.full` 三个语义关系；平台可做光学适配，但同类组件必须使用同一角色。
- 动效只用于说明状态和空间变化，使用 `motion.fast / normal / deliberate`；系统开启 Reduced Motion 时改为无动画或短淡入淡出，不能隐藏状态结果。

### 2.4 Reicon 功能 Icon 契约

- [ ] **AC-DSYS-06（单一 Icon 事实源）**：功能 Icon 统一使用固定 commit 的 Reicon 24×24 子集，`assets/icons/reicon/*.svg` 是唯一矢量事实源。Android Vector Drawable、iOS SVG Image Set、GPUI 编译期 SVG 与 HTML 可视化必须可追溯到该目录；不得使用 Emoji、Material Icons、SF Symbols、Lucide 或临时手绘图标替代产品功能 Icon。
- [ ] **AC-DSYS-07（语义映射）**：业务代码依赖语义 Icon，而不是任意文件名或字符串。相同意图在全端使用相同 Source glyph；未知、缺失或拼错名称必须在构建或测试中失败，禁止静默替换成 `info` 等其他图标。

| Semantic icon | Reicon Source glyph | 主要用途 |
| --- | --- | --- |
| `settings` | `settings` | 系统设置入口 |
| `receiverDevice` | `desktop` / 桌面导航复合语义使用 `monitor-phone` | Receiver 身份与连接目的地 |
| `discovering` | `radio` | 局域网发现状态 |
| `network` | `wifi` | 网络状态 |
| `secureConnection` | `shield-check` | 加密直连与已保护状态 |
| `switchCamera` | `camera-rotate` | 前后摄切换 |
| `exposure` | `sun` | 曝光补偿 |
| `mirror` | `flip-horizontal` | 水平镜像 |
| `interactionLock` | `lock` / `unlock` | 防误触状态 |
| `resetExposure` / `repairFrame` | `refresh` | 由所在组件与可访问名称区分两个明确动作 |
| `stopStream` | `stop` | 停止推流快门；使用 Reicon 的方形停止 glyph |
| `disconnect` | `phone-off` | 明确断开；实现前必须补入共享 Reicon 子集 |
| `overheat` | `flame` | 热保护降档 |
| `moreActions` | `more-horizontal` | 对象次级操作 |
| `navigateBack` | `chevron-left` | 移动端返回 |
| `rejected` | `xmark` | 拒绝或失败 |
| `expired` | `clock` | 短码超时 |

默认使用 Outline；Filled 只用于选中、完成或需要在紧凑尺寸区分的复合设备状态。功能 Icon 使用
`currentColor`/平台 tint 消费语义颜色，Asset 内不得固化页面颜色。App Icon、系统窗口装饰和硬件拓扑
插图是独立资产类型，不纳入 Reicon 功能 Icon 清单。

- [ ] **AC-DSYS-08（Icon 尺寸与可访问性）**：视觉尺寸只使用 `icon.compact = 16`、`icon.standard = 20`、`icon.emphasis = 24`、`icon.hero = 32`。视觉 glyph 与点击目标分离：Android Icon button 至少 48dp，iOS 至少 44pt，桌面紧凑 Icon button 使用组件系统 32px 语义尺寸。Icon-only 控件必须提供准确的无障碍名称；桌面同时提供 Tooltip；装饰 Icon 从语义树隐藏。

---

## 3. 📱 移动端 Sender 细化验收标准（Jetpack Compose & SwiftUI）

### 3.0 移动端整体结构与平台行为

- [ ] **AC-M-SHELL-01（主题与表面）**：Devices、Manual Connect、Pairing、Waiting 与 Settings 使用 Control context 并跟随系统明暗；不得继续以应用级强制 Dark mode 覆盖系统偏好。页面使用单一基面、分组和分隔线建立层级，不得把每个重复行包装为独立悬浮卡片。
- [ ] **AC-M-SHELL-02（原生组件）**：Android 与 iOS 使用各自原生 Top bar、List/Row、Switch、Text field、Sheet、Dialog、Focus 与 Back 行为。共享的是内容顺序、Semantic token、Icon 和状态，不要求两个平台像素完全一致。
- [ ] **AC-M-SHELL-03（可达性）**：主任务按视觉顺序进入焦点/朗读顺序；Icon-only 控件、连接状态、倒计时和重连状态具有准确名称和值；系统文字放大后说明和操作不得被固定高度裁切。

### 3.1 发现与主页（Screen 1）
- [ ] **AC-M-DISC-01**：原生顶栏左侧展示产品名，右侧紧凑状态显示当前 Wi-Fi 名称（如 `Wi‑Fi · Office‑5G`），最右侧为语义 `settings` Icon button；视觉 glyph 使用 standard 尺寸，点击目标遵守平台最小触控尺寸。
- [ ] **AC-M-DISC-02**：发现状态位于设备列表标题附近，使用语义 `discovering` Icon、品牌色动效和明确文案；发现完成/监听中使用稳定状态而非永久环境装饰。Reduced Motion 下停止呼吸缩放但保留文案与颜色变化。
- [ ] **AC-M-DISC-03**：已配对设备使用可扫描的 `DeviceRow`，包含固定 Receiver Icon lane、设备名、状态/最近连接信息和 trailing action lane；`已配对` Badge 使用信息/选中语义，`moreActions` 呼出原生管理菜单或 Sheet，主体点按直接连接。
- [ ] **AC-M-DISC-04**：未配对新电脑显示 `在线 · 首次连接需核对短码`；离线电脑降低强调但仍满足对比度，显示最近连接时间并保留可理解的管理入口。同行 DeviceRow 必须共享图标、标题、状态和操作对齐轴。
- [ ] **AC-M-DISC-05**：局域网未发现设备时使用同一页面层级中的克制空状态，不嵌套多层 Card；提供排查说明与一个清晰的「输入 IP 直连」次级动作。发现到 Receiver 后再进入双端短码核对页。

### 3.2 手动 IP 直连抽屉（Screen 2 - Manual Connect Sheet）
- [ ] **AC-M-MANUAL-01**：点击主页动作升起原生 Sheet，只接收局域网 `IP:端口`（如 `192.168.1.108:4433`）；通过 mDNS 选择设备时不展示此输入。紧凑窗口可使用半屏，键盘或较大文字需要更多空间时允许扩展到大 Detent/完整页面。
- [ ] **AC-M-MANUAL-02**：输入 IP:端口后，Sender 直接向目标 Endpoint 发起 QUIC/TLS 握手；连接建立后进入与自动发现路径相同的双端短码核对，短码不得被解释为地址查询码。
- [ ] **AC-M-MANUAL-03**：首屏 0 相机权限要求，相机权限延迟至正式进入推流（Screen 5）时按需申请。
- [ ] **AC-M-MANUAL-04**：连接流程不提供二维码生成或扫码入口，不引入 ZXing、ML Kit Barcode Scanning 或其他扫码 SDK。

### 3.3 配对确认与等待态（Screen 3 & Screen 4）
- [ ] **AC-M-PAIR-01**：Sender 与 Receiver 使用相同的安全连接信息顺序：Receiver 身份 → `secureConnection` 状态 → 大字号等宽只读 6 位短码（`482 917`）→ 剩余有效时间 → 核对说明 → 确认操作。移动端采用适合单手确认的布局，不复制桌面分栏。
- [ ] **AC-M-PAIR-02**：短码到期后确认按钮不可用，文案红字提示 `短码已过期`，并提供重新发起本次配对的明确操作。
- [ ] **AC-M-PAIR-03**：等待电脑确认过程中，若对端点击拒绝，即时切换为 Danger 状态、语义 `rejected` Reicon 和 `电脑端拒绝了连接` 文案；不得只依赖红色表达结果。

### 3.4 实时推流原生相机控制台（Screen 5 - 核心重点）
- [ ] **AC-M-LIVE-01（顶部 HUD）**：
  - 左侧胶囊显示绿点 + 电脑设备名 + 实时 RTT（如 `● Studio PC 18ms`）；
  - 中间工具栏依次使用语义 `exposure`、`mirror`、`interactionLock` Reicon；激活时使用 Camera context 的 selected token，并通过可访问状态描述报告当前值；
  - 右上角画质药丸：文本为 `1080P · 30` / `720P · 30` / `480P · 30`，**点按直接一键秒级轮换切换，严禁弹出全屏抽屉/菜单**。
- [ ] **AC-M-LIVE-02（设备过热）**：
  - 设备过热时画质药丸使用 Warning token，并附带语义 `overheat` Reicon 与 `720P · 30` 文案；
  - 强制降档至 720P，点击弹出 Toast 提示 `设备偏热保护中，1080P 暂不可选`。
- [ ] **AC-M-LIVE-03（取景器手势与安全框）**：
  - 取景画面正中常驻 `16:9` 电脑端裁切虚线框；
  - 点击取景器任意坐标，生成黄色方框对焦环（Focus Ring），伴随向内微缩与 800ms 渐隐淡出动效；
  - 双击取景器空白区域进入全屏沉浸模式（淡出所有浮层）。
- [ ] **AC-M-LIVE-04（极简调光条）**：
  - 调光条为紧凑单行：减少按钮 + `exposure` Reicon 与 `自动 / 提亮 +1` 状态 + 增加按钮 + `resetExposure` Reicon；
  - 视觉控件保持紧凑，但每个按钮使用平台最小触控目标；档位严格映射为 `自动` / `提亮 +1` / `提亮 +2` / `压暗 -1` / `压暗 -2`；
  - 取景画面实时叠加明暗滤镜（`brightness`）反馈。
- [ ] **AC-M-LIVE-05（底栏监控与镜头文字移除）**：
  - 左侧监控仅展示当前码率（`5.2 Mbps`）与网络质量（`30 FPS · 0% 丢包`）；
  - **彻底移除“前置镜头/后置镜头”等多余文字注释**（用户看取景画面即可直观确认）。
- [ ] **AC-M-LIVE-06（停止录像大圆钮）**：
  - 外层使用 `camera.stopTarget` 组件 token 的正圆半透明环，内层使用 `stopStream` Reicon 或由同一语义组件绘制的圆角停止方块；两端不得分别发明不同的停止符号；
  - 内外比例、Danger 颜色和 Armed 状态全部来自 `CameraStopControl` token，不在页面散落原始尺寸与色值；
  - 轻触一次外环变红呼吸脉冲并提示 `再次点击确认断开`，3 秒内二次点击安全挂断退出，3 秒超时自动复位。
- [ ] **AC-M-LIVE-07（原生翻转镜头按钮）**：
  - 右下角使用 Camera context 圆形控制，内嵌语义 `switchCamera` Reicon；视觉 Icon 与平台触控目标分离；
  - 点击时**图标自身带有 180° 平滑旋转动画**，取景器伴随瞬间重聚模糊滤镜，底层递增 `stream_epoch` 并请求 IDR 关键帧。
- [ ] **AC-M-LIVE-08（网络中断重连遮罩）**：
  - 网络突发中断时严禁直接闪退回主页，必须弹出半透明模糊重连遮罩；
  - 显示退避阶梯（`500ms` ➔ `1s` ➔ `2s` ➔ `5s`）并提供 `停止重连并退出` 按钮。

### 3.5 设置中心（Screen 6）
- [ ] **AC-M-SET-01**：顶栏左侧使用 `Chevron Left` 矢量返回图标。
- [ ] **AC-M-SET-02**：提供打开 App 自动直连开关、默认画质选择、已配对电脑管理（查看公钥指纹与撤销信任）与权限状态就绪清单。设置使用原生分组行、Switch、Disclosure 与 Sheet，不把每一项包装为独立品牌 Card。
- [ ] **AC-M-SET-03**：Android 与 iOS 使用相同设置对象、术语、顺序和状态语义；平台能力不同的行可以有不同说明，但不得用 `协议`、`语言模式` 等开发信息占据用户设置层级。

### 3.6 窗口尺寸、折叠屏与横竖屏

- [ ] **AC-M-ADAPT-01**：紧凑窗口为单列任务流；中等/扩展窗口通过最大内容宽度、稳定列或真实的辅助区域利用空间，不把手机 Card 等比例拉宽，也不在没有详情对象时强行引入 List-detail。
- [ ] **AC-M-ADAPT-02**：Camera context 在手机、平板和展开态折叠屏保持媒体优先并隐藏持续导航；控件避开 Display cutout、系统栏、导航栏和折叠铰链。横竖屏切换后停止按钮、HUD 与返回路径仍可达。
- [ ] **AC-M-ADAPT-03**：Android 在 Phone、Foldable、Tablet 代表窗口上具有 Preview/Screenshot 覆盖；iOS 在紧凑/常规 size class、横竖屏与 Dynamic Type 代表级别上具有 Preview/Snapshot 覆盖。未经产品确认不得用实验性 Grid、FlexBox、Styles 或 MediaQuery API 作为必要实现依赖。

---

## 4. 🖥️ 桌面端 Receiver 细化验收标准（GPUI Windows & macOS）

### 4.1 技术选型与组件基线
- [ ] **AC-D-TECH-01**：Windows 与 macOS 共用同一套 Rust GPUI 代码，**严禁引入 Electron、Tauri、WebView 或内嵌浏览器运行环境**。
- [ ] **AC-D-TECH-02**：视频监视器核心自定义组件必须封装为 **`VideoSurface`**，绑定 FrameHub 解码环形缓冲，保持 16:9 画幅与断流占位画面平滑切换。
- [ ] **AC-D-TECH-03（gpui-component 与 Tailwind CSS 4.0 对齐）**：
  - 桌面 UI 组件库必须基于 `gpui-component` 体系；
  - `ARCH-PICOO-UI-002` 与本规范中的共享 token 是视觉比例和语义角色的事实源；HTML 原型中的 Tailwind CSS 4.0 类名与 `@theme` 变量是该契约的可视化映射，不是在 Rust 中保留 CSS/Web 运行时的要求；
  - 间距、字号、图标和普通布局尺寸必须映射为 GPUI 的 `rem` scale helper 或 `gpui-component` 语义尺寸，产品颜色、圆角与阴影必须集中映射到 Picoo 语义主题，禁止在页面调用点散落原始色值和普通布局 `px(...)`；
  - Button、Switch、AlertDialog、滚动条等交互必须保留 `gpui-component` 的跨平台键盘、焦点、禁用态和 dismissal 契约；HTML 仅负责外观与信息架构，不能以像素复刻为由降级这些行为；
  - 允许且必须保留经产品确认的原型覆盖项：默认窗口为 1920×1080、最小窗口为 1180×720、连接页使用可用宽度而不保留 HTML 的 1160px 上限、实时预览严格保持 16:9；窗口边界属于平台物理尺寸，可使用 `px(...)`。
- [ ] **AC-D-TECH-04（Windows 产品进程）**：从资源管理器或开机启动打开桌面端时不得附带命令行窗口，状态检测也不得启动 `reg.exe` 等控制台子进程；普通权限启动只通过无注册表写入能力的 API 检测并启动已安装的虚拟摄像头，不得自动尝试写系统级 COM 注册表。修复注册只由 MSI 或用户明确触发的修复动作承担；显式修复必须触发 Windows UAC、避免阻塞 GPUI 线程和重复提交，并在当前虚拟摄像头界面内显示等待、成功或失败结果。
- [ ] **AC-D-TECH-05（macOS 图标启动）**：从 Finder、Dock 或 `open` 启动打包后的 `Picoo Camera.app` 必须及时显示主窗口；Shared Frame Ring 文件访问不得阻塞主线程启动，暂不可用时应降级为页面内状态而不是留下无窗口进程。

### 4.2 待机与连接主页（Desktop Connect View - 左右 58% : 42% 黄金分栏）
- [ ] **AC-D-LAYOUT-01（左右 58%:42% 黄金分栏）**：
  - 左侧 58% 宽度承载主机标识、配对短码状态与开始使用指引；
  - 右侧 42% 宽度由「Box 1 设备与连接状态机」与「Box 2 极简网络状态」纵向排列组成。
  - 连接页滚动由主内容面板统一持有，内容 inset 位于滚动容器内部，滚动条必须贴合面板尾边；两个分栏以相同 `rem` 比例和共同间距缩放，不得以固定像素分别修正。
- [ ] **AC-D-HOST-02（主机识别卡片）**：
  - 顶部展示纯粹设备名 `Studio PC`（彻底移除生硬的 `（本机）` 括号文字）；
  - 搭配绿色状态徽标 `<span class="w-1.5 h-1.5 rounded-full bg-emerald-500"></span> 接收端已就绪`；
  - 设备名下方不显示说明副标题；左侧标题分割线必须与右侧「设备与连接」标题分割线水平对齐；
  - 未收到配对请求时显示 `等待请求`；收到未配对 Sender 请求后，居中呈现本次握手的大号等宽 6 位配对短码（`482 917`）与局域网 IP 直连胶囊（`192.168.1.108:4433`，带一键复制）。
- [ ] **AC-D-SETTINGS-04（设置页面归属）**：
  - 通用页只显示电脑名称与桌面生命周期偏好，不得混入虚拟摄像头占位画面、可信设备管理或诊断选项；
  - 无视频流占位画面归属虚拟摄像头页，自动接受与可信设备管理归属连接页“设备与连接”卡片，日志级别与脱敏诊断导出归属帮助页诊断区。
- [ ] **AC-D-ONBOARDING-03（开始使用与硬件拓扑）**：
  - 左卡片底部包含与 HTML 原型一致的开始使用 3 步指南（1. 打开 Picoo Camera → 2. 选择此电脑 → 3. 核对短码并确认）；
  - “开始使用”与“设备与连接”使用相同标题字号和字重；真机拓扑卡片的上边界与“开始使用”标题对齐，下边界与第 3 步对齐；
  - 包含真机硬件互联拓扑图（手机 ➔ 无线波纹 ➔ 电脑），iPhone 必须保留原型中的左右金属边框、侧键与高光；拓扑卡片只呈现硬件连接 SVG，下方不附加状态文案；
  - “自动接受可信设备”使用设置行的次级字号，不得与卡片标题争夺层级。

### 4.3 右侧卡片：设备连接状态机与极简网络状态
- [ ] **AC-D-DEVICE-01（已信任与最近连接设备列表）**：
  - Receiver 不主动发现或连接 Sender。待机时 Box 1 只呈现本机可信设备存储中的已信任/最近连接设备，显示名称、平台、最近连接时间和等待/离线状态，不提供伪造的 `[ 连接 ]` 动作；
  - 卡片标题与设备条目必须使用同一固定版本 Reicon 的手机设备语义图标；不得退化为纯文字列表或平台 Emoji；
  - 列表拥有独立纵向滚动区域并填充右侧可用高度；空列表显示首次连接指引。
- [ ] **AC-D-DEVICE-02（推流状态无缝切换）**：
  - Sender 建立推流后，设备列表自动收起；
  - 原地切换为当前推流设备条目（显示 `实时推流中 · 1080p 30FPS` 及红色危险态 `[ 断开 ]` 按钮）；
  - 展开真实视频规格参数（H.264、720p30/1080p30、码率/电量）与 3 组已实现的镜头画面控制。
- [ ] **AC-D-NET-03（极简网络状态卡片）**：
  - Box 2 保持极简 4 行原生指标（网络: 局域网可用 / 发现服务: 在线 / 延迟: 低 / 安全: 已保护）。
  - 标题使用活动/指标图标；四行分别使用 Wi-Fi、服务、活动、安全语义 Reicon，值末尾使用完成或异常状态图标，不得只依赖颜色圆点表达状态。
  - 待机主卡底栏同时展示局域网监听状态与 `shield-check` 安全直连提示。

### 4.4 直播接收监视器与镜头控制面板
- [ ] **AC-D-LIVE-01**：推流时左侧大卡片自动切换为实时大屏视频流监视器（16:9 画幅、Canvas 动态推流渲染、左上角实时规格水印、右上角虚拟摄像头输出状态）。
- [ ] **AC-D-LIVE-02（镜头与画面控制）**：
  - 右侧 Box 1 展开 3 组真实可用的镜头控制按钮：
    1. `mirror` Reicon + **`镜像翻转`**（水平镜像翻转画面）；
    2. `switchCamera` Reicon + **`镜头切换`**（前置 / 后置主摄 / 超广角）；
    3. `repairFrame` Reicon + **`画面修复`**（请求 IDR 关键帧消除卡顿与花屏）。
  - 未实现本地截图保存前不展示或伪装 `拍照截图` 控件。

### 4.5 Sidebar 导航结构收敛
- [ ] **AC-D-NAV-01（4+2 极简导航体系）**：
  - 彻底移除一级导航中独立的「已信任设备」页面（多设备管理直接收敛于首页滚动卡片列表中）；
  - 彻底移除冗余的「系统状态」卡片；
  - Sidebar 导航项固定为：`连接`（首页）、`虚拟摄像头`、`网络`、`通用`，底部固定为 `帮助`、`关于` 与主题切换。
- [ ] **AC-D-NAV-02（100% Reicon 矢量图标标准化）**：
  - 所有产品功能图标严格采用 `dqev/reicon` 官方 24×24 像素网格矢量标准，严禁在功能交互中使用系统 emoji；
  - 一级导航图标必须与页面职责直接对应：`连接`使用同时表达手机与电脑的 `monitor-phone`，`虚拟摄像头`使用同时表达显示器与摄像头输出的 `monitor-camera`，不得继续使用泛化的 `home` 或 `monitor`；
  - 最小化、最大化、关闭等平台窗口装饰由 `gpui-component::TitleBar` 统一提供；Sidebar 折叠控制使用 Reicon Filled `sidebar-left` / `sidebar-right`，与导航图标保持同一图标体系。
- [ ] **AC-D-NAV-03（可折叠图标 Sidebar）**：
  - 桌面窗口使用贴边的单层工作区，不展示独立品牌标题栏，不保留工作区外侧留白，也不使用第二层圆角边框包裹 Sidebar 与主内容区；Sidebar 只拥有与主内容相邻的分割线；
  - Sidebar 默认展开并保持 HTML 原型的 `204px` 宽度，两端均可切换为 `48px` 图标态；“连接”必须保留在 Sidebar 导航列表并与其他导航项使用相同结构。折叠按钮位于 Sidebar 分割线右侧，通过共享行高和顶部 inset 与“连接”导航图标严格处于同一水平中心线；
  - 导航区域遵循官方 Sidebar 的紧凑密度：常规导航行高为 `32px`、功能图标为 `16px`，图标与文案使用 `8px` 语义间距；导航功能图标继续使用 Reicon；
  - 折叠按钮遵循 `gpui-component::SidebarToggleButton` 的紧凑 `ghost + small` 几何与状态语义，展开态显示 Reicon Filled `sidebar-left`，折叠态显示 Reicon Filled `sidebar-right`；应用层继续提供稳定控件 ID、中文悬浮提示与无障碍名称；
  - 展开与折叠必须复用官方 Sidebar 的 `200ms + ease_in_out_cubic` 裁剪宽度过渡：导航内容先按目标态排版，外层宽度连续插值，主内容与折叠按钮随 Sidebar 边界平滑移动，不得瞬间跳变或逐帧挤压导航文案；
  - Windows 不保留额外空标题行，主内容顶部工具行复用 `gpui-component::TitleBar`，折叠按钮、当前页面与最右侧最小化/最大化/关闭按钮处于同一行，未被控件占用的区域可拖拽窗口；macOS 最上方单独保留交通灯与拖拽安全行，Sidebar 导航及主内容工具行位于其下方；两端均不重复展示应用图标与 `Picoo Camera` 文案；
  - 折叠态仅显示导航、主题与展开控制图标，隐藏文案但保留选中态、稳定控件 ID、可聚焦按钮、无障碍名称与悬浮提示；
  - 折叠后释放的宽度由主内容区自动接管，主内容继续保持 `flex_1 + min_w_0` 与单一纵向滚动容器；折叠状态只属于当前桌面进程，不写入跨设备偏好。
  - 桌面应用首次启动默认使用 Picoo Light；Sidebar 主题按钮继续允许在 Picoo Light 与 Picoo Dark 之间切换。

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
   - 严禁同一语义在 Android、iOS 与 GPUI 分别使用 Reicon、Material Icons、SF Symbols 或另一 Icon family；
   - 严禁业务页面通过任意字符串选择 Asset，或在缺失图标时静默替换成另一个 glyph；
   - 严禁使用带有 `← 返回` 文字的旧式导航条。
4. ❌ **断开误触与假死**：
   - 严禁将断开连接做成无确认的一触即断；
   - 严禁在弱网断开时界面假死冻结，必须明确展示重连退避遮罩。
5. ❌ **扫码能力回流**：
   - 严禁添加二维码生成、二维码扫描、条码解析或扫码相机预览；
   - 严禁引入 ZXing、ML Kit Barcode Scanning 或其他仅服务于扫码的依赖。
6. ❌ **主题与组件再次分叉**：
   - 严禁移动端应用级强制深色覆盖 Control context；
   - 严禁 Screen/View 建立私有 Palette、随意尺寸或重复实现 Button、Badge、Sheet、DeviceRow 和 Camera HUD；
   - 严禁通过像素复刻桌面端破坏移动平台原生导航、焦点、文字缩放、触控目标或系统返回行为。

---

## 6. 验证与追溯映射

| 验收项分类 | 对应验证方式 | 关联 Use Case | 视觉/交互证据 |
| :--- | :--- | :--- | :--- |
| **共享主题与组件状态** | token 引用检查 + Light/Dark 截图 + Focus/Disabled/Error 状态测试 | ARCH-PICOO-UI-002 | 桌面与刷新后的移动原型 + 原生截图 |
| **Reicon 事实源与语义映射** | 共享 SVG/平台适配/引用完整性检查 + 无障碍名称测试 | ARCH-PICOO-UI-002 | Icon 清单与各平台代表控件截图 |
| **双端短码核对与确认** | 单元测试 + 双端联动模拟 + Dynamic Type/文字缩放截图 | PUC-001 | 首次配对场景 |
| **已配对快速直连** | 单元测试 + 状态快照检查 + DeviceRow 对齐检查 | PUC-002 | 已配对直连场景 |
| **短码核对与 IP 直连** | 协议状态测试 + 地址输入 UI + 双端确认检查 | PUC-008 | 手动连接场景 |
| **虚拟摄像头就绪** | 跨进程共享内存测试 + 驱动状态检查 | PUC-004 | 虚拟摄像头场景 |
| **画质/镜头/调光交互** | UI 自动化测试 + Reduced Motion/手势检查 | PUC-005 | Camera context 场景 |
| **移动窗口适配** | Phone/Foldable/Tablet 与 iOS size class 截图 + 安全区域检查 | ARCH-PICOO-UI-002 | 紧凑/中等/扩展代表窗口 |
| **弱网中断退避重连** | 模拟丢包/网络切断断言 | PUC-006 | 网络中断重连场景 |
| **信任设备管理与删除** | 状态持久化断言 + Destructive 语义检查 | PUC-007 | 删除配对设备场景 |
