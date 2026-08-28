# Linux 桌面验证闭环（REQ-PICOO-UI-010）

> Linux 运行同一套桌面 GPUI 壳，用来测功能和测 UI。  
> 它不是产品 Receiver，不注册会议软件虚拟摄像头。  
> 架构：[0009-desktop-gpui-mobile-native-ui-boundary.md](../architecture/0009-desktop-gpui-mobile-native-ui-boundary.md)  
> 需求：[ui.md](../requirements/ui.md) 中 `REQ-PICOO-UI-010`  
> 视觉源：[HTML 原型](../prototypes/picoo-camera-ui-prototype.html) 的 `#desktop-window` / `#d-view-idle` / `#d-modal-settings`

## 意义

Cloud Agent 与 Linux 开发机必须能看见、点到、对照原型验收同一套桌面壳。  
没有这条闭环，桌面 UI 只能靠结构对齐，无法验证功能是否可走完、像素是否对得上。

Windows 产品 Receiver（真 VCam / MSI）在这条闭环可验证之后再做，不并行改第二套壳。

## 要测什么

1. **功能**：启动 Receiver runtime → Waiting 展示 QR / advertise endpoint / mDNS 已起来 → 设置可改显示名与偏好并落盘 → 有流时进入 Live → 配对中弹出短码框。
2. **UI**：对照 HTML `d-view-idle`、`d-modal-settings`、`d-view-live`。强制 Dark Slate，标题栏为「Picoo Camera Receiver」+ GPUI badge + 设置齿轮，没有「等待连接 / 直播 / 设置」假导航。字体为 Bricolage Grotesque / Figtree / JetBrains Mono，不是系统默认 UI 字体。

## 范围

- 编译并启动 `picoo-desktop --features gpui-ui`
- 无参数或 `--gpui` 进入同一套壳
- 虚拟摄像头状态为 `Unsupported`，不得伪装成检测中、未安装或 Active
- VCam 不适用时默认进入 Waiting（HTML 主视觉），不把 First Launch 当成 Linux 默认页
- 可选 `PICOO_UI_PREVIEW_PAGE` 固定页面，便于截图

## 边界

- 不注册 v4l2loopback 或任何会议软件摄像头
- 不把 `PUC-004` / `REQ-PICOO-VCAM-*` 标为 Linux verified
- 不把 `REQ-PICOO-UI-0001` 仅因窗口能打开或截到一张图而标 `verified`
- First Launch 仍是 Windows / macOS 的 VCam 安装门，可用 `PICOO_UI_PREVIEW_PAGE=first-launch` 打开，但不是 Linux 验证主路径

## 可验证闭环

| 步 | 操作 | 功能验收 | UI 对照 |
| --- | --- | --- | --- |
| 1 | 启动（无 `PICOO_UI_PREVIEW_PAGE`） | runtime 监听 QUIC，生成 QR JSON | 深色 Waiting，标题栏与原型一致 |
| 2 | 看 Waiting | 二维码矩阵、`host:port`、已配对台数、VCam=`Unsupported` | `#d-view-idle`：Logo、标题、说明、状态胶囊、QR 卡 |
| 3 | 点标题栏设置齿轮 | Settings overlay 打开 | `#d-modal-settings`：分组行，不是默认肥 GroupBox |
| 4 | 改显示名并保存 | prefs 落盘，mDNS 名称更新 | 行内输入 +「保存显示名称」 |
| 5 | `PICOO_UI_PREVIEW_PAGE=live` | Live 壳可开（无 Sender 时占位） | `#d-view-live` HUD + 底栏遥测 |
| 6 | 真 Sender 配对（可选） | 弹出 6 位短码 | `#d-modal-pairing` |

## 启动

```bash
PICOO_INSTALL_GPUI=1 bash .cursor/install.sh
cargo xtask build linux
PICOO_PREFS=/tmp/picoo-preview/prefs.json \
  bash scripts/run-linux-gpui-preview.sh
```

`PICOO_UI_PREVIEW_PAGE` 取值：`first-launch`、`waiting`、`live`、`settings`。
