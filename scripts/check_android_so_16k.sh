#!/usr/bin/env bash
# Verify packaged arm64 .so PT_LOAD alignment ≥ 16 KiB (Android 15 / Xiaomi 15).
# Usage:
#   bash scripts/check_android_so_16k.sh [path/to/app.apk]
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APK="${1:-}"
if [[ -z "${APK}" ]]; then
  for cand in \
    "${ROOT}/apps/android/app/build/outputs/apk/release/app-release.apk" \
    "${ROOT}/apps/android/app/build/outputs/apk/debug/app-debug.apk"
  do
    if [[ -f "${cand}" ]]; then
      APK="${cand}"
      break
    fi
  done
fi

if [[ -z "${APK}" || ! -f "${APK}" ]]; then
  echo "check_android_so_16k: no APK found (pass path or build first)" >&2
  exit 1
fi

python3 - "${APK}" <<'PY'
import struct, sys, zipfile, tempfile, os

apk = sys.argv[1]
MIN_ALIGN = 16384
# Cold-start libs we ship/control. CameraX util may still be 4K until dependency bumps.
REQUIRED = {
    "libpicoo_ffi.so",
    "libpicoo_jni.so",
}
# Fail any arm64 .so with <16KiB LOAD align (CameraX / ML Kit included).
REQUIRE_ALL_ARM64 = True

def load_aligns(path: str):
    with open(path, "rb") as f:
        data = f.read(64)
        if data[:4] != b"\x7fELF":
            return []
        e_phoff = struct.unpack_from("<Q", data, 32)[0]
        e_phentsize, e_phnum = struct.unpack_from("<HH", data, 54)
        f.seek(e_phoff)
        ph = f.read(e_phentsize * e_phnum)
    aligns = []
    for i in range(e_phnum):
        off = i * e_phentsize
        p_type = struct.unpack_from("<I", ph, off)[0]
        if p_type != 1:
            continue
        p_align = struct.unpack_from("<Q", ph, off + 48)[0]
        aligns.append(p_align)
    return aligns

td = tempfile.mkdtemp(prefix="picoo-16k-")
fail = 0
checked = 0
with zipfile.ZipFile(apk) as z:
    for name in z.namelist():
        if not (name.startswith("lib/arm64-v8a/") and name.endswith(".so")):
            continue
        base = os.path.basename(name)
        out = os.path.join(td, base)
        with open(out, "wb") as f:
            f.write(z.read(name))
        aligns = load_aligns(out)
        ok = bool(aligns) and all(a >= MIN_ALIGN for a in aligns)
        tag = "OK" if ok else "FAIL"
        print(f"{tag} {base} PT_LOAD={aligns}")
        checked += 1
        if not ok and (REQUIRE_ALL_ARM64 or base in REQUIRED):
            fail += 1
        if base.startswith("libquiche-"):
            print(f"WARN unexpected orphan {base} in APK")
            fail += 1

if checked == 0:
    print("no arm64-v8a .so in APK", file=sys.stderr)
    sys.exit(1)
if fail:
    print(f"check_android_so_16k: {fail} required library alignment failure(s)", file=sys.stderr)
    sys.exit(1)
print(f"ok: {apk} ({checked} shared objects scanned)")
PY
