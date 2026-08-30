# Reicon 图标子集

这里保存 Picoo Camera 实际使用的 Reicon SVG，作为 Android、iOS、Windows、macOS 和 Web 原型的共享图标事实源。功能图标默认使用 Outline；明确的完成状态使用 Reicon Filled `check-circle`，Sidebar 中需要在 `18px` 下同时辨识两种设备的 `monitor-phone`、`monitor-camera` 以及折叠控制的 `sidebar-left` / `sidebar-right` 使用官方 Filled 权重。

- 上游：`dqev/reicon`
- 固定版本：`974cf2445404d6d59531336c9c8e74f74c0701aa`
- 提取源：该版本的 `data/icon-data.json`，按图标名与权重直接生成独立 SVG
- 网格：`24 × 24`
- 许可证：MIT，见同目录 `LICENSE`

只提交产品实际使用的单个 SVG，不引入 Reicon 完整依赖。`manifest.json` 固定上游版本并维护产品语义 → Source glyph 映射；业务页面只能依赖平台类型安全语义层。平台适配产物必须保持相同文件名和语义；Android 的 `res/drawable/reicon_*.xml` 是这些 SVG 的 Vector Drawable 转换结果，iOS 使用对应的 SVG Image Set。
