# PUC-006：网络中断后自动恢复传输

## 基本信息

- 类别：Product Resilience and Continuity
- 参与者：会议与录制用户
- 目标：在 Wi-Fi 波动、应用短暂非活动、网络接口变化或 QUIC 会话超时后，已配对设备自动重连并恢复传输，且不累积历史延迟

## 场景

已配对 Sender 与 Receiver 正在传输时，可能发生：Wi-Fi 短暂中断、手机应用短暂进入非活动状态、电脑网络接口变化、路由器切换信道或 QUIC 会话超时。

系统进入 `Reconnecting` 状态，按 500 ms、1 s、2 s、5 s、之后每 5 s 的退避尝试重连。UI 必须能区分 `Discovering`、`Pairing`、`Connecting`、`Negotiating`、`Streaming`、`Reconnecting`、`Disconnected`、`Permission Required`、`Virtual Camera Unavailable` 和 `Network Unstable`。

重连成功后：

1. 重新验证固定公钥；
2. 重新协商能力；
3. 恢复上次分辨率与镜像设置；
4. 请求新的 SPS/PPS；
5. 请求 IDR；
6. 恢复虚拟摄像头画面。

Receiver 的抖动缓冲、重组队列和 FrameHub 在网络异常时必须丢弃过期帧，不能因为重传或等待旧帧导致延迟持续累积。网络丢包 5% 时仍保持可用；恢复后延迟不应累积到 1 秒以上。

Receiver 每秒向 Sender 发送 RTT、丢包、抖动、重组丢弃、解码丢弃、帧龄、接收码率和抖动缓冲深度；Sender 根据统计动态调整码率，必要时从 1080p 降至 720p。

## 可观察结果

- 已配对设备在短暂断网后，目标恢复时间小于 5 s（健康局域网条件）。
- 重连后虚拟摄像头恢复实时画面，而不是长期冻结在最后一帧。
- UI 明确显示重连中、网络不稳定和已断开状态。
- 丢包 > 3% 或帧龄持续增加时，码率降低；条件稳定 5 秒后码率逐步提高。
- 连续传输 2 小时无崩溃；内存无持续增长。

## 边界

- 本 Use Case 覆盖局域网内可恢复的中断，不覆盖公网远程重连。
- 手机锁屏后继续传输不在当前范围；长时间后台可能被系统终止，此时用户需重新打开 Sender。
- 未配对设备的中断后不自动信任重连；公钥变化必须重新配对。
- 不能通过无限扩大缓冲区掩盖带宽不足。

## 相关 Architecture

- [ARCH-PICOO-SESSION-001](../../architecture/0005-session-reconnect-jitter-bitrate-boundary.md)
- [ARCH-PICOO-TRANSPORT-001](../../architecture/0002-quic-transport-encapsulation-boundary.md)
- [ARCH-PICOO-PROTOCOL-001](../../architecture/0003-picoo-camera-protocol-boundary.md)

## 相关 Requirements

- 待分解：`REQ-PICOO-SESSION-*`、`REQ-PICOO-TRANSPORT-*`
