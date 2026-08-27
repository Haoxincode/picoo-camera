# REQ-PICOO-STACK：Monorepo 与构建

| ID | 状态 | 来源 | 描述 | 验收 |
| --- | --- | --- | --- | --- |
| REQ-PICOO-STACK-001 | proposed | ARCH-PICOO-STACK-001 | Cargo workspace 包含 ARCH 定义的全部 Rust Core crate | `cargo test --workspace` 通过 |
| REQ-PICOO-STACK-002 | proposed | ARCH-PICOO-STACK-001 | `proto/picoo_camera.proto` 由 prost 生成控制消息类型 | `picoo-protocol` build.rs 生成类型 |
| REQ-PICOO-STACK-003 | proposed | ARCH-PICOO-STACK-001 | Android/iOS 通过 C ABI（cbindgen）调用 Rust Core | `picoo_ffi.h` 生成且符号稳定 |
| REQ-PICOO-STACK-004 | proposed | ci-and-build.md | xtask 提供 build/test/package 统一入口 | `cargo xtask --help` 列出 android/windows |
| REQ-PICOO-STACK-005 | proposed | ci-and-build.md | GitHub Actions ubuntu + windows job 产出 APK 与 Windows 安装包 | CI workflow 存在且 green |
