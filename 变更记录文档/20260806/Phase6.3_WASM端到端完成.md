# Phase 6.3 完成：WASM Component Model 端到端执行

> 日期：2026-08-07
> 提交：`830f8750`（第4步）、`4dde9f34`（第5步）
> 之前：`7e7aa92d`（bindgen）、`d335498d`（Host trait）

## 完成：6.3 全部 5 步

WASM 插件现在**真正可执行**（强隔离沙箱），与进程隔离插件双引擎完整。

| 步 | 交付 |
|---|---|
| 1 契约 | `wit/inkos.wit`（host + plugin interface + inkos-plugin world） |
| 2 绑定 | `wasmtime::component::bindgen!` 编译期生成 InkosPlugin 类型 + Host trait |
| 3 Host trait | `inkos::plugin::host::Host for PluginState`，委托 HostContext（复用 capability 校验） |
| 4 execute | Module→Component + Linker + `InkosPlugin::instantiate` + `inkos_plugin_plugin().call_invoke` |
| 5 端到端测试 | 真实 .wasm component（编译自 examples/wasm-plugin）3 测试通过 |

## 端到端链路（验证通过）

```
examples/wasm-plugin (wit-bindgen guest, wasm32-wasip2)
  → inkos_example_plugin.wasm component
宿主 WasmPlugin::new(Component::from_file)
  → execute: Linker 注册 WASI(add_to_linker_sync) + inkos host imports(add_to_linker)
  → InkosPlugin::instantiate(store, component, linker)
  → bindings.inkos_plugin_plugin().call_invoke(command, args_json)
  → 插件返回 result<string,string>（双层 Result：wasmtime trap + 插件语义）
  → JSON 反序列化
```

端到端测试（`tests/wasm_plugin_execution.rs`，3 passed）：
- echo 回显 args
- ping → {pong: true}
- 未知命令 → 插件 Err（execute 转 PluginError）

## 双引擎完整

| 引擎 | entrypoint | 隔离 | 端到端测试 |
|---|---|---|---|
| 进程隔离 | 脚本/可执行 | OS 进程 | 5（hello-plugin/plugin.sh） |
| WASM 沙箱 | .wasm component | Wasmtime 沙箱 + Fuel/Epoch | 3（examples/wasm-plugin） |

两者走同一权限模型：Host trait 委托 HostContext（capability 校验 + 路径沙箱）。

## 关键技术点

- **Component vs Module**：Component Model 类型化（wit 接口），core Module 是无类型；6.3 用 Component
- **WASI 注册**：wasm32-wasip2 target 默认依赖 wasi:io/poll，必须 `wasmtime_wasi::add_to_linker_sync`，否则实例化失败
- **双层 Result**：`call_invoke` 返回 `Result<Result<String,String>, TrapError>`——外层 wasmtime 资源/trap，内层插件语义 result
- **guest 路径**：wit-bindgen guest 的 export interface trait 在 `exports::inkos::plugin::plugin::Guest`（编译验证）

## 验证

- `cargo test`（全量）：328 lib + WASM 端到端 3 + 进程隔离 5 + 安全更新 4 + 项目 12 + 其他集成，**0 失败**
- `cargo clippy --all-targets -- -D warnings`：零告警
- `examples/wasm-plugin` 编译（wasm32-wasip2 + wit-bindgen 0.30）：成功，产真实 component

## 工具链

- 宿主：wasmtime 27（component-model + cranelift）+ wasmtime-wasi 27
- 插件 SDK：wit-bindgen 0.30 + wasm32-wasip2 target

Phase 6.3 完成。插件系统双引擎端到端可执行。
