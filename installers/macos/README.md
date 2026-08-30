# macOS Host App Packaging

`cargo xtask package macos` 负责生成产品形态的 ARM64 Host bundle：

```text
Picoo Camera.app/
└── Contents/
    ├── Info.plist
    ├── MacOS/picoo-desktop
    ├── Resources/PicooCamera.icns
    └── Library/SystemExtensions/
        └── com.haoxincode.picoo-camera.camera-extension.systemextension/
```

Host 与 Camera Extension 共享 `group.com.haoxincode.picoo-camera` App Group。
Host 签名输入还声明 App Sandbox、QUIC/mDNS 所需的 network client/server 与安装系统
扩展能力；Camera Extension 继续使用独立的 sandbox entitlement。

普通 `cargo xtask package macos` 的产物始终未签名，因此无论是否提供 Team ID，Host
Info.plist 都写入 `PicooUnsignedDevelopmentBuild=true`。Shared Ring 只根据该独立标记
选择 Application Support fallback，避免把正式 App Group 或 Team ID 误当作真实签名状态；同时
输出 `target/apple/PicooCamera-macOS.entitlements` 作为已展开的签名输入 scaffold。
发布打包必须传入 10 位 Apple Team ID，并用同一身份构建、签名
Host 与 Extension：

```bash
PICOO_APPLE_TEAM_ID=ABCDEFGHIJ cargo xtask package macos
```

`cargo xtask package macos` 继续产出并校验未签名 bundle，以及 Host/Extension 两份已展开
签名输入。只有 `cargo xtask release macos` 的内部 release package 会把 marker 设为 false，
随后立即签名；该命令要求显式递增的 release/build 版本、Developer ID
identity、Host/Extension Developer ID provisioning profile 与 Notary API Key；它会校验
profile 有效期、Developer ID 分发类型、授权证书、Team ID、Bundle ID、App Group 和
System Extension capability，由内而外签名后复核实际 signer/effective entitlements，再以
`notarytool`、`codesign`、`stapler`、`spctl` 完成公证与验证。

仓库的 `.github/workflows/release-apple.yml` 在 tag 或手动触发时调用该命令，凭据只来自
`apple-release` Environment Secrets。发布包仍必须放入 `/Applications`；Host 已通过
SystemExtensions 框架实现状态查询、激活、版本替换、用户批准等待、重启结果与停用移除。
用户批准、实际设备枚举与会议软件兼容性仍必须用签名真机验收，不能由未签名 CI 替代。

追溯：`REQ-PICOO-STACK-004`、`REQ-PICOO-STACK-007`、`REQ-PICOO-VCAM-006`、
`REQ-PICOO-VCAM-007`。
