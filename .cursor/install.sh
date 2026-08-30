#!/usr/bin/env bash
# Cloud Agent 环境安装脚本：为 picoo-camera 准备开发工具链。
#
# 基础镜像已内置 Rust / Cargo / rustup，因此这里只需：
#   1. 确认 Rust 工具链可用并补齐常用组件（rustfmt、clippy）。
#   2. 安装 lychee 链接校验器，用于校验 Design Specs 的追溯交叉引用。
#   3. Android job 按需安装 SDK/NDK 与 cargo-ndk。
#
# 该脚本必须是幂等的：可重复运行且不会追加状态或改写锁文件。
set -euo pipefail

LYCHEE_VERSION="0.24.2"
LYCHEE_URL="https://github.com/lycheeverse/lychee/releases/download/lychee-v${LYCHEE_VERSION}/lychee-x86_64-unknown-linux-musl.tar.gz"
# shellcheck source=../apps/android/toolchain.properties
source "$(dirname "$0")/../apps/android/toolchain.properties"
CARGO_NDK_VERSION="${PICOO_CARGO_NDK_VERSION}"
BIN_DIR="/usr/local/bin"

log() { printf '\n[install] %s\n' "$*"; }

install_lychee() {
  if command -v lychee >/dev/null 2>&1; then
    local current
    current="$(lychee --version 2>/dev/null | awk '{print $NF}')"
    if [ "${current}" = "${LYCHEE_VERSION}" ]; then
      log "lychee ${LYCHEE_VERSION} 已安装，跳过"
      return 0
    fi
  fi
  log "安装 lychee ${LYCHEE_VERSION}"
  local tmp
  tmp="$(mktemp -d)"
  trap 'rm -rf "${tmp}"' RETURN
  curl -fsSL "${LYCHEE_URL}" -o "${tmp}/lychee.tar.gz"
  tar -xzf "${tmp}/lychee.tar.gz" -C "${tmp}" --strip-components=1 \
    "lychee-x86_64-unknown-linux-musl/lychee"
  sudo install -m 0755 "${tmp}/lychee" "${BIN_DIR}/lychee"
  log "lychee 安装到 ${BIN_DIR}/lychee"
}

log "Rust 工具链"
rustc --version
cargo --version
if command -v rustup >/dev/null 2>&1; then
  rustup component add rustfmt clippy >/dev/null 2>&1 || log "rustfmt/clippy 组件补齐失败（可忽略）"
fi

install_lychee

if [ "${PICOO_INSTALL_ANDROID:-0}" = "1" ]; then
  if ! command -v unzip >/dev/null 2>&1; then
    log "安装 unzip（Android SDK 需要）"
    sudo apt-get update -qq
    sudo apt-get install -y -qq unzip
  fi
  # shellcheck source=/dev/null
  source "$(dirname "$0")/../scripts/setup-android-sdk.sh"
  if [ "$(cargo ndk --version 2>/dev/null || true)" != "cargo-ndk ${CARGO_NDK_VERSION}" ]; then
    log "安装 cargo-ndk ${CARGO_NDK_VERSION}"
    cargo install cargo-ndk --version "${CARGO_NDK_VERSION}" --locked
  fi
  if command -v rustup >/dev/null 2>&1; then
    rustup target add aarch64-linux-android >/dev/null 2>&1 || true
  fi
fi

log "工具链版本汇总"
rustc --version
cargo --version
lychee --version

log "完成"
