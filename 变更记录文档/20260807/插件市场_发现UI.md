# 插件市场：发现 UI（settings 面板）

> 日期：2026-08-07
> 范围：`src-tauri/picker/settings.html`
> 依赖：slice 4（`7c530708` config + 命令）

## 变更

settings.html 新增「插件市场」section——**首次端到端走通注册表管线**（config → fetch → 验签 → 兼容过滤 → 展示）：

- **刷新按钮** → `invoke("cmd_fetch_plugin_registry")` → 渲染条目列表。
- **条目展示**：名称+版本、`id`·作者·能力（复用 `capabilityLabel`）、描述。
- **i18n** zh/en：`market.title/muted/refresh/loading/empty/load_fail`。
- **a11y**：`role="list"` + `aria-label` + `role="status"`+`aria-live="polite"` 状态。
- **WCAG AAA**：复用既有 `plugin-info/name/meta/metrics` 类与调色板；新增 `ul#market-list` 列表样式。
- **错误兜底**：未配置注册表等 → catch 显示「加载市场失败：…未配置…」，提示去 config 填 `registry.url`+`pubkey`。
- init 自动 `loadMarket()`。

## 验证

- `node --check`（内联 JS 532 行）：语法 OK
- `cargo check`：通过（settings.html 为运行时静态资源，无 Rust 改动；`cmd_fetch_plugin_registry` 已注册）

## 当前状态

插件市场**浏览环路完整**：用户配置 registry 来源 → 面板展示经签名验证 + 宿主兼容过滤的可装插件。

## 后续 slice

- **市场安装流**：条目加「安装」按钮 → 后端 `cmd_install_from_registry(entry)`：下载包（`download_url`）→ `verify_bundle`（sha256+Ed25519）→ 复用既有 `install_plugin`（需明确 bundle 格式：tar.gz 解包到临时目录 vs 目录型包，并接入 `PluginManager::install_plugin`）。涉及包格式/解包/install 集成，建议单独 slice 聚焦。
