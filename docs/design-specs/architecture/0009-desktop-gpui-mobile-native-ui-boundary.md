# ARCH-PICOO-UI-001: 桌面 GPUI 与手机原生 UI 边界

Status: planned
Source: product PRD V1.0 / PUC-001 / PUC-005

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

桌面端直接使用完整的 `gpui-component → gpui-base → GPUI`。第一版不从 gpui-base 重做 Design System，仅定制颜色、字体、圆角、间距、暗色模式、状态色和品牌图标。

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
- **等待连接页**：等待 Sender、Show QR Code、虚拟摄像头 Ready 状态。
- **直播页**：VideoSurface 预览、设备名、分辨率/帧率/码率/延迟、网络质量、断开。
- **设置页**：显示名称、自动接受已配对设备、开机启动、托盘、占位画面、日志级别、已配对设备、虚拟摄像头修复、诊断导出。

手机端主要页面：

- **设备列表页**：Available Computers、Scan QR Code。
- **配对页**：六位短码确认。
- **传输页**：本机预览、连接质量、摄像头/分辨率/镜像控制、断开。

### UI 不承担的逻辑

- QUIC 事件循环与重连退避；
- H.264 编解码；
- 视频重组与抖动缓冲；
- 配对密码学；
- Shared Frame Ring 写入协议。

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

## 相关 Architecture

- [ARCH-PICOO-FRAME-001](0006-framehub-shared-frame-ring-boundary.md)
- [ARCH-PICOO-STACK-001](0001-rust-core-monorepo-boundary.md)
- [ARCH-PICOO-VCAM-001](0007-virtual-camera-platform-boundary.md)

## 相关 Requirements

- 待分解：`REQ-PICOO-UI-*`
