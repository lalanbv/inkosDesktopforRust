# 安全：exec_command 子进程超时 + 边读边限量

> 日期：2026-08-07
> 范围：`src/plugin/host_api.rs`
> commit：`ea4b47fc`

## 两个缺陷（同源于 `.output()`）

### 1. 无超时 → 宿主线程永久冻结

`std::process::Command::output()` 阻塞直到子进程退出。插件调
`system_command:sleep` 或任何挂死的命令 → 调用线程永久卡住。

`process.rs` 的 RPC 路径**已修**过同类缺陷（spawn 读线程 + `recv_timeout`），
此处未修——同一代码库内两条子进程路径防护不对等。

### 2. 输出截断不防 OOM

原实现读完全部输出再 `truncate()`。子进程输出 10GB 时 OOM 发生在
`.output()` **内部**，截断只减小返回值、不限制峰值内存。
原注释「防进程输出 GB 级数据 OOM 宿主」**实际不成立**。

## 修复：spawn + 限量读线程 + 轮询超时

| 措施 | 作用 |
|------|------|
| `try_wait` 轮询 + `EXEC_COMMAND_TIMEOUT`(30s) | 超时 → `kill()` + `wait()` 回收僵尸 + 安全审计日志 |
| `spawn_capped_reader` 达 1 MiB **停止读取** | 峰值内存有界（不是读完再截断），追加截断标记 |
| `stdin(Stdio::null())` | 防子进程等待输入而挂死 |
| stdout / stderr **各**独立线程 | 主线程顺序读会在一方管道写满时双方互等 → 死锁 |

达上限后停止读取，子进程继续写会被管道背压阻塞，随后由超时 kill 收尾。

## 超时可注入：让 kill 分支真正被覆盖

首版测试名为 `times_out_on_hanging_process`，实际断言的是「短命令不被误杀」——
**名不副实**：30s 生产超时无法在单测中真等，kill 分支零覆盖。

改为 `HostContext.exec_timeout` 字段 + `#[cfg(test)] with_exec_timeout()`：
测试缩到 300ms 直测 kill 路径，**0.31s 完成**（证明超时窗后立即返回，
而非等 `sleep 30` 自然结束）。生产走 `new()` 的默认 30s，无行为变化。

## 测试 +5

| 测试 | 验证 |
|------|------|
| `kills_hanging_process_on_timeout` | 300ms 超时 → Err 含「超时」，elapsed < 5s |
| `does_not_kill_within_timeout` | 反向：窗内完成的命令不被误杀 |
| `succeeds_and_captures_stdout` | 替换 `.output()` 后正常路径基线不变 |
| `capped_reader_truncates_at_limit` | `io::repeat` 无限流 → 内容恰为上限字节数 |
| `capped_reader_passes_through_small_output` | 未达上限原样返回，无标记 |

限量逻辑抽成独立函数 `spawn_capped_reader` 后可直接单测，
无需构造真实的大输出子进程。

## 验证

- `cargo test`：**542 passed / 0 failed**（432 lib + 110 集成/bin）
- `cargo clippy --all-targets -- -D warnings`：零警告
