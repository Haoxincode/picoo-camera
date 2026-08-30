# REQ-PICOO-FRAME：FrameHub 与 Shared Frame Ring

| ID | 状态 | 来源 | 描述 | 验收 |
| --- | --- | --- | --- | --- |
| REQ-PICOO-FRAME-001 | implemented | ARCH-PICOO-FRAME-001 | FrameHub 三槽环形缓冲，最新完整帧优先 | frame-hub 单元测试 |
| REQ-PICOO-FRAME-002 | implemented | ARCH-PICOO-FRAME-001 | 消费者慢时丢弃旧帧 | ring rapid overwrite 测试 |
| REQ-PICOO-FRAME-003 | implemented | PUC-004 | Shared Frame Ring（Win Named SHM / Linux flink） | producer/consumer + Windows 每槽 `LockFileEx` 异常租约恢复 + `paired_openh264_publishes_to_shared_frame_ring` |
| REQ-PICOO-FRAME-004 | implemented | PUC-004 | 无连接时输出占位画面（黑底+标志+Waiting for phone...） | placeholder 测试 |
| REQ-PICOO-FRAME-005 | implemented | PUC-006 | 中断时最多短暂重复最后一帧，随后重连占位 | `reconnecting_placeholder` + `disconnect_holds_last_frame_then_shows_placeholder` |
| REQ-PICOO-FRAME-006 | implemented | ARCH-PICOO-FRAME-001 / ARCH-PICOO-VCAM-001 | macOS 主应用与 Camera Extension 通过 App Group mmap 共享 NV12 帧环 | ABI v2；单 Producer 生命周期锁；Rust Writer→生产 Swift/C Reader 跨进程并发与双向异常退出恢复；`cargo xtask test macos` 验证；实际签名 App Group 读写仍待真机验收 |
| REQ-PICOO-FRAME-007 | verified | ARCH-PICOO-FRAME-001 / PUC-004 | Windows Shared Frame Ring 在 Receiver 正常重启、异常退出或定位器损坏后恢复，VCam Consumer 自动切换到新 Producer 代际 | 单 Producer 生命周期锁；损坏 flink 恢复；Consumer 代际检测；VCam 同序列新代际重连测试；Windows 子进程异常退出测试；`windows-latest` 上 `cargo xtask test windows` 验证 |
