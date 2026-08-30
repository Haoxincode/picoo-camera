# macOS Host App Packaging

`cargo xtask package macos` 负责生成产品形态的 ARM64 Host bundle：

```text
Picoo Camera.app/
└── Contents/
    ├── Info.plist
    ├── MacOS/picoo-desktop
    └── Library/SystemExtensions/
        └── com.haoxincode.picoo-camera.camera-extension.systemextension/
```

Host 与 Camera Extension 共享 `<TeamID>.com.haoxincode.picoo-camera` App Group。
Host 签名输入还声明 App Sandbox、QUIC/mDNS 所需的 network client/server 与安装系统
扩展能力；Camera Extension 继续使用独立的 sandbox entitlement。

无 `PICOO_APPLE_TEAM_ID` 时，xtask 显式使用 `UNSIGNED.` 前缀，避免把缺失 Team ID
伪装为可签名身份，并输出 `target/apple/PicooCamera-macOS.entitlements` 作为已展开的
签名输入 scaffold。发布打包必须传入 10 位 Apple Team ID，并用同一身份构建、签名
Host 与 Extension：

```bash
PICOO_APPLE_TEAM_ID=ABCDEFGHIJ cargo xtask package macos
```

当前 xtask 只产出并校验未签名 bundle 与签名输入。Developer ID、Hardened Runtime、嵌套代码由内
而外签名、公证、放入 `/Applications`、`OSSystemExtensionRequest` 激活和用户批准必须
在后续发布与真机验收中完成，不能由无签名 CI 结果替代。

追溯：`REQ-PICOO-STACK-004`、`REQ-PICOO-STACK-007`、`REQ-PICOO-VCAM-006`、
`REQ-PICOO-VCAM-007`。
