#!/usr/bin/env bash
# Lightweight architecture lint: Android must not recreate the Rust-owned TXT
# key schema. JNI behavior remains covered by Rust/JVM tests, not this grep.
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

if grep -Eq 'ALLOWED_(TXT_)?KEYS|setOf\([^)]*receiver_id' "$KT"; then
  echo "Android must not duplicate the Rust discovery TXT schema: $KT"
  exit 1
fi
if ! grep -Fq 'PicooNative.parseDiscoveryTxt' "$KT"; then
  echo "Android DiscoveryTxt must delegate validation to Rust: $KT"
  exit 1
fi
echo "ok architecture lint: discovery TXT schema remains Rust-owned:"
echo "$rust_keys" | sed 's/^/  /'
