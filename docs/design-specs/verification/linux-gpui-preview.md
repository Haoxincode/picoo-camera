# Linux GPUI 预览宿主（REQ-PICOO-UI-010）

> 这是验证宿主说明，不是 Linux 产品 Receiver。  
> 架构：[0009-desktop-gpui-mobile-native-ui-boundary.md](../architecture/0009-desktop-gpui-mobile-native-ui-boundary.md)  
> 需求：[ui.md](../requirements/ui.md) 中 `REQ-PICOO-UI-010`

## 场景

Cloud Agent 与 Linux 开发机需要看见同一套 GPUI 桌面壳（First Launch / Waiting / Live / Settings），才能对照 [HTML 原型](../prototypes/picoo-camera-ui-prototype.html) 做像素反馈。

## 范围

- 编译 `picoo-desktop --features gpui-ui`
- 无参数或 `--gpui` 启动 GPUI 壳
- 虚拟摄像头状态为 `Unsupported`
- 可选 `PICOO_UI_PREVIEW_PAGE` 固定页面，便于截图

## 边界

- 不注册 v4l2loopback 或任何会议软件摄像头
- 不把 `PUC-004` / `REQ-PICOO-VCAM-*` 标为 Linux verified
- 不把 `REQ-PICOO-UI-0001` 仅因 Linux 窗口能打开而标 `verified`

## 启动

```bash
PICOO_INSTALL_GPUI=1 bash .cursor/install.sh
cargo xtask build linux
PICOO_PREFS=/tmp/picoo-preview/prefs.json \
  PICOO_UI_PREVIEW_PAGE=waiting \
  bash scripts/run-linux-gpui-preview.sh
```

`PICOO_UI_PREVIEW_PAGE` 取值：`first-launch`、`waiting`、`live`、`settings`。
