# Android Sender

Jetpack Compose + Camera2 + MediaCodec + JNI → Rust Core FFI.

## 构建

```bash
cargo xtask build android
```

Gradle 项目在 `apps/android/` 落地后，CI `ubuntu-latest` job 将产出 debug/release APK。

## Requirement 映射

- REQ-PICOO-MEDIA-001..004
- REQ-PICOO-UI-003, REQ-PICOO-UI-005, REQ-PICOO-UI-006
- REQ-PICOO-DISCOVERY-005
