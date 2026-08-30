#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
svg_dir="${repo_root}/assets/icons/reicon"
android_drawable_dir="${repo_root}/apps/android/app/src/main/res/drawable"
ios_asset_dir="${repo_root}/apps/ios/PicooCamera/Assets.xcassets"
manifest_path="${svg_dir}/manifest.json"
android_adapter="${repo_root}/apps/android/app/src/main/kotlin/com/picoo/camera/ui/components/Reicon.kt"
ios_adapter="${repo_root}/apps/ios/PicooCamera/PicooDesignSystem.swift"

test -f "${svg_dir}/README.md"
test -f "${svg_dir}/LICENSE"
test -f "${manifest_path}"

command -v jq >/dev/null
jq -e '.schemaVersion == 1 and .upstream.grid == 24 and (.icons | length > 0)' \
  "${manifest_path}" >/dev/null

svg_count=0
for svg_path in "${svg_dir}"/*.svg; do
  svg_count=$((svg_count + 1))
  grep -Fq 'viewBox="0 0 24 24"' "${svg_path}" || {
    echo "Reicon SVG must use the 24x24 grid: ${svg_path}" >&2
    exit 1
  }
done

semantic_count="$(jq '.icons | length' "${manifest_path}")"
while IFS= read -r icon_name; do
  svg_path="${svg_dir}/${icon_name}.svg"
  android_path="${android_drawable_dir}/reicon_${icon_name}.xml"
  ios_imageset="${ios_asset_dir}/reicon_${icon_name}.imageset"

  test -f "${svg_path}" || {
    echo "Semantic Reicon has no shared SVG source: ${icon_name}" >&2
    exit 1
  }
  test -f "${android_path}" || {
    echo "Semantic Reicon has no Android adapter: ${icon_name}" >&2
    exit 1
  }
  test -f "${ios_imageset}/Contents.json" || {
    echo "Semantic Reicon has no iOS adapter: ${icon_name}" >&2
    exit 1
  }
  ios_filename="$(jq -r '.images[0].filename // empty' "${ios_imageset}/Contents.json")"
  test -n "${ios_filename}" && test -f "${ios_imageset}/${ios_filename}" || {
    echo "iOS Reicon adapter has no image payload: ${icon_name}" >&2
    exit 1
  }
done < <(jq -r '.icons[]' "${manifest_path}" | sort -u)

# File existence only proves that a glyph was copied. Also verify that each product semantic key
# is exposed by both typed platform adapters and maps to the manifest's pinned source glyph.
while IFS=$'\t' read -r semantic_name icon_name; do
  first_letter="$(printf '%s' "${semantic_name:0:1}" | tr '[:lower:]' '[:upper:]')"
  kotlin_name="${first_letter}${semantic_name:1}"

  grep -Fq "${kotlin_name}(R.drawable.reicon_${icon_name})" "${android_adapter}" || {
    echo "Android Reicon semantic mapping is missing or incorrect: ${semantic_name} -> ${icon_name}" >&2
    exit 1
  }
  grep -Fq "case ${semantic_name}" "${ios_adapter}" || {
    echo "iOS Reicon semantic case is missing: ${semantic_name}" >&2
    exit 1
  }
  grep -Fq "case .${semantic_name}: \"reicon_${icon_name}\"" "${ios_adapter}" || {
    echo "iOS Reicon semantic mapping is missing or incorrect: ${semantic_name} -> ${icon_name}" >&2
    exit 1
  }
done < <(jq -r '.icons | to_entries[] | [.key, .value] | @tsv' "${manifest_path}")

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

echo "Verified ${semantic_count} semantic icons, ${svg_count} shared Reicon SVGs, ${android_count} Android adapters, and iOS Image Sets."
