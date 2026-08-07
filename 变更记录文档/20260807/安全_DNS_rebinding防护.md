# 安全：DNS rebinding 防护（ureq Resolver IP pinning）

> 日期：2026-08-07
> 范围：`src-tauri/src/plugin/host_api.rs`
> 依赖：内网 IP 拦截（`4c60ef69` + `03a8cd84` IPv4-mapped）
> 目标：关闭 SSRF 最后残留向量——DNS rebinding

## 缺陷（此前残留）

内网 IP 拦截只查 URL 字面量 IP；对**域名**（如 `network: ["*"]` 插件请求 `http://allowed.com`，或允许域被劫持），`allowed.com` 解析阶段翻转为内网 IP（`127.0.0.1`/`169.254.169.254`）→ 绕过字面量检查。此前的「解析-连接」TOCTOU 是记录在案的残留。

## 修复（IP pinning）

自定义 ureq Resolver：
- **`filter_public_addrs(addrs: Vec<SocketAddr>) -> io::Result<Vec<SocketAddr>>`**：过滤掉 `is_internal_ip_addr` 的地址；全内网 → `Err`（拒绝）；纯函数，可单测。
- **`ssrf_resolve(netloc)`**：`to_socket_addrs` 解析 → `filter_public_addrs`。
- **`http_get`**：`AgentBuilder::new().redirects(0).resolver(ssrf_resolve).build()`——ureq 用 resolver **返回的**（已过滤）IP 连接，= **IP pinning**：解析与连接用同一组校验过的公网 IP，rebinding 翻转的内网 IP 被滤除。

## SSRF 防护栈（四层齐全，无残留）

| 层 | 防护 | 提交 |
|----|------|------|
| 域名白名单 | `check_network_domain`（后缀混淆防护已测） | 既有 |
| 禁重定向 | `redirects(0)` | `4a76524f` |
| 直连内网 IP 字面量 | `is_internal_ip`（含 IPv4-mapped） | `4c60ef69`/`03a8cd84` |
| **DNS rebinding IP pinning** | `ssrf_resolve` 过滤解析结果 | 本 slice |

## 验证

- `cargo clippy --all-targets -D warnings`：clean
- `cargo test`：lib **388 passed / 0 failed**（+`test_filter_public_addrs_dns_rebinding`：混合 `[127.0.0.1, 8.8.8.8]→[8.8.8.8]`、全内网 `[127.0.0.1, 10.0.0.1]→Err`、全公网原样）+ 全集成 0 失败
