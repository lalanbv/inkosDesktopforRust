# 插件市场：自动更新 UI

> 日期：2026-08-07
> 范围：`src-tauri/picker/settings.html`
> 依赖：自动更新后端（`8fe6762f`）

## 变更

settings.html 市场面板完成「检查更新 → 更新」闭环：

- **「检查更新」按钮** → `invoke("cmd_check_plugin_updates")` → 返回 `UpdateInfo[]`。
- **更新按钮渲染**：`renderMarket(entries, upMap)`——`upMap`（id→UpdateInfo）命中的条目显示「更新」按钮（primary），其余显示「安装」。`checkUpdates` 用结果构建 upMap 重渲染 `marketCache`。
- **更新流**：`updateFromRegistry(entry)` → `cmd_update_plugin_from_registry` → 成功后刷新已装列表 + 重新 checkUpdates（已更新插件不再标记）。
- `marketCache` 缓存最近市场拉取，供 checkUpdates 对照。
- i18n zh/en（`check_updates`/`checking_updates`/`no_updates`/`updates_found`/`update`/`updating`/`update_done`/`update_fail`）；a11y 复用 `role=status`+`aria-live`；WCAG AAA 配色沿用。

## 验证

- `node --check`（内联 JS 631 行）：语法 OK
- `cargo check`：通过（settings.html 为运行时静态资源，无 Rust 改动）

## 插件市场生命周期（完成）

浏览（`cmd_fetch_plugin_registry`）→ 安装（`cmd_install_from_registry`）→ 检查更新（`cmd_check_plugin_updates`）→ 更新（`cmd_update_plugin_from_registry`）。全链路经 Ed25519 验签 + bundle 校验 + 安全解压 + 路径白名单 + 下载限流。
