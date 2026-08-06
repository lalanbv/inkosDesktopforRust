# 变更记录：M3c 运行时 Node 自适应 bootstrap

| 项 | 值 |
|---|---|
| 日期 | 2026-08-06 |
| 类型 | feat（M3 第 3 子里程碑） |
| 范围 | engine/node 模块（region-aware mirror + 官方 SHASUMS 校验 + 解压缓存）+ spawn 接入 + dev engine 修复 |
| 测试 | 全绿（engine 20 单测含真实下载冒烟；clippy 净）；app 端到端冒烟 SPA 200 |
| 设计 | `03_M3设计/M3_总体设计.md` §4 M3c |

## 交付

app 无需系统 Node：首启按用户平台/地区下载便携 Node 22.11.0（LTS），本地缓存，后续启动零网络。

### 新增 `src-tauri/src/engine/node.rs`
- **平台检测** `PlatformArch::detect()`（cfg-based：darwin/linux/win × x64/arm64 × tar.gz/zip）+
  `cache_key`/`tarball_name`（对齐 nodejs.org 命名）。
- **region-aware mirror**（`MirrorSelector` trait + `LocaleMirrorSelector`）：
  - 检测 CN（locale 含 zh/CN 或时区 Asia/Shanghai 等）→ 优先 npmmirror，官方回退；
  - 其他 → 官方优先；`INKOS_NODE_MIRROR` env 覆盖（企业内网镜像）。
- **安全校验**：下载的二进制**始终用官方 nodejs.org `SHASUMS256.txt`（HTTPS）校验**——
  mirror 无法伪造（官方 checksum 唯一可信源）。`parse_shasums` 解析（basename + 跳非法）。
- **`BootstrappingResolver::resolve()`（async）**：缓存命中即返；否则 mirror 列表逐个下载（chunk 流式）→
  SHA256 校验 → 解压（unix flate2+tar / win zip，防 zip-slip）→ 返回 bin 路径。
  `with_progress(ProgressFn)` 回调；全失败清理半成品（调用方回退系统 node）。
- 版本固定 `NODE_BOOTSTRAP_VERSION="22.11.0"`（LTS，可复现 + 可校验；升级是显式 config 改动）。

### spawn 接入（`main.rs` spawn_sidecar_task）
- spawn sidecar **之前**解析 node：`BootstrappingResolver::new(runtime/node, LocaleMirrorSelector).resolve().await`
  → 成功用其路径作 `build_launch` 的 node_bin；失败回退系统 `"node"`（log 不阻塞）。

### 重要修复：dev engine 解析（`engine::resolve_engine_dir`）
- **问题**：`tauri.conf.json bundle.resources=["engine"]` 使 Tauri 在 dev 把 engine/ **复制到
  `target/debug/engine`**，node_modules 符号链接在复制中断裂 → `Cannot find package 'commander'`
  （resource_dir 在 dev 指 target/debug 副本）。
- **修法**：`#[cfg(debug_assertions)]` 优先 `CARGO_MANIFEST_DIR/engine`（src-tauri/engine 原始，符号链接
  完好）；release（prod）才用 `resource_dir/engine`（M3e 真实自包含 node_modules，无符号链接）。
- 4 单测覆盖（debug 偏好 / resource 优先 / 兜底）。

## 依赖

- 新增 `flate2`、`tar`（unix .tar.gz）；`zip`（仅 windows cfg）。

## 兼容性 / 扩展性 / 安全 / 性能

- **兼容**：三平台 × 双 arch；region-aware mirror 全球可用；async reqwest（避免 blocking 在
  Tauri 运行时内的 "Cannot start runtime within runtime" panic）。
- **扩展**：`MirrorSelector` trait（locale/企业/固定源）+ `ProgressFn` 回调（picker 进度，M3d 复用）。
- **安全**：官方 SHASUMS256 强校验（mirror 不可信）；HTTPS；zip-slip 防护；失败回退不崩溃。
- **性能**：缓存命中零网络；下载流式（64KiB chunk，不入内存）；低频（首启/升级），非热路径。
- **0GC**：Rust 壳先天；bootstrap 低频，分配可接受（架构 §9.1）。

## 验证

- engine 20 单测（含 `real_bootstrap_downloads_and_verifies` 真实下载 18.37s：npmmirror 下载 +
  官方 SHA256 校验 + 解压 + node bin 就位）。
- **app 端到端冒烟**（macOS）：auto 启动 → bootstrap（CN→npmmirror，缓存命中后秒返）→
  用下载的 node 起 sidecar → "InkOS Studio running :4567" → observer 接线 → **SPA 200 OK**。
- bootstrapped node 直接驱动 sidecar 也验证（`node -v`→v22.11.0 + 200 OK）。

## 延 M3e（已识别）

- prod 自包含 node_modules（dev 用 packages/cli/node_modules 符号链接；prod 需真实副本，pnpm deploy
  inject 或 npm pack 装配），M3e 打包构建 CI 验证。

## 下一步

M3d：双通道 updater（engine 通道：GitHub release→SHA256→原子替换+回滚；shell 通道：Tauri updater Ed25519）。
