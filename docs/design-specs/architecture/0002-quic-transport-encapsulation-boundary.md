# ARCH-PICOO-TRANSPORT-001: QUIC 传输与 picoo-transport 封装边界

Status: planned
Source: product PRD V1.0 / PUC-002 / PUC-006

## 背景

Picoo Camera 需要在局域网内同时承载两类数据：

- **控制数据**：Hello、Capabilities、Pairing、StartStream、CameraCommand、ReceiverStats 等，要求可靠、有序。
- **视频数据**：H.264 片段，允许丢包，要求低延迟，不能因等待旧片段导致延迟累积。

QUIC 同时提供 Reliable Stream 与 Datagram（RFC 9221），适合这一组合。四端统一选用 **Quinn + Rustls（ring provider）**：QUIC 状态机、可靠 Stream、Datagram 与 TLS 均由 Cargo 依赖管理，不引入 BoringSSL 或 CMake 构建步骤。

## 架构决策

业务代码禁止直接调用 `quinn::Connection`。所有平台只依赖统一封装：

```rust
trait PicooTransport {
    fn connect(&mut self, endpoint: Endpoint) -> Result<SessionId>;
    fn send_control(&mut self, message: ControlMessage) -> Result<()>;
    fn send_video(&mut self, packet: VideoPacket) -> Result<()>;
    fn poll_event(&mut self) -> Option<TransportEvent>;
    fn close(&mut self, reason: CloseReason);
}
```

`picoo-transport` crate 独占 Quinn API，负责运行时适配、连接表、UDP I/O、定时器和发送节奏控制。业务 crate 不依赖 Quinn、Tokio 或 Rustls 的具体类型。

### QUIC 实现选型

| 候选 | 适用性判断 |
| --- | --- |
| Quinn + Rustls ring | 采用。支持可靠 Stream 与 Datagram，Android/iOS/Windows/macOS 可由 Cargo 统一构建；ring 可能编译少量 C/汇编，但不依赖 CMake。 |
| Cloudflare quiche | 不采用。协议能力满足需求，但默认 BoringSSL 构建要求 CMake，扩大本地与 CI 工具链。 |
| s2n-quic | 不采用。能力完整且可接 Rustls，但高层异步 Provider 模型与当前 `PicooTransport` 同步事件边界的适配成本不低于 Quinn，移动端验证积累也不是本项目的优先选择。 |

### 连接角色

```text
Receiver: QUIC Server + mDNS Advertiser
Sender:   QUIC Client + mDNS Browser（或手动 IP Endpoint）
```

每个会话只建立一条 QUIC Connection：

```text
QUIC Connection
  ├── Bidirectional Reliable Stream  → Protobuf 控制消息
  └── QUIC Datagram                  → H.264 VideoPacket 片段
```

QUIC ALPN：`picoocam/2`

### 重连与退避

已配对设备在 Wi-Fi 短暂中断、网络接口变化或 QUIC 会话超时后，Transport 层配合 Session 层按 500 ms、1 s、2 s、5 s、之后每 5 s 退避重连。重连成功后必须重新验证固定公钥，而不是仅凭历史 SessionId 信任。

## 不采用的方案

### TCP + 自定义 UDP 双栈

不采用。QUIC 已统一可靠与不可靠通道，并内置 TLS；额外维护 TCP 控制通道增加状态同步复杂度。

### WebRTC

不采用。第一版不需要浏览器端、NAT 穿透或 SFU；WebRTC 栈体积和信令复杂度与局域网-only 目标不匹配。

### 业务层直接使用 Quinn API

不采用。异步运行时、证书与 QUIC 连接生命周期必须留在 Transport Adapter 内，见 [ARCH-PICOO-STACK-001](0001-rust-core-monorepo-boundary.md)。

### 为密码学引入实验性的纯 Rust Provider

不采用。当前使用 Quinn 官方支持的 Rustls ring provider；不以“完全没有 C/汇编”为目标牺牲生产安全性和平台验证成熟度。

## 约束

- 四端必须能构建并链接同一 Quinn/Rustls 版本族。
- QUIC 构建不得要求 CMake、BoringSSL 或 NASM；平台仍可使用 Cargo、NDK Clang、MSVC 与系统 SDK。
- 视频 Datagram 不请求重传旧片段。
- 控制 Stream 消息丢失或乱序由 QUIC 保证；应用层仍需做协议版本与配对状态校验。

## 相关 Use Case

- [PUC-002](../use-cases/product/puc-002-discover-and-connect-paired-receiver.md)
- [PUC-006](../use-cases/product/puc-006-auto-reconnect-after-network-interruption.md)
- [PUC-008](../use-cases/product/puc-008-connect-with-code-or-ip.md)

## 相关 Architecture

- [ARCH-PICOO-STACK-001](0001-rust-core-monorepo-boundary.md)
- [ARCH-PICOO-PROTOCOL-001](0003-picoo-camera-protocol-boundary.md)

## 相关 Requirements

- 待分解：`REQ-PICOO-TRANSPORT-*`
