# Picoo Camera iOS Sender

状态：SwiftUI 设备流程、Rust C ABI 状态桥、mDNS/手动直连、AVFoundation 采集和 VideoToolbox H.264 媒体链路为 `implemented`；iPhone 真机 480p30 / 720p30 / 1080p30、弱网 ABR、方向与视觉验收仍待升级为 `verified`。

## 边界

iOS Sender 使用 SwiftUI 承载设备列表、手动连接、配对和传输页面；使用 AVFoundation 管理相机与预览，使用 VideoToolbox 输出低延迟 H.264 Access Unit。编码后的数据通过 `PicooCore.xcframework` 的 C ABI 进入 Rust Core，原始摄像头帧不跨 FFI。

平台层负责：

- Camera 与 Local Network 权限；
- `AVCaptureSession` / `VTCompressionSession` 生命周期；
- 前后台、旋转、热状态和防锁屏；
- SwiftUI 状态桥接。

Rust Core 继续负责协议、QUIC、发现、配对、会话、分包、重连和码率控制。

当前 UI 以 `SenderAppModel` 的单向状态流驱动，使用 Swift Observation（`@Observable`）观察 Rust C ABI 状态快照。设备页启动时不会请求 Camera 权限；只有会话进入传输页才请求权限并启动 `AVCaptureSession`。相机配置、切换和 `startRunning()` / `stopRunning()` 由独立 actor 串行化，`AVCaptureVideoPreviewLayer` 固定在 `MainActor` 创建和展示。

媒体路径固定为 `AVCaptureVideoDataOutput (420v) → VTPixelTransferSession（仅尺寸不同时）→ VTCompressionSession → AVCC Access Unit → PicooCore C ABI`。采集锁定 30 FPS；480p 使用系统 Pixel Transfer 将 720p 采集帧缩放为 854×480。编码器要求系统硬件 H.264，启用实时模式、禁用帧重排、每 2 秒产生 IDR，并使用 Main 4.0（不可用时回退 Baseline 4.0）。Swift 只把编码后的 AU、SPS/PPS、时间戳和 `stream_epoch` 交给 Rust；`CVPixelBuffer` 永不跨 FFI。编码 AU 使用有界 GOP-aware 队列，积压或 epoch/连接切换时丢弃依赖帧直至新 IDR；Rust 同时拒绝离线 AU 并在断连时清空待发分片，避免 H.264 数据无界增长或跨连接发送旧画面。Rust 的 480p/720p/1080p ABR、关键帧请求和远端 CameraCommand 会回送到原生编码/相机 actor；设备旋转由 `AVCaptureDevice.RotationCoordinator` 写入 `StreamConfig.rotation`，前置摄像头的本机预览镜像与远端镜像保持独立。

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
- AVFoundation / VideoToolbox / Network / UIKit / Security / SystemConfiguration 与本地 `PicooCore.xcframework`；
- Reicon SVG 仅以本地 Asset Catalog 资源引入，不依赖图标库。

工程不使用 CocoaPods、Carthage、第三方 Swift Package 或项目生成器。当前 `.app` 是无签名的 Simulator 验证产物，不包含 Provisioning Profile 或 App Store 配置。

## 正式发布

受保护的 `Apple Release` workflow 使用临时 Keychain 导入固定 Apple Distribution identity，并将 App Store provisioning profile 以其 UUID 安装到 runner。以下命令由该 workflow 调用，缺少任一身份、版本或 profile 输入时直接失败：

```bash
cargo xtask release ios
```

该命令从 workspace SemVer 取得 `CFBundleShortVersionString`，使用显式递增的 `CFBundleVersion`，生成 device ARM64 Archive 并导出 `target/apple/PicooCamera-iOS.ipa`。导出后会复核 signature、Apple Distribution authority、Team ID、leaf certificate、`com.picoo.camera`、版本、arm64 slice、effective entitlements 与 embedded profile UUID。workflow 在生成 SPDX SBOM 和 provenance 前删除 P12、临时 Keychain 与安装的 profile。真实凭据首次绿测、App Store Connect 校验和真机覆盖安装仍需作为外部验收证据记录。

## 追溯

- `REQ-PICOO-STACK-003`
- `REQ-PICOO-STACK-007`
- `REQ-PICOO-STACK-008`
- `REQ-PICOO-MEDIA-011`
- `REQ-PICOO-DISCOVERY-008`
- `REQ-PICOO-UI-010`
