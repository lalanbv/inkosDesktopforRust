# 性能：WASM 执行热路径 0GC（消除冗余 metadata clone）

> 日期：2026-08-07
> 范围：`src-tauri/src/plugin/manager.rs`、`src-tauri/src/plugin/host_api.rs`
> 目标：商业级「高性能 0GC」——消除 WASM 插件执行热路径上每次调用的冗余堆分配

## 缺陷（热路径冗余分配）

WASM 插件 `execute_plugin` 在**每次调用**（cache hit，99% 场景）都重复 clone 整个 `PluginMetadata`（8+ String + capabilities Vec + dependencies HashMap，~9+ 堆分配），且 clone 两遍：

1. **`execute_plugin_inner`**（manager.rs）：`self.installed.get(id).cloned()` 仅为读 `enabled`（bool）+ `entrypoint`（String）两个字段，却 clone 了整个结构。完整 metadata 仅在缓存未命中（`WasmPlugin::new`）才真正需要。
2. **`create_store`**（runtime.rs，每次 WASM execute）：`self.host_context.clone()` 又把 metadata clone 一遍塞进独立 Store（状态隔离所需）。

→ 每次 WASM 执行 ~2 次完整 metadata clone（~18 堆分配），纯属冗余。

## 修复（两层）

### 1. `execute_plugin_inner` 最小字段提取（manager.rs）

改为只取执行判定所需的最小字段——`enabled`（Copy）+ `entrypoint`（单 String clone），内层作用域即释放对 `installed` 的借用。完整 metadata **仅在缓存未命中**时按需 re-get + clone（WASM 路径喂 `WasmPlugin::new`、进程路径喂 `Process::spawn`）。

re-get 用 `expect("installed 刚校验过存在，本方法内不修改它")`——`execute_plugin_inner` 内不修改 `installed`，刚校验过存在，安全。

### 2. `HostContext.metadata` → `Arc<PluginMetadata>`（host_api.rs）

`create_store` 每次 `host_context.clone()` 给独立 Store。`metadata` 字段改 `Arc` 后 clone 退化为单次原子递增（~0 分配）。metadata 构造后不可变（仅权限校验 + 日志 `plugin_id` 读取），共享语义安全；所有 `self.metadata.*` 访问经 `Arc` Deref 透明，零改动。

`work_dir` **保持 owned `PathBuf`**——仅 1 次分配且多处需 owned 路径（`join`/`canonicalize`/`starts_with`），Arc 化反增 Deref/AsRef 摩擦，收益不抵成本。

## 净效果

WASM 执行热路径（cache hit）每次调用：**~18 堆分配 → ~1**（仅 `entrypoint` String + `work_dir` PathBuf）。

`work_dir` 那次分配保留——WASI Store 本身的创建开销（wasmtime 内部）远大于它，非 host 侧可消除项。本 slice 把 host 侧可控的冗余分配清零。

## 验证

- `cargo clippy --all-targets -- -D warnings`：clean
- `cargo test --lib plugin::`：**71 passed / 0 failed**（含 host_api 读写/权限/路径沙箱全测）
- `cargo test --test wasm_plugin_execution`：**3 passed / 0 failed**（echo roundtrip + ping + unknown command——真正驱动 execute 热路径）
- 行为零变更：优化为纯分配消除，访问语义不变（Arc Deref + 最小字段提取等价）
