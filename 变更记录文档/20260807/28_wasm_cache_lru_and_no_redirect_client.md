# 性能与安全：wasm_cache 真 LRU + NoRedirectClient 类型层不变量

> 日期：2026-08-07
> 范围：`src/plugin/manager.rs`、`src/plugin/registry.rs`、`src/commands/registry.rs`
> commits：`f45c5c83`（LRU）、`b15b2dc2`（断言补强）、NoRedirectClient 迁移

## 一、wasm_cache 驱逐策略：哈希序 → 真 LRU

### 缺陷

两处驱逐（`execute_plugin_inner`、`broadcast_event`）用
`wasm_cache.keys().next()`——`HashMap` 上取的是**哈希序首个**，与访问热度无关。

缓存满时若驱逐的恰是当前反复调用的热插件 → 抖动循环：
**编译 → 插入 → 下次调用又被驱逐 → 重编译**。每次 Cranelift 编译数十至
数百毫秒，是实打实的热路径开销。极端情况可驱逐**刚插入**的条目
（`insert` 在驱逐之后，但下次调用另一未缓存插件时哈希序首个可能就是它）。

### 修复

| 新增 | 作用 |
|------|------|
| `wasm_lru_tick: HashMap<String,u64>` + `wasm_lru_counter: u64` | 每 key 的最近使用序号 |
| `select_lru_victim(keys, tick) -> Option<&str>` | 纯函数选最小序号者（可单测） |
| `wasm_lru_touch(id)` | 缓存命中与插入后记账 |
| `wasm_cache_make_room()` | 达上限时驱逐最冷者（替换两处重复） |
| `wasm_cache_evict(id)` | cache + tick 成对删除（替换三处生命周期 remove） |

驱逐**选择**抽成纯函数，因为它正是缺陷所在——直接单测「热插件不被驱逐」，
不需要真实 `.wasm`（`WasmPlugin` 构造需真文件，无法直接填充缓存）。

`wasm_cache_evict` 单点化防漂移：`wasm_cache` 与 `wasm_lru_tick` 必须成对增删，
否则 uninstall/disable/update 后残留 tick 条目永不回收 → 无界增长。

未记账条目（未 touch 过）视作最冷（`unwrap_or(0)`），优先驱逐。
`u64` 计数器溢出需 1.8e19 次调用，非实际风险。

### 测试 +3

`test_select_lru_victim_picks_least_recently_used`（hot 不被驱逐，touch 后受害者变更）、
`test_select_lru_victim_untracked_is_coldest`、`test_select_lru_victim_empty_is_none`

## 二、NoRedirectClient：SSRF 不变量前移到类型

### 缺陷

`http_fetch(client: &reqwest::Client, ...)` 的 SSRF 保证**依赖调用方传入禁重定向
client**。跟随重定向会让 `https://ok.example.com` 302 到内网地址，请求在 reqwest
内部已实际发出——`is_internal_ip` 只看到原始 URL，第一层防护被绕过。

任何未来调用点传默认 client 即静默失去防护，编译器不提示。
**实证**：迁移时发现 5 处测试调用点用的正是 `reqwest::Client::new()`（默认策略、
跟随重定向）——旧签名下它们编译通过，防护形同虚设。

### 为何不能运行时断言

reqwest 的 `redirect()` 只存在于 `ClientBuilder`；`Client` 不暴露策略查询接口，
`http_fetch` 内部无法自检。

### 修复：新类型 + 唯一构造入口

```rust
pub struct NoRedirectClient(reqwest::Client);

impl NoRedirectClient {
    pub fn new(timeout: Duration) -> Result<Self> {
        // 强制 Policy::none()，无法绕过
    }
    fn as_inner(&self) -> &reqwest::Client { &self.0 }  // 只出借
}
```

`as_inner` 私有 → 外部无法用任意 client 构造本类型。`http_fetch` 签名改为
`&NoRedirectClient`，不变量在**编译期**成立。

> 注：`resp.status().is_success()` 会把 3xx 当失败拒绝，但那是响应阶段——
> SSRF 的危害在请求**已发出**，不在响应码。防护必须在发起前。

## 三、其他

- `copy_dir_all` 拒绝符号链接：`std::fs::copy` 会跟随 symlink 读取目标内容，
  本地安装含指向 `/etc/passwd` 的链接可将敏感文件复制进插件目录。与 tar 解压侧对齐。
- `exec_command` 审计日志统一 `target: "inkos.plugin.security"`（拒绝与授权执行都记）。
- `test_consecutive_failures_auto_disable` 补 `recently_auto_disabled` 内容断言——
  原只验证计数，内容错（如 push 空串）不会被发现。

## 验证

- `cargo test`：**528 passed / 0 failed**
- `cargo clippy --all-targets -- -D warnings`：零警告
