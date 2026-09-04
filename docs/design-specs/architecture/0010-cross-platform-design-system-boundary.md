# ARCH-PICOO-UI-002: 跨端视觉语义、原生适配与 Icon 边界

Status: planned
Source: BUC-001 / PUC-001 / PUC-002 / PUC-005 / PUC-007 / PUC-008 / REQ-PICOO-UI-0001

## 背景

Picoo Camera 的 Sender 与 Receiver 承担不同任务，也使用不同原生 UI 技术。桌面端已经通过
`gpui_kit::component::Theme` 形成浅色/深色语义主题；Android 与 iOS 仍各自维护深色常量和页面级几何，
导致相同产品在颜色、层级、组件密度和 Icon 语义上持续漂移。

跨端一致性不等于复用同一份布局代码或追求像素相同。用户需要识别出它们属于同一个产品，同时仍能
依赖 Android、iOS、Windows 和 macOS 熟悉的导航、焦点、手势、窗口和无障碍行为。

## 架构决策

### 共享设计契约，不共享渲染层

全端共享三层设计契约：

1. **Primitive tokens**：稳定的品牌色、中性色、状态色、间距、圆角、字号比例、Icon 尺寸和动效时长。
2. **Semantic tokens**：按意图命名的 `surface`、`content`、`border`、`action`、`status`、`focus` 和
   `camera overlay` 角色；组件不得直接消费 Primitive。
3. **Component tokens**：`DeviceRow`、`StatusBadge`、`PairingCode`、`PrimaryAction`、
   `CameraHUD`、`CameraControl`、`SettingsGroup` 和 `Sheet` 的几何与状态组合。

各平台只共享上述命名、语义、状态和比例，不共享 View/Composable/Element 代码：

| 平台 | 语义主题入口 | 原生组件边界 |
| --- | --- | --- |
| Android | Compose `MaterialTheme` + Picoo `CompositionLocal` | Compose 导航、Sheet、焦点、手势、语义树与 48dp 触控目标 |
| iOS | Asset Catalog Any/Dark + `Color`/`Font`/Environment 扩展 | SwiftUI Navigation、Sheet、Dynamic Type、VoiceOver 与 44pt 触控目标 |
| Windows / macOS | `gpui_kit::component::Theme` + Picoo 语义扩展 | GPUI Kit 中 GPUI Component 层的窗口、键盘、焦点、Overlay、Tooltip 与组件尺寸 |
| HTML 原型 | CSS custom properties / Tailwind 语义映射 | 只作为经 Design Spec 对齐后的可视化验收入口，不是运行时依赖 |

Tailwind 类名可以表达比例和语义，但不得进入原生运行时。Compose Styles 仍是实验性能力，不作为
Picoo 设计系统成立的前提；若未来引入，必须只适配本契约，不得产生第二套 token 或组件语义。

### 控制界面与相机界面

Picoo 使用两种有明确职责的视觉上下文：

- **Control context**：设备发现、手动连接、配对、等待和设置。Android 与 iOS 默认跟随系统浅色/深色；
  桌面首次启动默认 Picoo Light，并允许用户切换 Picoo Light / Picoo Dark。该上下文强调安静、精确、
  单层表面、稳定对齐和稀缺的强调色。
- **Camera context**：Sender 实时取景。无论系统主题如何均使用深色沉浸式表面，确保预览、HUD 和相机
  控件在不同画面上可读。Camera context 只覆盖取景页面，不得通过强制应用级深色模式影响 Control
  context。

两个上下文共享品牌色、状态色、等宽数据排版、Icon 语义与组件状态。Camera context 允许专用的黑色
基面、半透明 Overlay 和白色前景，但这些值仍必须通过 `camera.*` 语义 token 提供。

### 视觉层级和组件语法

- 使用结构、间距、字重和分隔线建立层级，再使用颜色和容器。
- 同一局部决策区只保留一个 Primary action。普通命令使用原生默认、Outline 或 Ghost 语义。
- Control context 避免卡片嵌套和“每行一个悬浮卡片”；重复设备与设置项优先使用共享表面上的原生行、
  分组和分隔线。
- 状态色只表达状态，不作为装饰；Badge 只承载可快速扫描的短状态。
- 组件必须覆盖适用的 Rest、Pressed、Focus、Selected、Disabled、Loading、Error 与 Open 状态；桌面
  另外覆盖 Hover，移动端不得依赖 Hover 才能发现关键操作。
- 间距关系使用 `2 / 4 / 8 / 12 / 16 / 24 / 32` 的紧凑比例；实现消费语义 token 或平台 scale，
  不在页面散落这些数值。
- UI 正文使用各平台系统 UI 字体；短码、IP:Port、延迟、帧率、码率和丢包率使用平台等宽字体与表格数字。

### Reicon 与产品图形资产边界

功能性 UI Icon 统一使用仓库固定版本的 Reicon 子集，`assets/icons/reicon/*.svg` 是唯一矢量事实源。
需要使用的图标先进入共享子集和语义清单，再生成或校验平台适配物：

- Android：本地 Vector Drawable + 类型安全的语义枚举；
- iOS：SVG Image Set + 类型安全的语义封装，不允许业务页面拼接 Asset 字符串；
- GPUI：编译期嵌入的共享 SVG + 穷尽语义映射，未知名称不得静默替换为其他图标；
- HTML：引用同一 SVG 子集或等价的内联源，不复制另一套路径数据。

Icon 契约分为三层：

1. **Source glyph**：Reicon 上游名称、权重和固定 commit。
2. **Semantic icon**：如 `receiverDevice`、`discovering`、`secureConnection`、`exposure`、`mirror`、
   `interactionLock`、`switchCamera`、`stopStream`、`settings`；业务代码只依赖该层。
3. **Presentation token**：视觉尺寸、颜色、容器和交互状态。共享视觉尺寸为 compact 16、standard 20、
   emphasis 24、hero 32；触控/点击目标由平台组件单独保证，不能用放大 glyph 代替可用的点击区域。

默认使用 Outline 权重；Filled 只用于明确的选中、完成或需要在紧凑尺寸区分的复合设备状态。Icon 使用
当前语义前景色着色，普通、选中、成功、警告、危险和禁用状态分别消费对应 token，不在 Asset 中固化
页面颜色。Icon-only 控件必须提供无障碍名称；桌面同时提供 Tooltip。装饰 Icon 从语义树隐藏。

Reicon 不承担产品 App Icon、平台窗口装饰和硬件示意图职责：

- App Icon 是独立品牌资产，遵守各平台 Launcher/App Icon 规范；
- 最小化、最大化、关闭、macOS 交通灯等由平台窗口系统或 `TitleBar` 提供；
- iPhone、MacBook 和连接拓扑属于插图/设备框，不作为功能按钮 Icon。

不得混用 Emoji、Material Icons、SF Symbols、Lucide 或临时手绘图标替代产品功能 Icon。只有平台拥有且
用户依赖其固定含义的系统装饰可以保留平台原生形态。

### 移动窗口与设备形态适配

移动端基于可用窗口而不是设备型号做布局决策：

- 紧凑窗口使用单列任务流；手动连接、画质选择和设备管理使用原生 Sheet/页面层级。
- 中等或扩展窗口增加内容宽度约束、稳定对齐列或辅助区域，不简单放大字体和卡片；只有真实存在
  list-detail 关系时才采用多 Pane。
- 相机取景属于需要完整媒体空间的任务，在手机、平板和展开态折叠屏上均可保持沉浸；持续导航、
  Bottom bar 或 Navigation rail 在取景时隐藏。
- 折叠铰链、Display cutout、状态栏、导航栏、IME 和横竖屏变化不得遮挡配对主操作、停止推流按钮或
  HUD；预览可以裁切，控制必须留在安全可达区域。
- Android 实现采用项目已选择的稳定 Compose/Navigation 能力。实验性的 Grid、FlexBox、Styles 或
  MediaQuery API 只有在项目版本满足、产品确有需要且显式接受实验性风险时才能引入。

### 无障碍与动效

- 文字缩放后允许布局重排，不得依靠固定高度裁掉状态、说明或操作。
- 状态不得只通过颜色表达；Icon、短文案和可访问状态描述共同表达结果。
- Icon-only 控件必须具有准确名称；状态切换控件必须报告当前状态和值。
- 动效只解释状态变化和空间连续性，遵守 Reduced Motion；关闭动效后流程与状态仍完整可理解。
- Overlay 关闭后恢复焦点；破坏性操作明确说明对象和结果，并使用 Destructive 语义。

## 产物与依赖方向

```text
Design Specs / shared semantic contract
  -> platform token adapters
  -> platform component primitives
  -> feature screens

assets/icons/reicon/*.svg + semantic icon manifest
  -> Android Vector Drawable / typed enum
  -> iOS SVG Image Set / typed wrapper
  -> GPUI compile-time SVG mapping
```

Feature screen 不得反向定义全局 token，不得直接从另一平台复制组件实现，也不得绕过语义 Icon 层读取
任意图标文件。

## 不采用的方案

### 把桌面页面等比例缩小到手机

不采用。桌面依赖持续导航、鼠标、键盘和可调整窗口；手机依赖触控、单手可达、系统 Sheet 与沉浸取景。
统一的是产品语义和视觉关系，不是信息密度与布局形状。

### 全应用强制深色

不采用。强制深色让 Control context 与桌面默认浅色割裂，也忽略系统外观偏好。只有 Camera context
固定使用深色。

### 运行时共享 Tailwind、WebView 或跨端 UI Framework

不采用。Tailwind 只用于原型和 token 表达；运行时继续使用 Compose、SwiftUI 与 GPUI。

### 平台各自挑选“相似”系统 Icon

不采用。不同 Icon family 的几何、描边和语义会再次造成产品分裂。平台只适配 Reicon 事实源。

## 约束

- Design Spec 是设计语义的第一事实源；HTML 原型更新到本架构后才可作为新移动端视觉验收证据。
- 颜色、字体、间距、圆角、Icon、状态和动效必须可追溯到共享或组件 token。
- 任何新增功能 Icon 都必须先确定语义、上游名称和适用状态，再进入平台代码。
- 共享设计契约不得侵入 Rust Core、媒体、协议或会话状态机。

## 相关 Use Case 与 Requirement

- [PUC-001 首次安装与配对](../use-cases/product/puc-001-first-install-and-pairing.md)
- [PUC-002 发现并连接已配对 Receiver](../use-cases/product/puc-002-discover-and-connect-paired-receiver.md)
- [PUC-005 推流中调整相机](../use-cases/product/puc-005-adjust-camera-during-streaming.md)
- [PUC-007 管理已配对设备](../use-cases/product/puc-007-manage-paired-devices.md)
- [PUC-008 通过发现或 IP 连接](../use-cases/product/puc-008-connect-with-code-or-ip.md)
- [REQ-PICOO-UI-0001 全端 UI 交互设计与验收](../requirements/req-picoo-ui-0001-native-camera-and-desktop-gpui-acceptance.md)
- `REQ-PICOO-UI-012`（见 [requirements/ui.md](../requirements/ui.md)）
