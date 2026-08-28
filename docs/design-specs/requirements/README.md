# Requirements

本目录维护 Picoo Camera 的稳定 Requirement ID，作为代码、测试与验收的映射层。

当前范围：**Android Sender + Windows Receiver（Android→Windows）**，覆盖全部 7 个 PUC 与 BUC-001 在该组合下的可观察结果。

## ID 状态

所有 REQ 初始状态为 `proposed`。实现并验证后更新为 `implemented` / `verified`。

**V1 差距审计**（实现 vs 真机）：[verification/android-win-v1-gap-audit.md](../verification/android-win-v1-gap-audit.md)

**UI 1:1 验收审计**（REQ-PICOO-UI-0001）：[verification/req-picoo-ui-0001-gap-audit.md](../verification/req-picoo-ui-0001-gap-audit.md)

## 验证清单（Android→Windows）

| 文档 | 关闭的 REQ / PUC |
| --- | --- |
| [ci-artifacts.md](../verification/ci-artifacts.md) | STACK-005 — CI 产物下载 |
| [device-e2e-android-win11.md](../verification/device-e2e-android-win11.md) | BUC-001, PUC-001..003, 005..007, PRD §21 功能/性能抽样 |
| [vcam-meeting-apps.md](../verification/vcam-meeting-apps.md) | **VCAM-005**, PUC-004 |
| [session-005-soak-loopback.md](../verification/session-005-soak-loopback.md) | SESSION-005 loopback 中间证据（非真机关闭） |

## 文档索引

| 区域 | 文件 | 说明 |
| --- | --- | --- |
| `PICOO-STACK` | [stack.md](stack.md) | Monorepo、xtask、FFI、CI |
| `PICOO-PROTOCOL` | [protocol.md](protocol.md) | PCP/1、VideoPacket、控制消息 |
| `PICOO-TRANSPORT` | [transport.md](transport.md) | QUIC 封装、连接角色 |
| `PICOO-SESSION` | [session.md](session.md) | 会话状态、重连、抖动缓冲、码率 |
| `PICOO-DISCOVERY` | [discovery.md](discovery.md) | mDNS、二维码兜底 |
| `PICOO-PAIRING` | [pairing.md](pairing.md) | 配对、公钥固定、撤销 |
| `PICOO-MEDIA` | [media.md](media.md) | Android 采集编码、Windows 解码 |
| `PICOO-FRAME` | [frame.md](frame.md) | FrameHub、Shared Frame Ring |
| `PICOO-VCAM` | [vcam.md](vcam.md) | Windows 虚拟摄像头 |
| `PICOO-UI` | [ui.md](ui.md) | Compose / GPUI 页面与状态 |
| `PICOO-UI` | [req-picoo-ui-0001-native-camera-and-desktop-gpui-acceptance.md](req-picoo-ui-0001-native-camera-and-desktop-gpui-acceptance.md) | **全端 UI 1:1 验收规范**（REQ-PICOO-UI-0001，`proposed`） |
| `PICOO-PRIVACY` | [privacy.md](privacy.md) | 日志脱敏、本地-only |

## 追溯链

```text
BUC-001 / PUC-* → REQ-* → crates/apps/tests → validation
```
