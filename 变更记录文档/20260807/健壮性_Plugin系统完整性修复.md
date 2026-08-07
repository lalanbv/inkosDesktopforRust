# 健壮性：Plugin 系统完整性修复弧线

> 日期：2026-08-07
> 范围：`src/plugin/process.rs`、`src/plugin/manager.rs`
> 触发：系统性排查 plugin 生命周期与并发健壮性

## 修复汇总（6 commits）

### R1 — PluginProcess.call() RPC 超时保护（`4d49a7fd`）

**缺陷**：`read_line` 阻塞无超时——进程插件挂死时 `execute_plugin`（持 `&mut self`）永久冻结整个插件系统，无法恢复。

**修复**：spawn 读线程 + `channel::recv_timeout(5s)`：超时 → kill 子进程（best-effort）+ `ExecutionFailed`。`ExecutionFailed` 触发 `consec_failures++` → 达阈值自动禁用（联动容错链）。

### R2 — 进程隔离路径死进程检测与重生（`6f845a05`）

**缺陷**：超时 kill 后进程仍留 `running` HashMap，下次 `execute_plugin_inner` 直接复用死进程 → 立即 IO 错误，不自动恢复。

**修复**：复用前调 `process.is_alive()`（`try_wait` 非阻塞），死进程 `running.remove` → 自动重生（现有 spawn 逻辑天然覆盖）。与 R1 构成完整容错链：**超时→kill→检测→重生**。

### R3 — update_plugin 停止旧版进程插件（`5aa30347`）

**缺陷**：`update_plugin` evict 了 `wasm_cache` 但不 stop `running` 中的旧进程——更新后旧版进程仍运行，下次调用复用旧版代码，新版永不生效。

**修复**：`update_plugin` 加 `running.remove + process.stop()`（对称 `uninstall_plugin`）。

### R4 — consec_failures 生命周期完整性（`212c3dea`、`291bfb42`）

**缺陷**：`update_plugin` / `uninstall_plugin` / `disable_plugin` 均不清理 `consec_failures` HashMap：
- 卸载后重装同名插件 → 继承旧失败计数 → 可能误触发自动禁用
- 更新后新版继承旧版失败 → 新版本被误禁
- 手动禁用后重启 → 计数持续累积

**修复**：三个路径均加 `self.consec_failures.remove(id)`。加上自动禁用已有的清理，生命周期完整：install(天然) / update / disable(手动+自动) / uninstall。

## 测试（+2）

- `test_is_alive_detects_killed_process`：spawn cat → kill → 50ms → `is_alive()=false`（直接验证 R2 底层机制；cat 不可用时 skip）
- `test_consecutive_failures_auto_disable`：5 次连续失败 → 自动禁用（`consec_failures` 联动链）

## 验证

- `cargo clippy --all-targets -- -D warnings`：clean（全 6 commits）
- `cargo test`：**508 passed / 0 failed**（60 commits，零回归）
