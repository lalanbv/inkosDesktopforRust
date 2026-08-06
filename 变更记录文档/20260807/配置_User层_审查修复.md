# User 配置层审查修复（H1/H2/M1/M4）

> 日期：2026-08-07
> 范围：`src-tauri/src/commands/config.rs`、`src-tauri/src/config/loader.rs`、`src-tauri/src/config/validation.rs`
> 触发：ecc:rust-reviewer 对 `c64627bb`（User 层）独立审查发现 2 HIGH + 5 MEDIUM

## 背景

新增 User/Global 配置层后，按 code-review 规则（文件系统操作 + 用户输入为强制审查触发器）派发独立 rust-reviewer 审查。审查判定 Block（2 HIGH 须合并前修）。本次修关键 4 项，其余 3 项记录为后续。

## 修复

### H1 — update_config 持 config mutex 做 fsync（HIGH）
原 `update_config` 先 `state.config.lock().await` 再 `save_*_config`（含 `atomic_write_0600` 的 `sync_all`/fsync）。fsync 抖动可达数十毫秒~秒级，持锁期间阻塞 tokio worker，所有 `get_config` 等命令被卡住。User 层无门槛高频放大此问题。

**修复**：重构为先持久化（无 config 锁）→ 再取锁 `set_*`（纯内存）。三分支（User/Workspace/Project）统一；崩溃一致性靠「下次启动 init_manager 从文件加载」保证。顺带让 System 分支不再无谓取锁。

### H2 — reset_config(User) 仅清内存、重启被"复活"（HIGH）
`reset_config(User)` 只 `clear_user()`，但 `init_manager → apply_user_config` 每次启动无条件重载 `user.toml` → 用户「重置全局配置」后重启，配置复活，违反最小惊讶。

**修复**：`ConfigLoader::delete_user_config()`（幂等：NotFound 视为成功）；`reset_config(User)` 先删文件再清内存。Workspace/Project 同类问题记录为后续（它们按需重载，影响不直接）。

### M1 — init_manager 静默吞错（MEDIUM）
`AppState::new` 的 `init_manager().unwrap_or_else(|_| ...)` 丢弃错误；损坏 `user.toml` 静默回退默认、无日志线索。

**修复**：`init_manager` 改为容错——`apply_user_config` 失败时 `tracing::error!` + 跳过（不让单文件损坏使整个 init 失败）；`AppState::new` 回退路径补日志。

### M4 — validation 校验缺口（MEDIUM）
`validate()` 原仅覆盖 level/channel/proxy 前缀。User 层是无门槛常驻可写面，下列字段无校验放大误操作/攻击面：`version_policy`（Fixed/Range 垃圾值进入更新引擎）、`check_interval_hours=0`（忙循环）、`retention_days=0`（立即清日志）、`timeout_seconds=0`（瞬时超时）。

**修复**：补 `validate_numerics`（三字段 `>=1`）+ `validate_version_policy`（Fixed/Range 用 `semver` 的 `Version`/`VersionReq` 解析）。proxy 暂保留前缀校验（无 `url` 依赖，reqwest 下游兜底）。

## 推迟（记录为后续，不扩散本 PR 范围）

- **M2** `last_valid_config`/`watched_paths` 的 std Mutex `.expect()` 中毒即扼杀热重载 task —— 系统性既有模式（reload/watcher 全模块如此），应统一改造为锁恢复，非 User 层独有。
- **M3** `update_config` 写入触发 watcher 回放（自触发事件循环）—— 结果幂等无害，仅多一次磁盘读 + emit；彻底修复需自写时间窗去重。
- **M5** `apply_user_config` 在 `exists()` 与 `load()` 间的良性 TOCTOU（文件被删则 `set_user(default)`，违反 user=None 不变量，但合并结果不受影响）。

## 验证

- `cargo clippy --all-targets -D warnings`：clean
- `cargo test`：lib **351 passed / 0 failed**（+5 新测试）+ 全集成 0 失败
- 新测试：`test_delete_user_config_idempotent`、`test_init_manager_corrupt_user_config_falls_back`、`test_validate_zero_numerics_rejected`、`test_validate_invalid_version_policy_rejected`、`test_validate_valid_version_policy_ok`
