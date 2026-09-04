# REQ-PICOO-FRAME：FrameHub 与 Shared Frame Ring

| ID | 状态 | 来源 | 描述 | 验收 |
| --- | --- | --- | --- | --- |
| REQ-PICOO-FRAME-001 | deprecated | ARCH-PICOO-FRAME-001 | 历史同进程 FrameHub 三槽模型；已由 REQ-PICOO-FRAME-009 的容量一 LatestFrameStore 取代，三槽协议只保留在 Shared Frame Ring | 不再导出 `FrameHub` / `FrameSlot` / ready-state API |
| REQ-PICOO-FRAME-002 | implemented | ARCH-PICOO-FRAME-001 | 同进程消费者慢时由 LatestFrameStore 覆盖旧帧；跨进程消费者仍由 Shared Frame Ring 丢弃旧帧 | held consumer Arc 不阻塞 publish + ring rapid overwrite 测试 |
| REQ-PICOO-FRAME-003 | implemented | PUC-004 | Shared Frame Ring；Windows 生产环使用 `%ProgramData%\Picoo Camera` mmap 文件，使交互用户 Receiver 与 Local Service / Session 0 Frame Server 共享同一帧 identity；同会话 named SHM 仅用于测试 | 文件环 producer/consumer roundtrip + Windows 每槽 `LockFileEx` 异常租约恢复 + MSI `MsiLockPermissionsEx` 目录/LS/BU ACE 校验 + `paired_openh264_publishes_to_shared_frame_ring` |
| REQ-PICOO-FRAME-004 | implemented | PUC-004 | 无连接时输出占位画面（黑底+标志+Waiting for phone...） | placeholder 测试 |
| REQ-PICOO-FRAME-005 | implemented | PUC-006 | Receiver 统一拥有连接状态并在中断后向 Shared Frame Ring 发布重连占位；VCam 不得把 Producer 存活期间短暂无新序号误判为断线，Producer 退出后最多短暂重复最后一帧再切占位 | `reconnecting_placeholder` + `disconnect_holds_last_frame_then_shows_placeholder` + Windows Producer 生命周期锁探测 |
| REQ-PICOO-FRAME-006 | implemented | ARCH-PICOO-FRAME-001 / ARCH-PICOO-VCAM-001 | macOS 主应用与 Camera Extension 通过 App Group mmap 共享 NV12 帧环 | ABI v2；单 Producer 生命周期锁；Rust Writer→生产 Swift/C Reader 跨进程并发与双向异常退出恢复；`cargo xtask test macos` 验证；实际签名 App Group 读写仍待真机验收 |
| REQ-PICOO-FRAME-007 | verified | ARCH-PICOO-FRAME-001 / PUC-004 | Windows Shared Frame Ring 在 Receiver 正常重启或异常退出后恢复；生产文件环保持稳定 identity，同会话测试环仍支持定位器代际切换 | 文件环 reopen + 单 Producer 生命周期锁 + 文件环 Producer 子进程异常退出恢复；named 测试环损坏 flink 恢复与 Consumer 代际检测；`windows-latest` 上 `cargo xtask test windows` 验证 |
| REQ-PICOO-FRAME-008 | implemented | ARCH-PICOO-RUNTIME-001 | Shared Ring 每帧只覆盖 `[..data_length]`，不得清零整个最大 slot；创建/generation 初始化负责清零，隐私需要时仅在大→小切换清理一次尾部 | 生产代码无逐帧 `pixels.fill(0)`；短帧 roundtrip/尾部不可读测试；写流量 benchmark |
| REQ-PICOO-FRAME-009 | implemented | ARCH-PICOO-RUNTIME-001 | 同进程使用 LatestFrameStore 与 `Arc<VideoFrame>` 的容量一/latest-only 语义；Shared Frame Ring 名称和三槽并发协议保持不变 | held consumer Arc 不阻塞发布；Preview 只克隆完整帧 Arc；旧 FrameHub API 搜索为零 |
| REQ-PICOO-FRAME-010 | implemented | ARCH-PICOO-RUNTIME-001 | Decoder、Preview、Shared Ring Writer 与可选 Recorder 共享不可变帧；融合方向变换由有界 FrameBufferPool 复用 backing storage，池耗尽时临时分配而不反压 Decoder，会话 teardown 使旧 lease 失效 | 最后一个 Bytes/Arc 消费者释放后回池；buffer 数与总 capacity 上界、慢消费者、clear generation、尺寸变化测试；Receiver 发布不再 clone pixel Bytes；长期 soak 仍待持续验证后升级为 verified |
