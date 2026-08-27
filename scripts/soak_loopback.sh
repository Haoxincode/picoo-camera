#!/usr/bin/env bash
# REQ-PICOO-SESSION-005 soak helper — paired loopback FrameHub stress.
#
# Default: 60s smoke soak (CI-friendly). For PRD §21 2h:
#   SOAK_SECONDS=7200 ./scripts/soak_loopback.sh
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

SOAK_SECONDS="${SOAK_SECONDS:-60}"
SAMPLE_EVERY="${SAMPLE_EVERY:-5}"

echo "soak_loopback: duration=${SOAK_SECONDS}s sample_every=${SAMPLE_EVERY}s"
echo "Building paired loopback binary harness via cargo test (filter)…"

# Run a dedicated soak test that loops until env deadline.
export PICOO_SOAK_SECONDS="$SOAK_SECONDS"
export PICOO_SOAK_SAMPLE_EVERY="$SAMPLE_EVERY"

cargo test -p picoo-receiver --lib soak_paired_loopback_memory_stable -- --ignored --nocapture 2>&1 |
  tee /tmp/picoo-soak-loopback.log

echo "soak finished; log: /tmp/picoo-soak-loopback.log"
