# 插件运行时：WASM Linker 复用（execute 性能优化）

> 日期：2026-08-07
> 范围：`src-tauri/src/plugin/runtime.rs`
> 目标：「高性能 0GC」——消除 execute 热路径的重复 Linker 构造/注册开销

## 问题

`WasmPlugin::execute` 每次**重建 `Linker`** 并重新注册 WASI（`add_to_linker_sync`）+ host imports（`InkosPlugin::add_to_linker`）。`Engine` 与 `Component` 已复用，但 Linker 重建是非平凡开销（注册所有 WASI 函数 + host trait 绑定），在 execute 热路径上每次重复。

## 变更

`WasmPlugin` 新增 `linker: Linker<PluginState>` 字段：
- **`new`**：一次性构建 Linker（`Linker::new` + `add_to_linker_sync` + `InkosPlugin::add_to_linker`），存为字段。
- **`execute`**：移除每次的 Linker 重建 + 注册，改用 `&self.linker` 实例化。

## 设计

- **Wasmtime Linker 是实例化模板**：注册 host 函数定义，**不持有 Store 状态**。跨多次 `instantiate` 复用是其设计用途（标准 perf 模式）。
- **状态隔离不变**：每 `execute` 仍 `create_store()`（新 Store + 新 `PluginState` + 新 fuel/epoch），保证一次执行 = 干净实例。Linker 复用不破坏隔离。
- **`Linker<T>` Send+Sync**：可作 `WasmPlugin` 字段（线程安全共享）。

## 验证

- `cargo clippy --all-targets -D warnings`：clean
- `cargo test`：lib **383 passed / 0 failed** + 全集成 0 失败，含：
  - `wasm_plugin_execution`：echo roundtrip / ping / unknown-command（行为保持）
  - `perf_plugin_wasm`：`wasm_execute_echo_latency_acceptable` 延迟回归门（复用后仍达标）
- 行为零变化（纯 perf：减少重复构造/注册，结果一致）

## 收益

execute 热路径省去每次 Linker 构造 + WASI/host 重注册开销。对高频插件调用（如格式化器每 keystroke 调用）累积收益显著。具体幅度由 `criterion --bench plugin_execute` 量化（既有 bench）。
