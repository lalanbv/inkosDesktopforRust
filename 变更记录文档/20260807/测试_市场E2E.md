# 测试：插件市场安装链 E2E 集成测试

> 日期：2026-08-07
> 范围：`src-tauri/tests/marketplace_e2e.rs`（新）
> 目标：端到端验证命令层（`cmd_*`，受 `tauri::State` 限制不可单测）之下的安全关键安装管线

## 变更

`tests/marketplace_e2e.rs`——直接驱动命令底层 pub API 链：

- **`marketplace_install_pipeline_end_to_end`**（`#[tokio::test]`）：
  1. 构造插件（plugin.toml + plugin.wasm）→ 打包 tar.gz（`build_tar_gz`）
  2. `sha256` + Ed25519 签名 bundle
  3. 构造注册表 TOML（含 bundle sha + 签名）+ 签名注册表
  4. `fetch_registry`（**mock 传输**返回 registry + `.sig`）→ 验签注册表 + 解析
  5. `find_latest("my-plugin")` → entry
  6. `verify_bundle(bundle, pubkey)` → sha256 + Ed25519 通过
  7. `safe_extract_tar_gz(bundle, temp)` → 解压出 plugin.toml + plugin.wasm
  8. `PluginManager::install_plugin(temp)` → 断言 id/version + 二次装拒绝
- **`marketplace_rejects_tampered_bundle`**：合法条目 + 篡改 bundle（首字节翻转）→ `verify_bundle` sha256 不符 → 拒。

## 覆盖

命令层（`cmd_fetch_plugin_registry`/`cmd_install_from_registry`）只是 `tauri::State` 包装，无法单测。本测试直接驱动其底层 pub API（`fetch_registry`/`verify_bundle`/`safe_extract_tar_gz`/`install_plugin`）——端到端覆盖**注册表验签 + 解析 + bundle 验签 + 安全解压 + 安装**完整安全关键链。

## 验证

- `cargo clippy --all-targets -D warnings`：clean
- `cargo test --test marketplace_e2e`：**2 passed / 0 failed**（全链路安装 + 篡改拒绝）
