# 安全：修复 http_get 重定向 SSRF

> 日期：2026-08-07
> 范围：`src-tauri/src/plugin/host_api.rs`
> 目标：「最安全可靠」——补齐插件网络边界的 SSRF 防护

## 缺陷

`http_get` 用 `ureq::get(url).call()`——ureq 2.x **默认跟随最多 5 次重定向**。攻击场景：插件声明 `network: ["allowed.com"]`（白名单通过），`allowed.com` 响应 `302 → http://169.254.169.254/...`（云元数据）或 `http://localhost:xxxx`/`http://192.168.x.x`，ureq 跟随后续请求到内网目标，而**重定向目标不经 `check_network_domain` 复核** → **重定向 SSRF**。恶意插件（或被劫持的允许域）可借此探测/访问内网服务、窃取云元数据。

## 修复

`ureq::AgentBuilder::new().redirects(0).build()` **禁用重定向跟随**：
- 允许域 302 时，插件拿到 3xx 响应（不跟随）。
- 插件若手动跟进，每次都再经 `http_get` → `check_network_domain` 复核白名单。
- 白名单在每个**实际请求**生效，无法借重定向绕过。

## 顺带核查（无缺口）

- **read_file / list_dir**：均经 `normalize_path`（`canonicalize` 解析 `..`/符号链接 + `starts_with(work_dir)`）——正确，无 write_file 同类「校验-写入路径不一致」缺陷。
- **write_file**：本会话早期（`ec1e6d9f`）已修。
- **extract_host**：对 `http://allowed@evil`（userinfo 技巧）提取 `allowed@evil` 整串——不匹配白名单 → 保守**过拦**（非可利用）。
- **exec_command**：WASM Host trait 不暴露（wit 无导入），不可达（`ec1e6d9f` 已加契约注释）。

## 验证

- `cargo clippy --all-targets -D warnings`：clean
- `test_http_get_no_redirect_ssrf`：本地 TcpListener 响应 `302 → http://127.0.0.1:1`（不可达）；修复（`redirects(0)`）返回 Ok（3xx，不跟随），缺陷（默认跟随）会连 `127.0.0.1:1` 拒绝 → Err。测试通过（断言 Ok）。

## 残留（已知，非本次范围）

- DNS rebinding（允许域短暂解析到内网 IP）：彻底防护需在连接时校验解析 IP（拒私网/loopback/链路本地）。当前白名单 + 禁重定向已挡大多数 SSRF；DNS rebinding 为高级向量，单独加固。
- `network: ["*"]` 允许任意域：by-design（插件显式声明全开放）；SSRF 风险由声明方承担。
