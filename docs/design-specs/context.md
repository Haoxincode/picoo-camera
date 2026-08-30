# Design Specs Context：管理规范、追溯规则与概念对齐

仓库名：`picoo-camera` · 产品名：**Picoo Camera**

这份文件不是 Picoo Camera 的产品总览。它是 `docs/design-specs/` 下的管理上下文，用于统一 Design Specs 的文档空间、长期评审层、需求分解流程、追溯 ID 规则和核心术语。

后续 Agent Markdown、Design Specs、代码实现和测试都应该以这里的规则作为共同上下文。

产品基线维护于 [docs/product/picoo-camera-prd-v1.0-2026-08-27.md](../product/picoo-camera-prd-v1.0-2026-08-27.md)。已确认的产品决策应同步更新产品基线；Use Case 与 Architecture 负责将其分解为长期契约，但不替代其中的功能需求编号（`FR-*`）、非功能需求、测试设计与验收标准；后续 Requirement 分解应以当前产品基线为依据。

## 文档分层

### `docs/design-specs/context.md`

维护长期规则和术语：

- Design Specs 如何管理。
- 需求如何分解成稳定 ID。
- 代码、测试、配置和生成产物如何映射到 ID。
- 核心术语的推荐含义。

这份文件应该保持稳定、短而明确。它不记录具体功能方案，也不替代某个具体 Design Spec。

### `docs/design-specs/use-cases/`

维护长期可评审的 Business Use Case 与 Product Use Case。

Use Case 分为两层：

- `business/` 维护 `BUC-*`。Business Use Case 描述用户希望完成的完整业务成果。
- `product/` 维护 `PUC-*`。Product Use Case 描述用户通过 Picoo Camera 完成目标时可感知的产品行为、交互结果和边界。

Business Use Case 通过 `supported by` 关系连接一个或多个 Product Use Case。Product Use Case 继续连接 Architecture 和 Requirement，形成从业务目标到产品实现与验证的追溯链。

### `docs/design-specs/architecture/`

维护长期可评审的架构选择、抽象边界和设计品味判断。

Architecture 和 Use Case 一样是长期评审入口。它描述模块边界、数据流、状态边界、责任划分和被排除的实现路径。

### `docs/design-specs/requirements/`

维护分解后的稳定 Requirement ID。Requirements 是代码映射层；后续代码注释、测试和配置应优先映射到这里的 `REQ-*` ID。

当前第一版以 Use Case 和 Architecture 为主；Requirement 分解见 [requirements/README.md](requirements/README.md)。Android + Windows 已进入功能实现与验证，iOS + macOS 已进入平台构建基线与原生边界实现。

## ID 管理规则

### ID 类型

- `BUC-###`：Business Use Case ID。
- `PUC-###`：Product Use Case ID。
- `SCN-<AREA>-NNN`：Showcase Scenario ID。用于具体演示，不替代 Use Case 或 Requirement。
- `ARCH-<AREA>-NNN`：Architecture ID。
- `REQ-<AREA>-NNN`：Requirement ID。
- `TC-<AREA>-NNN`：Test Case ID，可选。

### Area 命名

Picoo Camera 推荐起步命名：

- `PICOO-STACK`：Rust workspace、monorepo、FFI 与 xtask 边界。
- `PICOO-TRANSPORT`：QUIC 传输、连接表、事件循环与封装边界。
- `PICOO-PROTOCOL`：PCP/2 控制消息、视频包头与版本协商。
- `PICOO-DISCOVERY`：mDNS/DNS-SD、手动 IP 直连与设备模型。
- `PICOO-PAIRING`：首次确认、公钥固定与可信设备关系。
- `PICOO-SESSION`：会话状态、重连、抖动缓冲与码率控制。
- `PICOO-MEDIA`：采集、编码、解码与方向处理。
- `PICOO-FRAME`：FrameHub、共享帧环与多路消费。
- `PICOO-VCAM`：Windows 与 macOS 虚拟摄像头。
- `PICOO-UI`：桌面 GPUI 与手机原生 UI 边界。
- `PICOO-PRIVACY`：隐私、日志脱敏与本地-only 约束。

### ID 生命周期

每个稳定 ID 应有状态：`proposed`、`planned`、`implemented`、`verified`、`deprecated`。

ID 一旦被代码、测试或评审引用，就不要重编号。废弃时标记 `deprecated`，不要删除后复用。

### 追溯链

```text
User request / product requirement
  -> Business Use Case
  -> Product Use Case / Architecture
  -> Requirement ID
  -> code / test / config
  -> validation result
```

## Agent Markdown 书写规则

- 默认使用中文，除非源材料是英文标准、API 或外部规范。
- 先梳理需求，再分解需求，不要直接从一句想法跳到代码方案。
- 涉及实现的设计文档必须有可追溯 ID。
- 长期 Use Case 和 Architecture 应描述场景、意义、范围、边界和约束，不应写成阶段计划、实现流水账或外部项目复盘。
- 长期文档应避免使用“第一阶段”“第二阶段”“首版”“后续再说”等路线图式措辞；如果需要限定范围，应写成“本 Architecture 约束什么 / 不约束什么”。

## 核心术语

| 术语 | 含义 | 使用边界 |
| --- | --- | --- |
| `Sender` | 运行在 Android 或 iPhone 上的手机端应用，负责摄像头采集、硬件编码和向 Receiver 发送视频。 | 不称为 Client 或 Mobile App 作为架构角色名；UI 层可显示产品名。 |
| `Receiver` | 运行在 Windows 或 macOS 上的桌面应用，负责发现、配对、接收、解码、预览和驱动虚拟摄像头。 | 不称为 Server 作为用户可见产品名；协议层 Receiver 承担 QUIC Server 角色。 |
| `Rust Core` | 由多个 `picoo-*` crate 组成的共享业务核心，统一协议、传输、会话、配对、分包、抖动缓冲、码率控制、指标和 FFI。 | 不负责各平台 Camera、MediaCodec、VideoToolbox、虚拟摄像头安装 UI 和系统权限弹窗。 |
| `Picoo Camera Protocol (PCP/2)` | Sender 与 Receiver 之间的控制与视频传输协议，QUIC ALPN 为 `picoocam/2`。 | 控制消息走可靠 Stream；视频片段走 QUIC Datagram。 |
| `FrameHub` | 桌面端解码帧的统一出口，采用固定容量三槽环形缓冲，同时服务 GPUI 预览与虚拟摄像头 Producer。 | 一条视频流只解码一次；消费者变慢时丢弃旧帧。 |
| `Shared Frame Ring` | 主应用与虚拟摄像头扩展/组件之间的跨进程 NV12 帧共享区。Windows 使用 Named Shared Memory；macOS 使用 App Group mmap。 | 第一版不依赖 IOSurface 或跨进程 GPU 纹理共享。 |
| `Virtual Camera` | 向操作系统注册的标准摄像头设备，统一名称为 `Picoo Camera`。 | Windows 使用 MF Virtual Camera；macOS 使用 CMIO Camera Extension。 |
| `Pairing` | 首次加密连接建立后，Receiver 基于本次挑战生成六位配对短码并通过可靠控制 Stream 发给 Sender；两端显示相同短码，用户分别确认一致后固定双方公钥并建立可信设备关系。 | 短码是本次握手的人工核对值，不由用户输入，也不负责解析网络地址；未配对设备不得接收视频或驱动虚拟摄像头输出。 |
| `stream_epoch` | 标识一次连续视频流世代的递增计数，用于摄像头切换、分辨率变化、编码器重建和连接恢复后的帧重组隔离。 | Receiver 不得将不同 epoch 的片段组成同一帧。 |
| `VideoPacket` | 固定二进制结构的 H.264 视频片段包头，承载 `stream_epoch`、`frame_id`、分片索引和载荷。 | 不使用 Protobuf 承载每个视频片段。 |

## 产品边界摘要

Picoo Camera 第一版约束：

- 仅在同一 Wi-Fi 局域网内工作，不依赖云服务器、账号系统、USB、ADB、浏览器或公网穿透。
- 支持 Android/iOS Sender 与 Windows/macOS Receiver 的四种组合。
- 视频格式为 H.264 480p30 / 720p30 / 1080p30；音频继续使用电脑麦克风。
- 业务状态、协议、传输、配对、重连和码率控制尽可能统一使用 Rust Core。
- Windows 与 macOS 桌面 UI 共用一套 GPUI 代码；手机端分别使用 Jetpack Compose 与 SwiftUI。

明确不在第一版范围：USB/ADB、公网远程、手机麦克风、本地录像、4K/60FPS/HEVC、多手机输入、AI 美颜、Linux Receiver、浏览器 Receiver、账号与云同步。
