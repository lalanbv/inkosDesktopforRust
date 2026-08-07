# 变更记录：WASM 插件实例内存上限（StoreLimits）

## 日期
2026-08-07

## 变更类型
feat(security): WASM 插件单实例内存上限

## 问题
`create_store()` 仅设置 fuel + epoch 超时（CPU 资源），但未限制内存分配。
恶意/失控插件可通过 `memory.grow` 无限扩张，导致宿主进程 OOM。

## 修复
- 新增 `const WASM_MAX_MEMORY_BYTES: usize = 64 * 1024 * 1024`（64 MiB）
- `PluginState` 新增 `limits: StoreLimits` 字段
- `create_store()` 用 `StoreLimitsBuilder::new().memory_size(WASM_MAX_MEMORY_BYTES).build()`
  构建限制器，并通过 `store.limiter(|state| &mut state.limits)` 绑定
- 超限 `memory.grow` → 插件侧得到 OOM trap，宿主进程不受影响

## 累计测试数
511 passed, 0 failed（全量）
