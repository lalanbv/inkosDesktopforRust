# 测试：修复 metrics 测试 timing flaky

> 日期：2026-08-07
> 范围：`src-tauri/src/plugin/manager.rs`（测试）
> 目标：消除 `test_metrics_counts_failures` 的偶发失败（CI 可靠性）

## 问题

`test_metrics_counts_failures` 调 `execute_plugin("nonexistent", ...)`（NotFound 路径：HashMap 查找 + Err，常 **< 1 微秒**），然后断言 `exec_total_us > 0` 与 `avg_us > 0`。但 `execute_plugin` 用 `as_micros()` 计时，sub-μs 操作取整为 **0** → 断言偶发失败（并行负载下该路径耗时浮动，0 与非 0 不定）。这是**真实的 timing flaky**，非 panic hook 干扰（crash 测试已用 `GLOBAL_STATE_LOCK` 序列化）。

## 变更

改为**确定性**断言：
- 保留 `exec_count == 1`、`exec_failures == 1`（核心：调用与失败被计数）。
- 移除 flaky 的 `exec_total_us > 0` / `avg_us > 0`。
- 新增 `avg_us == exec_total_us`：`metrics()` 中 `avg_us = total_us / count`，`count == 1` 时 `avg == total`——验证平均计算正确性，且对 sub-μs（两者皆 0）同样成立。

## 验证

- `cargo clippy --all-targets -D warnings`：clean
- `cargo test --lib plugin::manager::tests::test_metrics`：连跑 5 次，每次 2 passed / 0 failed（稳定，不再 flaky）

## 说明

`exec_total_us` 取整为 0 对 sub-μs 操作是**准确**的（该操作确实 < 1μs），不是 bug；原测试断言 `> 0` 对此类操作过严。生产计时逻辑（`as_micros`）保持不变——微秒粒度对插件 execute（通常 ms 级）足够；仅测试断言修正。
