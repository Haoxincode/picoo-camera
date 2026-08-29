# Android Sender

Jetpack Compose + Camera2 + MediaCodec + Rust JNI。

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

> 小米 15 / Android 15 等 16KB 页设备：单一 `libpicoo_ffi.so` 必须 16KB 对齐。
> 默认 NDK **28.0.12674087**。

### 跨机型兼容（16KB 页）

| 平台 | 说明 |
| --- | --- |
| Android 15+ 16KB 页旗舰（如小米 15） | 需要本仓库对齐后的 APK；未对齐会冷启动闪退 |
| 传统 4KB 页 Android 机 | **兼容**：16KB 对齐的 `.so` 可在 4KB 页上加载 |
| iPhone / iOS | **不适用**：Mach-O 体系，无 Android ELF 16KB 对齐问题；iOS 不在 V1 范围 |

预览与推流走 Camera2 + MediaCodec；手动连接仅解析用户输入的 `IP:端口`，不引入扫码 SDK。

## 架构

```text
Camera2 Capture Session → MediaCodec InputSurface (H.264)
Kotlin/Compose 预览 → Rust JNI exports (libpicoo_ffi.so) → Rust Core
（下一步：编码 AU → QUIC Datagram）
```

## Requirement 映射

- REQ-PICOO-STACK-003：Kotlin → Rust JNI → Rust Core
- REQ-PICOO-MEDIA-001..004
- REQ-PICOO-UI-003, REQ-PICOO-UI-005, REQ-PICOO-UI-006
- REQ-PICOO-DISCOVERY-005
