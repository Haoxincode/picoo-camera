# REQ-PICOO-SESSION：会话与弹性

| ID | 状态 | 来源 | 描述 | 验收 |
| --- | --- | --- | --- | --- |
| REQ-PICOO-SESSION-001 | implemented | ARCH-PICOO-SESSION-001 | UI 可观察会话状态枚举完整 | `ReceiverStatus`/`SenderStatus` + `as_label`/`as_code`；VCam/权限/网络不稳定接线；FFI 7/8 |
| REQ-PICOO-SESSION-002 | implementing | ARCH-PICOO-SESSION-001 | Receiver 以单调时钟和媒体 PTS 建立相对时间线；正常播放 target 根据到达变化与解码耗时在 16–80ms 自适应；异常恢复 deadline 取 `2 × target + 一帧周期` 与 `RTT + 3 × 网络抖动 + 一帧周期` 较大者，并限制在 200–300ms；两者不得混为固定播放延迟；target、实际停留和 occupancy 分别统计 | jitter controller 分位数/快升慢降/deadline/离散 occupancy 测试 + media deadline 测试 + receiver stats |
| REQ-PICOO-SESSION-003 | implemented | ARCH-PICOO-SESSION-001 | 跨 AU 乱序在自适应恢复 deadline 前可完成；Reassembly 尚有更旧 frame_id 未决时不得先播放更新 AU；每片 ingest 前执行 expiry，每个有界入站批次后给播放队列调度机会；Jitter 的帧数容量必须覆盖 V1 30 FPS 下最大 300 ms 恢复 deadline、80 ms 播放 target 和一帧余量，不能先于时间边界误淘汰；过期/越过播放点的旧 AU 丢弃，关键帧丢则请求一次 IDR，迟到尾片或容量淘汰片不得重建旧 AU | packet deadline/跨 AU 乱序/oldest unresolved frame/tombstone + jitter 完整预算容量/late-playout 测试 + `DropKeyframeTailTransport` + 真机连续入站调度验证 |
| REQ-PICOO-SESSION-004 | implemented | PUC-006 | 重连后恢复分辨率/镜像并请求 SPS/PPS/IDR | sender reconnect 集成；`reconnect_churn_smoke_five_rounds` + `reconnect_churn_fifteen_rounds`（N=50 `--ignored`） |
| REQ-PICOO-SESSION-005 | implemented | PRD §21 | 2h 长稳无崩溃、内存不增长 | `soak_harness_smoke_five_seconds`（CI）；`scripts/soak_loopback.sh` + ignored 长 soak（`SOAK_SECONDS=7200`） |
| REQ-PICOO-SESSION-006 | implemented | PRD §21 | 丢包约 5% 时会话仍可用；恢复后 frame_age &lt;1s | stub + `paired_openh264_remains_usable_under_five_percent_loss`；`scripts/loss_loopback.sh` |
| REQ-PICOO-SESSION-007 | implemented | PRD §21 | 健康网络 E2E P50&lt;150ms / P95&lt;250ms（传输路径）；缺少 Sender/Receiver 时钟同步前，UI 只把 QUIC RTT 标为链路延迟，不得用 Receiver 本地解码后 frame age 相加伪造 E2E；Receiver ABR 丢包率只使用入站视频重组损失，不使用本端发出的控制包损失 | stub + `paired_openh264_e2e_latency_p50_under_budget`；`ReceiverRuntime` RTT 指标与 Receiver `app_loss`；真机 camera→VCam 与时钟同步 E2E 仍待 |
| REQ-PICOO-SESSION-008 | implemented | PRD §8.1 / PUC-006 | 短暂断网后恢复 &lt;5s | `brief_disconnect_recovers_streaming_under_five_seconds` + churn smoke |
| REQ-PICOO-SESSION-009 | implemented | PUC-005 | 用户主动断开后保持 Disconnected，不自动重连 | `user_disconnect_stays_disconnected_without_reconnect` + Android Disconnect |
| REQ-PICOO-SESSION-010 | implemented | ARCH-PICOO-SESSION-001 | Receiver 遇到不完整/迟到的非 `DISCARDABLE` AU 或 Decoder error 后进入 `AwaitingRefresh`，清空未播放 AU、reset Decoder、自动 IDR 请求合并限频并丢弃 delta AU；只有当前 epoch 的完整 IDR 被 Decoder 接受后恢复 Healthy | packet/jitter reference-loss 单元测试；Decoder fake：error→reset→delta 丢弃→IDR 恢复；IDR request 限频；Windows 真机弱网恢复仍待 |
| REQ-PICOO-SESSION-011 | planned | ARCH-PICOO-RUNTIME-001 | Receiver/Sender 状态拆为 connection、trust、stream、output、health 正交维度；UI 单标签只从组合状态派生 | 组合状态矩阵；NetworkUnstable/VCamUnavailable 不覆盖 Connected/Streaming 事实 |
| REQ-PICOO-SESSION-012 | planned | ARCH-PICOO-RUNTIME-001 | Session Core 以纯 reducer 返回 Effect；transport、decoder、store、clock、frame sink 均为 adapter；所有 teardown 收敛到一个幂等入口 | 虚拟时钟 reducer 单测；任意失败路径终态；重复 teardown 无副作用 |
| REQ-PICOO-SESSION-013 | planned | ARCH-PICOO-RUNTIME-001 | Network health 使用 episode：连续差窗口进入、连续健康窗口恢复，记录 causes/start/worst loss；ABR 快策略与 UI 慢滞回分离 | 2 个差窗口进入、5 个健康窗口恢复；标签不逐秒抖动 |
| REQ-PICOO-SESSION-014 | planned | ARCH-PICOO-RUNTIME-001 | 每个 stream generation 建立 sender/receiver 单调时钟 affine mapping，明确 capture→encode→arrival→jitter→decode→Frame/VCam 延迟；未建立映射时不得发布总延迟 | ping/pong offset/drift 仿真；generation 切换重置；真机 glass-to-glass 对照 |
