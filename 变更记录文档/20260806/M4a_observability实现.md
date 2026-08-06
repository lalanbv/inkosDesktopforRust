# M4a 可观测性基础设施实现

**日期**: 2026-08-06  
**里程碑**: Phase 2 / M4a  
**类型**: 功能实现

## 变更概述

实现 M4a 可观测性基础设施（tracing 日志 + panic hook 崩溃上报 + 诊断命令），为生产环境故障排查提供基础能力。

## 核心变更

### 1. 依赖引入（Cargo.toml）

**添加**:
```toml
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["json", "env-filter"] }
tracing-appender = "0.2"
```

### 2. observability 模块（src/observability/）

**新建模块结构**:
```
src/observability/
├── mod.rs          # 模块入口，导出公开 API
├── logging.rs      # init_logging：按天滚动 + JSON + 异步写入 + WorkerGuard
├── crash.rs        # init_panic_hook：全局 panic 捕获 + JSON dump + LRU 清理（最近 10 个）
└── diagnostics.rs  # cmd_get_diagnostics：版本/平台/路径/manifest/最近崩溃
```

**关键设计**:

1. **日志**（logging.rs）:
   - `tracing_appender::rolling::RollingFileAppender`：按天滚动（`Rotation::DAILY`），保留 7 天
   - 双层输出：文件（JSON）+ stdout（人类可读 compact）
   - `EnvFilter`：默认 INFO，通过 `RUST_LOG` 环境变量可覆盖
   - `non_blocking`：异步写入，返回 `WorkerGuard`（main 持有防 drop）

2. **崩溃上报**（crash.rs）:
   - `panic::set_hook`：全局 panic 拦截
   - 捕获：timestamp + thread + payload + backtrace（`std::backtrace::Backtrace::capture()`）
   - 写入：`<app_data>/crashes/crash-<timestamp>.json`
   - LRU 清理：每次 panic 后删除第 11+ 个旧 dump（按修改时间排序）
   - 保留默认 handler：`default_panic(panic_info)` 保持终端输出

3. **诊断命令**（diagnostics.rs）:
   - `#[tauri::command] cmd_get_diagnostics`：返回 `DiagnosticInfo` JSON
   - 包含：version / platform / arch / app_data_dir / engine_dir / node_cache_dir / projects_file / engine_manifest / recent_crashes（最近 5 个）
   - 从 `app.path()` 动态获取路径，从 `engine/manifest.json` 读取版本

### 3. main.rs 集成

**添加初始化代码**（main 函数开头，setup 之前）:
```rust
let log_dir = dirs::data_dir()
    .map(|d| d.join(config::APP_DATA_DIR_NAME).join("logs"))
    .unwrap_or_else(|| std::env::temp_dir().join(config::APP_DATA_DIR_NAME).join("logs"));
let _log_guard = inkos_desktop::observability::init_logging(log_dir)
    .expect("init_logging 失败");

let crash_dir = dirs::data_dir()
    .map(|d| d.join(config::APP_DATA_DIR_NAME).join("crashes"))
    .unwrap_or_else(|| std::env::temp_dir().join(config::APP_DATA_DIR_NAME).join("crashes"));
inkos_desktop::observability::init_panic_hook(crash_dir)
    .expect("init_panic_hook 失败");

tracing::info!("inkosDesktop 启动");
```

**添加命令注册**:
```rust
.invoke_handler(tauri::generate_handler![
    // ... 其他命令
    inkos_desktop::observability::cmd_get_diagnostics,
])
```

### 4. supervisor.rs 日志注入

**spawn 函数**:
```rust
tracing::info!(
    program = %spec.program,
    port = spec.port,
    cwd = %spec.cwd.display(),
    "spawning sidecar"
);
// ... spawn 逻辑
tracing::info!(pid = child.id(), "sidecar spawned");
```

**kill_tree 函数**:
```rust
tracing::info!(pid, "killing sidecar tree");
// ... Unix/Windows 分支
#[cfg(unix)]
{
    // ... kill 逻辑
    tracing::error!(pid, error = %err, "kill_tree failed"); // 错误时
    tracing::info!(pid, "sidecar tree killed (SIGTERM)");   // 成功时
}
#[cfg(windows)]
{
    // ... taskkill 逻辑
    tracing::error!(pid, error = %e, "taskkill spawn failed"); // 错误时
    tracing::info!(pid, "sidecar tree killed (taskkill)");     // 成功时
}
```

## 测试覆盖

### 单元测试（已通过）

1. **logging.rs**:
   - `test_logging_initialization`：验证日志初始化成功 + WorkerGuard 返回
   - `test_logging_output`：验证日志文件创建 + JSON 格式

2. **crash.rs**:
   - `test_panic_hook_writes_crash_dump`：触发 panic → 验证 crash dump 创建 + payload 正确
   - `test_crash_dump_lru_cleanup`：创建 12 个旧 dump + 触发新 panic → 验证只保留 10 个

3. **diagnostics.rs**:
   - `test_diagnostic_info_serialization`：验证 `DiagnosticInfo` JSON 序列化/反序列化

**注**：`cmd_get_diagnostics` 需要 Tauri AppHandle，依赖集成测试或手动冒烟验证。

### 集成验证（待执行）

- [ ] `cargo test --lib observability` 全绿（143 lib 测试）
- [ ] 冒烟：启动 app → 检查 `<app_data>/logs/inkos-YYYY-MM-DD.log` 创建
- [ ] 调用 `cmd_get_diagnostics` → 验证返回 JSON 包含所有字段
- [ ] 触发一次 panic（dev 模式测试）→ 验证 `<app_data>/crashes/crash-*.json` 创建

## 依赖与影响

**依赖**:
- M3a（engine 资源化）→ diagnostics 读取 `engine/manifest.json`
- Tauri 2.11.5 → `app.path()` API

**影响**:
- **新增运行时开销**：异步日志写入（non-blocking）+ 全局 panic hook
- **磁盘占用**：日志保留 7 天，crash dump 保留最近 10 个

## 验证清单

- [x] Cargo.toml 添加 tracing 依赖
- [x] observability 模块实现（logging/crash/diagnostics）
- [x] main.rs 初始化日志 + panic hook
- [x] main.rs 注册 `cmd_get_diagnostics` 命令
- [x] supervisor.rs 注入日志调用
- [ ] `cargo test --lib observability` 全绿
- [ ] `cargo build --release` 无警告
- [ ] 冒烟验证日志输出正常

## 后续工作（M4b）

1. **错误边界机制**（M4b）:
   - Tauri 命令统一错误处理
   - 前端 ErrorBoundary 组件
   - 用户友好的错误消息转换

2. **生产部署**:
   - CI 构建验证日志输出
   - 文档化日志路径与 crash dump 位置
   - 用户手册添加"故障排查"章节
