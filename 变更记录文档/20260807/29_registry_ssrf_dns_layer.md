# 安全：registry SSRF 补域名解析层校验

> 日期：2026-08-07
> 范围：`src/plugin/registry.rs`、`src/plugin/manager.rs`、`src/plugin/host_api.rs`
> commit：`df49786a`
> 前置：`27_extract_host_url_crate_parser_parity.md`（第一层：host 字面量）

## 缺陷

`http_fetch` 的 SSRF 防护只有一层——`is_internal_ip(host)` 检查 IP 字面量。
对**域名**该函数 parse 失败恒返回 false，于是：

恶意发布者让 `download_url` 指向自己控制的域名，A 记录填 `169.254.169.254`
（云元数据）或 `127.0.0.1` → 检查放行 → 宿主发出请求 → 元数据泄露。

注册表 Ed25519 签名**不阻止**这点：签名只证明索引未被中间人篡改，
不证明发布者本身无恶意或未被入侵。旧注释已承认此假设，但未补防护。

## 修复：发请求前显式预解析

```rust
let host = extract_host(url).ok_or_else(|| anyhow!("无法解析 host"))?;
if is_internal_ip(&host) { bail!(...); }        // 第一层：IP 字面量
check_resolved_addrs_public(&host).await?;      // 第二层：域名解析结果
```

`check_resolved_addrs_public` 在 blocking 池调 `to_socket_addrs`，
结果交给 `filter_public_addrs`——**与 `host_api` 的 ureq resolver 同一实现**，
单点定义防两侧 SSRF 判定漂移。解析结果为空或含内网 IP → `Err`。

## 为何不用 reqwest 的 `dns_resolver` 钩子

首个实现用 `ClientBuilder::dns_resolver(Arc::new(SsrfResolver))`。看似正解
（还附带 IP pinning 消除 TOCTOU），源码路径也确认 `config.dns_resolver` 会
替换 `GaiResolver` 并传入 `HttpConnector`。

**但实测该钩子从未被调用**：

| 探针 | 结果 |
|------|------|
| resolver 内写标记文件 | `NOT CALLED` |
| resolver 恒返回 `Err` + 请求公网域名 | 请求**成功** |
| `localhost` 请求错误 | `operation timed out`、`is_connect=false` |

排除的假设：feature gate（`dns_resolver` 无 gate）、测试输出捕获（改写文件仍
NOT CALLED）、系统代理（无代理环境变量）、多版本 reqwest（0.13.4 仅为
tauri-plugin-updater 私有依赖，本 crate 直连 0.12.28 且读的正是该版源码）。

根因未定位，但**结论已足够**：钩子不可依赖，且失效时**无编译期提示**——
防护静默归零。显式预解析不依赖任何库钩子行为，且错误语义可直接断言。

## 测试语义：可断言的拒绝原因

`localhost` 用例断言错误链含 `"SSRF"` 或 `"内网"`，而非 `assert!(is_err())`。
后者无法区分「被拒绝」与「本机恰好无服务监听」——宿主起了服务就变假绿。
这与前一轮 `27_` 的同类教训一致。

**验证信号**：SSRF 拦截测试 6 项共 **0.38s**，零网络往返，证明全部在
发起连接前拦截。

## 残留窗口

预解析与实际连接是两次独立 DNS 查询，其间记录可变（TOCTOU rebinding）。
彻底消除需 IP pinning（把已校验 IP 直接交给连接层），reqwest 无此接口。
bundle 的 Ed25519 签名是该窗口的第二道防线——即使连到攻击者控制的内网地址，
返回内容也过不了验签。

## 同轮附带

- **`copy_dir_all` 拒绝符号链接**：`std::fs::copy` 跟随 symlink 读取目标内容，
  本地目录安装含指向 `/etc/passwd` 的链接可将敏感文件复制进插件目录。
  与 tar 解压侧（`safe_extract_tar_gz`）对齐。
- **`exec_command` 审计日志**统一 `target: "inkos.plugin.security"`，
  拒绝与授权执行均记录（高危操作全程可追溯）。

## 验证

- `cargo test`：**529 passed / 0 failed**
- `cargo clippy --all-targets -- -D warnings`：clean
