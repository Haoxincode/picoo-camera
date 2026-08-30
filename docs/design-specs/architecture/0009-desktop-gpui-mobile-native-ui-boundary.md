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
- **连接页的设备卡片**：显示可信设备、自动接受可信设备偏好与逐台移除入口；设备信任管理不混入通用页。
- **虚拟摄像头页**：显示系统设备与 Shared Frame Ring 状态，提供平台正确的检测、安装、激活或修复入口，并管理无视频流时的占位画面。
- **通用页**：只承载电脑名称与桌面生命周期偏好（关闭窗口后后台运行、登录时启动）；Windows 托盘与 macOS Dock/后台行为使用平台正确文案，不互相借用平台术语。
- **帮助页诊断区**：承载日志级别与脱敏诊断导出，避免把面向故障排查的能力混入日常通用设置。

桌面一级导航由 GPUI View 持有进程内展开/折叠状态：原生窗口标题栏独立位于工作区上方，标题栏下方由一个内嵌圆角边框共同包住 Sidebar 与主内容区；外框拥有工作区外边界，Sidebar 只拥有与主内容相邻的分割线。展开态遵循 HTML 原型的 `204px` 导航布局，折叠态收敛为 `48px` 图标栏。宽度变化复用官方 Sidebar 的 `200ms + ease_in_out_cubic` 外层裁剪过渡：导航内容按目标宽度一次排版，工作区只对裁剪宽度插值，避免逐帧重排文案。折叠控制属于主内容区顶部工具行，不占用 Sidebar 导航行，并遵循 `gpui-component::SidebarToggleButton` 的紧凑几何和方向状态语义；图标使用 Reicon Filled `sidebar-left` / `sidebar-right`，与业务导航保持统一视觉重量；应用层补充稳定 ID、中文 Tooltip 与无障碍名称。macOS 不在标题栏重复展示应用图标和应用名，折叠控制始终位于标题栏下方且与 Sidebar 分割线相邻，不进入系统窗口控制区；Windows 标题栏保留品牌图标和标题。两端继续复用 `gpui-component::TitleBar` 的窗口装饰与拖拽契约。该状态只改变视图几何与标签可见性，不进入 `ReceiverRuntime`、协议状态或跨设备偏好；导航按钮在两种状态下保持相同的稳定 ID、页面 Action、选中态和无障碍语义。

手机端主要页面：

- **设备列表页**：Available Computers、已信任状态、手动 IP 直连。
- **配对页**：显示六位配对短码，供用户与桌面端核对并确认一致。
- **传输页**：本机预览、连接质量、摄像头/分辨率/镜像控制、断开。

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

UI 同样不承担二维码生成、二维码解析或扫码相机预览；连接页不得引入扫码 SDK。

这些由 Rust Core 或平台媒体层负责，UI 只发送命令并渲染状态快照。

## 不采用的方案

### 桌面 Electron / WebView UI

不采用。与 GPUI 跨 Windows/macOS 代码共享目标冲突，且增加运行时体积。

### 手机端 GPUI 或 Flutter

不采用。摄像头与权限 API 必须原生，Compose/SwiftUI 足够承载薄 UI。

### UI 内嵌解码预览链路

不采用。预览纹理来自 FrameHub 已解码帧，不在 View 层重复解码。

## 约束

- `gpui`、`gpui_platform`、`gpui-component` 必须在 workspace 根统一锁定 Git revision。
- UI 必须能区分 PRD 定义的连接与错误状态。
- 权限必须在用户执行相应操作时请求，不在启动后一次性弹出全部权限。

## 相关 Use Case

- [PUC-001](../use-cases/product/puc-001-first-install-and-pairing.md)
- [PUC-005](../use-cases/product/puc-005-adjust-camera-during-streaming.md)
- [PUC-008](../use-cases/product/puc-008-connect-with-code-or-ip.md)

## 相关 Architecture

- [ARCH-PICOO-FRAME-001](0006-framehub-shared-frame-ring-boundary.md)
- [ARCH-PICOO-STACK-001](0001-rust-core-monorepo-boundary.md)
- [ARCH-PICOO-VCAM-001](0007-virtual-camera-platform-boundary.md)

## 相关 Requirements

- [REQ-PICOO-UI-0001（全端 UI 交互设计与细化验收规范）](../requirements/req-picoo-ui-0001-native-camera-and-desktop-gpui-acceptance.md)
- `REQ-PICOO-UI-001` … `REQ-PICOO-UI-009`（见 [requirements/ui.md](../requirements/ui.md)）
- 桌面远程摄像头控制：`REQ-PICOO-UI-009`（PUC-005）
