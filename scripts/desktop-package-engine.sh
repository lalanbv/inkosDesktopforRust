#!/usr/bin/env bash
# 组装 inkos engine（src-tauri/engine/）：inkos 自包含运行时。
# 零修改 inkos 源码；只读 packages/ 与 node_modules。
#
# 产物布局（dev；prod 由 M3e CI 把 node_modules 符号链接换为真实自包含）：
#   src-tauri/engine/
#   ├── dist/            = packages/cli/dist 副本（入口 dist/index.js）
#   ├── node_modules     -> 仓库根 node_modules（符号链接；dev 解析用）
#   └── manifest.json    = EngineManifest（engine_version/node_min/built_at）
#
# cliPackageRoot（studio.ts 经 import.meta.url 算 = launch_engine_dir）的
# `node_modules/@actalk/inkos-studio` 候选经符号链接命中 repo node_modules（pnpm）。
# ESM `import '@actalk/inkos-core'` 经 node 向上解析亦命中。
set -euo pipefail

# CI=true：pnpm 在无 TTY（CI/脚本）下默认就绪，跳过 modules 清理确认等交互提示。
export CI=true

ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"

ENGINE="$ROOT/src-tauri/engine"

# 1) 构建 inkos（core + cli + studio dist）——仅当产物缺失时。
# 条件化：仓库已构建（如曾跑 desktop-build-inkos.sh）则跳过，避免 pnpm install
# 在 inkos 上游 pnpm.overrides 弃用（v10+）下触发 lockfile mismatch（M3e CI 另解）。
echo "[engine] 检查 inkos 构建产物 ..."
if [ -f packages/cli/dist/index.js ] && [ -f packages/studio/dist/index.html ]; then
  echo "[engine] packages/*/dist 已存在，跳过构建"
else
  echo "[engine] 安装依赖 + 构建 inkos ..."
  pnpm install --frozen-lockfile
  pnpm build
fi

# 2) 校验关键构建产物存在。
test -f packages/cli/dist/index.js     || { echo "FAIL: packages/cli/dist/index.js 缺失"; exit 1; }
test -f packages/studio/dist/index.html || { echo "FAIL: packages/studio/dist/index.html 缺失"; exit 1; }

# 3) 清理 + 重组装 engine/。
echo "[engine] 组装 $ENGINE ..."
rm -rf "$ENGINE"
mkdir -p "$ENGINE"

# cli dist + package.json → engine/（镜像 packages/cli 结构：dist + package.json +
# node_modules）。使 `node engine/dist/index.js` 与 `node packages/cli/dist/index.js`
# 解析路径等价（cliPackageRoot = engine，与 packages/cli 同构）。
cp -R packages/cli/dist "$ENGINE/dist"
cp packages/cli/package.json "$ENGINE/package.json"

# node_modules 链接/拷贝：
# - dev（默认）：符号链接 → packages/cli/node_modules（pnpm 按包隔离依赖在此）。
# - prod（INKOS_ENGINE_PROD=1，CI 用）：cp -L 解引用拷贝真实内容（自包含，无符号链接，
#   打包进 .app 后在用户机可用；体积大但正确，Phase 2 SEA 优化）。
LN_TARGET="$ROOT/packages/cli/node_modules"
if [ -d "$LN_TARGET" ]; then
  if [ "${INKOS_ENGINE_PROD:-0}" = "1" ]; then
    echo "[engine] PROD 模式：cp -L 拷贝自包含 node_modules（可能数分钟）..."
    mkdir -p "$ENGINE/node_modules"
    # cp -L 解引用 pnpm 的符号链接/硬链接，产出真实自包含 node_modules。
    cp -RL "$LN_TARGET/." "$ENGINE/node_modules/"
  else
    ln -sfn "$LN_TARGET" "$ENGINE/node_modules"
  fi
else
  echo "WARN: $LN_TARGET 不存在，跳过 node_modules（CLI 依赖无法解析）"
  echo "      先运行 ./scripts/desktop-build-inkos.sh 安装依赖"
fi

# 4) 写 manifest.json（EngineManifest）。
ENGINE_VERSION="$(node -p "require('./packages/cli/package.json').version")"
NODE_MIN="22.0.0"
BUILT_AT="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
# engine_sha256：本地组装备注为空；M3e 发布 CI 打包 engine tarball 后回填真实 SHA256
# （updater 下载校验用）。EngineManifest.engine_sha256 为必填字符串，空串合法。
cat > "$ENGINE/manifest.json" <<EOF
{
  "engine_version": "$ENGINE_VERSION",
  "engine_sha256": "",
  "node_min": "$NODE_MIN",
  "built_at": "$BUILT_AT"
}
EOF

# 5) 终态校验。
test -f "$ENGINE/dist/index.js" || { echo "FAIL: $ENGINE/dist/index.js 缺失"; exit 1; }
test -f "$ENGINE/manifest.json" || { echo "FAIL: manifest.json 缺失"; exit 1; }

echo "OK: engine 组装完成 ($ENGINE)"
echo "    engine_version=$ENGINE_VERSION  node_min=$NODE_MIN  built_at=$BUILT_AT"
echo "    验证：node $ENGINE/dist/index.js studio --port 4567"
