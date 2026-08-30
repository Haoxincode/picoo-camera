#!/usr/bin/env bash
# 安装 Android SDK / NDK，供 Gradle 与 cargo-ndk 构建 Sender APK。
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=../apps/android/toolchain.properties
source "${ROOT}/apps/android/toolchain.properties"

# GitHub-hosted runners pre-set ANDROID_HOME to /usr/local/lib/android/sdk.
# That tree often lacks NDK 28 licenses and may not be writable — prefer a
# per-job SDK under $HOME unless the caller opts into the preinstalled root.
if [ "${PICOO_KEEP_ANDROID_HOME:-0}" = "1" ] \
  && { [ -n "${ANDROID_HOME:-}" ] || [ -n "${ANDROID_SDK_ROOT:-}" ]; }; then
  ANDROID_HOME="${ANDROID_HOME:-${ANDROID_SDK_ROOT}}"
else
  ANDROID_HOME="${PICOO_ANDROID_HOME:-${HOME}/android-sdk}"
fi

# NDK r28+: 16 KB page-size support for Xiaomi 15 / Android 15 Rust cdylib.
NDK_VERSION="${PICOO_ANDROID_NDK_VERSION}"
BUILD_TOOLS="${PICOO_ANDROID_BUILD_TOOLS}"
PLATFORM="${PICOO_ANDROID_PLATFORM}"
CMDLINE_TOOLS="${ANDROID_HOME}/cmdline-tools/latest"

log() { printf '\n[android-sdk] %s\n' "$*"; }

write_licenses() {
  local root="$1"
  mkdir -p "${root}/licenses"
  # Hashes accepted by Android Gradle Plugin / sdkmanager (NDK side-by-side included).
  printf '%s\n' "24333f8a63b6825ea9c5514f83c2829b004d1fee" \
    >"${root}/licenses/android-sdk-license"
  printf '%s\n' "84831b9409646a918e30573bab259c9cb6408dd" \
    >"${root}/licenses/android-sdk-preview-license"
  printf '%s\n' "601085b94cd77f0b54ff86406957099eceba260" \
    >"${root}/licenses/android-googletv-license"
  printf '%s\n' "d56f5187479451bea6f1ad8c0b0e5bb0" \
    >"${root}/licenses/android-sdk-arm-dbt-license"
  printf '%s\n' "d975f751698bdc7cf7de205b7219edf7bb966ad3340b4cb4efbc5" \
    >"${root}/licenses/intel-android-extra-license"
  printf '%s\n' "33b6a2b64607f11b759f320ef9dff4ae5c047d5" \
    >"${root}/licenses/google-gdk-license"
  printf '%s\n' "e9acab5b5fbb560a72cfaecce8946896ff6aab9" \
    >"${root}/licenses/mips-android-sysimage-license"
}

export_github_env() {
  if [ -n "${GITHUB_ENV:-}" ]; then
    {
      echo "ANDROID_HOME=${ANDROID_HOME}"
      echo "ANDROID_NDK_HOME=${ANDROID_NDK_HOME}"
    } >>"${GITHUB_ENV}"
  fi
}

ensure_cmdline_tools() {
  if [ -x "${CMDLINE_TOOLS}/bin/sdkmanager" ]; then
    return 0
  fi
  if [ "$(uname -s)" != "Linux" ]; then
    log "ERROR: Android command-line tools missing at ${CMDLINE_TOOLS}"
    log "Install Android SDK Command-line Tools from Android Studio, then rerun with PICOO_KEEP_ANDROID_HOME=1."
    return 1
  fi
  log "安装 Android command-line tools 到 ${ANDROID_HOME}"
  mkdir -p "${ANDROID_HOME}/cmdline-tools"
  tmp="$(mktemp -d)"
  trap 'rm -rf "${tmp}"' RETURN
  curl -fsSL "https://dl.google.com/android/repository/commandlinetools-linux-11076708_latest.zip" \
    -o "${tmp}/cmdline-tools.zip"
  unzip -q "${tmp}/cmdline-tools.zip" -d "${tmp}/cmdline-tools-unpack"
  rm -rf "${CMDLINE_TOOLS}"
  mv "${tmp}/cmdline-tools-unpack/cmdline-tools" "${CMDLINE_TOOLS}"
}

write_licenses "${ANDROID_HOME}"
# Also seed the runner preinstall root so accidental sdk.dir fallbacks still work.
if [ -d /usr/local/lib/android/sdk ]; then
  write_licenses /usr/local/lib/android/sdk || true
fi

ensure_cmdline_tools
export ANDROID_HOME
export PATH="${CMDLINE_TOOLS}/bin:${ANDROID_HOME}/platform-tools:${PATH}"

NEED_INSTALL=0
if [ ! -d "${ANDROID_HOME}/platforms/${PLATFORM}" ]; then
  NEED_INSTALL=1
fi
if [ ! -d "${ANDROID_HOME}/ndk/${NDK_VERSION}" ]; then
  NEED_INSTALL=1
fi
if [ ! -d "${ANDROID_HOME}/build-tools/${BUILD_TOOLS}" ]; then
  NEED_INSTALL=1
fi
if [ ! -d "${ANDROID_HOME}/platform-tools" ]; then
  NEED_INSTALL=1
fi

if [ "${NEED_INSTALL}" = "1" ]; then
  log "接受 SDK 许可并安装 platform / build-tools / NDK ${NDK_VERSION}"
  # Non-interactive license acceptance (in addition to hashed license files).
  yes 2>/dev/null | sdkmanager --licenses >/dev/null || true
  sdkmanager "platform-tools" "platforms;${PLATFORM}" "build-tools;${BUILD_TOOLS}" "ndk;${NDK_VERSION}"
else
  log "Android SDK 已安装 (${PLATFORM}, NDK ${NDK_VERSION}) at ${ANDROID_HOME}"
fi

export ANDROID_NDK_HOME="${ANDROID_HOME}/ndk/${NDK_VERSION}"
export PATH="${ANDROID_HOME}/platform-tools:${PATH}"
export_github_env

if [ ! -d "${ANDROID_NDK_HOME}" ]; then
  log "ERROR: NDK missing at ${ANDROID_NDK_HOME}"
  exit 1
fi

log "ANDROID_HOME=${ANDROID_HOME}"
log "ANDROID_NDK_HOME=${ANDROID_NDK_HOME}"
sdkmanager --list_installed 2>/dev/null | grep -E 'platforms;|ndk;|build-tools;' || true
