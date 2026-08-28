# Picoo 桌面字体（REQ-PICOO-UI-0001）

对应 HTML 原型 `:root`：

| 角色 | 字体 | 用在 |
| --- | --- | --- |
| Display | Bricolage Grotesque | 标题、品牌、设置/配对大标题 |
| Body | Figtree | 正文、标签、按钮（`Theme.font_family`） |
| Mono | JetBrains Mono | IP:Port、短码、遥测（`Theme.mono_font_family`） |

三套字体都是 SIL OFL 1.1。源文件来自 [google/fonts](https://github.com/google/fonts)，再实例化出静态字重，并把 name ID 1 写成族名，方便 GPUI / FreeType 按族匹配。

不要在应用视图里写新的族名；读 `picoo_theme` 常量或 `cx.theme()`。
