# 插件系统文档

> 本文档由契约护栏测试（`src-tauri/tests/plugin_doc_contract.rs`，568 号）锁定到代码事实：
> WIT 接口函数名、能力类型、Tauri 命令名单均为护栏断言项——改动对应代码而不更新本文档会
> 在 `cargo test` 门禁红。R46 漂移清偿：本文件 568 号前把管理界面误写为另一种前端框架、
> 把开发模型误写为裸 extern "C" 导出，与实现（React studio 无插件面 + 桌壳 settings.html
> 管理窗 + WIT Component Model）漂移，已按现状如实改写。

## 概述

Inkos Desktop 插件系统提供**双执行路径**的第三方扩展：

1. **WASM 路径**（`plugin.wasm` 入口）——wasmtime Component Model 强沙箱：
   WIT 接口契约（`src-tauri/wit/inkos.wit`）+ `component::bindgen!` 类型化调用 +
   fuel/epoch/内存/函数表四层资源限制。插件对宿主能力的调用走 `host` 接口，
   每次调用经 capability 校验。
2. **进程隔离路径**（其他可执行入口）——JSON-RPC over stdio：宿主 spawn 长驻
   子进程（`PluginProcess`），首次调用 spawn 后续复用，含死进程检测。
   此路径**无宿主能力回调**（不暴露 Host API，备案 568 号）。

两条路径由入口文件扩展名分派（`PluginManager::execute_plugin_inner`）：
`.wasm` 走沙箱引擎（编译产物 LRU 缓存），其余走进程隔离。

## 架构

```
┌──────────────────────────────────────────────┐
│   桌壳管理窗（src-tauri/picker/settings.html，│
│   原生 JS + Tauri invoke；React studio 无插件面）│
└───────────────────┬──────────────────────────┘
                    │ Tauri 命令（list_plugins / execute_plugin / …）
┌───────────────────▼──────────────────────────┐
│   src-tauri（Rust）                           │
│                                              │
│   PluginManager（manager.rs）                 │
│   - install / uninstall / enable / disable   │
│   - 按入口扩展名分派执行路径                    │
│   - 遥测（次数/耗时/失败）+ 连续失败自动禁用      │
│                                              │
│   ┌──────────────── WASM 路径 ─────────────┐ │
│   │ runtime.rs：wasmtime Component Model   │ │
│   │  wit/inkos.wit → component::bindgen!   │ │
│   │  Host trait 委托 host_api::HostContext │ │
│   │  （capability 校验 + 路径沙箱）          │ │
│   │  fuel / epoch / 内存 / 函数表 限制       │ │
│   └────────────────────────────────────────┘ │
│   ┌────────────── 进程隔离路径 ─────────────┐ │
│   │ process.rs：JSON-RPC over stdio        │ │
│   │  长驻子进程 + 死进程检测                 │ │
│   │  （无宿主能力回调，备案 568 号）          │ │
│   └────────────────────────────────────────┘ │
└──────────────────────────────────────────────┘
```

## 插件结构

每个插件是一个包含以下文件的目录：

```
my-plugin/
├── plugin.toml          # 插件 manifest（必需）
├── plugin.wasm          # WASM 入口（或可执行文件 → 进程隔离路径）
└── README.md            # 说明文档（可选）
```

### plugin.toml 格式

```toml
[plugin]
id = "my-plugin"                    # 唯一标识符
name = "My Plugin"                  # 显示名称
version = "1.0.0"                   # 语义化版本
description = "Plugin description"  # 简短描述
author = "Your Name"                # 作者
homepage = "https://..."            # 主页（可选）
license = "MIT"                     # 许可证
abi_version = "1"                   # ABI 版本
entrypoint = "plugin.wasm"          # 入口文件（.wasm → WASM 路径；其他 → 进程路径）

# 依赖的其他插件（id → 版本约束）
[dependencies]

# 权限声明
[[capabilities]]
type = "read_project"               # 读取项目文件

[[capabilities]]
type = "write_project"              # 写入项目文件

[[capabilities]]
type = "filesystem"                 # 文件系统访问（指定路径）
path = "./data"

[[capabilities]]
type = "network"                    # 网络访问（域名白名单）
domains = ["api.example.com"]

[[capabilities]]
type = "system_command"             # 系统命令执行（命令白名单）
allowed_commands = ["ffmpeg"]       # fail-closed：缺省空名单 = 无命令可执行
```

## 权限模型

### 权限类型（`plugin/types.rs` `Capability` 枚举，五值）

1. **read_project** - 读取工作目录内文件
2. **write_project** - 写入工作目录内文件
3. **filesystem** - 指定路径访问（`path` 字段，相对工作目录，防路径逃逸）
4. **network** - HTTP GET 域名白名单（`domains` 字段）
5. **system_command** - 系统命令执行，**命令白名单**（`allowed_commands` 字段；
   缺省为空白名单即 fail-closed 无命令可执行——对称 Network 模型，非裸开关）

权限校验单点在 `host_api::HostContext`：每次 Host API 调用先查 capability、
再规范化路径防逃逸、再校验白名单，最后执行。进程隔离路径 spawn 时的命令
白名单与 `host_api` 共用同一单点（`apply_env_allowlist`），防两侧策略漂移。

## Host API（仅 WASM 路径）

WIT 契约 `src-tauri/wit/inkos.wit` 的 `host` interface（宿主暴露给插件）：

```wit
read-file: func(path: string) -> result<string, string>;      # 需 read_project / filesystem
write-file: func(path: string, content: string) -> result<_, string>;  # 需 write_project / filesystem
list-dir: func(path: string) -> result<list<string>, string>; # 需 read_project
http-get: func(url: string) -> result<string, string>;        # 需 network + 域名白名单
log: func(level: string, message: string);                    # 结构化日志（error/warn/info/debug/trace）
exec-command: func(command: string, args: list<string>) -> result<exec-result, string>;
                                                              # 需 system_command + 命令在白名单内
```

宿主侧由 bindgen 生成的 `Host` trait 承接（`plugin/runtime.rs`），逐方法委托
`HostContext` 复用同一套权限模型——WASM 插件与进程隔离插件不另造权限。

> **进程隔离路径差异（568 号备案）**：进程插件与宿主之间只有 JSON-RPC 的
> invoke 通道（宿主→插件），插件**无法反调宿主能力**（无 Host API 回调通道）。
> 需要宿主能力的插件应使用 WASM 路径。

## WIT 插件契约

WASM 插件实现 `plugin` interface（三个方法）：

```wit
init: func(config: string) -> result<_, string>;              # 宿主每次 execute 前调用（Store 状态隔离）
invoke: func(command: string, args: string) -> result<string, string>;  # 核心：JSON 入 JSON 出
on-event: func(event: string, payload: string);               # 可选事件钩子，失败不阻断
```

语义与进程隔离插件的 JSON-RPC 方法名一致，便于上层统一。

## 执行与资源限制（WASM 路径）

- **fuel**：默认 10M 条指令，死循环毫秒级中断。
- **epoch**：默认 10 tick deadline。
- **内存**：单实例 64 MiB（超限 `memory.grow` 得 OOM trap，不 OOM 宿主）。
- **函数表**：单实例 1M 元素（防 table.grow 攻击）。
- **stdio 零继承**：插件不读宿主 stdin / 不写宿主 stdout；调试输出走 `log` 接口。
- **编译缓存**：Cranelift AOT 产物按插件 id LRU 缓存（uninstall/update 逐出）。

## 管理与健康

### Tauri 命令（`plugin/commands.rs` + `commands/registry.rs`）

```typescript
// 插件管理
await invoke("list_plugins");
await invoke("get_plugin", { id });
await invoke("install_plugin", { path });
await invoke("uninstall_plugin", { id });
await invoke("enable_plugin", { id });
await invoke("disable_plugin", { id });
await invoke("execute_plugin", { id, command, args });
await invoke("get_plugin_metrics");
await invoke("cmd_broadcast_event", { event, payload });

// 插件管理窗（原生 settings.html；React studio 无插件面）
await invoke("cmd_open_plugin_manager");

// 插件注册表（远程源安装与更新）
await invoke("cmd_fetch_plugin_registry");
await invoke("cmd_install_from_registry");
await invoke("cmd_update_plugin_from_registry", { id });
await invoke("cmd_check_plugin_updates");
await invoke("cmd_list_plugin_versions", { id });
```

### 健康与遥测

- **执行遥测**：每次 `execute_plugin` 记录次数/耗时（μs）/失败数，经 tracing
  结构化日志（`inkos.plugin.exec` target）落 JSON 日志，`get_plugin_metrics` 暴露聚合。
- **连续失败自动禁用**：同一插件连续失败达阈值自动 `disable_plugin` 并发健康
  事件（`inkos.plugin.health` target），防异常插件持续拖慢运行时；成功归零计数。

## 开发 WASM 插件（Component Model）

1. 以 **wasm32-wasip2** 目标构建实现 `inkos:plugin` world 的 component
   （`wit-bindgen` 生成插件侧绑定；契约见 `src-tauri/wit/inkos.wit`）。
2. 实现 `init` / `invoke` / `on-event` 三方法（`invoke` 收 JSON 字符串返 JSON 字符串）。
3. 宿主能力经导入的 `host` interface 调用（受 capability 运行时校验）。
4. 目录放入 `plugins_dir`，在管理窗安装并审查权限请求。

## 开发进程隔离插件

1. 任意语言实现 JSON-RPC 2.0 over stdio 的可执行程序：
   读宿主请求（`RpcRequest`：`jsonrpc`/`id`/`method`/`params`）、写响应
   （`RpcResponse`：`result` 或 `error`）。协议封装见 `plugin/protocol.rs`。
2. manifest `entrypoint` 指向该可执行文件；首次 invoke 时宿主 spawn 并长驻复用。
3. 无宿主能力回调（见上）；命令白名单经 `apply_env_allowlist` 注入子进程环境。

## 安全考虑

1. **双路径隔离**——WASM 沙箱（Component Model + 四层资源限制）与进程隔离（OS 进程边界）。
2. **权限最小化**——只声明必需权限；`system_command` 白名单 fail-closed。
3. **路径验证**——HostContext 规范化路径防逃逸。
4. **审计**——敏感操作与执行遥测落结构化 JSON 日志。
5. **零信任缺省**——无 capability 声明即无能力；空白名单即无命令。

## 限制和约束

- 插件不能直接访问 Tauri 命令（命令面属宿主 UI 层）。
- 不支持热重载（安装/启停经管理界面操作，执行路径按需重载）。
- 不支持插件间通信。
- 进程隔离路径无宿主能力回调（需宿主能力请用 WASM 路径）。

## 故障排除

- **插件无法加载**：检查 plugin.toml 格式、入口文件存在性；WASM 入口编译失败
  会在装载时即报错（坏文件/不合法 wasm 不等第一次 execute）。
- **权限被拒绝**：确认 capabilities 声明、路径在允许范围、命令在 `allowed_commands`
  白名单内。
- **插件被自动禁用**：连续失败达阈值触发（见「健康与遥测」），修复后手动重新启用。
- **日志**：应用数据目录（app_data）下 `logs/`（`paths.rs` `log_dir`），结构化 JSON 格式。
