# REQ-PICOO-SESSION-005：2h 长稳证据（loopback 中间结果）

> 状态说明：本文件记录 **Linux loopback** 长稳中间证据。  
> **不能**单独把 `REQ-PICOO-SESSION-005` 升为 `verified`；PRD §21 要求 Win11 + Android 真机 1080p30 连续 2h。

## 运行方式

```bash
SOAK_SECONDS=7200 bash scripts/soak_loopback.sh
# 日志：/opt/cursor/artifacts/soak_7200s.log
```

## 通过门槛（loopback）

- 进程无崩溃退出
- `last_elapsed_s >= 7200`
- `delta_rss_kb = last_rss - first_rss <= 65536`（64 MiB 软上限，与测试一致）

## 汇总命令

```bash
awk -F'[= ]+' '/soak sample/{e=$4+0;r=$8+0; if(!n++){fe=e;fr=r} le=e;lr=r} END{printf "first_elapsed_s=%d first_rss_kb=%d last_elapsed_s=%d last_rss_kb=%d delta_rss_kb=%d\n",fe,fr,le,lr,lr-fr}' /opt/cursor/artifacts/soak_7200s.log
```

## 当前记录（Cloud Agent）

| 字段 | 值 |
| --- | --- |
| 开始时间（UTC） | 2026-08-27 ~21:32 |
| 日志路径 | `/opt/cursor/artifacts/soak_7200s.log` |
| 中间样例（~104 min） | `elapsed=6240s rss_kb=18128`（RSS 平坦） |
| 最终汇总 | _待 7200s 结束后填写_ |
| 结论 | _待填：loopback PASS/FAIL；真机仍待_ |

## 真机（关闭本 REQ 必需）

- [ ] Win11 + Android 1080p30 连续 2h
- [ ] 无崩溃、内存不持续增长
- [ ] 证据：截图/录屏或诊断导出（脱敏）
