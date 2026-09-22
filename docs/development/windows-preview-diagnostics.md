# Windows 预览灰屏诊断

追溯：REQ-PICOO-PRIVACY-003、REQ-PICOO-MEDIA-063、ARCH-PICOO-UI-001。

保持手机连接，在桌面连接页复现至少 10 秒，然后进入「帮助 → 导出诊断信息」。导出文件
`%TEMP%\picoo-diagnostics.json` 包含 session 与 preview 两个独立事实域；无需开启 Trace。
点击「打开所在文件夹」后提供这一个 JSON 即可。命令行 `--export-diagnostics` 不读取运行中
GUI，因此不包含活动 preview。未安装包含本变更的构建时，旧 JSON 没有 preview 字段。

`app_version` 是基础 SemVer；`build_number` 是编译时注入的 CI 构建号（本地构建未设置则省略）。

## 判读

`preview.stages` 的每一项包含 `count`、`last_generation` 和 `last_age_ms`。计数按整个预览
pipeline 生命周期累积，不能跨阶段简单相减推导丢帧；UI 会重绘同一张纹理，也会覆盖待准备帧。
`current_generation` 随源失效推进。最近事件的 generation 不同于当前值时属于旧源；
没有发生过的事件其时间为 null。进入帮助页后可见需求停止是正常的，应结合最近进展时间判读。

| 阶段 | 含义与边界 |
| --- | --- |
| `poll` / `visible_poll` | UI pump 次数 / Live 连接页的 pump 次数；visible 不等于系统窗口无遮挡 |
| `source_available` / `source_admitted` | pump 观察到最新帧 / 帧通过 connection generation 准入的次数，不是唯一帧数 |
| `viewport_demand` | 从可见元素回调消费到有效物理尺寸需求 |
| `submitted` / `pending_replaced` | 请求进入单槽 / 新请求覆盖尚未开始的旧请求 |
| `prepare_started` / `prepare_succeeded` / `prepare_failed` | 后台开始准备 / 已准备平台 surface / 返回原生错误；started 持续不完成时检查 GPU 工作等待 |
| `stale_completion` | 准备结果的 generation 已失效，正常丢弃 |
| `surface_presented` / `surface_rejected` | VideoSurface 接受结果 / 拒绝旧 sequence；接受尚不代表实际绘制 |
| `draw_attempted` | Windows GPUI 调用了 surface 的读取回调 |
| `texture_read_acquired` | 已取得 GPU 共享纹理读取权并进入 GPUI draw 回调 |
| `draw_submitted` | GPUI draw 回调及读取提交成功；不是 GPU 完成、swapchain Present 或肉眼看到画面的证明 |
| `draw_busy` | 共享访问暂忙而跳过绘制；不能算绘制成功 |
| `draw_failed` | GPU 读取/绘制边界返回错误 |

`last_error` 只保留最近一条原生预览错误及其阶段、代际、帧龄；后续成功不会抹掉它。
错误摘要上限 512 字符，无控制字符，不采集画面。`draw_observation_supported=false` 表示该
平台没有接入绘制观察，不能用绘制计数为零判断故障。

## 验证边界

平台无关测试验证停顿与 busy 不会被算作成功、代际与错误有界、并发计数和 JSON 导出脱敏。
macOS 本地验证共享预览代码编译及可执行测试。Windows draw hook 必须通过 Windows runner
构建并在真实机器复现后验证；本地 macOS 的通过结果不构成 Windows 灰屏已修复的证据。
