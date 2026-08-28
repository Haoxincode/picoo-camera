# Android Sender

Jetpack Compose + Camera2 + MediaCodec + JNI → Rust Core FFI.

## 构建

```bash
# 首次：安装 Android SDK / NDK / cargo-ndk
PICOO_INSTALL_ANDROID=1 bash .cursor/install.sh

# 构建 debug APK（含 Rust FFI）
cargo xtask build android
# 或
./apps/android/gradlew -p apps/android assembleDebug

# Linux / Cloud Agent x86_64 仿真器（真机仍默认只编 arm64-v8a）
PICOO_ANDROID_ABIS=x86_64 ./apps/android/gradlew -p apps/android assembleDebug
```

产物：`apps/android/app/build/outputs/apk/debug/app-debug.apk`

Release（签名前可用）与 16KB 页校验：

```bash
cargo xtask package android   # 含 scripts/check_android_so_16k.sh
```

> 小米 15 / Android 15 等 16KB 页设备：`libpicoo_jni.so` / `libpicoo_ffi.so` 必须 16KB 对齐，
> 且 `DT_NEEDED` 为 `libpicoo_ffi.so`（不可含构建机绝对路径）。默认 NDK **28.0.12674087**。

### 跨机型兼容（16KB 页）

| 平台 | 说明 |
| --- | --- |
| Android 15+ 16KB 页旗舰（如小米 15） | 需要本仓库对齐后的 APK；未对齐会冷启动闪退 |
| 传统 4KB 页 Android 机 | **兼容**：16KB 对齐的 `.so` 可在 4KB 页上加载 |
| iPhone / iOS | **不适用**：Mach-O 体系，无 Android ELF 16KB 对齐问题；iOS 不在 V1 范围 |

扫码路径依赖 CameraX **1.4.2+**（其 native 亦需 16KB）；预览/推流仍走 Camera2 + MediaCodec。

## 架构

```text
Camera2 Capture Session → MediaCodec InputSurface (H.264)
Kotlin/Compose 预览 → JNI (libpicoo_jni.so) → C ABI (libpicoo_ffi.so) → Rust Core
（下一步：编码 AU → QUIC Datagram）
```

## Requirement 映射

- REQ-PICOO-STACK-003：JNI → C ABI → Rust
- REQ-PICOO-MEDIA-001..004
- REQ-PICOO-UI-003, REQ-PICOO-UI-005, REQ-PICOO-UI-006
- REQ-PICOO-DISCOVERY-005
