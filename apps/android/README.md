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
```

产物：`apps/android/app/build/outputs/apk/debug/app-debug.apk`

Release（签名前可用）与 16KB 页校验：

```bash
cargo xtask package android   # 含 scripts/check_android_so_16k.sh
```

> 小米 15 / Android 15 等 16KB 页设备：`libpicoo_jni.so` / `libpicoo_ffi.so` 必须 16KB 对齐，
> 且 `DT_NEEDED` 为 `libpicoo_ffi.so`（不可含构建机绝对路径）。默认 NDK **28.0.12674087**。

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
