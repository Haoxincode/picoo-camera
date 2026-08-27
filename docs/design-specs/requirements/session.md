# REQ-PICOO-SESSION：会话与弹性

| ID | 状态 | 来源 | 描述 | 验收 |
| --- | --- | --- | --- | --- |
| REQ-PICOO-SESSION-001 | implemented | ARCH-PICOO-SESSION-001 | UI 可观察会话状态枚举完整 | `ReceiverStatus`/`SenderStatus` + `as_label`/`as_code`；VCam/权限/网络不稳定接线；FFI 7/8 |
| REQ-PICOO-SESSION-002 | implemented | ARCH-PICOO-SESSION-001 | 抖动缓冲目标 50ms，最大 120ms | jitter crate + receiver 重组后接入 |
| REQ-PICOO-SESSION-003 | implemented | ARCH-PICOO-SESSION-001 | 不完整非关键帧丢弃，关键帧丢则请求 IDR | packet `take_keyframe_loss` + EncoderCommand |
| REQ-PICOO-SESSION-004 | implemented | PUC-006 | 重连后恢复分辨率/镜像并请求 SPS/PPS/IDR | sender reconnect 集成测试 |
| REQ-PICOO-SESSION-005 | implemented | PRD §21 | 2h 长稳无崩溃、内存不增长 | `scripts/soak_loopback.sh` + ignored soak（Linux 用 OpenH264 AU；默认 60s；`SOAK_SECONDS=7200` 做 2h） |
| REQ-PICOO-SESSION-006 | implemented | PRD §21 | 丢包约 5% 时会话仍可用；恢复后 frame_age &lt;1s | `LossyVideoTransport` + `paired_loopback_remains_usable_under_five_percent_loss` / `scripts/loss_loopback.sh` |
| REQ-PICOO-SESSION-007 | implemented | PRD §21 | 健康网络 E2E P50&lt;150ms / P95&lt;250ms（传输路径） | `paired_loopback_e2e_latency_p50_under_budget`（真机 camera→VCam 仍待） |
| REQ-PICOO-SESSION-008 | implemented | PRD §8.1 / PUC-006 | 短暂断网后恢复 &lt;5s | `brief_disconnect_recovers_streaming_under_five_seconds` |
| REQ-PICOO-SESSION-009 | implemented | PUC-005 | 用户主动断开后保持 Disconnected，不自动重连 | `user_disconnect_stays_disconnected_without_reconnect` + Android Disconnect |
