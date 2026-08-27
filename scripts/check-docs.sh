#!/usr/bin/env bash
# 校验 README 与 Design Specs 中的内部交叉引用链接是否可解析。
#
# 该仓库的核心资产是可追溯的 Design Specs：BUC / PUC / ARCH 之间通过相对
# Markdown 链接互相引用。这个脚本用 lychee 在离线模式下校验这些相对链接，
# 确保追溯链不因文件移动或改名而断裂。
#
# 用法：scripts/check-docs.sh
set -euo pipefail

cd "$(dirname "$0")/.."

if ! command -v lychee >/dev/null 2>&1; then
  echo "错误：未找到 lychee，请先运行 .cursor/install.sh" >&2
  exit 127
fi

exec lychee \
  --offline \
  --no-progress \
  README.md \
  './docs/**/*.md'
