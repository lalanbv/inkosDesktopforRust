# 变更记录：M3d 双通道 updater（engine + shell）

| 项 | 值 |
|---|---|
| 日期 | 2026-08-06 |
| 类型 | feat（M3 第 4 子里程碑） |
| 范围 | updater 模块（engine 通道 + shell 通道）+ 命令 + app_data/engine 优先 + Tauri updater 插件 |
| 测试 | 全绿（updater 单测 + 原子替换回滚 TDD + 真实 GitHub check 冒烟；clippy 净） |
| 设计 | `03_M3设计/M3_总体设计.md` §4 M3d |

## 交付

主动检查并更新到本仓 GitHub 最新版本：engine 通道（原子替换+回滚）+ shell 通道（Ed25519 自更新）。

### 新增 `src-tauri/src/updater/`
- **`mod.rs`**：`is_newer(current, latest_tag)`（semver 比较，容错非 semver）+ `ReleaseInfo` DTO。4 单测。
- **`engine.rs`**：
  - `atomic_replace_with_rollback(engine_dir, bak_dir, new_dir, health_check)`：**纯逻辑 TDD 核心**——
    备份→替换→健康预检失败回滚（POSIX rename 原子；Windows 目标预清空安全）。4 单测（成功+备份/回滚/首装无 bak/刷新旧 bak）。
  - `EngineChannel`（async）：GitHub `releases/latest` → `is_newer` 比对 → 下载平台无关 bundle `engine-{ver}.tar.gz`
    + `.sha256` → SHA256 校验 → 解压（复用 node::extract_archive）→ 原子替换 + 回滚。
  - `bundle_name(ver)` = `engine-{ver}.tar.gz`（inkos dist 纯 JS，平台无关单资产）。
  - `#[ignore]` 真实 GitHub check 冒烟（2.19s 通过，无 release 时优雅 None）。

### `main.rs` 接线
- **engine 解析优先 app_data/engine**（`resolve_launch_engine` 辅助）：updater 更新副本（可写）优先；
  缺则回退 dev/prod 源。下次启动用新 engine（M3d updater 写 app_data/engine）。
- **shell 通道**：`tauri-plugin-updater` + Ed25519（`tauri.conf.json plugins.updater.pubkey/endpoints`）。
- **UpdaterState**（repo + current_engine_version + engine_dir/bak_dir/staging_dir）setup 写入。
- **3 命令**（自定义，默认可 invoke）：
  - `cmd_check_updates` → `{engine, shell}` ReleaseInfo（失败保守视为无更新）。
  - `cmd_apply_engine_update` → check + apply（下载/校验/原子替换/回滚）。
  - `cmd_apply_shell_update` → Tauri updater `download_and_install`（下载+Ed25519 验签+安装）。

### `tauri.conf.json`
- `plugins.updater`：`endpoints`（本仓 latest.json）+ `pubkey`（**占位，M3e 生成 Ed25519 keypair 替换**）。

## 上游同步链路（M3e 完成闭环）

engine 更新不直接拉 Narcooo/inkos：M3e `desktop-sync-regression.yml` 定时 `git merge upstream/master` →
bump 版本 → tag → `desktop-build.yml` 构建并发布 engine bundle + `latest.json`（Ed25519 签名）到**本仓**
Releases → 本机 updater 拉本仓 release（单一可信源）。

## 依赖

- 新增 `semver`（版本比较）、`tauri-plugin-updater`（shell 通道）；reqwest 加 `json` feature。

## 兼容性 / 扩展性 / 安全 / 性能

- **兼容**：engine bundle 平台无关（纯 JS）；shell 走 Tauri updater 跨平台。
- **扩展**：`UpdateChannel` 模式（engine/shell 可加自托管/企业源）；命令驱动（前端可调）。
- **安全**：engine bundle SHA256 强校验（.sha256 asset）；shell Ed25519 验签；原子替换+回滚（健康预检失败不破损）。
- **性能**：低频（手动/定时检查）；下载流式；staging 用后清理。

## 延 M3e（已识别）

- engine bundle 发布 + `latest.json` 生成 + Ed25519 keypair 生成 + 签名——M3e CI 闭环后 e2e 可验。
- 当前 `pubkey` 为占位（M3e `tauri signer generate` 生成真实 keypair：pubkey 入 tauri.conf.json，private key 作 CI secret）。

## 下一步

M3e：CI（三平台构建 + 同步回归 + SSE 契约 + engine bundle 发布 + Ed25519 keypair）。
