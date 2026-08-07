# 插件市场：注册表 bundle 校验 + 宿主兼容性（slice 2）

> 日期：2026-08-07
> 范围：`src-tauri/src/plugin/registry.rs`
> 依赖：slice 1（`2cec4a48` 注册表索引地基）

## 变更

补全市场「校验」半边（纯逻辑、安全核心，无需网络/UI）：

- **`RegistryEntry::is_compatible_with(host_version, host_abi)`**：semver `min_host_version ≤ host_version` 且 `abi_version` 精确匹配（v1；未来可扩 ABI 兼容矩阵）。供发现层过滤不兼容插件。
- **`RegistryEntry::verify_bundle(bundle, pubkey)`**：sha256 完整性（`sha2`）+ Ed25519 来源签名（复用 `updater::sig` 的 `encode_hex`/`decode_hex`/`verify`）。下载后、安装前调用，任一失败拒绝该包。

复用既有 `sig`/`sha2`，不引入新依赖。

## 验证

- `cargo clippy --all-targets -D warnings`：clean
- `cargo test`：registry 子模块 **19 测试全过**（slice 1 的 15 + 本 slice 4：`is_compatible_with` 兼容/不兼容、`verify_bundle` 合法/篡改/错钥）

## 后续 slice

- **fetch**：HTTPS 拉取 `registry.toml` + `.sig` → `verify_signature` → `parse`（用 reqwest）。
- **发现 UI**：settings 面板列出注册表插件，`is_compatible_with` 过滤。
- **下载-安装流**：按条目下载包 → `verify_bundle` → 复用既有 `install_plugin`。
