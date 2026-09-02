# Requirements

本目录维护 Picoo Camera 从 Business Use Case、Product Use Case 和 Architecture 分解得到的稳定 **Requirement ID (`REQ-*`)**。

Requirements 是代码实现、单元测试和验收测试的直接映射层。

当前实现审查包含两类证据：**Android Sender + Windows Receiver** 的功能实现与产物验证，以及 **iOS Sender + macOS Receiver** 的 Apple 原生边界实现。iOS SwiftUI 设备流程、发现/手动直连、AVFoundation 420v 采集、VideoToolbox 硬件 H.264 与 Rust FFI 媒体入口，以及 macOS VideoToolbox→NV12 解码已进入 `implemented`；macOS Camera Extension 的 Rust Host 激活/停用边界与签名、公证流水线已进入仓库。iPhone→macOS 真机媒体验收、使用真实 Developer ID 凭据的首次 release 绿测、Camera Extension 用户批准/重启枚举和会议软件验收仍按对应 Requirement 独立推进。

## ID 状态

所有 REQ 初始状态为 `proposed`。实现并验证后更新为 `implemented` / `verified`。

## 验证清单（Android→Windows）

| 文档 | 关闭的 REQ / PUC |
| --- | --- |
| [ci-artifacts.md](../verification/ci-artifacts.md) | STACK-005 — CI 产物下载 |
| [device-e2e-android-win11.md](../verification/device-e2e-android-win11.md) | BUC-001，PUC-001、002、004..008，PRD §21 功能/性能抽样 |
| [vcam-meeting-apps.md](../verification/vcam-meeting-apps.md) | **VCAM-005**, PUC-004 |
| [session-005-soak-loopback.md](../verification/session-005-soak-loopback.md) | SESSION-005 loopback 中间证据（非真机关闭） |

## 文档索引

| 区域 | 文件 | 说明 |
| --- | --- | --- |
| `PICOO-STACK` | [stack.md](stack.md) | Monorepo、xtask、FFI、CI |
| `PICOO-PROTOCOL` | [protocol.md](protocol.md) | PCP/4、FEC VideoPacket、控制消息 |
| `PICOO-TRANSPORT` | [transport.md](transport.md) | QUIC 封装、连接角色 |
| `PICOO-SESSION` | [session.md](session.md) | 会话状态、重连、抖动缓冲、码率 |
| `PICOO-DISCOVERY` | [discovery.md](discovery.md) | mDNS、手动 IP 直连、配对短码核对 |
| `PICOO-PAIRING` | [pairing.md](pairing.md) | 配对、公钥固定、撤销 |
| `PICOO-MEDIA` | [media.md](media.md) | Android/iOS 采集编码、Windows/macOS 解码 |
| `PICOO-FRAME` | [frame.md](frame.md) | FrameHub、Shared Frame Ring |
| `PICOO-VCAM` | [vcam.md](vcam.md) | Windows MF 与 macOS Camera Extension |
| `PICOO-UI` | [ui.md](ui.md) | Compose / SwiftUI / GPUI 页面与状态 |
| `PICOO-UI` | [req-picoo-ui-0001-native-camera-and-desktop-gpui-acceptance.md](req-picoo-ui-0001-native-camera-and-desktop-gpui-acceptance.md) | **全端 UI 语义一致与原生适配验收规范**（REQ-PICOO-UI-0001，`planned`） |
| `PICOO-PRIVACY` | [privacy.md](privacy.md) | 日志脱敏、本地-only |

## 追溯链

```text
BUC-001 / PUC-* → REQ-* → crates/apps/tests → validation
```
