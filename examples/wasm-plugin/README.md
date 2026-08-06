# inkos WASM 插件 SDK 脚手架

> Phase 6.3 插件作者侧。与宿主侧（`src-tauri/src/plugin/runtime.rs` 的 `wasmtime::component::bindgen!`）共享 `wit/inkos.wit` 契约。

## 这是什么

编写 inkos WASM 插件的最小模板：实现 `wit/inkos.wit` 的 `plugin` interface（`init`/`invoke`/`on-event`），编译为 WASM Component，安装到 inkosDesktop 即可被宿主类型化、沙箱化调用。

## 前置

```bash
rustup target add wasm32-wasip2   # Component Model target（Node 22+ / wasmtime 27 支持）
```

## 构建

```bash
cd examples/wasm-plugin
cargo build --target wasm32-wasip2 --release
# 产物：target/wasm32-wasip2/release/inkos_example_plugin.wasm
```

## 安装

把 `.wasm` 放到插件目录，配 `plugin.toml`（`entrypoint = "plugin.wasm"`）。格式参考 `src-tauri/examples/plugins/hello-plugin/plugin.toml`（进程隔离示例）；WASM 版仅 `entrypoint` 指向 `.wasm`。

## 契约

`wit/inkos.wit`（与宿主共享）：
- `host` interface（宿主暴露）：`read-file`/`write-file`/`list-dir`/`log`，受 capability 校验
- `plugin` interface（插件实现）：`init`/`invoke`/`on-event`
- `world inkos-plugin`：`import host` + `export plugin`

## 双引擎

inkosDesktop 插件双引擎分派（见 `src-tauri/src/plugin/manager.rs`）：
- `*.wasm` → WASM 沙箱（Component Model，强隔离）
- 脚本/可执行 → 进程隔离（JSON-RPC over stdio）

本脚手架面向 WASM 引擎。进程隔离见 `hello-plugin/plugin.sh`。

## 状态

宿主侧 `wasmtime::component::bindgen!` 已就绪（`runtime.rs`，编译期生成绑定 + Host trait）。本插件侧 `wit_bindgen::generate!` 生成 guest 绑定。完整端到端调用（宿主 `Component` 链接器 + `invoke` 类型化调用 + 真实 component 测试）是 Phase 6.3 剩余步骤，见 `src-tauri/wit/inkos.wit` 顶部「完整执行路径」。
