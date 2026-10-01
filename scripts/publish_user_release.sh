#!/usr/bin/env bash
# Publish a GitHub Release that contains only packages an ordinary user can install.
#
# Actions artifacts stay complete for development, verification, SBOMs, and
# provenance. This script is the filter for the Release page.
#
#   scripts/publish_user_release.sh --self-check
#   scripts/publish_user_release.sh --ci-run 36280746371 --tag v0.1.649 --notes-file notes.md
#   scripts/publish_user_release.sh --ci-run <id> --tag vX.Y.Z --dry-run
#
# A signed package replaces the unsigned package for the same platform.
# macOS is included only from the notarized release artifact. iOS packages,
# the Windows loose bundle, AAB, and SBOM files are never Release assets.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"

# name kind slot priority inner-file release-suffix
# drop rows: name drop reason...
artifact_table() {
  cat << 'EOF'
android-apk-debug user android 1 app-debug.apk Android-debug.apk
windows-msi user windows 1 PicooCamera.msi Windows-unsigned.msi
android-signed-release user android 2 app-release.apk Android.apk
windows-signed-release user windows 2 PicooCamera.msi Windows.msi
macos-signed-notarized-release user macos 2 PicooCamera-macOS.zip macOS.zip
windows-bundle drop 开发态松散文件，安装使用 MSI
windows-recording-fixtures drop 录制验证样本
windows-mux-probe drop mux 探针
macos-app-unsigned drop 未签名，不能激活 Camera Extension
ios-rust-core-xcframework drop 开发库
apple-recording-fixtures drop 录制验证样本
ios-app-unsigned drop 模拟器包，不能安装到 iPhone
ios-signed-app-store-release drop IPA 交给 App Store Connect，用户不从 Release 下载
fuzz-artifacts drop 夜间模糊测试样本
EOF
}

artifact_kind() {
  local name="$1"
  local line
  line="$(artifact_table | awk -v name="$name" '$1 == name { print; exit }')"
  if [[ -n "$line" ]]; then
    awk '{ print $2 }' <<<"$line"
    return
  fi
  if [[ "$name" == windows-vcam-host-evidence-* ]]; then
    echo "drop"
    return
  fi
  echo "unknown"
}

# Print one KEEP line per selected platform: release-name, artifact, inner file.
# Higher priority wins inside a slot, so a signed package replaces the unsigned one.
select_release_files() {
  local tag="$1"
  shift
  local available=("$@")
  local slot name kind row_slot priority inner suffix present item
  local best_priority best_artifact best_inner best_suffix
  for slot in android windows macos; do
    best_priority=0
    best_artifact=""
    best_inner=""
    best_suffix=""
    while read -r name kind row_slot priority inner suffix; do
      [[ "$row_slot" == "$slot" ]] || continue
      present=0
      for item in "${available[@]}"; do
        if [[ "$item" == "$name" ]]; then
          present=1
        fi
      done
      if [[ "$present" -eq 1 && "$priority" -gt "$best_priority" ]]; then
        best_priority="$priority"
        best_artifact="$name"
        best_inner="$inner"
        best_suffix="$suffix"
      fi
    done < <(artifact_table | awk '$2 == "user" { print $1, $2, $3, $4, $5, $6 }')
    if [[ -n "$best_suffix" ]]; then
      printf 'KEEP PicooCamera-%s-%s %s %s\n' \
        "$tag" "$best_suffix" "$best_artifact" "$best_inner"
    fi
  done
}

workflow_artifact_names() {
  awk '
    /uses: actions\/upload-artifact/ { grab = 1; next }
    grab && $1 == "name:" {
      name = $0
      sub(/^[[:space:]]*name:[[:space:]]*/, "", name)
      gsub(/["'\'']/, "", name)
      print name
      grab = 0
    }
  ' \
    "$ROOT/.github/workflows/ci.yml" \
    "$ROOT/.github/workflows/nightly-validation.yml" \
    "$ROOT/.github/workflows/release-android.yml" \
    "$ROOT/.github/workflows/release-windows.yml" \
    "$ROOT/.github/workflows/release-apple.yml" \
    "$ROOT/.github/workflows/windows-vcam-host.yml"
}

self_check() {
  local fail=0
  local names name kind
  names="$(workflow_artifact_names | sort -u)"
  if [[ -z "$names" ]]; then
    echo "no upload-artifact names found" >&2
    exit 1
  fi
  while IFS= read -r name; do
    kind="$(artifact_kind "$name")"
    if [[ "$kind" == "unknown" ]]; then
      echo "unclassified artifact: $name" >&2
      fail=1
    else
      echo "ok: $name -> $kind"
    fi
  done <<<"$names"
  while IFS= read -r name; do
    if ! grep -qx "$name" <<<"$names"; then
      echo "table entry is not uploaded by a workflow: $name" >&2
      fail=1
    fi
  done < <(artifact_table | awk '{ print $1 }')
  if ! grep -q '^windows-vcam-host-evidence-' <<<"$names"; then
    echo "windows-vcam-host evidence artifact is missing from the workflows" >&2
    fail=1
  fi

  local ci_names=(
    android-apk-debug
    windows-msi
    windows-bundle
    windows-recording-fixtures
    windows-mux-probe
    macos-app-unsigned
    ios-rust-core-xcframework
    apple-recording-fixtures
    ios-app-unsigned
  )
  local got expect
  got="$(select_release_files v0.1.649 "${ci_names[@]}")"
  expect=$'KEEP PicooCamera-v0.1.649-Android-debug.apk android-apk-debug app-debug.apk\nKEEP PicooCamera-v0.1.649-Windows-unsigned.msi windows-msi PicooCamera.msi'
  if [[ "$got" != "$expect" ]]; then
    echo "CI selection mismatch" >&2
    printf 'got:\n%s\n' "$got" >&2
    fail=1
  else
    echo "ok: unsigned CI release keeps the Android APK and Windows MSI"
  fi

  got="$(select_release_files v0.2.0 \
    android-apk-debug android-signed-release \
    windows-msi windows-signed-release \
    macos-app-unsigned macos-signed-notarized-release \
    ios-app-unsigned ios-signed-app-store-release ios-rust-core-xcframework)"
  expect=$'KEEP PicooCamera-v0.2.0-Android.apk android-signed-release app-release.apk\nKEEP PicooCamera-v0.2.0-Windows.msi windows-signed-release PicooCamera.msi\nKEEP PicooCamera-v0.2.0-macOS.zip macos-signed-notarized-release PicooCamera-macOS.zip'
  if [[ "$got" != "$expect" ]]; then
    echo "signed selection mismatch" >&2
    printf 'got:\n%s\n' "$got" >&2
    fail=1
  else
    echo "ok: signed packages replace unsigned ones, and iOS stays off the Release"
  fi

  if (( fail )); then
    exit 1
  fi
}

usage() {
  cat << 'EOF'
usage: scripts/publish_user_release.sh --self-check
       scripts/publish_user_release.sh --ci-run <id> --tag vX.Y.Z [--notes-file FILE] [--dry-run]
              [--android-signed-run <id>] [--windows-signed-run <id>] [--macos-signed-run <id>]
EOF
}

run_artifact_names() {
  local run_id="$1"
  gh api "repos/${REPO}/actions/runs/${run_id}/artifacts" --paginate \
    --jq '.artifacts[].name'
}

run_sha() {
  local run_id="$1"
  gh api "repos/${REPO}/actions/runs/${run_id}" --jq '.head_sha'
}

download_artifact() {
  local run_id="$1"
  local name="$2"
  local dest="$3"
  gh run download "$run_id" --repo "$REPO" --name "$name" --dir "$dest"
}

publish() {
  local ci_run="" tag="" notes_file="" dry_run=0
  local android_signed_run="" windows_signed_run="" macos_signed_run=""
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --ci-run) ci_run="$2"; shift 2 ;;
      --tag) tag="$2"; shift 2 ;;
      --notes-file) notes_file="$2"; shift 2 ;;
      --dry-run) dry_run=1; shift ;;
      --android-signed-run) android_signed_run="$2"; shift 2 ;;
      --windows-signed-run) windows_signed_run="$2"; shift 2 ;;
      --macos-signed-run) macos_signed_run="$2"; shift 2 ;;
      *) usage >&2; exit 2 ;;
    esac
  done
  if [[ -z "$ci_run" || -z "$tag" ]]; then
    usage >&2
    exit 2
  fi
  if [[ ! "$tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "tag must look like v0.1.649" >&2
    exit 2
  fi
  if [[ -n "$notes_file" && ! -f "$notes_file" ]]; then
    echo "notes file not found: $notes_file" >&2
    exit 2
  fi

  REPO="$(gh repo view --json nameWithOwner --jq '.nameWithOwner')"
  local sha available=() run_id
  sha="$(run_sha "$ci_run")"
  while IFS= read -r name; do
    available+=("$name")
  done < <(run_artifact_names "$ci_run")

  local slot signed_id signed_sha
  for slot in android windows macos; do
    case "$slot" in
      android) signed_id="$android_signed_run" ;;
      windows) signed_id="$windows_signed_run" ;;
      macos) signed_id="$macos_signed_run" ;;
    esac
    [[ -n "$signed_id" ]] || continue
    signed_sha="$(run_sha "$signed_id")"
    if [[ "$signed_sha" != "$sha" ]]; then
      echo "$slot signed run $signed_id is $signed_sha, CI run is $sha" >&2
      exit 1
    fi
    while IFS= read -r name; do
      available+=("$name")
    done < <(run_artifact_names "$signed_id")
  done

  local plan
  plan="$(select_release_files "$tag" "${available[@]}")"
  if [[ -z "$plan" ]]; then
    echo "no user-installable packages on the selected runs" >&2
    exit 1
  fi
  echo "$plan"
  if grep -q 'Android-debug.apk\|Windows-unsigned.msi' <<<"$plan"; then
    echo "unsigned packages selected; the Release stays a prerelease"
  fi
  if (( dry_run )); then
    return
  fi

  if gh release view "$tag" --repo "$REPO" >/dev/null 2>&1; then
    echo "release $tag already exists; refusing to replace it" >&2
    exit 1
  fi

  local work asset_dir
  work="$(mktemp -d "${TMPDIR:-/tmp}/picoo-user-release.XXXXXX")"
  asset_dir="$work/assets"
  mkdir -p "$asset_dir"
  trap 'rm -rf "$work"' EXIT

  local release_name artifact inner src found
  while read -r _ release_name artifact inner; do
    if [[ "$artifact" == "android-signed-release" ]]; then
      run_id="$android_signed_run"
    elif [[ "$artifact" == "windows-signed-release" ]]; then
      run_id="$windows_signed_run"
    elif [[ "$artifact" == "macos-signed-notarized-release" ]]; then
      run_id="$macos_signed_run"
    else
      run_id="$ci_run"
    fi
    download_artifact "$run_id" "$artifact" "$work/raw-$artifact"
    found=()
    while IFS= read -r src; do
      found+=("$src")
    done < <(find "$work/raw-$artifact" -type f -name "$inner")
    if [[ "${#found[@]}" -ne 1 ]]; then
      echo "expected one $inner in $artifact, found ${#found[@]}" >&2
      exit 1
    fi
    cp "${found[0]}" "$asset_dir/$release_name"
  done <<<"$plan"

  (
    cd "$asset_dir"
    shasum -a 256 PicooCamera-* > SHA256SUMS.txt
  )
  local notes="$notes_file"
  if [[ -z "$notes" ]]; then
    notes="$work/notes.md"
    cat > "$notes" << EOF
预发布 ${tag}。只附上可以安装使用的包。

- 提交：\`${sha}\`
- 构建：https://github.com/${REPO}/actions/runs/${ci_run}

未签名的 macOS 包不能激活 Camera Extension，iOS 安装包不从这里下载。SHA256SUMS.txt 是所附安装包的校验值。
EOF
  fi

  local -a assets=()
  local path
  while IFS= read -r path; do
    assets+=("$path")
  done < <(find "$asset_dir" -type f | sort)
  gh release create "$tag" \
    --repo "$REPO" \
    --target "$sha" \
    --title "Picoo Camera ${tag}" \
    --prerelease \
    --notes-file "$notes" \
    "${assets[@]}"
}

case "${1:-}" in
  --self-check) self_check ;;
  -h|--help|"") usage ;;
  *) publish "$@" ;;
esac
