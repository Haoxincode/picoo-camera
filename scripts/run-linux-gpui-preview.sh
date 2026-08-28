#!/usr/bin/env bash
# REQ-PICOO-UI-010: start the Linux GPUI preview host under Xvfb when needed.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

BIN="${PICOO_DESKTOP_BIN:-$ROOT/target/release/picoo-desktop}"
if [[ ! -x "$BIN" ]]; then
  BIN="$ROOT/target/debug/picoo-desktop"
fi
if [[ ! -x "$BIN" ]]; then
  echo "error: picoo-desktop not built. Run: cargo xtask build linux" >&2
  exit 1
fi

export PICOO_PREFS="${PICOO_PREFS:-/tmp/picoo-preview/prefs.json}"
mkdir -p "$(dirname "$PICOO_PREFS")"

if [[ -z "${DISPLAY:-}" ]]; then
  exec xvfb-run -a --server-args="-screen 0 1280x800x24" "$BIN" --gpui "$@"
fi

exec "$BIN" --gpui "$@"
