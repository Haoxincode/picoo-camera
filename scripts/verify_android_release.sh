#!/usr/bin/env bash
# Verify stable Android release identity and version — REQ-PICOO-STACK-008.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=../apps/android/toolchain.properties
source "${ROOT}/apps/android/toolchain.properties"

APK="${1:-${ROOT}/apps/android/app/build/outputs/apk/release/app-release.apk}"
AAB="${2:-${ROOT}/apps/android/app/build/outputs/bundle/release/app-release.aab}"
: "${ANDROID_HOME:?ANDROID_HOME is required}"
: "${PICOO_ANDROID_SIGNER_SHA256:?PICOO_ANDROID_SIGNER_SHA256 is required}"
: "${PICOO_BUILD_NUMBER:?PICOO_BUILD_NUMBER is required}"

fail() {
  printf '[android-release] ERROR: %s\n' "$*" >&2
  exit 1
}

[[ -f "${APK}" ]] || fail "APK missing: ${APK}"
[[ -f "${AAB}" ]] || fail "AAB missing: ${AAB}"
APKSIGNER="${ANDROID_HOME}/build-tools/${PICOO_ANDROID_BUILD_TOOLS}/apksigner"
APKANALYZER="${ANDROID_HOME}/cmdline-tools/latest/bin/apkanalyzer"
[[ -x "${APKSIGNER}" ]] || fail "apksigner missing: ${APKSIGNER}"
[[ -x "${APKANALYZER}" ]] || fail "apkanalyzer missing: ${APKANALYZER}"

normalize_fingerprint() {
  tr '[:upper:]' '[:lower:]' | tr -d ':[:space:]'
}

"${APKSIGNER}" verify --verbose --print-certs "${APK}" > "${RUNNER_TEMP:-/tmp}/picoo-apksigner.txt"
actual_apk_fingerprint="$(
  sed -n 's/^Signer #1 certificate SHA-256 digest: //p' "${RUNNER_TEMP:-/tmp}/picoo-apksigner.txt" \
    | head -n 1 \
    | normalize_fingerprint
)"
expected_fingerprint="$(printf '%s' "${PICOO_ANDROID_SIGNER_SHA256}" | normalize_fingerprint)"
[[ "${expected_fingerprint}" =~ ^[0-9a-f]{64}$ ]] \
  || fail "PICOO_ANDROID_SIGNER_SHA256 must be exactly 32 SHA-256 bytes"
[[ -n "${actual_apk_fingerprint}" ]] || fail "APK signer SHA-256 was not reported"
apk_signer_count="$(sed -n 's/^Signer #[0-9][0-9]* certificate SHA-256 digest: //p' "${RUNNER_TEMP:-/tmp}/picoo-apksigner.txt" | wc -l | tr -d '[:space:]')"
[[ "${apk_signer_count}" = "1" ]] || fail "APK must have exactly one release signer"
[[ "${actual_apk_fingerprint}" =~ ^[0-9a-f]{64}$ ]] || fail "APK signer digest is malformed"
[[ "${actual_apk_fingerprint}" = "${expected_fingerprint}" ]] \
  || fail "APK signer fingerprint does not match the protected release identity"

jarsigner -verify "${AAB}" >/dev/null
actual_aab_fingerprint="$(
  keytool -J-Duser.language=en -J-Duser.country=US -printcert -jarfile "${AAB}" \
    | sed -n 's/^[[:space:]]*SHA256:[[:space:]]*//p' \
    | head -n 1 \
    | normalize_fingerprint
)"
[[ -n "${actual_aab_fingerprint}" ]] || fail "AAB signer SHA-256 was not reported"
[[ "${actual_aab_fingerprint}" =~ ^[0-9a-f]{64}$ ]] || fail "AAB signer digest is malformed"
[[ "${actual_aab_fingerprint}" = "${expected_fingerprint}" ]] \
  || fail "AAB signer fingerprint does not match the protected release identity"

workspace_version="$(sed -n 's/^version = "\([0-9][0-9]*\.[0-9][0-9]*\.[0-9][0-9]*\)"$/\1/p' "${ROOT}/Cargo.toml" | head -n 1)"
application_id="$("${APKANALYZER}" manifest application-id "${APK}")"
version_name="$("${APKANALYZER}" manifest version-name "${APK}")"
version_code="$("${APKANALYZER}" manifest version-code "${APK}")"
[[ "${application_id}" = "com.picoo.camera" ]] || fail "unexpected application ID: ${application_id}"
[[ "${version_name}" = "${workspace_version}" ]] || fail "versionName ${version_name} != ${workspace_version}"
[[ "${version_code}" = "${PICOO_BUILD_NUMBER}" ]] || fail "versionCode ${version_code} != ${PICOO_BUILD_NUMBER}"

printf '[android-release] verified app=%s version=%s (%s) signer=%s\n' \
  "${application_id}" "${version_name}" "${version_code}" "${expected_fingerprint}"
