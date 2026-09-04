#!/usr/bin/env bash
# Human-driven Android→Windows V1 E2E checklist printer.
# Does NOT talk to devices; guides operators through device-e2e-android-win11.md.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DOC="$ROOT/docs/design-specs/verification/device-e2e-android-win11.md"
VCAM="$ROOT/docs/design-specs/verification/vcam-meeting-apps.md"
CI="$ROOT/docs/design-specs/verification/ci-artifacts.md"

echo "=== Picoo Camera · Android→Windows device E2E ==="
echo "Docs:"
echo "  - $DOC"
echo "  - $VCAM"
echo "  - $CI"
echo
echo "0) Download android-signed-release from a protected release run and windows-msi from tip green CI."
echo "1) Win11 admin: install PicooCamera.msi; confirm system camera lists Picoo Camera."
echo "2) Android 10+ ARM64: install APK; grant Camera / Nearby Wi-Fi / Notifications."
echo "3) Same LAN: mDNS discovery or manual IP endpoint; enter the Receiver's 6-digit code."
echo "4) Streaming: desktop preview + VCam; flip / 480·720·1080 / mirror / thermal toast."
echo "5) Disconnect Wi-Fi 10s → reconnect backoff → IDR recovery; placeholder on stop."
echo "6) Paired list delete both ends; re-pair required."
echo "7) Meeting apps matrix: $VCAM (Zoom/Teams/腾讯/OBS/browser)."
echo "8) Optional soak: SESSION-005 2h @1080p30; SESSION-007 E2E latency sample."
echo
echo "Record evidence under docs/design-specs/verification/artifacts/ (gitignored if configured)."
echo "CI cannot close REQ-PICOO-VCAM-005 — complete the meeting-apps checklist on Win11."
