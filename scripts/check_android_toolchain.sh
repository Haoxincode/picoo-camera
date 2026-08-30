#!/usr/bin/env bash
# Verify the exact Rust/Android native toolchain used by Gradle, xtask and CI.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=../apps/android/toolchain.properties
source "${ROOT}/apps/android/toolchain.properties"

fail() {
  printf '[android-toolchain] ERROR: %s\n' "$*" >&2
  exit 1
}

command -v cargo >/dev/null 2>&1 || fail "cargo is not installed"
actual_cargo_ndk="$(cargo ndk --version 2>/dev/null || true)"
expected_cargo_ndk="cargo-ndk ${PICOO_CARGO_NDK_VERSION}"
[ "${actual_cargo_ndk}" = "${expected_cargo_ndk}" ] \
  || fail "expected ${expected_cargo_ndk}, got ${actual_cargo_ndk:-not installed}"

if [ -n "${ANDROID_NDK_HOME:-}" ]; then
  ndk_home="${ANDROID_NDK_HOME}"
elif [ -n "${ANDROID_HOME:-}" ]; then
  ndk_home="${ANDROID_HOME}/ndk/${PICOO_ANDROID_NDK_VERSION}"
elif [ -n "${ANDROID_SDK_ROOT:-}" ]; then
  ndk_home="${ANDROID_SDK_ROOT}/ndk/${PICOO_ANDROID_NDK_VERSION}"
else
  fail "ANDROID_HOME, ANDROID_SDK_ROOT or ANDROID_NDK_HOME must be set"
fi

[ -d "${ndk_home}" ] || fail "NDK ${PICOO_ANDROID_NDK_VERSION} missing at ${ndk_home}"
source_properties="${ndk_home}/source.properties"
[ -f "${source_properties}" ] || fail "NDK source.properties missing at ${source_properties}"
actual_ndk="$(sed -n 's/^Pkg.Revision[[:space:]]*=[[:space:]]*//p' "${source_properties}" | head -n 1)"
[ "${actual_ndk}" = "${PICOO_ANDROID_NDK_VERSION}" ] \
  || fail "expected NDK ${PICOO_ANDROID_NDK_VERSION}, got ${actual_ndk:-unknown}"

if command -v rustup >/dev/null 2>&1; then
  rustup target list --installed | grep -Fxq 'aarch64-linux-android' \
    || fail "Rust target aarch64-linux-android is not installed"
fi

printf '[android-toolchain] cargo-ndk=%s ndk=%s\n' \
  "${PICOO_CARGO_NDK_VERSION}" "${PICOO_ANDROID_NDK_VERSION}"
