# Picoo 桌面字体（REQ-PICOO-UI-0001）

对应 HTML 原型 `:root`：

| 角色 | 字体 | 用在 |
| --- | --- | --- |
| Display | Bricolage Grotesque | 标题、品牌、设置/配对大标题 |
| Body | Figtree | 正文、标签、按钮（`Theme.font_family`） |
| Mono | JetBrains Mono | IP:Port、短码、遥测（`Theme.mono_font_family`） |
| Han fallback | Noto Sans SC | Figtree / Bricolage 没有的汉字 |

拉丁三套是 SIL OFL 1.1，来自 [google/fonts](https://github.com/google/fonts)。中文回退链：

`Noto Sans SC` → `Noto Sans CJK SC` / `Source Han Sans SC` → `PingFang SC` → `Microsoft YaHei UI`

`NotoSansSC-Variable.ttf` 只打进 **Linux** 验证包。Windows / macOS 走系统黑体，不嵌 CJK。

不要在应用视图里写新的族名；读 `picoo_theme` 的 `FONT_*`、`body_font()`、`display_font()` 或 `cx.theme()`。
