# BUC-001：使用手机作为会议与录制的高质量无线摄像头

## 基本信息

- 类型：Business Use Case
- 业务场景：远程会议、在线培训、直播录制、视频面试、内容创作
- 主要角色：会议与录制用户
- 核心成果：在不理解网络协议、编码器或虚拟驱动的前提下，将手机摄像头作为电脑会议或录制软件可直接选用的高质量无线摄像头

## 业务目标

会议与录制用户希望利用手机摄像头通常优于笔记本内置 webcam 的画质、视角灵活性和前后摄切换能力，在电脑端的腾讯会议、Zoom、Microsoft Teams、OBS 或浏览器会议中直接使用，而不需要 USB 数据线、ADB、OBS 中转、浏览器页面或云端账号。

用户只需要：

- 电脑与手机连接同一 Wi-Fi；
- 安装并打开 Picoo Camera 桌面端与手机端；
- 在会议软件中选择名为 `Picoo Camera` 的摄像头。

## 业务价值

- 以低于外接 USB 摄像头或专业采集卡的门槛，获得更稳定的手机级画质。
- 前后摄切换使用户可以在人像特写与环境展示之间快速调整。
- 全程在局域网内完成，视频不经过云端，降低隐私与合规顾虑。
- 不需要理解 IP、端口、编码格式或驱动安装细节。

## 参与角色

### 会议与录制用户

希望参加远程会议、进行屏幕共享配合真人出镜、录制教程或进行视频面试的普通用户。该角色不需要网络或音视频工程背景。

## 触发场景

### 场景一：在家办公参加视频会议

用户打开腾讯会议或 Zoom，发现笔记本摄像头画质不足或角度受限，希望改用手机的广角或后置摄像头作为视频源。

### 场景二：使用 OBS 或直播软件录制

内容创作者希望将手机作为无线机位接入 OBS，同时继续使用电脑麦克风收音。

### 场景三：临时借用他人电脑开会

用户在自己的手机上安装 Sender，连接到已安装 Receiver 的会议室电脑或同事电脑，无需在该电脑上重复配置 USB 设备。

### 场景四：企业访客 Wi-Fi 或 mDNS 受限网络

用户无法通过自动发现找到电脑，但仍希望通过输入桌面端显示的局域网 `IP:端口` 和六位连接码建立可信连接。

## 完成标准

- 用户能在 Android 或 iPhone 与 Windows 或 macOS 的任意一种组合上完成从安装到会议选用的完整流程。
- 会议软件中能看到并选用 `Picoo Camera`，画面来自当前连接的手机摄像头。
- 用户能在手机端切换前后摄像头、720p/1080p 和镜像，并在数秒内恢复可用画面。
- Wi-Fi 短暂中断后，用户无需重新走完整首次配对流程即可恢复传输。
- 未配对的陌生设备不能向会议软件输出视频。

## 范围边界

### 在本 Business Use Case 范围内

- 同一 Wi-Fi 局域网内的无线摄像头传输。
- 720p30 与 1080p30 视频规格。
- 前后摄像头切换、镜像与曝光补偿。
- 虚拟摄像头注册与会议软件兼容。
- 自动发现、手动 IP 直连、连接码配对与重连。

### 不在本 Business Use Case 范围内

- 跨公网远程连接。
- 手机麦克风传输；音频继续使用电脑麦克风。
- 多手机同时输入、一机多电脑、4K/60FPS/HEVC。
- 本地录像、AI 美颜、背景替换。
- 账号、组织、订阅与云同步。
- Linux Receiver 或浏览器 Receiver。

## 支撑的 Product Use Case

- [PUC-001](../product/puc-001-first-install-and-pairing.md)：首次安装、激活虚拟摄像头并完成设备配对
- [PUC-002](../product/puc-002-discover-and-connect-paired-receiver.md)：自动发现并连接已配对电脑
- [PUC-004](../product/puc-004-use-virtual-camera-in-meeting-apps.md)：在会议软件中使用虚拟摄像头
- [PUC-005](../product/puc-005-adjust-camera-during-streaming.md)：会议中调整摄像头、分辨率与镜像
- [PUC-006](../product/puc-006-auto-reconnect-after-network-interruption.md)：网络中断后自动恢复传输
- [PUC-007](../product/puc-007-manage-paired-devices.md)：管理已配对设备与撤销信任
- [PUC-008](../product/puc-008-connect-with-code-or-ip.md)：通过连接码授权或 IP 地址直连电脑

## 相关 Architecture

- [ARCH-PICOO-STACK-001](../../architecture/0001-rust-core-monorepo-boundary.md)
- [ARCH-PICOO-VCAM-001](../../architecture/0007-virtual-camera-platform-boundary.md)
- [ARCH-PICOO-DISCOVERY-001](../../architecture/0008-discovery-and-pairing-security-boundary.md)
