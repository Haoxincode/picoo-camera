# REQ-PICOO-PAIRING：配对与安全

| ID | 状态 | 来源 | 描述 | 验收 |
| --- | --- | --- | --- | --- |
| REQ-PICOO-PAIRING-001 | proposed | PUC-001 | 首次连接双方显示相同六位短码 | pairing crate 确定性测试 |
| REQ-PICOO-PAIRING-002 | proposed | PUC-001 | 确认后保存 device_id/name/public_key/fingerprint/paired_at | 持久化测试 |
| REQ-PICOO-PAIRING-003 | proposed | PUC-001 | 未配对设备不得 StartStream/收视频/驱动 VCam | 隔离集成测试 |
| REQ-PICOO-PAIRING-004 | proposed | PUC-002 | 已配对设备验证固定公钥，变化则拒绝 | 公钥变更测试 |
| REQ-PICOO-PAIRING-005 | proposed | PUC-007 | 双端可删除配对，删除后须重新确认 | 撤销测试 |
