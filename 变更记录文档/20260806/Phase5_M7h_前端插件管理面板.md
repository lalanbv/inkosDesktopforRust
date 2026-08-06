# Phase 5 后续：M7h 前端插件管理面板

> 日期：2026-08-06
> 提交：`03032b59`
> 对应 Phase 6 规划 6.4 的最小可用版

## 目标

补全插件系统的用户可见入口：让用户能图形化管理插件（列表/启用/禁用/卸载/安装），无需手改 plugin.toml 或调命令。

## 交付物

### 1. `picker/settings.html` —— 插件管理面板
沿用 picker 的 vanilla JS + `window.__TAURI__.core.invoke` 风格（零构建链，零依赖）。
- **列表**：每个插件显示 名称 · 版本 · 作者 · 权限徽章 · 启用/禁用状态
- **启用/禁用开关** → `enable_plugin` / `disable_plugin`
- **卸载**（带 `confirm` 确认，防误删）→ `uninstall_plugin`
- **从目录安装**：复用 `cmd_pick_project_dialog` 选插件目录 → `install_plugin`
- **刷新**按钮 + 错误兜底（`unhandledrejection` 全局捕获）
- 权限 capability → 中文标签映射，未知值原样回显不抛错

### 2. `main.rs cmd_open_plugin_manager` —— 独立窗口
```rust
WebviewWindowBuilder::new(&app_handle, "plugin-manager", WebviewUrl::App("settings.html".into()))
    .title("inkosDesktop · 插件管理")
    .inner_size(720.0, 560.0)
    .min_inner_size(480.0, 360.0)
```
- **已存在则聚焦**（`get_webview_window` + `show` + `set_focus`），不重复创建
- 与 picker / sidecar 主窗口分离，生命周期独立

### 3. `picker/index.html` —— 入口链接
项目选择器底部加「⚙ 插件管理」次要链接按钮（`.link-btn` 样式，不抢"选择文件夹"主操作），点击调 `cmd_open_plugin_manager`。

## 架构决策

**为什么独立窗口而非塞进 picker？**
picker 定位是"项目选择器"——用户选完项目窗口就导航到 sidecar（inkos Studio web）。把插件管理塞进 picker 会定位混乱。独立窗口让插件管理在任何时候都可从 picker 入口打开，生命周期不受项目选择/sidecar 启动影响。

**为什么不用框架（React/Svelte）？**
inkosDesktop 的发布前端（picker）是单 HTML 文件，`withGlobalTauri` + vanilla JS，零构建链。settings.html 沿用同一风格，保持架构一致性，无新增构建依赖。

## 验证

- `cargo check`：通过
- settings.html / picker JS 语法校验（node vm.Script）：通过
- `cargo test --lib`：311 passed / 0 failed
- `cargo clippy --all-targets -- -D warnings`：零告警

## 完整插件系统现状

| 能力 | 状态 |
|---|---|
| 数据结构与权限模型 | ✅ |
| 生命周期管理（安装/卸载/启用/禁用） | ✅ |
| Host API（路径沙箱 + 审计） | ✅ |
| 进程隔离执行（真执行，端到端验证） | ✅ |
| WASM 沙箱骨架（加载编译为真） | ✅ |
| Tauri 命令层（7 命令） | ✅ |
| **前端管理面板（本次）** | ✅ |
| WASM Component Model 完整调用 | ⏳ 独立阶段（Phase 6 的 6.3） |

插件系统对终端用户现在**端到端可用**：picker 入口 → 管理面板 → 安装/启用/禁用/卸载；开发者写的进程型插件可通过 JSON-RPC 真执行。
