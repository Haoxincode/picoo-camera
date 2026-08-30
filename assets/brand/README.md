# Picoo Camera 品牌图标

本目录保存产品级品牌图标，与 `assets/icons/reicon/` 的功能性 UI Icon 分离。

- `app-icon-master.svg`：彩色 App Icon 矢量事实源，用于 Windows EXE、开始菜单、
  已安装应用列表，以及 macOS Finder、Dock、Launchpad 和 Spotlight。
- `tray-icon-master.svg`：为 16–32 px 通知区域单独简化的符号事实源，不直接缩小完整 App Icon。
- `windows/PicooCamera.ico`、`macos/PicooCamera.icns`：由事实源生成并随平台包发布的二进制资源。

主符号用相交的电脑与手机轮廓表达“手机画面进入电脑”，共享镜头表达两端共同提供一个系统摄像头。
品牌资产不得用 Reicon、Emoji 或平台功能图标替代。

在 macOS 上更新矢量事实源后运行：

```bash
cargo xtask generate brand-icons
```
