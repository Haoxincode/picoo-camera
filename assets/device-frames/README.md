# 设备外框 SVG

这里保存桌面 Receiver「开始使用」硬件拓扑使用的真机外框，直接从
[`picoo-camera-receiver.html`](../../picoo-camera-receiver.html) 中抽取，确保 GPUI 原生实现与 HTML 原型使用同一份几何结构。

- `iphone-16-max.svg`：原型中的深色模式 iPhone 16 Max 外框，源自
  [`sneas/telephone`](https://github.com/sneas/telephone)，固定上游 commit
  `c1644a3d49dcd50ebf8c76306409c4b1d9b7a2b4`，MIT 许可证见同目录 `LICENSE`。
- `macbook-pro-light.svg` / `macbook-pro-dark.svg`：Picoo HTML 原型自行定义的
  MacBook Pro 外框，不属于 `sneas/telephone`。

Android Sender 的发现空状态从这两份 SVG 生成
`apps/android/app/src/main/res/drawable-nodpi/picoo_macbook_pro_{light,dark}.png`。Android
Compose 只原生加载 BitmapDrawable / VectorDrawable，而该真机框包含渐变与滤镜；派生 PNG 保留 SVG
为唯一几何事实源，避免为移动端手绘第二套电脑插图。

实现前评估过 AndroidSVG 1.4 运行时解析方案；当前只需要两张固定主题资产，引入额外 SVG runtime
不如确定性的派生资源合适。该判断依据 Android 官方说明：Compose `painterResource` 原生只加载
BitmapDrawable / VectorDrawable，复杂多色资产更适合位图，而 Vector Asset Studio 只支持 SVG 子集。

普通功能图标继续使用 `assets/icons/reicon/`；设备外框只用于展示真实硬件拓扑，
不作为操作图标使用。
