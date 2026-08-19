#!/usr/bin/env bash
# 组装 Rust 全量包（inkos-engine 0.1.0 发布物）。
#
# 产物（engine-rs/dist/，git-ignored）：
#   inkos-engine-{ver}-{rust-triple}.tar.gz        = server bin + manifest.json + README + static/（studio 前端）
#   inkos-engine-{ver}-{rust-triple}.tar.gz.sha256  = tarball 校验和（发布上传伴生文件）
#
# 前置：
#   1. ./scripts/desktop-build-inkos.sh 已跑（packages/studio/dist 为真实构建——
#      strangler_duel 会用 34 字节桩覆写 dist/index.html，duel 后必须重建）。
#   2. 本机 rustc 可用（host triple 自动探测）。
#
# manifest.json 键与 Node engine bundle（desktop-package-engine.sh）同约定：
# engine_version / engine_sha256 / built_at，另附 rust_target / git_commit。
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
ENGINE_RS="$ROOT/engine-rs"
cd "$ENGINE_RS"

VERSION="$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -1)"
TRIPLE="$(rustc -vV | sed -n 's/^host: //p')"
COMMIT="$(git rev-parse --short HEAD)"
BUILT_AT="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
PKG="inkos-engine-$VERSION-$TRIPLE"

# 1) release 构建（lib + bin inkos-engine-server）。
echo "[rust-engine] cargo build --release ..."
cargo build --release
BIN="$ENGINE_RS/target/release/inkos-engine-server"
test -x "$BIN" || { echo "FAIL: $BIN 缺失"; exit 1; }

# 2) 暂存目录：bin + studio 静态前端（INKOS_STATIC_DIR 浏览器直连面）。
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
mkdir -p "$STAGE/$PKG"
cp "$BIN" "$STAGE/$PKG/inkos-engine-server"

if [ -f "$ROOT/packages/studio/dist/index.html" ]; then
  cp -R "$ROOT/packages/studio/dist" "$STAGE/$PKG/static"
else
  echo "WARN: packages/studio/dist 缺失——纯 API 包（无浏览器直连面）"
fi

# 3) manifest（二进制 SHA256——与 Node engine bundle 的 updater 校验约定同构）。
SHA="$(shasum -a 256 "$BIN" | awk '{print $1}')"
cat > "$STAGE/$PKG/manifest.json" <<EOF
{
  "engine_version": "$VERSION",
  "engine_sha256": "$SHA",
  "rust_target": "$TRIPLE",
  "git_commit": "$COMMIT",
  "built_at": "$BUILT_AT"
}
EOF

# 4) README（运行契约：env 面与端口）。
cat > "$STAGE/$PKG/README.md" <<'EOF'
# inkos-engine（Rust 全量包）

inkos 业务引擎的 Rust 全量移植：16 业务域库 + axum `/api/v1/*` 契约面（与
Node sidecar 同契约，strangler duel 8/8 对跑验证）。本包为独立 HTTP 服务
`inkos-engine-server`，同位替换 Node sidecar。

## 运行

```bash
./inkos-engine-server
# inkos-engine-server listening on 127.0.0.1:8787
curl http://127.0.0.1:8787/api/v1/health   # {"ok":true,"version":"0.1.0"}
```

浏览器直连模式（带 studio 前端）：`INKOS_STATIC_DIR="$PWD/static" ./inkos-engine-server`
然后访问 http://127.0.0.1:8787/ 。

## 环境变量

| 变量 | 语义 | 默认 |
| --- | --- | --- |
| `INKOS_PROJECT_ROOT` | 项目根（books/、inkos.json） | CWD |
| `INKOS_PORT` | 监听端口 | 8787 |
| `INKOS_LLM_BASE_URL` / `INKOS_LLM_API_KEY` / `INKOS_LLM_MODEL` | 默认 LLM 端点 | — |
| `INKOS_LLM_MAX_TOKENS` | 默认 max_tokens | 8192 |
| `INKOS_LLM_STREAM` | 流式开关（true/1/yes） | 流式 |
| `INKOS_LLM_API_FORMAT` | API 方言 | — |
| `INKOS_LLM_FIRST_EVENT_TIMEOUT_MS` / `INKOS_LLM_STREAM_IDLE_TIMEOUT_MS` | 流超时（首事件/空闲） | INTERACTIVE 120s/90s · PIPELINE 300s/180s |
| `INKOS_AGENT_<NAME>_MODEL` / `_BASE_URL` / `_API_KEY` / `_MAX_TOKENS` | agent 级覆盖（writer/planner/composer/reviser/auditor/chapter-analyzer/state-validator/writer-settler） | — |
| `INKOS_STATIC_DIR` | 静态前端目录（`/assets/*` + SPA 回退） | 未设=纯 API |

LLM 端点解析序：inkos.json 服务项 + secrets 优先，不可用回退 `INKOS_LLM_*`。

## 完整性校验

`manifest.json` 的 `engine_sha256` 对应包内 `inkos-engine-server` 二进制；
tarball 级校验用伴生 `.sha256` 文件（`shasum -a 256 -c`）。
EOF

# 5) tar + sha256（detar 后顶层目录即 $PKG，无绝对路径前缀；
#    .sha256 用 shasum 标准行格式——`shasum -a 256 -c` 可直接校验）。
mkdir -p "$ENGINE_RS/dist"
OUT="$ENGINE_RS/dist/$PKG.tar.gz"
tar czf "$OUT" -C "$STAGE" "$PKG"
shasum -a 256 "$OUT" > "$OUT.sha256"

echo "OK: $OUT"
echo "    engine_version=$VERSION rust_target=$TRIPLE commit=$COMMIT"
echo "    sha256=$(awk '{print $1}' "$OUT.sha256")"
