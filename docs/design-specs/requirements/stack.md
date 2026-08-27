# REQ-PICOO-STACK：Monorepo 与构建

| ID | 状态 | 来源 | 描述 | 验收 |
| --- | --- | --- | --- | --- |
| REQ-PICOO-STACK-001 | implemented | ARCH-PICOO-STACK-001 | Cargo workspace 包含 ARCH 定义的全部 Rust Core crate | workspace members + `cargo test` |
| REQ-PICOO-STACK-002 | implemented | ARCH-PICOO-STACK-001 | `proto/picoo_camera.proto` 由 prost 生成控制消息类型 | `picoo-protocol` build.rs |
| REQ-PICOO-STACK-003 | implemented | ARCH-PICOO-STACK-001 | Android/iOS 通过 C ABI（cbindgen）调用 Rust Core | `picoo_camera.h` + JNI |
| REQ-PICOO-STACK-004 | implemented | ci-and-build.md | xtask 提供 build/test/package 统一入口 | `cargo xtask --help` |
| REQ-PICOO-STACK-005 | implemented | ci-and-build.md | GitHub Actions ubuntu + windows job 产出 APK 与 Windows 安装包 | workflow 存在；本机已验证 Android debug APK；Windows MSI 与 CI green 待 Actions 配额 |
