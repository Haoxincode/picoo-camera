#!/usr/bin/env bash
# Compile+run VCam NV12 size policy tests on Linux (no MF required).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SRC="$ROOT/extensions/windows-virtual-camera/mf-source"
OUT="${TMPDIR:-/tmp}/picoo_test_vcam_format"
g++ -std=c++17 -Wall -Wextra -I"$SRC/include" \
  "$SRC/tests/test_vcam_format.cpp" -o "$OUT"
"$OUT"
