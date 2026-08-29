# REQ-PICOO-STACK：Monorepo 与构建

| ID | 状态 | 来源 | 描述 | 验收 |
| --- | --- | --- | --- | --- |
| REQ-PICOO-STACK-001 | implemented | ARCH-PICOO-STACK-001 | Cargo workspace 包含 ARCH 定义的全部 Rust Core crate | workspace members + `cargo test` |
| REQ-PICOO-STACK-002 | implemented | ARCH-PICOO-STACK-001 | `proto/picoo_camera.proto` 由 prost 生成控制消息类型，构建期使用 vendored `protoc` | `picoo-protocol` build.rs；开发机无需单独安装 `protoc` |
| REQ-PICOO-STACK-003 | planned | ARCH-PICOO-STACK-001 | Android 通过 Rust JNI exports、iOS 通过 C ABI 调用 Rust Core | Android `cargo-ndk` 单一 Rust JNI `.so` 须 16KB `PT_LOAD` 对齐；iOS 保留 `picoo_camera.h`；`scripts/check_android_so_16k.sh` |
| REQ-PICOO-STACK-004 | implemented | ci-and-build.md | xtask 提供 build/test/package 统一入口 | `cargo xtask --help`；`test protocol` / `test linux` |
| REQ-PICOO-STACK-005 | implemented | ci-and-build.md | GitHub Actions ubuntu + windows job 产出 APK 与 Windows 安装包 | workflow 存在；`xtask package android`→APK/AAB + 16KB so 门禁；NDK 28；Windows job 钉扎 WiX 5.0.2 + `PICOO_REQUIRE_MSI=1`；MSI artifact hard-fail |
| REQ-PICOO-STACK-006 | implemented | ARCH-PICOO-STACK-001 | 仓库构建不依赖 CMake 或 C++/MSBuild 工程；QUIC、Android JNI 与 Windows VCam 均由 Cargo 管理 | 仓库无 `CMakeLists.txt`、VCXPROJ、C++ VCam Source 或对应构建调用；Windows VCam 为 `windows-rs` `cdylib`；CI 不安装或调用 CMake/MSBuild |
