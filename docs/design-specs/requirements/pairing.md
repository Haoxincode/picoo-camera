# REQ-PICOO-PAIRING：配对与安全

| ID | 状态 | 来源 | 描述 | 验收 |
| --- | --- | --- | --- | --- |
| REQ-PICOO-PAIRING-001 | deprecated | PUC-001 / PUC-008 | 旧流程由 Receiver 发送其计算的短码，且 transcript 没有真实私钥签名/channel binding；由 PAIRING-007..009 取代 | 只保留为旧行为追溯，不构成安全验收 |
| REQ-PICOO-PAIRING-002 | deprecated | PUC-001 | 旧 trust record 保存 pseudo-public-key；由 PAIRING-007/010 取代且不得迁移为可信身份 | 旧 store 拒绝并重新配对 |
| REQ-PICOO-PAIRING-003 | implementing | PUC-001 | 未认证设备不得 StartStream/收视频/驱动 VCam 或执行特权控制；未认证 StopStream 不破坏活动认证事务 | 现有媒体门禁测试 + PAIRING-009 challenge-response 完成后升级 implemented |
| REQ-PICOO-PAIRING-004 | deprecated | PUC-002 | 旧实现只比较对端声明的 public-key bytes，不能证明私钥持有；由 PAIRING-009 取代 | 仅保留 public-key change 回归，不作为认证证据 |
| REQ-PICOO-PAIRING-005 | implemented | PUC-007 | 双端可删除配对，删除后须重新确认；删除与持久化由活动 Rust Session 事务化执行，持久化失败回滚内存信任状态 | 逐行 Remove + clear；列表展示平台 + last_connected；`clear_trusted_devices_requires_repair`；`failed_trusted_device_persist_rolls_back_memory_state` |
| REQ-PICOO-PAIRING-006 | implemented | PUC-007 / ARCH-PICOO-DISCOVERY-001 | 仅当未知 Sender 身份完成双端短码配对提交后，Receiver 才生成一次性同名历史清理决策；决策冻结 revision、候选 ID 与指纹，并与新可信身份原子持久化，重启后恢复原快照，可信重连不得重新生成；桌面明确列出候选指纹，用户选择清理时只事务化撤销该快照并保留当前身份，选择保留时持久化消费决策，候选失效或持久化失败不得扩大删除集合；设备名只用于提出候选，不得用于授予、继承或验证信任 | `same_name_identities_are_candidates_but_remain_distinct_trust_keys` + `newly_paired_identity_can_replace_same_name_history`（含重启恢复）+ `trusted_reconnect_never_emits_identity_replacement_decision` + `keeping_same_name_identities_consumes_only_the_current_decision`（含重启不再出现）+ `failed_identity_history_replace_rolls_back_memory` + 指纹前缀碰撞扩展测试 + GPUI AlertDialog 排队与候选明示；完整 GPUI 交互仍按 `REQ-PICOO-UI-0001` 验收 |
| REQ-PICOO-PAIRING-007 | planned | ARCH-PICOO-DISCOVERY-001 | 每台设备由 OS CSPRNG 生成 Ed25519 静态身份密钥；公钥派生 device ID，私钥进入 Android Keystore、Apple Keychain、Windows CNG/DPAPI，Linux/Test 使用权限受限原子文件 adapter | 真签名/验签单测 + 四平台持久化/重启测试 + 私钥不出安全存储/日志审计 |
| REQ-PICOO-PAIRING-008 | planned | ARCH-PICOO-DISCOVERY-001 | 首次配对 SAS 由双方独立从双方公钥、双方随机 nonce、设备 ID 与 TLS exporter 派生；双方分别确认后以 Ed25519 签完整 transcript | 双端 SAS 一致、MITM 双 channel SAS 不同、transcript/domain/nonce/generation 重放拒绝 |
| REQ-PICOO-PAIRING-009 | planned | ARCH-PICOO-DISCOVERY-001 | 重连必须 challenge-response 证明持有已固定公钥对应私钥，认证完成前媒体和特权控制保持关闭 | 已配对成功；仅复用 public-key bytes、签名错误、旧 challenge、旧 generation 全部拒绝 |
| REQ-PICOO-PAIRING-010 | planned | ARCH-PICOO-DISCOVERY-001 | 旧伪公钥 identity 与 trust store 全部失效且不做静默迁移；身份读取/解密/一致性失败 fail closed，UI 提供修复或显式重置配对 | v1 store 被拒绝；身份损坏不生成 ephemeral identity；双端全量重新配对验收 |
