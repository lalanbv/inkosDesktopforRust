# 变更记录：M3e CI（三平台构建 + 同步回归 + SSE 契约 + engine bundle 发布 + Ed25519）

| 项 | 值 |
|---|---|
| 日期 | 2026-08-06 |
| 类型 | ci（M3 第 5 子里程碑） |
| 范围 | desktop-build.yml + desktop-sync-regression.yml + engine PROD 打包 + Ed25519 keypair |
| 验证 | YAML 合法（无 tab）/ tauri.conf.json 合法 / Rust build 净；CI 本身待 push 触发 |
| 设计 | `03_M3设计/M3总体设计.md` §4 M3e |

## 交付

### 新增 `.github/workflows/desktop-build.yml`（零交叉：不碰 inkos ci.yml/release.yml）
- **触发**：tag `v*`（发版）/ PR + push master（构建校验）/ workflow_dispatch。
- **matrix**：macos-arm64、macos-x64、ubuntu-x64、windows-x64（4 配置）。
- pnpm 9（与 inkos engines 一致，避 overrides 弃用）+ node 22 + rust stable + rust-cache。
- Linux webkit2gtk 系统依赖。
- `pnpm build` → `INKOS_ENGINE_PROD=1 desktop-package-engine.sh`（**PROD 自包含 node_modules**，cp -L 解引用，解 M3d 延期项）。
- **engine bundle 发布物**：`tar engine → engine-{ver}.tar.gz + .sha256`（engine 通道 updater 拉取）。
- **tauri-action**：构建 + 签名（gated on `APPLE_*` / `TAURI_SIGNING_PRIVATE_KEY` secrets）+ tag 发版 draft +
  自动生成 `latest.json`（Ed25519 签名，shell 通道 updater manifest）。
- engine bundle 上传到 release（`softprops/action-gh-release`）+ 产物 artifact。

### 新增 `.github/workflows/desktop-sync-regression.yml`
- **触发**：每日定时（03:17 UTC，避开整点）+ workflow_dispatch。
- `git fetch upstream && git merge upstream/master`（冲突 abort + 失败开 issue）。
- `pnpm build` → `inkos doctor`（结构性，无 LLM）→ **SSE 契约测**（`cargo test --test sse_contract`，
  防 upstream broadcast 改动静默打断 observer）→ Rust 测试套。
- 失败 → `JasonEtco/create-an-issue` 用 `.github/desktop-sync-failure.md` 模板开 issue 告警。
- 成功 → push merge（近零冲突时自动兑现同步）。
- 「写一章冒烟」需 LLM key（CI 无 key），用 doctor + SSE 契约替代；写章留 opt-in（文档说明）。

### `.github/desktop-sync-failure.md`（issue 模板）
- 同步回归失败时自动开 issue（含原因分类 + 处置指引：SSE 契约失败→更新 default_table 等）。

### `scripts/desktop-gen-updater-key.sh`
- 生成 Ed25519 keypair（`tauri signer generate`）+ 打印 pubkey（写 tauri.conf.json）/ private key（CI secret）指引。

### `scripts/desktop-package-engine.sh` PROD 模式
- `INKOS_ENGINE_PROD=1` → `cp -L` 解引用 node_modules（自包含，无符号链接，打包进 .app 后用户机可用；
  体积大但正确，Phase 2 SEA 优化）。默认 dev 仍符号链接。

### Ed25519 pubkey 嵌入 `tauri.conf.json`
- shell updater pubkey 嵌入（dev keypair，生成于本里程碑）。**生产前建议重新生成自控私钥**
  （`scripts/desktop-gen-updater-key.sh`，私钥仅用户持有）。私钥设 CI secret `TAURI_SIGNING_PRIVATE_KEY`。

## 兼容性 / 扩展性 / 安全

- **兼容**：4 平台配置（双 mac arch）；pnpm 9 对齐 inkos；Linux 系统依赖标准。
- **扩展**：matrix 可加 arm64-linux；签名 secrets gating（无 secret 自动 ad-hoc，有则全签）。
- **安全**：secrets 全 CI 侧（不在仓）；Ed25519 验签；issue 告警透明。
- **零修改**：新增 workflow（不碰 inkos 既有 ci.yml/release.yml）。

## 延 M3f / 后续

- 签名 secrets 文档 + OSS 分发指引（macOS xattr / Windows SmartScreen / Linux AppImage）→ M3f。
- CI 首次 push 验证 green（本地无法跑，待 push GitHub）。
- 写一章冒烟（需 LLM key）作 opt-in workflow（后续）。

## 下一步

M3f：签名（ad-hoc + CI gating）+ OSS 分发指引文档。
