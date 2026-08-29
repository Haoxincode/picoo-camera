# Picoo Camera iOS Sender

状态：SwiftUI 设备流程、Rust C ABI 状态桥、mDNS/手动直连和 AVFoundation 权限/预览边界为 `implemented`；VideoToolbox H.264、远端控制消费、真机媒体链路与视觉验收仍为 `planned`。

## 边界

iOS Sender 使用 SwiftUI 承载设备列表、手动连接、配对和传输页面；使用 AVFoundation 管理相机与预览，使用 VideoToolbox 输出低延迟 H.264 Access Unit。编码后的数据通过 `PicooCore.xcframework` 的 C ABI 进入 Rust Core，原始摄像头帧不跨 FFI。

平台层负责：

- Camera 与 Local Network 权限；
- `AVCaptureSession` / `VTCompressionSession` 生命周期；
- 前后台、旋转、热状态和防锁屏；
- SwiftUI 状态桥接。

Rust Core 继续负责协议、QUIC、发现、配对、会话、分包、重连和码率控制。

当前 UI 以 `SenderAppModel` 的单向状态流驱动，使用 Swift Observation（`@Observable`）观察 Rust C ABI 状态快照。设备页启动时不会请求 Camera 权限；只有会话进入传输页才请求权限并启动 `AVCaptureSession`。相机配置、切换和 `startRunning()` / `stopRunning()` 由独立 actor 串行化，`AVCaptureVideoPreviewLayer` 固定在 `MainActor` 创建和展示。

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

构建还会先使用 iPhone Simulator SDK 的 Clang 将 `scripts/apple_ffi_smoke.c` 与 ARM64 simulator staticlib 完整链接，再编译链接使用该 module 的 Swift 6 App，避免只验证 archive 生成而遗漏 Apple linker 或 Swift module 兼容性。运行态测试会实际创建、查询和销毁 opaque Sender handle。

安装 iPhone Simulator runtime 的机器还可以执行：

```bash
cargo xtask test ios
```

`xtask` 会按数值版本选择最新的可用 iPhone Simulator，并运行 Swift Testing 编写的 `PicooSenderSessionTests`。本机只有 iOS SDK、没有 simulator runtime 时，Swift 源码和测试包可编译链接；包含 SVG Asset Catalog 的最终 App 打包及运行态测试由安装了 runtime 的 macOS CI runner 承担。

`xtask` 显式固定 `IPHONEOS_DEPLOYMENT_TARGET=18.0`，避免产物的最低版本随 CI runner 或本机 Xcode SDK 漂移。Apple 开发与发布链路只支持 ARM64，不生成 Intel Simulator slice。

## Swift 技术栈

- Xcode 26.6，Swift 6 语言模式，当前工具链编译器为 Apple Swift 6.3.3；
- Swift 6 strict concurrency、默认 `MainActor` 与 approachable concurrency；
- SwiftUI + Observation、Swift Concurrency actor、Swift Testing；
- AVFoundation / Network / UIKit / Security / SystemConfiguration 与本地 `PicooCore.xcframework`；
- Reicon SVG 仅以本地 Asset Catalog 资源引入，不依赖图标库。

工程不使用 CocoaPods、Carthage、第三方 Swift Package 或项目生成器。当前 `.app` 是无签名的 Simulator 验证产物，不包含 Provisioning Profile 或 App Store 配置。

## 追溯

- `REQ-PICOO-STACK-003`
- `REQ-PICOO-STACK-007`
- `REQ-PICOO-MEDIA-011`
- `REQ-PICOO-DISCOVERY-008`
- `REQ-PICOO-UI-010`
