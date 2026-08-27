# PUC-001：首次安装、激活虚拟摄像头并完成设备配对

## 基本信息

- 类别：Product Onboarding and Trust Establishment
- 参与者：会议与录制用户
- 目标：在桌面端与手机端完成首次安装、虚拟摄像头激活、权限授予和双向确认的首次配对

## 场景

用户先在 Windows 或 macOS 上安装 Picoo Camera Desktop。首次启动时，应用检查虚拟摄像头是否已安装或激活：Windows 需完成 Media Source 组件安装与注册；macOS 需引导用户批准随附的 Camera Extension 系统扩展。

用户在 Android 或 iPhone 上安装 Sender 应用。当用户尝试发现或连接电脑时，系统按平台要求请求摄像头、局域网和 Bonjour/NSD 相关权限；权限必须在相应操作触发时请求，而不是应用启动后一次性弹出全部权限。

手机端通过 mDNS 发现附近 Receiver，或用户通过桌面端二维码发起连接。首次连接时，双方进入配对流程：手机和电脑同时显示由握手上下文派生的相同六位短认证码。用户确认两端数字一致后，系统保存对方公钥、设备名称和时间戳，建立可信设备关系。

配对完成前，Receiver 不得向虚拟摄像头输出真实视频，也不得接受摄像头切换、分辨率修改或码率控制。

## 可观察结果

- 桌面端首次启动能明确显示虚拟摄像头状态：`Installed` / `Not Installed`，并提供安装或修复入口。
- macOS 首次使用时能引导用户完成 Camera Extension 批准；Windows 安装器能注册 `Picoo Camera` 虚拟摄像头。
- 手机端授予必要权限后，能浏览 `_picoocam._udp.local` 服务或扫描桌面端二维码。
- 首次配对时，手机与电脑显示相同六位配对码；用户确认后，双方保存 `device_id`、`device_name`、`public_key`、`certificate_fingerprint`、`paired_at`。
- 未完成配对的连接尝试不能接收视频、不能驱动虚拟摄像头、不能读取对方设备详细信息。
- 配对成功后，Receiver 进入等待连接或直播就绪状态；Sender 可将该 Receiver 记为已配对设备。

## 边界

- 本 Use Case 不包含后续自动重连逻辑的细节，重连行为由 [PUC-006](puc-006-auto-reconnect-after-network-interruption.md) 描述。
- 本 Use Case 不要求账号登录或云端同步配对关系；可信设备列表仅保存在本地。
- mDNS 被屏蔽时的发现失败由 [PUC-003](puc-003-qr-code-fallback-connection.md) 覆盖，但首次配对确认流程与本 Use Case 相同。
- 手机端传输期间必须保持应用前台；锁屏后继续传输不在当前范围。

## 相关 Architecture

- [ARCH-PICOO-DISCOVERY-001](../../architecture/0008-discovery-and-pairing-security-boundary.md)
- [ARCH-PICOO-VCAM-001](../../architecture/0007-virtual-camera-platform-boundary.md)
- [ARCH-PICOO-UI-001](../../architecture/0009-desktop-gpui-mobile-native-ui-boundary.md)

## 相关 Requirements

- 待分解：`REQ-PICOO-PAIRING-*`、`REQ-PICOO-VCAM-*`、`REQ-PICOO-DISCOVERY-*`
