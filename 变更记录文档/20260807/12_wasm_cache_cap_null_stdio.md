# 变更记录：wasm_cache 容量上限 + WASI null stdio

## 日期
2026-08-07

## 变更类型
feat(security/perf): wasm_cache 32 条容量守卫 + WASI null stdio

## 1. wasm_cache 容量上限

### 问题
`wasm_cache: HashMap<String, WasmPlugin>` 随已安装插件数无界增长。
每条编译产物约数 MB（Cranelift AOT），大量插件可耗尽进程内存。

### 修复
新增 `const WASM_CACHE_CAPACITY: usize = 32`，在两处 `wasm_cache.insert` 前
加守卫：满时驱逐 keys().next()（任意一条），带 `tracing::debug!` 记录。

## 2. WASI null stdio

### 问题
`WasiCtxBuilder::new().inherit_stdio()` 让插件能读宿主 stdin。
在 Tauri GUI 场景无意义，但若 app 在 CLI 启动时有控制终端，插件可借此读取用户输入。

### 修复
改为 `WasiCtxBuilder::new().build()`（空 stdio，不继承任何 fd）。
插件调试输出应通过 `host.log` WIT 接口而非 stdio。

## 累计测试数
511 passed, 0 failed（全量）
