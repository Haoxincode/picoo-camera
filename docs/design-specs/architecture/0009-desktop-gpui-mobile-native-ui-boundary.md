# ARCH-PICOO-UI-001: 桌面 GPUI 与手机原生 UI 边界

Status: planned
Source: product PRD V1.0 / PUC-001 / PUC-005 / PUC-008

## 背景

Picoo Camera 有四端 UI，但职责不同：手机端 UI 薄，主要负责发现、配对、预览和控制；桌面端 UI 负责预览、状态、设置和虚拟摄像头管理。UI 不应持有视频处理或协议逻辑。

## 架构决策

### 技术选型

| 应用 | UI 技术 |
| --- | --- |
| Android Sender | Jetpack Compose |
| iOS Sender | SwiftUI |
| Windows Receiver | GPUI + gpui-component |
| macOS Receiver | GPUI + gpui-component |

不引入：Flutter、React Native、Electron、Tauri、WebView、GPUI Mobile。

颜色、排版、间距、圆角、动效、组件状态和功能 Icon 的跨端语义由
[ARCH-PICOO-UI-002](0010-cross-platform-design-system-boundary.md) 约束。本 Architecture 只决定平台 UI
技术、状态所有权和运行时边界，不允许任何平台以“原生实现”为由另建一套产品视觉语义。

### gpui-component 使用方式

桌面端直接使用完整的 `gpui-component → gpui-base → GPUI`。第一版不从 gpui-base 重做 Design System，仅定制颜色、字体、圆角、间距、明暗主题（默认亮色）、状态色和品牌图标。

第一版使用的组件：Button、Card、Badge、Select、Switch、Slider、Dialog、Tooltip、Popover、Toast、Progress、Separator、Icon。

第一版唯一核心自定义组件：**VideoSurface**

职责：

- 接收平台视频纹理；
- 保持画面比例；
- 支持镜像与旋转；
- 支持占位画面；
- **不** 拥有解码器或网络会话。

VideoSurface 的平台资源必须是有界的。macOS 使用 GPUI 原生 `surface(CVPixelBuffer)` 视频合成路径，
由 latest-only 后台准备器向最多三个 CoreVideo 缓冲写入画面；不得把连续视频帧作为唯一 ID 的
`RenderImage` 送入静态 Sprite Atlas。GPUI 尚未提供原生视频 Surface 的平台，允许使用
`RenderImage` 回退，但替换帧时必须同步驱逐上一帧的 atlas entry，任何帧率和运行时长下都只能
保留常数数量的 CPU/GPU 帧资源。缓冲耗尽时丢弃预览帧，不得反压 LatestFrameStore、解码或网络会话。

### 桌面状态边界

GPUI View 不直接持有 QUIC Connection、Decoder 或 Frame Buffer。统一状态模型：

```rust
struct DesktopAppState {
    receiver_status: ReceiverStatus,
    discovered_devices: Vec<DeviceSummary>,
    active_session: Option<SessionSummary>,
    virtual_camera: VirtualCameraStatus,
    metrics: StreamMetrics,
    last_error: Option<AppError>,
}
```

后台 Rust Core 通过事件更新状态；GPUI 只观察并渲染。

### 页面结构

桌面端主要页面：

- **首次启动页**：虚拟摄像头安装状态与引导。
- **等待连接页**：等待 Sender、局域网 `IP:端口`、虚拟摄像头 Ready 状态；收到未配对连接请求后显示六位配对短码。
- **直播页**：VideoSurface 预览、设备名、分辨率/帧率/码率/延迟、网络质量、远程摄像头控制（前后摄 / 远端镜像）、断开。
- **网络页运行诊断**：桌面端集中呈现 Receiver 已经拥有的网络、重组、解码和输出状态，并维护最近 10 分钟的有界内存摘要。`ReceiverSnapshot` 必须保留 `ReceiverStats` 是否存在，View 只消费派生后的诊断状态；无样本不等于零丢失。手机端继续消费控制面反馈做码率控制，但不增加可见诊断面板。VCam 进程内的 RequestSample/fresh/cached/placeholder 指标属于独立平台故障域，只有建立明确的跨进程指标边界后才进入桌面展示。
- **连接页的设备卡片**：显示可信设备、自动接受可信设备偏好与逐台移除入口；设备信任管理不混入通用页。
- **虚拟摄像头页**：显示系统设备与 Shared Frame Ring 状态，提供平台正确的检测、安装、激活或修复入口，并管理无视频流时的占位画面。
- **通用页**：只承载电脑名称与桌面生命周期偏好（关闭窗口后后台运行、登录时启动）；Windows 托盘与 macOS Dock/后台行为使用平台正确文案，不互相借用平台术语。
- **帮助页诊断区**：承载日志级别与脱敏诊断导出，避免把面向故障排查的能力混入日常通用设置。

桌面一级导航由 GPUI View 持有进程内展开/折叠状态。窗口采用贴边的单层工作区，不再叠加品牌图标、应用标题、外侧留白或包住 Sidebar 与主内容的第二层圆角边框；Sidebar 只拥有与主内容相邻的分割线。展开态遵循 HTML 原型的 `204px` 导航布局，折叠态收敛为 `48px` 图标栏。宽度变化复用官方 Sidebar 的 `200ms + ease_in_out_cubic` 外层裁剪过渡：导航内容按目标宽度一次排版，工作区只对裁剪宽度插值，避免逐帧重排文案。“连接”必须保留在 Sidebar 导航列表并与其他导航项共享相同结构；主内容侧的折叠控制通过与首个“连接”导航行共用高度和顶部 inset，严格位于同一水平中心线，同时保持在 Sidebar 分割线右侧。Windows 不保留额外空标题行，主内容顶部工具行复用 `gpui-component::TitleBar` 的拖拽和窗口按钮契约，最小化、最大化、关闭位于同一行最右侧；macOS 单独保留最上方交通灯与拖拽安全行，导航和主内容工具行位于其下方。两端都不重复展示相机图标或 `Picoo Camera` 文案。折叠控制遵循 `gpui-component::SidebarToggleButton` 的紧凑几何和方向状态语义，图标使用 Reicon Filled `sidebar-left` / `sidebar-right`，应用层补充稳定 ID、中文 Tooltip 与无障碍名称。该状态只改变视图几何与标签可见性，不进入 `ReceiverRuntime`、协议状态或跨设备偏好；导航按钮在两种状态下保持相同的稳定 ID、页面 Action、选中态和无障碍语义。

桌面默认窗口与最小窗口统一为 `1440×900`，不记忆上次尺寸。该尺寸是完整产品布局的支持边界，不通过隐藏指标、折叠操作、缩短文案或 icon-only 模式提供更窄的响应式降级。待机连接页保留主区域与固定宽度「设备与连接」辅助栏；进入 Live 后辅助栏整体退出，工作区改由单行命令/状态顶栏和占满剩余空间的 16:9 `VideoSurface` 组成。顶栏左侧常驻设备名、帧率、链路延迟、接收码率和网络质量；分辨率选择、「连接详情」与带文字的镜像、切换、修复、断开动作同在右侧。帧率、延迟、码率和网络质量各自使用独立圆角方块，分辨率与连接详情使用与镜头控制相同的 outline 按钮，顶栏与预览之间用细分隔线分开。唯一 `Live` 标记位于预览左上角。Popover 仅承载身份、虚拟摄像头和深入链路诊断，不承担主信息的响应式收纳。两种页面结构都由同一 `DesktopPage` 状态切换，不在设备辅助栏保留第二套直播控制实现。

手机端主要页面：

- **设备列表页**：使用 Control context 展示 Available Computers、已信任状态和手动 IP 直连，跟随系统明暗。
- **配对页**：使用 Control context 显示 Receiver 身份、六位配对短码、有效期和双端确认。
- **设置页**：使用平台原生分组、Switch、Disclosure 与 Sheet 管理自动直连、画质、信任设备和权限状态。
- **传输页**：进入独立 Camera context，保持深色沉浸预览、连接质量、摄像头/分辨率/镜像控制和安全断开；不得把应用级强制深色泄漏到其他页面。

Android Compose 的可渲染状态集中在 screen-level `SenderHomeState`，`LaunchedEffect` 只负责把
Rust 原子快照和平台媒体结果写入该 holder；编码器指令与 epoch 事务由非 Compose 的
`EncoderReconfigurationCoordinator` 持有。iOS 使用 `SenderAppModel` 的 Observation 状态，
同样不让 View 持有 Rust handle 或编码器 apply 事务。

### UI 不承担的逻辑

- QUIC 事件循环与重连退避；
- H.264 编解码；
- 视频重组与抖动缓冲；
- 配对密码学；
- Shared Frame Ring 写入协议。
- 运行指标的采样口径、阈值与故障归因；这些由 Receiver/指标模型提供，View 只负责准确命名和分层呈现。

UI 同样不承担二维码生成、二维码解析或扫码相机预览；连接页不得引入扫码 SDK。

这些由 Rust Core 或平台媒体层负责，UI 只发送命令并渲染状态快照。

## 不采用的方案

### 桌面 Electron / WebView UI

不采用。与 GPUI 跨 Windows/macOS 代码共享目标冲突，且增加运行时体积。

### 手机端 GPUI 或 Flutter

不采用。摄像头与权限 API 必须原生，Compose/SwiftUI 足够承载薄 UI。

### UI 内嵌解码预览链路

不采用。预览纹理来自 LatestFrameStore 已解码帧，不在 View 层重复解码。

## 约束

- `gpui`、`gpui_platform`、`gpui-component` 必须在 workspace 根统一锁定 Git revision。
- UI 必须能区分 PRD 定义的连接与错误状态。
- 权限必须在用户执行相应操作时请求，不在启动后一次性弹出全部权限。

## 相关 Use Case

- [PUC-001](../use-cases/product/puc-001-first-install-and-pairing.md)
- [PUC-005](../use-cases/product/puc-005-adjust-camera-during-streaming.md)
- [PUC-008](../use-cases/product/puc-008-connect-with-code-or-ip.md)

## 相关 Architecture

- [ARCH-PICOO-UI-002](0010-cross-platform-design-system-boundary.md)
- [ARCH-PICOO-FRAME-001](0006-framehub-shared-frame-ring-boundary.md)
- [ARCH-PICOO-STACK-001](0001-rust-core-monorepo-boundary.md)
- [ARCH-PICOO-VCAM-001](0007-virtual-camera-platform-boundary.md)

## 相关 Requirements

- [REQ-PICOO-UI-0001（全端 UI 交互设计与细化验收规范）](../requirements/req-picoo-ui-0001-native-camera-and-desktop-gpui-acceptance.md)
- `REQ-PICOO-UI-001` … `REQ-PICOO-UI-012`（见 [requirements/ui.md](../requirements/ui.md)）
- 桌面远程摄像头控制：`REQ-PICOO-UI-009`（PUC-005）
