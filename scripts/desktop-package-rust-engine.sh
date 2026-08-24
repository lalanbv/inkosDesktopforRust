#!/usr/bin/env bash
# 组装桌壳 Rust 引擎资源目录 src-tauri/engine-rust/（绞杀者终切 164 号）。
#
# 布局（tauri.conf.json bundle.resources["engine-rust"] → resource_dir/engine-rust）：
#   src-tauri/engine-rust/
#   ├── inkos-engine-server   release 二进制（壳层 rustbin::resolve_server_bin 消费）
#   ├── static/               packages/studio/dist 副本（INKOS_STATIC_DIR 直连面）
#   └── manifest.json         engine_version/engine_sha256/rust_target/git_commit/built_at
#
# 前置：
#   1. ./scripts/desktop-build-inkos.sh 已跑（packages/studio/dist 为真实构建——
#      strangler_duel 的 34 字节桩会覆写 dist/index.html，duel 后必须重建）。
#   2. 本机 rustc 可用（host triple 自动探测）。
#
# manifest 键与 Node engine bundle（desktop-package-engine.sh）及 Rust 全量包
# （package-rust-engine.sh）同约定——未来 Rust 引擎 updater 的校验路径同构。
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
ENGINE_RS="$ROOT/engine-rs"
DEST="$ROOT/src-tauri/engine-rust"

VERSION="$(sed -n 's/^version = "\(.*\)"$/\1/p' "$ENGINE_RS/Cargo.toml" | head -1)"
TRIPLE="$(rustc -vV | sed -n 's/^host: //p')"
COMMIT="$(git rev-parse --short HEAD)"
BUILT_AT="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

# 1) release 构建（lib + bin inkos-engine-server）。
echo "[engine-rust] cargo build --release ..."
(cd "$ENGINE_RS" && cargo build --release)
BIN="$ENGINE_RS/target/release/inkos-engine-server"
test -x "$BIN" || { echo "FAIL: $BIN 缺失"; exit 1; }

# 2) 组装（清旧全量重建——脚本幂等）。
rm -rf "$DEST"
mkdir -p "$DEST/static"
cp "$BIN" "$DEST/inkos-engine-server"

if [ -f "$ROOT/packages/studio/dist/index.html" ]; then
  # 一致性闸门：duel 桩（34 字节 index）不得混入发布资源。
  SIZE="$(stat -f%z "$ROOT/packages/studio/dist/index.html" 2>/dev/null || stat -c%s "$ROOT/packages/studio/dist/index.html")"
  if [ "$SIZE" -le 64 ]; then
    echo "FAIL: packages/studio/dist/index.html 仅 ${SIZE}B（疑为 strangler_duel 桩）——先跑 desktop-build-inkos.sh 重建" >&2
    exit 1
  fi
  cp -R "$ROOT/packages/studio/dist/." "$DEST/static/"
else
  echo "WARN: packages/studio/dist 缺失——纯 API 资源（webview 导航将 404）"
  rmdir "$DEST/static"
fi

# 3) manifest（二进制 SHA256，与 Node bundle 的 updater 校验约定同构）。
SHA="$(shasum -a 256 "$BIN" | awk '{print $1}')"
cat > "$DEST/manifest.json" <<EOF
{
  "engine_version": "$VERSION",
  "engine_sha256": "$SHA",
  "rust_target": "$TRIPLE",
  "git_commit": "$COMMIT",
  "built_at": "$BUILT_AT"
}
EOF

echo "OK: $DEST"
echo "    engine_version=$VERSION rust_target=$TRIPLE commit=$COMMIT"
echo "    sha256=$SHA"
