# 安全：http_get 累计请求数 + 响应字节双维度配额

> 日期：2026-08-07
> 范围：`src/plugin/host_api.rs`
> commit：`a5ffdf53`

## 缺陷

`http_get` 已有单次响应 8 MiB 上限（防单个巨响应 OOM），但**无频率与总量约束**。
插件可循环发请求：

1. **耗宿主带宽**——低速链路上足以让应用不可用
2. **流量放大器**——目标站点看到的是**宿主 IP**，被限流/封禁的是用户，
   而非插件作者。这是把用户机器变成攻击跳板。

SSRF 那一层管的是「能不能访问内网」，与「能发多少次」正交，不覆盖此风险。

## 双维度（缺一不可）

| 维度 | 上限 | 防的是 |
|------|------|--------|
| 累计请求次数 | 1000 | 高频小请求（1000 次 × 1 KB） |
| 累计响应字节 | 256 MiB | 低频大响应（单次 8 MiB 合法，**32 次即 256 MiB**） |

只做请求数：32 次大响应就是 256 MiB 流量，远在 1000 次以下。
只做字节数：上千次小请求字节数不高，但一样是对第三方的高频轰击。

## 实现要点

### Arc<AtomicU64> 跨 clone 共享

`HostContext` 每次 `execute` 被 clone 进新 Store（`runtime::create_store`）。
配额若随 clone 归零则**形同虚设**——插件每次调用重新计数即可无限请求。
与 `written_bytes` 同一理由，同一模式。

### 检查在发请求前

被拒的请求不该产生出网流量。测试据此断言**错误类型**：
配额耗尽返回 `PermissionDenied`，而网络失败是 `ExecutionFailed`——
错误类型本身即证明拦截位置在出网之前。

### fetch_update 原子检查+自增

```rust
self.http_requests.fetch_update(SeqCst, SeqCst, |cur| (cur < req_quota).then(|| cur + 1))
```

`load` 后 `fetch_add` 是两步，并发调用可同时通过检查 → 超发。
`fetch_update` 是 compare-exchange 循环，检查与自增原子。

### take() 上限取 min(单次上限, 剩余配额)

```rust
let read_cap = MAX_HTTP_RESPONSE_BYTES.min(self.http_byte_quota.saturating_sub(used_bytes));
```

否则剩余配额只剩 1 KB 时，末次请求仍可读满 8 MiB → 突破累计配额。

累加**实际读取**字节而非 `read_cap`——短响应不该多记。

## 测试 +4

| 测试 | 验证 |
|------|------|
| `request_quota_denies_after_exhausted` | 前 2 次 `ExecutionFailed`（过配额、网络失败），第 3 次 `PermissionDenied`；被拒不计数 |
| `quota_shared_across_clones` | clone 看到已用配额，且受同一上限约束 |
| `byte_quota_denies_when_exhausted` | 字节配额独立生效（请求数远未用尽） |
| `quota_not_consumed_by_capability_denial` | 无 Network 能力的拒绝发生在配额检查前，不消耗配额（否则掩盖真实权限错误） |

用 `.invalid` TLD（RFC 2606 保留，保证不可解析）：15 项 http 测试
共 **0.40s**，零真实网络往返。

## 验证

- `cargo test`：**546 passed / 0 failed**（439 lib + 107 集成/bin）
- `cargo clippy --all-targets -- -D warnings`：零警告
