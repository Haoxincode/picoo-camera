# Picoo Camera iOS Sender

状态：Rust Core 构建基线已实现；SwiftUI、AVFoundation 与 VideoToolbox 接入为 `planned`。

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

产物为 Cargo target directory 下的 `apple/PicooCore.xcframework`，包含：

- iOS device `arm64`；
- iOS Simulator `arm64`；
- `picoo_camera.h`；
- `module.modulemap`（Swift module 名为 `PicooCore`）。

构建还会使用 iPhone Simulator SDK 的 Clang 将 `scripts/apple_ffi_smoke.c` 与 ARM64 simulator staticlib 完整链接，避免只验证 archive 生成而遗漏 Apple linker 兼容性。

`xtask` 显式固定 `IPHONEOS_DEPLOYMENT_TARGET=18.0`，避免产物的最低版本随 CI runner 或本机 Xcode SDK 漂移。Apple 开发与发布链路只支持 ARM64，不生成 Intel Simulator slice。

当前产物不是 iOS App，不包含签名、Provisioning Profile 或 App Store 配置。

## 追溯

- `REQ-PICOO-STACK-003`
- `REQ-PICOO-STACK-007`
- `REQ-PICOO-MEDIA-011`
- `REQ-PICOO-DISCOVERY-008`
- `REQ-PICOO-UI-010`
