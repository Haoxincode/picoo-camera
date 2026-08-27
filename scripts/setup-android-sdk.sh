#!/usr/bin/env bash
# 安装 Android SDK / NDK，供 Gradle 与 cargo-ndk 构建 Sender APK。
set -euo pipefail

ANDROID_HOME="${ANDROID_HOME:-${HOME}/android-sdk}"
# NDK r28+: 16 KB page-size support for Xiaomi 15 / Android 15 (libpicoo_jni / STL).
NDK_VERSION="${PICOO_ANDROID_NDK_VERSION:-28.0.12674087}"
BUILD_TOOLS="${PICOO_ANDROID_BUILD_TOOLS:-34.0.0}"
PLATFORM="${PICOO_ANDROID_PLATFORM:-android-34}"
CMDLINE_TOOLS="${ANDROID_HOME}/cmdline-tools/latest"

log() { printf '\n[android-sdk] %s\n' "$*"; }

if [ -d "${ANDROID_HOME}/platforms/${PLATFORM}" ] && [ -d "${ANDROID_HOME}/ndk/${NDK_VERSION}" ]; then
  log "Android SDK 已安装 (${PLATFORM}, NDK ${NDK_VERSION})"
  export ANDROID_HOME
  export ANDROID_NDK_HOME="${ANDROID_HOME}/ndk/${NDK_VERSION}"
  export PATH="${CMDLINE_TOOLS}/bin:${ANDROID_HOME}/platform-tools:${PATH}"
  return 0 2>/dev/null || exit 0
fi

log "安装 Android command-line tools 到 ${ANDROID_HOME}"
mkdir -p "${ANDROID_HOME}/cmdline-tools"
tmp="$(mktemp -d)"
trap 'rm -rf "${tmp}"' EXIT

curl -fsSL "https://dl.google.com/android/repository/commandlinetools-linux-11076708_latest.zip" \
  -o "${tmp}/cmdline-tools.zip"
unzip -q "${tmp}/cmdline-tools.zip" -d "${tmp}/cmdline-tools-unpack"
rm -rf "${CMDLINE_TOOLS}"
mkdir -p "${ANDROID_HOME}/cmdline-tools"
mv "${tmp}/cmdline-tools-unpack/cmdline-tools" "${CMDLINE_TOOLS}"

export ANDROID_HOME
export PATH="${CMDLINE_TOOLS}/bin:${PATH}"

log "接受 SDK 许可并安装 platform / build-tools / NDK"
mkdir -p "${ANDROID_HOME}/licenses"
tee "${ANDROID_HOME}/licenses/android-sdk-license" >/dev/null <<'EOF'
24333f8a63b6825ea9c5514f83c2829b004d1fee
EOF
tee "${ANDROID_HOME}/licenses/android-sdk-preview-license" >/dev/null <<'EOF'
84831b9409646a918e30573bab259c9cb6408dd
EOF
tee "${ANDROID_HOME}/licenses/android-googletv-license" >/dev/null <<'EOF'
601085b94cd77f0b54ff86406957099eceba260
EOF
tee "${ANDROID_HOME}/licenses/android-sdk-arm-dbt-license" >/dev/null <<'EOF'
d975f751698bdc7cf7de205b7219edf7bb966ad3340b4cb4efbc5
EOF
tee "${ANDROID_HOME}/licenses/intel-android-extra-license" >/dev/null <<'EOF'
d975f751698bdc7cf7de205b7219edf7bb966ad3340b4cb4efbc5
EOF
tee "${ANDROID_HOME}/licenses/google-gdk-license" >/dev/null <<'EOF'
33b6a2b64607f11b759f320ef9dff4ae5c047d5
EOF
tee "${ANDROID_HOME}/licenses/mips-android-sysimage-license" >/dev/null <<'EOF'
e9acab5b5fbb560a72cfaecce8946896ff6aab9
EOF

sdkmanager "platform-tools" "platforms;${PLATFORM}" "build-tools;${BUILD_TOOLS}" "ndk;${NDK_VERSION}"

export ANDROID_NDK_HOME="${ANDROID_HOME}/ndk/${NDK_VERSION}"
export PATH="${ANDROID_HOME}/platform-tools:${PATH}"

if [ -n "${GITHUB_ENV:-}" ]; then
  {
    echo "ANDROID_HOME=${ANDROID_HOME}"
    echo "ANDROID_NDK_HOME=${ANDROID_NDK_HOME}"
  } >>"${GITHUB_ENV}"
fi

log "ANDROID_HOME=${ANDROID_HOME}"
log "ANDROID_NDK_HOME=${ANDROID_NDK_HOME}"
sdkmanager --list_installed | grep -E 'platforms;|ndk;|build-tools;' || true
