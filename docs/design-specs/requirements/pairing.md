# REQ-PICOO-PAIRING：配对与安全

| ID | 状态 | 来源 | 描述 | 验收 |
| --- | --- | --- | --- | --- |
| REQ-PICOO-PAIRING-001 | implemented | PUC-001 | 首次连接双方显示相同六位短码；Sender 持久化 DeviceIdentity | pairing + FFI identity 测试 |
| REQ-PICOO-PAIRING-002 | implemented | PUC-001 | 确认后保存 device_id/name/public_key/fingerprint/paired_at | `trusted_store_persists_after_pairing` |
| REQ-PICOO-PAIRING-003 | implemented | PUC-001 | 未配对设备不得 StartStream/收视频/驱动 VCam | paired loopback 集成测试 |
| REQ-PICOO-PAIRING-004 | implemented | PUC-002 | 已配对设备验证固定公钥，变化则拒绝 | `public_key_change_rejects_auto_connect` |
| REQ-PICOO-PAIRING-005 | implemented | PUC-007 | 双端可删除配对，删除后须重新确认 | 逐行 Remove + clear；列表展示平台 + last_connected；`clear_trusted_devices_requires_repair` |
