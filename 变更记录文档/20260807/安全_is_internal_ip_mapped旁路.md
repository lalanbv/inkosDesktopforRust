# 安全：is_internal_ip 修复 IPv4-mapped IPv6 旁路

> 日期：2026-08-07
> 范围：`src-tauri/src/plugin/host_api.rs`
> 修复：`4c60ef69`（内网 IP 拦截）引入的缺陷

## 缺陷

`is_internal_ip`（`4c60ef69`）的 IPv6 分支仅检查 `is_loopback`/`is_unspecified`/`is_unique_local`/`is_unicast_link_local`。但 **IPv4-mapped IPv6**（`::ffff:127.0.0.1`、`::ffff:169.254.169.254`）是合法 IPv6 表示，不命中上述 v6 判定 → **绕过内网 IP 拦截**。攻击者可用 mapped 形式直连 loopback/云元数据，绕过 `4c60ef69` 的防护。

## 修复

`is_internal_ip_addr`（拆出的核心）：V6 分支先用 `to_ipv4_mapped()` 提取映射的 IPv4 → 复查 v4 内网判定。`::ffff:127.0.0.1` → `Some(127.0.0.1)` → v4 `is_loopback` → 拦截。非 mapped 的 v6 走原 v6 判定。

## 验证

- `cargo clippy --all-targets -D warnings`：clean
- `test_is_internal_ip_classification`：新增 `::ffff:127.0.0.1`、`::ffff:169.254.169.254` 断言（均应判内网）——通过
