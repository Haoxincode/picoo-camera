#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
svg_dir="${repo_root}/assets/icons/reicon"
android_drawable_dir="${repo_root}/apps/android/app/src/main/res/drawable"

test -f "${svg_dir}/README.md"
test -f "${svg_dir}/LICENSE"

svg_count=0
for svg_path in "${svg_dir}"/*.svg; do
  svg_count=$((svg_count + 1))
  grep -Fq 'viewBox="0 0 24 24"' "${svg_path}" || {
    echo "Reicon SVG must use the 24x24 grid: ${svg_path}" >&2
    exit 1
  }
done

android_count=0
for vector_path in "${android_drawable_dir}"/reicon_*.xml; do
  android_count=$((android_count + 1))
  icon_name="$(basename "${vector_path}" .xml)"
  icon_name="${icon_name#reicon_}"
  svg_path="${svg_dir}/${icon_name}.svg"

  test -f "${svg_path}" || {
    echo "Android Vector Drawable has no shared Reicon SVG source: ${vector_path}" >&2
    exit 1
  }

  grep -Fq 'android:viewportWidth="24"' "${vector_path}" || {
    echo "Android vector must use viewportWidth=24: ${vector_path}" >&2
    exit 1
  }
  grep -Fq 'android:viewportHeight="24"' "${vector_path}" || {
    echo "Android vector must use viewportHeight=24: ${vector_path}" >&2
    exit 1
  }
done

test "${svg_count}" -gt 0

if grep -R -n -E \
  'material-icons-extended|androidx\.compose\.material\.icons' \
  "${repo_root}/apps/android/app/build.gradle.kts" \
  "${repo_root}/apps/android/app/src/main/kotlin"; then
  echo "Android must use the local Reicon subset, not a complete icon dependency." >&2
  exit 1
fi

echo "Verified ${svg_count} shared Reicon SVGs and ${android_count} Android zero-dependency adapters."
