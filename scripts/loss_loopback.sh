#!/usr/bin/env bash
# Paired loopback under configurable video datagram loss — PRD §21 / REQ-PICOO-SESSION-006.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
export LOSS_RATIO="${LOSS_RATIO:-0.05}"
echo "Running 5% loss resilience test (LOSS_RATIO=${LOSS_RATIO})…"
cargo test -p picoo-receiver paired_loopback_remains_usable_under_five_percent_loss -- --nocapture
