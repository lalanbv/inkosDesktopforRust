# 安全：插件进程 stderr 捕获限量转发

> 日期：2026-08-07
> 范围：`src/plugin/process.rs`
> commit：`27468012`

## 缺陷

`PluginProcess::spawn` 用 `stderr(Stdio::inherit())`——插件进程直接持有宿主
stderr 的文件描述符，可无限量写入：

| 后果 | 说明 |
|------|------|
| 刷爆日志文件 | 无行数/字节上限 |
| 污染终端 | 与宿主日志交错，无法区分 |
| **输出无归属** | 分不清哪条来自哪个插件 |
| **可伪造宿主日志行** | 直接写符合宿主格式的假日志 |

最后一条最关键：37 号刚为 `name`/`description` 等字段加了控制字符拒绝，
防的正是「插件伪造结构化日志行污染诊断依据」。而 stderr 这条路径
**完全绕过**那层校验——插件想写什么格式就写什么格式。

## 修复

`Stdio::piped()` + detached 排空线程 `spawn_stderr_drain`：

- 单行截断 `STDERR_LINE_MAX_BYTES = 2 KiB`（单行数 MB 会撑爆日志条目）
- 总行数封顶 `STDERR_MAX_LINES = 1000`，达限后**继续排空但不再转发**
- 带 `plugin_id` 归属写入 `tracing`，`target = "inkos.plugin.stderr"`
- 插件输出作**字段值**（`output = %line`）而非 message 模板——
  由 tracing 格式化层负责转义，插件无法伪造日志结构

## 排空是硬性要求，不是可选优化

管道缓冲区（通常 64 KiB）写满后，插件的 `write` 会**阻塞**，卡死其主循环
→ 所有 RPC 超时。所以即使达到转发上限也必须继续 `read`，只是丢弃内容。

同理用 `read_until(b'\n')` 而非 `BufRead::lines()`：后者遇非 UTF-8 字节返回
`Err`，若据此 `break` 就停止排空 → 同样阻塞。按字节读 + lossy 转换。

## 测试 +2

| 测试 | 验证 |
|------|------|
| `stderr_drain_does_not_block_plugin_writes` | 插件先写 512 KiB stderr（远超缓冲）再应答 RPC → `call` 成功 |
| `stderr_drain_handles_invalid_utf8_and_exits_on_eof` | 非法 UTF-8 不中断排空，进程正常退出（线程不泄漏） |

洪泛测试直接构造 `PluginProcess` 字段（而非走 `spawn`），因为需要一个
可控输出量的"插件"——用 `sh` 脚本即可，验证的是宿主侧排空行为。

### 反向验证

把排空线程换成 `let _keep = child.stderr.take()`（持有但不读）重跑：

```
timeout 60 cargo test ... → 无输出，被 timeout 杀掉
```

测试**卡死**而非失败——这比断言失败更直接地证明了缺陷本体就是写阻塞。

## 验证

- `cargo test`：**556 passed / 0 failed**
- `cargo clippy --all-targets -- -D warnings`：零警告
