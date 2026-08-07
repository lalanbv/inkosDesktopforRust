# 安全：extract_host 换用 url crate，消除解析器分歧

> 日期：2026-08-07
> 范围：`src/plugin/host_api.rs`、`Cargo.toml`
> commit：`0b548402`
> 前置：`26_extract_host_backslash_authority.md`（反斜杠单点修复，本轮升级为根治）

## 根因

第一层防护（域名白名单 `check_network_domain` + IP 字面量拦截 `is_internal_ip`）
用手写 URL 解析取 host；实际发请求的 ureq 2.12 依赖 `url` 2.5（WHATWG 解析器）。
两个解析器 → **「校验看到的 host」≠「连接目标 host」**，每处分歧都是绕过点。

## 实测三类分歧

| # | 载荷 | 手写解析 | url crate（= 实连目标） |
|---|------|---------|------------------------|
| 1 | `http://127.0.0.1\x@ok.example.com/` | authority 不以 `\` 终止，`rsplit_once('@')` 取尾 → `ok.example.com`（白名单通过） | `\` 终止 authority → **127.0.0.1** |
| 2 | `http://①②⑧.⓪.⓪.①/` | 普通域名（`is_internal_ip` 恒 false） | IDNA ToASCII → **128.0.0.1** |
| 3 | `http://12\n7.0.0.1/` | 含控制字符的域名 | 剥离 tab/CR/LF → **127.0.0.1** |

`"*"` 通配白名单插件由此可直连 loopback 与云元数据；第一层完全失效。

## 修复

显式声明 `url = "2.5"`（与 ureq 传递依赖同为 2.5.8 → 同一解析器，分歧从根消除）：

```rust
let host = match url::Url::parse(url).ok()?.host()? {
    url::Host::Domain(d) => d.to_lowercase(), // IDNA 已 ToASCII
    url::Host::Ipv4(v4) => v4.to_string(),
    url::Host::Ipv6(v6) => v6.to_string(),
};
```

### 关键陷阱：IPv6 必须走 Host::Ipv6，不能用 host_str()

`host_str()` 对 IPv6 **保留方括号**（`https://[::1]/` → `"[::1]"`），而
`"[::1]".parse::<IpAddr>()` 失败 → `is_internal_ip` 恒 false → IPv6 内网字面量
在 `host_api` 与 `registry::http_fetch` 两处**全部放行**。

本轮首次实现就踩中，被 `test_http_fetch_rejects_ipv6_loopback_literal` 捕获
（错误信息为「GET 失败」而非「SSRF 拒绝」——请求已实际发出）。`Host::Ipv6`
给出 `Ipv6Addr`，其 `Display` 无括号，与 `is_internal_ip` 的 parse 契约一致。

## 连接期兜底为何不可依赖

`ssrf_resolve`（ureq resolver 钩子）确实拿到规范化后的 netloc 并过滤内网 IP，
但目标是 IP 字面量时 `to_socket_addrs` **不走 DNS**——此前那版测试
`assert!(ctx.http_get(url).is_err())` 的「通过」可能只因 localhost 无监听服务，
**宿主机 127.0.0.1:80 有服务即变假绿**。防护必须落在第一层。

## 验证信号

六种规范化形式的拦截测试：**15.79s → 0.00s**。零网络往返即被拒，
证明在发起连接前拦截，而非依赖环境恰好无服务。

## 行为变化

`extract_host("example.com")`（无 scheme 裸串）现返回 `None`。
`check_network_domain` 对 `None` fail-closed 拒绝，与 ureq 同样无法请求该串一致；
旧测试固化的是手写解析的宽松语义，已更新。

## 验证

- `cargo test`：**523 passed / 0 failed**
- `cargo clippy --all-targets -- -D warnings`：clean
