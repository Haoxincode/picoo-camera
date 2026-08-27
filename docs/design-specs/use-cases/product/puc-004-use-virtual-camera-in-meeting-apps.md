# PUC-004：在会议软件中使用 Picoo Camera 虚拟摄像头

## 基本信息

- 类别：Product Output and Meeting Integration
- 参与者：会议与录制用户
- 目标：让腾讯会议、Zoom、Microsoft Teams、OBS 和浏览器会议等应用能够发现并选用名为 `Picoo Camera` 的系统虚拟摄像头

## 场景

Sender 与 Receiver 建立传输后，Receiver 将解码帧写入 FrameHub，并同步更新 Shared Frame Ring。虚拟摄像头组件从 Shared Frame Ring 读取最新 NV12 帧，向操作系统注册的标准摄像头设备输出画面。

用户在腾讯会议、Zoom、Microsoft Teams、OBS 或 Chrome/Edge/Safari 浏览器会议的视频设置中选择 `Picoo Camera`。会议软件看到的是来自手机摄像头的实时画面，音频仍使用电脑本地麦克风。

当没有手机连接时，虚拟摄像头输出纯黑背景、`Picoo Camera` 标志和 `Waiting for phone...` 占位画面，而不是随机噪声、冻结的旧会议画面或未定义行为。连接暂时中断时，最多短暂重复最后一帧，随后切换到重连占位画面。

## 可观察结果

- Windows 和 macOS 均在系统摄像头列表中注册统一名称 `Picoo Camera`。
- 目标会议软件能选择并使用 `Picoo Camera`；画面方向与比例由 Receiver 处理，不依赖会议软件自行旋转。
- 无连接时显示定义的占位画面；不是黑屏死机或不可枚举设备。
- 会议软件关闭并重新打开后，仍可选择 `Picoo Camera`。
- 虚拟摄像头扩展/组件进程不直接持有 QUIC 连接、解码器或网络会话；只消费 Shared Frame Ring。

## 边界

- 本 Use Case 不保证所有第三方软件的无缺陷兼容，但第一版验收必须覆盖 PRD 列出的目标会议软件。
- FaceTime 可发现性检查属于 macOS 兼容验证，不要求 FaceTime 完整通话链路。
- 本 Use Case 不包含手机麦克风或系统音频环回。
- Linux 虚拟摄像头不在当前范围。

## 相关 Architecture

- [ARCH-PICOO-VCAM-001](../../architecture/0007-virtual-camera-platform-boundary.md)
- [ARCH-PICOO-FRAME-001](../../architecture/0006-framehub-shared-frame-ring-boundary.md)
- [ARCH-PICOO-MEDIA-001](../../architecture/0004-cross-platform-media-pipeline-boundary.md)

## 相关 Requirements

- 待分解：`REQ-PICOO-VCAM-*`、`REQ-PICOO-FRAME-*`
