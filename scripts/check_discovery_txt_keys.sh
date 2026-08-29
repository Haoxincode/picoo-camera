#!/usr/bin/env bash
# Ensure Android DiscoveryTxt keys stay aligned with Rust ALLOWED_TXT_KEYS
# (REQ-PICOO-DISCOVERY-005).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RUST="$ROOT/crates/picoo-discovery/src/types.rs"
KT="$ROOT/apps/android/app/src/main/kotlin/com/picoo/camera/discovery/DiscoveryTxt.kt"

rust_keys=$(python3 - <<PY
import re
from pathlib import Path
text = Path("$RUST").read_text()
block = re.search(r"ALLOWED_TXT_KEYS[^=]*=\s*&\[[^\]]*\]", text, re.S).group(0)
print("\n".join(sorted(re.findall(r'"([^"]+)"', block))))
PY
)

kt_keys=$(python3 - <<PY
import re
from pathlib import Path
text = Path("$KT").read_text()
block = re.search(r"ALLOWED_KEYS[^=]*=\s*setOf\([^)]*\)", text, re.S).group(0)
print("\n".join(sorted(re.findall(r'"([^"]+)"', block))))
PY
)

if [[ "$rust_keys" != "$kt_keys" ]]; then
  echo "TXT key mismatch between Rust and Android:"
  echo "--- rust ---"
  echo "$rust_keys"
  echo "--- kotlin ---"
  echo "$kt_keys"
  exit 1
fi
echo "ok discovery TXT keys aligned:"
echo "$rust_keys" | sed 's/^/  /'
