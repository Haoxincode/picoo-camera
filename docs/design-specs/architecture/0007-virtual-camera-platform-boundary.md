# ARCH-PICO-VCAM-001: 虚拟摄像头平台边界

Status: planned
Source: product PRD V1.0 / PUC-001 / PUC-004

## 背景

会议软件通过操作系统标准摄像头 API 枚举设备。Pico Camera 必须在 Windows 与 macOS 上注册统一名称 **`Pico Camera`**，并向 Zoom、Teams、腾讯会议、OBS 和浏览器会议提供稳定 NV12 帧流。

Windows 与 macOS 的虚拟摄像头机制不同，但产品语义一致：无连接占位、有连接实时帧、扩展进程不持有网络逻辑。

## 架构决策

### 统一产品名称

Windows 与 macOS 都向系统注册：**Pico Camera**

### Windows

使用 Media Foundation Virtual Camera API（最低 Windows 11 Build 22000）：

```text
MFCreateVirtualCamera
  → IMFVirtualCamera
  → Custom IMFMediaSource (PicoVirtualCameraSource.dll)
  → Shared Frame Ring
```

组件包括：

- `Pico Camera Desktop.exe`
- `PicoVirtualCameraSource.dll`
- Installer
- Shared Frame Ring

Media Source 作为独立组件安装并注册，由 Windows Frame Server 加载。

### macOS

使用 Core Media I/O Camera Extension（最低 macOS 12.3）：

```text
Pico Camera Desktop.app
  → App Group Container
  → mmap Shared Frame Ring
  → Pico Camera Extension.systemextension
```

Camera Extension 作为桌面应用随附的系统扩展，首次使用时由用户批准。扩展是独立进程边界；主应用不得把网络会话逻辑放入扩展。

### 数据流

```text
Rust Receiver Core
  → Decode once
  → FrameHub
  → Shared Frame Ring
  → Virtual Camera Extension / MF Media Source
  → Zoom / Teams / 腾讯会议 / OBS / Browser
```

### 安装与修复

- Windows 安装器：注册 COM/Media Foundation 组件、配置防火墙规则、卸载清理。
- macOS 发布：签名、Hardened Runtime、Developer ID、Notarization、扩展激活引导。
- 桌面设置页提供虚拟摄像头状态检查与修复入口。

## 不采用的方案

### macOS DAL 插件

不采用。Camera Extension 是 Apple 推荐的现代系统扩展机制。

### 在虚拟摄像头进程内运行 QUIC 或解码

不采用。扩展/Media Source 只读 Shared Frame Ring。

### Linux v4l2loopback 第一版

不采用。第一版只覆盖 Windows 与 macOS Receiver。

## 约束

- 未配对或未连接时输出定义占位画面，不是不可枚举设备或随机噪声。
- 会议软件关闭并重新打开后仍可选择 Pico Camera。
- 虚拟摄像头组件升级必须与 Desktop 主应用版本兼容，并通过安装器或应用内修复流程处理。

## 相关 Use Case

- [PUC-001](../use-cases/product/puc-001-first-install-and-pairing.md)
- [PUC-004](../use-cases/product/puc-004-use-virtual-camera-in-meeting-apps.md)

## 相关 Architecture

- [ARCH-PICO-FRAME-001](0006-framehub-shared-frame-ring-boundary.md)
- [ARCH-PICO-STACK-001](0001-rust-core-monorepo-boundary.md)

## 相关 Requirements

- 待分解：`REQ-PICO-VCAM-*`
