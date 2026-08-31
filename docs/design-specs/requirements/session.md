# REQ-PICOO-SESSION：会话与弹性

| ID | 状态 | 来源 | 描述 | 验收 |
| --- | --- | --- | --- | --- |
| REQ-PICOO-SESSION-001 | implemented | ARCH-PICOO-SESSION-001 | UI 可观察会话状态枚举完整 | `ReceiverStatus`/`SenderStatus` + `as_label`/`as_code`；VCam/权限/网络不稳定接线；FFI 7/8 |
| REQ-PICOO-SESSION-002 | implemented | ARCH-PICOO-SESSION-001 | 抖动缓冲目标 50ms，最大 120ms | jitter crate + receiver 重组后接入 |
| REQ-PICOO-SESSION-003 | implemented | ARCH-PICOO-SESSION-001 | 跨 AU 乱序在 120ms 截止前可完成；每片 ingest 前执行 expiry，过期/越过播放点的旧 AU 丢弃，关键帧丢则请求一次 IDR，迟到尾片或容量淘汰片不得重建旧 AU | packet deadline/跨 AU 乱序/terminal tombstone + jitter late-playout 测试 + `DropKeyframeTailTransport` + `incomplete_keyframe_requests_idr_and_recovers_framehub` |
| REQ-PICOO-SESSION-004 | implemented | PUC-006 | 重连后恢复分辨率/镜像并请求 SPS/PPS/IDR | sender reconnect 集成；`reconnect_churn_smoke_five_rounds` + `reconnect_churn_fifteen_rounds`（N=50 `--ignored`） |
| REQ-PICOO-SESSION-005 | implemented | PRD §21 | 2h 长稳无崩溃、内存不增长 | `soak_harness_smoke_five_seconds`（CI）；`scripts/soak_loopback.sh` + ignored 长 soak（`SOAK_SECONDS=7200`） |
| REQ-PICOO-SESSION-006 | implemented | PRD §21 | 丢包约 5% 时会话仍可用；恢复后 frame_age &lt;1s | stub + `paired_openh264_remains_usable_under_five_percent_loss`；`scripts/loss_loopback.sh` |
| REQ-PICOO-SESSION-007 | implemented | PRD §21 | 健康网络 E2E P50&lt;150ms / P95&lt;250ms（传输路径）；缺少 Sender/Receiver 时钟同步前，UI 只把 QUIC RTT 标为链路延迟，不得用 Receiver 本地解码后 frame age 相加伪造 E2E；Receiver ABR 丢包率只使用入站视频重组损失，不使用本端发出的控制包损失 | stub + `paired_openh264_e2e_latency_p50_under_budget`；`ReceiverRuntime` RTT 指标与 Receiver `app_loss`；真机 camera→VCam 与时钟同步 E2E 仍待 |
| REQ-PICOO-SESSION-008 | implemented | PRD §8.1 / PUC-006 | 短暂断网后恢复 &lt;5s | `brief_disconnect_recovers_streaming_under_five_seconds` + churn smoke |
| REQ-PICOO-SESSION-009 | implemented | PUC-005 | 用户主动断开后保持 Disconnected，不自动重连 | `user_disconnect_stays_disconnected_without_reconnect` + Android Disconnect |
| REQ-PICOO-SESSION-010 | implemented | ARCH-PICOO-SESSION-001 | Receiver 遇到不完整/迟到的非 `DISCARDABLE` AU 或 Decoder error 后进入 `AwaitingRefresh`，清空未播放 AU、reset Decoder、自动 IDR 请求合并限频并丢弃 delta AU；只有当前 epoch 的完整 IDR 被 Decoder 接受后恢复 Healthy | packet/jitter reference-loss 单元测试；Decoder fake：error→reset→delta 丢弃→IDR 恢复；IDR request 限频；Windows 真机弱网恢复仍待 |
