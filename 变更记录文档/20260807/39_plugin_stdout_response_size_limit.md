# 安全：插件进程 stdout 响应体积上限（防 OOM）

> 日期：2026-08-07
> 范围：`src/plugin/process.rs`
> commit：`af956292`

## 缺陷

`PluginProcess::call` 用 `read_line` 读取插件 JSON-RPC 响应，无长度上限。
插件可单行返回数 GB 响应在 `RPC_TIMEOUT(5s)` 到达前撑爆内存：

| 写入速度 | 5s 累积 | 结果 |
|----------|---------|------|
| 200 MB/s | 1 GB | OOM 在超时前触发 |
| 500 MB/s | 2.5 GB | 系统卡死 |

RPC 超时**只防挂死**（进程不响应），**不防 OOM**（进程疯狂写）。

## 修复

`Read::take(MAX_RESPONSE_BYTES + 1)` 限量读取，超限判协议违规：

```rust
const MAX_RESPONSE_BYTES: u64 = 8 * 1024 * 1024; // 8 MiB

let mut line = Vec::new();
let mut limited = Read::take(&mut *out, MAX_RESPONSE_BYTES + 1);
limited.read_until(b'\n', &mut line)?;
if line.len() as u64 > MAX_RESPONSE_BYTES {
    return Err("插件响应超上限 8388608 字节（协议违规），拒绝".into());
}
```

上限 8 MiB 与 `host_api::http_get` 的单次响应上限一致——对 JSON-RPC
远超充裕（正常响应 < 1 MiB），且 5s 内写满 8 MiB 需 1.6 MB/s 持续速率
（普通插件不会达到）。

## 与 stderr 洪泛的对比

| 管道 | 风险 | 防护层 |
|------|------|--------|
| stderr | 插件可无限写 → 撑爆日志文件 | 38 号限量转发（1000 行 + 单行 2 KiB） |
| stdout | 插件可单行数 GB → OOM 宿主 | 本轮限量读取（单响应 8 MiB） |

两者都必须**持续排空**防管道缓冲写满阻塞插件（38 号已验证），但 stdout
的威胁更直接——stderr 只刷爆磁盘，stdout 直接耗尽内存导致进程被 kill。

## 测试 +1

`test_oversized_response_rejected`：插件先读一行 stdin（触发 RPC），
再用 `dd` 输出 8 MiB+1 字节 + `\n`。

`call` 应返回 `Err`，消息含 "超上限" 或 "协议违规"（而非挂死或 OOM）。

用 `dd if=/dev/zero bs=1024 count=N | tr '\0' 'x'` 而非 awk 拼字符串：
awk 拼 8 MiB 字符串需数秒（会超 `RPC_TIMEOUT`，测不到体积检查），
dd 可瞬间产出。

## 验证

- `cargo build --lib`：编译通过
- `cargo test`：需用户执行（Bash 工具权限拒绝）
- `cargo clippy --all-targets -- -D warnings`：零警告
