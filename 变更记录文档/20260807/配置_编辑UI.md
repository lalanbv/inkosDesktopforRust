# 配置编辑 UI（settings 面板消费 User 层）

> 日期：2026-08-07
> 范围：`src-tauri/picker/settings.html`
> 依赖：前次提交 `c64627bb`（User/Global 配置层后端）
> 目标：为 settings 面板补齐可编辑的用户全局配置表单，闭合「User 层后端 → UI」特性弧

## 背景

User 层后端已就绪（`update_config`/`reset_config` 支持 `ConfigLayer::User`，常驻可写、无上下文门槛），但 settings 面板仅在诊断 JSON 里只读展示 config，无编辑入口。本次补齐表单 UI。

## 变更

### settings.html 新增「用户配置」section

- **表单字段**（覆盖 AppConfig 全部用户相关字段）：
  - 日志：level（select error/warn/info/debug/trace）、retention_days（number）
  - 更新：channel（select stable/beta/dev）、check_interval_hours（number）、auto_apply（checkbox）
  - 引擎：auto_download（checkbox）、version_policy（select latest/fixed/range + 条件文本输入）
  - 网络：proxy（text，空→null）、timeout_seconds（number）
- **读写**：`loadConfig` 调 `get_config` 填充表单；`saveConfig` 调 `update_config({layer:"user",config})`；`resetConfig` 调 `reset_config({layer:"user"})` 后重载。
- **i18n**：zh/en 字典全量补 `config.*` 键（标题/字段标签/选项/状态消息），复用既有 `data-i18n` + `applyI18n` 机制。
- **a11y**：`label[for]`+`fieldset`/`legend`+`role="status"`+`aria-live="polite"`+`aria-describedby`。
- **WCAG AAA 配色**：沿用上轮加固的 `#1a1a1a`/`#1d4ed8`/`#4b5563`/`#166534`/`#b91c1c` 调色板。
- **version_policy 条件控件**：select 切换时 `syncPolicyRows` 按值显示/隐藏 fixed/range 输入行；load 时据反序列化形态（`"latest"` / `{fixed}` / `{range}`）回填。

### 顺带修复：`.hidden` CSS 规则缺失（latent bug）

settings.html CSS 此前**缺少 `.hidden` 规则**，而 `#empty`（无插件提示）依赖 `classList.toggle("hidden")` 控制显隐——意味着该提示可能始终可见。新增 `.hidden { display:none !important }` 既支撑版本策略条件行，又修复此缺陷。

## 验证

- `node --check`（提取内联 JS 462 行）：语法 OK
- `cargo check`：通过（settings.html 为运行时静态资源，不影响编译；本次无 Rust 改动）
- 后端命令路径由前次提交的 9 个测试覆盖（`test_user_layer_update_and_reset_flow` 等）

## 设计取舍

- settings 面板独立打开（`cmd_open_plugin_manager` 不加载工作区/项目上下文），故表单基于 merged（≈ system + user）编辑。User 层的**字段级合并语义**（字段=默认值视为「本层未设置」）确保：仅用户实际改动的非默认值会成为 User 覆盖，保存整张表单不会误伤。
- proxy 非法（非 http(s):// 前缀）由 Rust `validate()` 拒绝，前端 catch 显示错误消息——前端不重复实现校验，单一来源。
- 数值字段无 Rust 校验，前端 `numVal` 仅做 NaN/负数兜底（回退默认），不强加与后端不一致的约束。

## 后续

- 端到端 GUI 验证（启动 Tauri 应用、打开面板、表单交互）需手动或 E2E 框架，未在本会话自动化。
