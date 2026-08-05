#!/usr/bin/env bash
# 在 inkos fork 仓库根构建（mono-repo）。零修改 inkos 源码。
set -euo pipefail
ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"
echo "Installing deps ..."
pnpm install --frozen-lockfile
echo "Building inkos (core + cli + studio) ..."
pnpm build
test -f packages/cli/dist/index.js     || { echo "cli dist missing"; exit 1; }
test -f packages/studio/dist/index.html || { echo "studio SPA dist missing"; exit 1; }
echo "OK: inkos built in place (repo root)."
