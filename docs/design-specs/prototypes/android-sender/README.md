# Android Sender 交互原型

可点击 HTML，用于与产品/设计对齐手机端三页（REQ-PICOO-UI-003 / PRD §17），**不是**运行时 UI。

## 打开方式

- 仓库内：`docs/design-specs/prototypes/android-sender/index.html`
- Artifact：`/opt/cursor/artifacts/picoo-android-sender-prototype.html`

用浏览器直接打开即可，无需构建。

## 覆盖路径

1. 设备列表：发现 Ready / Paired、已配对管理、扫码兜底、权限提示
2. 配对：六位短码 + 确认/取消
3. 传输：全屏预览、前后摄、720/1080、本机镜像、EV、断开、过热降档提示

对齐通过后，再落 Jetpack Compose，不引入 WebView。
