# Android Sender 交互原型

可点击 HTML，用于与产品/设计对齐手机端四屏（REQ-PICOO-UI-003 / PUC-003 / PRD §17），**不是**运行时 UI。

## 打开方式

- 仓库内：`docs/design-specs/prototypes/android-sender/index.html`
- Artifact：`/opt/cursor/artifacts/picoo-android-sender-prototype.html`

用浏览器直接打开即可，无需构建。

## 覆盖路径

1. **发现**（设备列表）：mDNS 发现 Ready / Paired、已配对管理、权限提示；**发现为空**时显示空态并突出扫码兜底
2. **扫码**（第 4 屏）：模拟相机取景 → 识别成功 → 进入配对（PUC-003；电脑端需 Show QR Code）
3. **配对**：六位短码 + 确认/取消（与 mDNS 路径相同）
4. **传输**：全屏预览、前后摄、720/1080、本机镜像、EV、断开、过热降档提示

## QR 何时出现（产品规则）

| 条件 | 手机端行为 |
|------|------------|
| mDNS 发现列表**为空**（企业 Wi‑Fi / 访客隔离等） | 空态 + **「扫描二维码连接」升为首要操作**（主色按钮） |
| 发现列表**有设备** | 列表为主路径；「扫描二维码连接」为**次要操作**（ghost 按钮） |
| 用户主动点「扫描二维码连接」 | 随时进入扫码屏（需相机权限，REQ-PICOO-UI-006） |

扫码成功后：解析 host/port/nonce → 直连 QUIC → 未配对走短码（PUC-001），已配对跳过短码（与 mDNS 一致）。

## 演示快捷入口（右侧）

- **发现为空 → 扫码配对**：空列表 + 自动打开扫码屏
- **列表有设备 → 手动扫码**：保留发现列表，演示次要扫码入口

对齐通过后，再落 Jetpack Compose，不引入 WebView。
