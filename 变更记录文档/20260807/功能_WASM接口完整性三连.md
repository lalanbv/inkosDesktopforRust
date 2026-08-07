# 功能：WASM 接口完整性三连（exec-command + init + on-event 全接线）

> 日期：2026-08-07
> 范围：`wit/inkos.wit`、`src/plugin/runtime.rs`、`src/plugin/manager.rs`、`src/plugin/commands.rs`、`src-tauri/src/bin/main.rs`、`picker/index.html`、`picker/settings.html`
> 触发：SystemCommand 白名单就位但 WIT 未接线 + WIT 契约声明的 `plugin.init`/`plugin.on-event` 宿主从不调用

## 缺陷（改动前）

1. **`exec-command` 未接线**：WIT `host` interface 无此方法，SystemCommand 白名单虽就位但 WASM 插件永远不可达（Host trait 的 `exec_command` 真实存在且有白名单检查，但 WIT 未暴露 → WASM 插件调不到）
2. **`plugin.init` 永不调用**：WIT 契约要求宿主在首次 invoke 前调 `init(config)`，但 `WasmPlugin::execute` 从不调用 → 插件依赖 init 做初始化时静默失败
3. **`plugin.on-event` 永不调用**：WIT 契约声明插件侧 hook，宿主应在项目打开/插件管理操作时调用，但无任何调用链 → WASM 插件事件 hook 永远静默

## 变更

### Slice 1：WIT exec-command 接线

#### `wit/inkos.wit`
- 新增 `record exec-result { stdout, stderr, exit-code }`
- 新增 `exec-command: func(command: string, args: list<string>) -> result<exec-result, string>`
- doc 注明仅允许白名单命令 + 需 `system_command` capability

#### `src/plugin/runtime.rs`
- `impl Host for HostContext` 新增 `exec_command` 实现：
  - 调用 `self.exec_command(command, args)` → `ExecCommandResponse`
  - 映射为 WIT `ExecResult { stdout, stderr, exit_code }` + `Result<_, String>`

### Slice 2：WIT init 调用 + on-event 基础设施

#### `src/plugin/runtime.rs`
- `WasmPlugin` 新增 `call_init(&mut store, config)` 公开方法（调 `plugin.init`）
- `WasmPlugin` 新增 `call_on_event(&mut store, event, payload)` 公开方法（调 `plugin.on-event`）
- `execute` 在 invoke 前**强制**调 `call_init("{}")`（空配置作默认值），失败 → 中止 → PluginError

#### `src/plugin/manager.rs`
- `WasmPlugin` 新增 `broadcast_event(&mut self, event, payload)` 方法：
  - 创建独立 Store（事件隔离）
  - 调 `call_init + call_on_event`，失败 warn + 继续（best-effort）
- `PluginManager` 新增 `broadcast_event(&mut self, event, payload)` 方法：
  - 遍历所有**已启用 WASM** 插件（跳过进程隔离 / 禁用插件）
  - 每个调 `plugin.broadcast_event`，单个失败 warn + 继续
- 测试新增 `test_broadcast_event_skips_process_isolation` / `test_broadcast_event_skips_disabled` / `test_broadcast_event_no_wasm_plugins`（无 WASM → no-op）

#### `src/plugin/commands.rs`
- 新增 `cmd_broadcast_event(event, payload, state)` Tauri 命令

#### `src-tauri/src/bin/main.rs`
- `.invoke_handler` 注册 `cmd_broadcast_event`

### Slice 3：前端事件广播接入

#### `picker/index.html`
- `chooseProject` 成功后调 `invoke("cmd_broadcast_event", {event: "project.opened", payload: {path}})`，best-effort（.catch warn）

#### `picker/settings.html`
- `togglePlugin` 成功后 → `plugin.enabled` / `plugin.disabled`
- `uninstallPlugin` 成功后 → `plugin.uninstalled`
- `installFromDir` + `installFromRegistry` 成功后 → `plugin.installed`（含 `{id, version}`）
- 所有广播 best-effort（.catch warn，不阻断 UI 流程）

## 覆盖链路

- **exec-command**：WASM plugin.wasm 调 `host.exec-command` → WIT bindgen → `HostContext::exec_command` → 白名单检查 + `std::process::Command` → 返回 stdout/stderr/exit-code
- **init**：`WasmPlugin::execute` → `call_init("{}")` → plugin.init → (插件初始化逻辑) → `invoke` 主流程
- **on-event 完整链**：picker 选项目 → `cmd_broadcast_event("project.opened")` → `PluginManager::broadcast_event` → 遍历已启用 WASM → `WasmPlugin::broadcast_event` → `call_init + call_on_event` → plugin.on-event

## 验证

- `cargo clippy --all-targets -- -D warnings`：clean（3 slice 均）
- `cargo test`：**397 passed / 0 failed**（+3 broadcast_event 测试）
- 51 commits，零回归
