# 插件市场：config 配置 + Tauri 命令（slice 4）

> 日期：2026-08-07
> 范围：`src-tauri/src/config/{types,validation}.rs`、`src-tauri/src/plugin/{mod,registry}.rs`、`src-tauri/src/commands/registry.rs`（新）、`src-tauri/src/commands/mod.rs`、`src-tauri/src/main.rs`
> 依赖：slice 1/2/3（`2cec4a48`/`52d6dccf`/`81b7cb3f`）

## 变更

把注册表接入应用——配置驱动的可信来源 + 前端可调命令：

- **`config/types.rs`**：`AppConfig` 加 `PluginRegistryConfig { url, pubkey }`（`#[derive(Default)]`，`url`/`pubkey` 同时配置=启用市场，同时留空=禁用）。
- **`config/validation.rs`**：`validate_registry`——`url` 须 `http(s)://`、`pubkey` 须 64 位 hex、两者须成对（同时配置或同时留空）。
- **`plugin/mod.rs`**：`pub const HOST_ABI_VERSION: &str = "1"`（宿主 ABI，兼容性检查用）。
- **`plugin/registry.rs`**：`pub use decode_hex`（re-export `updater::sig::decode_hex`，封装——命令层仅依赖 `plugin::registry` API，不直接耦合 `updater::sig`）。
- **`commands/registry.rs`**（新）：`cmd_fetch_plugin_registry`——读 `registry.url`+`pubkey` → `decode_hex` → 构 reqwest client（超时取自 `network.timeout_seconds`）→ `fetch_registry` → `is_compatible_with`（host 版本 = `CARGO_PKG_VERSION` + `HOST_ABI_VERSION`）过滤 → 返回可装条目 `Vec<RegistryEntry>`。
- **`commands/mod.rs`** + **`main.rs`**：声明 + 注册命令。

## 设计要点

- **持锁守则**：命令单次取 config 锁仅提取 `url`/`pubkey`/`timeout` 后立即释放，再做网络 I/O（与 `update_config` H1 修复同守则，不阻塞 tokio worker）。
- **闭包生命周期**：传给 `fetch_registry` 的传输闭包须 own url（future 不借用闭包参数生命周期），仅借用长效 client——满足 `Fn(&str) -> Fut`（Fut 独立于参数生命周期）。
- **可测性**：命令本身不可单测（`tauri::State` 无公开构造），端到端由集成测试覆盖——与既有 config 命令同模式（见 `commands/config.rs` 注释）。核心 `fetch_registry`/`is_compatible_with`/`validate_registry` 均已单测。

## 验证

- `cargo clippy --all-targets -D warnings`：clean（含 `derivable_impls` 修正 + 闭包生命周期修正）
- `cargo test`：lib **375 passed / 0 failed**（+`test_validate_registry_config`）+ 全集成 0 失败

## 后续 slice

- **发现 UI**：settings 面板加「插件市场」section，调 `cmd_fetch_plugin_registry` 列条目（名称/版本/能力/作者），兼容性已后端过滤；未配置时提示去 config 填 registry。
- **下载-安装流**：按选中条目下载包 → `verify_bundle`（sha256+签名）→ 复用既有 `install_plugin`。
