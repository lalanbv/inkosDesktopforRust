# M2：配置热重载 mutex 中毒恢复加固

> 日期：2026-08-07
> 范围：`src-tauri/src/config/reload.rs`、`src-tauri/src/config/watcher.rs`
> 触发：rust-reviewer 审查 M2 项（前次《配置_User层_审查修复》推迟项之一）

## 问题

`reload.rs` 与 `watcher.rs` 的 std `Mutex` 用 `.expect("X mutex 中毒")` 取锁。锁中毒（某次 panic 持有该锁后产生）时，`.expect` 再次 panic；若发生在 `poll_config_changes` 后台 task 内，该 task 静默死亡——**配置热重载永久失效，文件监听事件再也不被处理**，watcher mpsc 通道最终填满。这是静默失效（最坏一类可靠性缺陷）。

## 修复

12 处统一改为 `.unwrap_or_else(|e| e.into_inner())`——从 `PoisonError` 恢复 guard 而非 panic：

| 文件 | mutex | 处数 |
|------|-------|------|
| `reload.rs` | `last_valid_config` | 4 |
| `watcher.rs` | `watched_paths` | 7 |
| `watcher.rs` | `debounce_map` | 1 |

这些是非关键缓存数据（`last_valid_config` 仅作回退值；`watched_paths`/`debounce_map` 是监听表），恢复而非 panic 是正确语义。中毒根因（引发它的那次 panic）本身已在日志可观测，恢复后系统继续运转。

## 验证

- `cargo clippy --all-targets -D warnings`：clean
- `cargo test`：lib **351 passed / 0 failed** + 全集成 0 失败（恢复路径对正常无中毒场景行为不变——锁成功时 guard 一致返回）

## 仍推迟

- **M3** watcher 自触发回放（update_config 写入触发 UserChanged 重载）——结果幂等无害，彻底修复需自写时间窗去重，价值低于风险。
- **M5** `apply_user_config` 的 `exists()`/`load()` 间良性 TOCTOU——合并结果不受影响，仅违反 user=None 不变量（cosmetic）。
