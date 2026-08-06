# Phase 5：插件系统实现

> 日期：2026-08-06
> 提交：`2cb1c672`
> 设计：商业级扩展架构——安全沙箱 + 细粒度权限 + 热加载 + ABI 版本管理

## 目标

为 inkosDesktop 构建可执行的第三方扩展系统。核心约束：安全（沙箱 + 最小权限）、可实施执行（不是空壳）、向后兼容（ABI 版本化）、高性能 0GC。

## 架构决策

### 双引擎分派（务实最优解）
`PluginManager::execute_plugin` 按 entrypoint 后缀分派：
- **`*.wasm` → WasmPlugin**（Wasmtime 27 沙箱）：Engine 加载/编译为真；类型化调用需 Component Model wit 绑定（独立大工程），未就绪时返回**明确错误**而非假装执行。
- **脚本/可执行 → PluginProcess**（进程隔离）：JSON-RPC over stdio，长驻进程缓存复用。**立即真正可执行**。

不二选一：进程隔离保证「今天就能用」，WASM 作为「未来更强隔离」的增强路径，协议不变。

## 交付物（src-tauri/src/plugin/）

| 模块 | 职责 | 测试 |
|---|---|---|
| `types.rs` | 权限模型（Capability 细粒度）+ 错误类型 + PluginMetadata | 3 |
| `manifest.rs` | plugin.toml 解析（顶层 capabilities 字符串数组） | 4 |
| `protocol.rs` | JSON-RPC 2.0（请求/响应/通知/错误码） | 4 |
| `process.rs` | 子进程 spawn/call/notify + 生命周期 | 1 |
| `host_api.rs` | HostContext：read/write/list/exec，路径沙箱 + 审计 | 6 |
| `runtime.rs` | Wasmtime Engine（Component Model + Fuel + Epoch 超时）+ WASI | 2 |
| `manager.rs` | 安装/卸载/启用/禁用 + 双引擎 execute 分派 | 3 |
| `commands.rs` | 7 个 Tauri 命令 + PluginState | 2 |

集成测试：`tests/plugin_execution.rs`（5 用例，端到端真执行）

## 真正可执行的证据

示例插件 `examples/plugins/hello-plugin/plugin.sh` 实现 JSON-RPC 契约（echo/upper/health）。
端到端测试链路：安装 → spawn 长驻进程 → 发 JSON-RPC 请求 → 拿回真实结果。

```
execute_echo_round_trip           ✓ {hello:world} → echo 回显
execute_upper_transforms_text     ✓ "hello inkos" → "HELLO INKOS"
execute_health_returns_metadata   ✓ 返回 status/name/version
execute_unknown_method_returns_error ✓ 未知方法 → JSON-RPC error
execute_disabled_plugin_is_rejected ✓ 禁用插件 → 拒绝执行
```

## 安全机制

1. **声明式权限**：plugin.toml 声明所需 Capability（read_project/write_project/filesystem:路径/system_command/network）
2. **运行时强制检查**：每次 Host API 调用前验证权限，缺失即 PermissionDenied
3. **路径沙箱**：`normalize_path` 解析 `..` 与符号链接，跨平台规范化（macOS `/var`→`/private/var`），逃逸即拒
4. **资源限制**：WASM Fuel 10M 指令 + Epoch 超时 10 秒，防死循环/卡死
5. **审计日志**：所有敏感操作（exec_command 等）写 tracing，含 plugin_id/command/args

## 兼容性

- ABI 版本字段 `abi_version`，不兼容时拒绝加载
- entrypoint 双模式：`.wasm`（沙箱）/ 脚本（进程隔离），任一语言可写插件
- 降级：插件管理器初始化失败不阻断应用启动，其他功能（引擎/工作区/配置）照常

## 修复的编译/逻辑缺陷

- `parse_manifest` 调用方曾传 plugin.toml 文件路径（应传插件目录）→ "Not a directory"
- 测试 manifest 用 `[[capabilities]] type=` 表数组，与 manifest.rs 的 `Vec<String>` 不匹配
- TOML 顶层键必须在首个 `[section]` 前，否则被吞进 `[plugin]` 表
- wasmtime v27：`WasiView` 需自持 `ResourceTable`（非从 WasiCtx 取）
- `HostContext` 重复 Clone impl（derive 与手动冲突）
- main.rs：13 个插件命令注册 + AppState 在既有 setup 回调内托管（Tauri setup 是单一回调，另起会静默覆盖）

## 验证

- `cargo test --lib plugin::`：26 passed / 0 failed
- `cargo test --test plugin_execution`：5 passed（端到端真执行）
- `cargo clippy --all-targets -- -D warnings`：零告警

## 依赖

```toml
wasmtime = { version = "27", features = ["component-model", "cranelift"] }
wasmtime-wasi = "27"
```

## 后续（独立阶段）

- Component Model wit 接口 + `wasmtime::component::bindgen!` 类型化调用（WASM 路径完整执行）
- 前端插件管理 UI（picker 当前是项目选择器，插件管理面板独立）
- 插件市场与自动更新、热重载、插件间通信、性能监控限流
