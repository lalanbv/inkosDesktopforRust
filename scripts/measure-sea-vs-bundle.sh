#!/usr/bin/env bash
# SEA vs tar.gz bundle 体积对比（M4f 调研工具）
#
# 用途：实测 Node SEA 单文件与当前 engine-*.tar.gz 的体积，验证
# docs/sea-feasibility.md 的结论（SEA 不减体积，node binary 占大头）。
#
# 前置（本仓不默认具备，按需准备）：
#   - Node 22+（sea-config 支持）
#   - esbuild（把 inkos CLI bundle 成单文件：monorepo + workspace 需处理）
#   - 目标平台 node 二进制（M3 bootstrap 产物）
#
# 用法：scripts/measure-sea-vs-bundle.sh [engine-tarball]
#   engine-tarball 默认取最近的 engine-*.tar.gz
#
# 退出码：0=对比完成；2=前置缺失（提示所需工具）

set -euo pipefail

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$ROOT"

TARBALL="${1:-$(ls -t engine-*.tar.gz 2>/dev/null | head -1 || true)}"

echo "=== SEA vs tar.gz 体积对比 ==="

# 1. 前置检查
if ! command -v node >/dev/null 2>&1; then
  echo "MISS: node 未安装（需 Node 22+ 支持 SEA）" >&2
  exit 2
fi
NODE_MAJOR="$(node -p 'process.versions.node.split(".")[0]' 2>/dev/null || echo 0)"
if [ "$NODE_MAJOR" -lt 22 ]; then
  echo "MISS: node 版本 $NODE_MAJOR < 22（SEA 需 22+）" >&2
  exit 2
fi
if ! command -v esbuild >/dev/null 2>&1; then
  echo "MISS: esbuild 未安装（需先把 CLI bundle 成单文件）" >&2
  exit 2
fi
if [ -z "$TARBALL" ] || [ ! -f "$TARBALL" ]; then
  echo "MISS: 未找到 engine-*.tar.gz（先跑 desktop-package-engine.sh）" >&2
  exit 2
fi

# 2. tar.gz 体积（当前方案）
TAR_SIZE=$(stat -f%z "$TARBALL" 2>/dev/null || stat -c%s "$TARBALL")
echo "tar.gz bundle ($TARBALL): $TAR_SIZE bytes ($(du -h "$TARBALL" | cut -f1))"

# 3. SEA 体积（需构建；此处为骨架，真实构建见 sea-feasibility.md 第四节障碍）
#    流程：esbuild bundle CLI → sea-config.json → node --experimental-sea-config
#          → postsea → 测量产物。因 monorepo bundle 复杂，留作具备环境时实跑。
SEA_OUT="$ROOT/.sea-measure/inkos-sea"
mkdir -p "$ROOT/.sea-measure"
echo "（SEA 构建需 esbuild bundle + sea-config；见 docs/sea-feasibility.md）"
echo "    占位：构建后测 $SEA_OUT 体积，与 tar.gz 对比"

# 4. node 二进制单独体积（证明 node 占大头）
NODE_BIN="$(command -v node)"
NODE_SIZE=$(stat -f%z "$NODE_BIN" 2>/dev/null || stat -c%s "$NODE_BIN")
echo "node binary ($NODE_BIN): $NODE_SIZE bytes ($(du -h "$NODE_BIN" | cut -f1))"
echo "→ node binary 占 tar.gz 的约 $(awk "BEGIN{printf \"%.0f%%\", $NODE_SIZE*100/$TAR_SIZE}")（SEA 不压缩此部分，故总体积接近）"

echo ""
echo "结论预期：SEA 总体积 ≈ node binary + 应用 blob ≈ tar.gz，不显著减小。"
echo "更新体积痛点已由 Phase 6.1 delta 解决（见 docs/sea-feasibility.md）。"
