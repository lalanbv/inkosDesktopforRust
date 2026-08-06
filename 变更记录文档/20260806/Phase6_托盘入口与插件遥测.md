# Phase 6 推进：托盘入口 + 插件执行遥测

> 日期：2026-08-06
> 提交：`03032b59`（前端面板）、`d6a61389`（托盘入口）、`2ffa007e`（遥测）
> 对应 Phase 6 规划：6.4（前端面板，完整）+ 6.2（遥测，最小版）

## 6.4 前端管理面板（完整）

### settings.html（picker/ 目录，沿用 vanilla JS + withGlobalTauri）
- 列表：名称/版本/作者/权限徽章/启用状态
- 启用/禁用 → `enable_plugin`/`disable_plugin`
- 卸载（带 confirm）→ `uninstall_plugin`
- 从目录安装（复用 `cmd_pick_project_dialog`）→ `install_plugin`

### 双入口（全生命周期可达）
1. **picker 底部链接**「⚙ 插件管理」→ `cmd_open_plugin_manager`
2. **托盘菜单**「插件管理…」→ 复用 `open_manager_window`

公共窗口逻辑抽到 `plugin::commands::open_manager_window`（已存在则聚焦，否则新建 720×560 窗口），命令与托盘共用，避免两处漂移。

## 6.2 插件执行遥测（最小版）

### 无锁原子统计（热路径零分配）
`PluginManager` 加三个 `AtomicU64`（Relaxed 排序）：
- `exec_count`：execute_plugin 调用次数（含失败）
- `exec_total_us`：累计耗时（微秒）
- `exec_failures`：失败次数

### 计时埋点
`execute_plugin` 用 `Instant::now`→`elapsed`，结束无锁累加；失败时 `exec_failures+1`。真实执行逻辑下沉到 `execute_plugin_inner`，保持埋点与逻辑分离。

### 暴露
- `PluginManager::metrics()` → `PluginMetrics { exec_count, exec_total_us, exec_failures, avg_us }`
- `get_plugin_metrics` Tauri 命令，前端/诊断可查

### 价值
插件调用性能可观测——次数、总/均耗时、失败率，为优化和故障定位提供数据。读取近似（Relaxed），对监控足够；精确快照需锁，不值得。

## 验证

- `cargo test --lib`：313 passed / 0 failed（含 `test_metrics_initial_zero`、`test_metrics_counts_failures`）
- `cargo clippy --all-targets -- -D warnings`：零告警
- settings.html / picker JS 语法校验（node vm.Script）：通过

## Phase 6 进度

| 子项 | 状态 |
|---|---|
| 6.4 前端管理面板 | ✅ 完整（面板 + picker 入口 + 托盘入口） |
| 6.2 遥测 | ✅ 最小版（插件执行 metrics） |
| 6.5 生产签名 | ⏳ CI 注入密钥，部署侧 |
| 6.1 增量更新 | ⏳ 周级工程，需服务端 delta 管道 |
| 6.3 WASM Component Model | ⏳ 周级工程，需 wit + bindgen |
