# 插件市场：注册表 fetch（slice 3）

> 日期：2026-08-07
> 范围：`src-tauri/src/plugin/registry.rs`
> 依赖：slice 1/2（`2cec4a48` / `52d6dccf`）

## 变更

拉取+验签+解析闭环（`fetch_registry` + 生产传输 `http_fetch`）：

- **`fetch_registry(url, pubkey, fetch)`**：`fetch` 注入 HTTP 传输（`Fn(&str) -> Future<Output = Result<Vec<u8>>>`）。拉 `registry_url` 字节 + `{url}.sig` hex → `verify_signature` → `parse`。
- **`http_fetch(client, url)`**：生产 reqwest `GET` → bytes（注册表是小 TOML，无需流式）。

## 设计：注入 HTTP 传输

`fetch` 参数化传输——验签+解析逻辑可**脱离网络单测**（测试用桩闭包返回 canned 字节，无需 mock HTTP 服务器）；生产传 `|u| http_fetch(&client, u)`。这是最可测 + 最佳扩展性的形态（未来可换传输、加缓存、加重试而不改核心）。

注册表 URL **不强制 https**：注册表本身经 Ed25519 签名，签名是信任锚——即便经 http，无私钥的 MITM 也无法伪造合法签名（https 仅纵深防御）。插件包 `download_url` 的 https 强制在条目校验中保留（安装链的额外保障）。

## 验证

- `cargo clippy --all-targets -D warnings`：clean
- `cargo test`：registry 子模块 **23 测试全过**（slice 1/2 的 19 + 本 slice 4：fetch 合法 / 坏签名 / 坏 TOML / fetch 错误）

## 后续 slice

- **Tauri 命令 + 配置**：`AppConfig` 加 `PluginRegistryConfig { url, pubkey }`；`cmd_fetch_plugin_registry` 读配置 → 构 reqwest client → `fetch_registry` → 按兼容性过滤返回。
- **发现 UI**：settings 面板列出注册表插件（`is_compatible_with` 过滤、显示版本/能力）。
- **下载-安装流**：按条目下载包 → `verify_bundle` → 复用既有 `install_plugin`。
