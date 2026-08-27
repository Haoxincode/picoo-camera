#!/usr/bin/env bash
# Local Android arm64 FFI + debug APK when GitHub Actions is unavailable.
# REQ-PICOO-TRANSPORT-005 / REQ-PICOO-STACK-005
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

rustup target add aarch64-linux-android >/dev/null
OUT_JNI="${OUT_JNI:-$ROOT/apps/android/app/src/main/jniLibs}"
echo "Building picoo-ffi for arm64-v8a…"
cargo ndk -t arm64-v8a -o "$OUT_JNI" build -p picoo-ffi --release

echo "Assembling debug APK (skips cargoBuildFfi; uses prebuilt jniLibs)…"
(
  cd "$ROOT/apps/android"
  ./gradlew :app:assembleDebug -x cargoBuildFfi
)

APK="$ROOT/apps/android/app/build/outputs/apk/debug/app-debug.apk"
ls -lh "$APK"
echo "APK ready: $APK"
