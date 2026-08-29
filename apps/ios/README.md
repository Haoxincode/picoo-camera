# Picoo Camera iOS Sender

状态：原生 SwiftUI App 壳与 Rust C ABI 生命周期接入为 `implemented`，远端 Simulator XCTest 尚待验证；设备流程、AVFoundation 与 VideoToolbox 接入仍为 `planned`。

## 边界

iOS Sender 使用 SwiftUI 承载设备列表、手动连接、配对和传输页面；使用 AVFoundation 管理相机与预览，使用 VideoToolbox 输出低延迟 H.264 Access Unit。编码后的数据通过 `PicooCore.xcframework` 的 C ABI 进入 Rust Core，原始摄像头帧不跨 FFI。

平台层负责：

- Camera 与 Local Network 权限；
- `AVCaptureSession` / `VTCompressionSession` 生命周期；
- 前后台、旋转、热状态和防锁屏；
- SwiftUI 状态桥接。

Rust Core 继续负责协议、QUIC、发现、配对、会话、分包、重连和码率控制。

## 构建基线

在 macOS + Xcode 环境执行：

```bash
rustup target add aarch64-apple-ios aarch64-apple-ios-sim
cargo xtask build ios
```

该命令会在仓库固定的 `target/apple/` 下构建两个无签名产物，不受自定义 Cargo target directory 影响：

- `PicooCore.xcframework`：Rust Core 的 C ABI；
- `ios-app/PicooCamera.app`：ARM64 iPhone Simulator App，用于编译与链接门禁。

同时生成保留 Apple bundle 外层目录、文件权限和符号链接的 `PicooCore.xcframework.zip` 与 `PicooCamera.app.zip`，供 CI 上传。

执行一次 `cargo xtask build ios` 后，可直接在 Xcode 打开 `PicooCamera.xcodeproj` 运行、测试和使用 Preview；工程默认引用仓库 `target/apple/PicooCore.xcframework`，`xtask`/CI 仍可通过 build setting 显式覆盖。

XCFramework 包含：

- iOS device `arm64`；
- iOS Simulator `arm64`；
- `picoo_camera.h`；
- `module.modulemap`（Swift module 名为 `PicooCore`）。

构建还会先使用 iPhone Simulator SDK 的 Clang 将 `scripts/apple_ffi_smoke.c` 与 ARM64 simulator staticlib 完整链接，再编译链接使用该 module 的 Swift 6 `PicooSenderSession`，避免只验证 archive 生成而遗漏 Apple linker 或 Swift module 兼容性。运行态测试会实际创建、查询和销毁 opaque Sender handle。

安装 iPhone Simulator runtime 的机器还可以执行：

```bash
cargo xtask test ios
```

`xtask` 会按数值版本选择最新的可用 iPhone Simulator，并运行 `PicooSenderSessionTests`。本机只有 iOS SDK、没有 simulator runtime 时，`build ios` 仍可完整编译链接，运行态测试由 macOS CI runner 承担。

`xtask` 显式固定 `IPHONEOS_DEPLOYMENT_TARGET=18.0`，避免产物的最低版本随 CI runner 或本机 Xcode SDK 漂移。Apple 开发与发布链路只支持 ARM64，不生成 Intel Simulator slice。

工程只使用 SwiftUI、XCTest 与 Apple 系统 Framework；不使用 CocoaPods、Carthage、第三方 Swift Package 或项目生成器。当前 `.app` 是无签名的 Simulator 验证产物，不包含 Provisioning Profile 或 App Store 配置。

## 追溯

- `REQ-PICOO-STACK-003`
- `REQ-PICOO-STACK-007`
- `REQ-PICOO-MEDIA-011`
- `REQ-PICOO-DISCOVERY-008`
- `REQ-PICOO-UI-010`
