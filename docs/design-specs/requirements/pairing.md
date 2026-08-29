# REQ-PICOO-PAIRING：配对与安全

| ID | 状态 | 来源 | 描述 | 验收 |
| --- | --- | --- | --- | --- |
| REQ-PICOO-PAIRING-001 | implemented | PUC-001 / PUC-008 | 未配对 QUIC/TLS 连接建立后，Receiver 生成绑定本次挑战的六位配对短码并通过可靠控制 Stream 发给 Sender；两端显示同一短码且用户分别确认一致后才建立信任；成功、拒绝、中断或 60 秒到期后短码失效 | `pairing_short_code_matches_on_both_ends` + `pairing_challenge_expires_clears_short_code` + `pairing_confirm_before_desktop_confirm_is_ignored` + 双端 UI 状态映射 |
| REQ-PICOO-PAIRING-002 | implemented | PUC-001 | 确认后保存 device_id/name/public_key/fingerprint/paired_at | `trusted_store_persists_after_pairing` |
| REQ-PICOO-PAIRING-003 | implemented | PUC-001 | 未配对设备不得 StartStream/收视频/驱动 VCam；未配对 StopStream 不拆除配对挑战 | `unpaired_sender_video_is_dropped` + `unpaired_video_keeps_shared_ring_on_placeholder` + `unpaired_start_stream_is_rejected` + `unpaired_stop_stream_is_ignored_without_teardown` |
| REQ-PICOO-PAIRING-004 | implemented | PUC-002 | 已配对设备验证固定公钥，变化则拒绝（SessionError PUBLIC_KEY_CHANGED，不进入重新配对） | `public_key_change_rejects_auto_connect` |
| REQ-PICOO-PAIRING-005 | implemented | PUC-007 | 双端可删除配对，删除后须重新确认 | 逐行 Remove + clear；列表展示平台 + last_connected；`clear_trusted_devices_requires_repair` |
