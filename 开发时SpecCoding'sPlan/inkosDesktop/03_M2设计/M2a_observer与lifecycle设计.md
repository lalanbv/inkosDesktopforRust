# M2a 设计 spec：observer + lifecycle + 信号钩子

> inkosDesktop Phase 1 M2a。基于架构 v1.2 §4/§6.2/§11 + M1 已实现的 supervisor/isolation/lifecycle 基建。
> 上游：Narcooo/inkos v1.6.3。仓库：`/Users/lalanbv/GitProject/inkosDesktopforRust`（mono-repo，master 已含 M1）。

| 项 | 值 |
|---|---|
| spec 版本 | 1.0 |
| 日期 | 2026-08-06 |
| 状态 | 设计已定（按用户"完成所有后续任务"指令推进），待 writing-plans |
| 前置 | M1（v0.1.0-m1）：supervisor/paths/isolation/lifecycle(SidecarState) 已就位 |
| 后续 | M2b（secrets Route A）独立 spec |

---

## 1. 目标（M2a 交付）

在 M1 可运行壳之上，补齐**原生桌面体验**与**进程韧性**：

- **observer**：旁路订阅 inkos SSE，把写作/agent 完成等事件路由为系统原生通知 + 托盘角标
- **lifecycle**：托盘图标 + 最小化到托盘保活后台写作（daemon）+ 退出确认
- **信号钩子**：SIGINT/SIGTERM/SIGBREAK → graceful kill_tree，补 M1 只覆盖正常关窗（RunEvent::Exit）、漏信号强杀的缺口（消解 M1 parked：SIGKILL 孤儿 + spawn→insert 窄竞态）
- **M1 清理**：删 supervisor 死 `INKOS_PROJECT_ROOT` env；main.rs eval 的 JS 字符串改 `serde_json::to_string`

非目标（M2b/M3）：keychain 密钥（M2b）、全局快捷键（M3）、loopback 运行时强制（M3）、Windows 端到端（M3）。

## 2. 模块设计

### 2.1 observer（`src-tauri/src/observer/`，每文件 <400 行）

- `SseClient`：订阅 `http://127.0.0.1:{port}/api/v1/events`（用 `reqwest` + 手写 SSE 解析，或 `eventsource-stream`；避免重依赖）。断线指数退避重连（上限 30s，复用 `config::HEALTH_PROBE_INTERVAL` 思路）。
- `EventHandler` trait（责任链，可插拔）：`fn handle(&self, event: &SseEvent) -> anyhow::Result<()>`。实现：`NativeNotifier`、`TrayBadge`。
- `Router`：事件名 → 命中处理器链。
- **默认路由表**（M2a 固定，M3 加配置 UI）：

| 事件 | NativeNotifier | TrayBadge | 备注 |
|---|---|---|---|
| `write:complete` / `draft:complete` | ✅（仅窗口失焦/最小化时） | +1 | 章节写完 |
| `book:created` | ✅（失焦时） | +1 | 新建书 |
| `agent:complete` | ✅（失焦时） | +1 | agent 完成 |
| `daemon:chapter` | ❌ | +1 | daemon 写完一章（静默通知，仅角标） |
| `log`/`tool:*`/`llm:progress`/`context:*`/`ping` | ❌ | ❌ | 调试日志，不打扰 |
| 未知事件 | ❌ | ❌ | 忽略（架构 §6.2 容错，防上游新增） |

- observer 是 Tauri 异步任务（非 sidecar 进程）。`SseClient::run` 内部对单次连接错误做退避重连、不退出；若整个 observer 任务 panic/结束，由一个轻量 **watcher 任务**（`tauri::async_runtime`）重启 observer。inkos/SPA 不受影响（observer 与 sidecar 进程独立）。
- 与 SPA 的关系：observer **只读** SSE，零写入 inkos，零修改 SPA。

### 2.2 lifecycle（扩 M1 `src-tauri/src/lifecycle.rs`）

- **托盘**（Tauri 2 `TrayIconBuilder`）：图标 + 菜单 `[显示窗口 | daemon: 运行中/已停 | 退出]`。
- **关窗→托盘保活**：窗口关闭事件 → 隐藏（`window.hide()`）不退出；daemon（studio 进程内 Scheduler）继续后台写作。
- **退出**：托盘"退出" 或 主菜单退出 → `cleanup_sidecar`（kill_tree）→ `app.exit(0)`。
- **托盘角标**：observer 的 `TrayBadge` 更新角标计数；点击托盘 → 显示窗口 + 清零角标。
- daemon 状态查询：observer 订阅的 `daemon:started/stopped` 事件维护状态，刷新托盘菜单项文案。

### 2.3 信号钩子（`src-tauri/src/lifecycle.rs` 或新 `signals.rs`）

- `tokio::signal::ctrl_c()`（跨平台 SIGINT/Ctrl+C）
- Unix：`tokio::signal::unix(SignalKind::terminate())`（SIGTERM）；可选 SIGHUP（重载配置，M2a 先忽略）
- Windows：`SIGBREAK`（随 ctrl_c 覆盖）
- 收到信号 → 调 `cleanup_sidecar`（kill_tree 整组）→ exit。**解决 M1 parked**：SIGKILL 仍无法捕获（OS 直接杀），但 SIGTERM/SIGINT（正常 `kill`/Ctrl+C 覆盖）现在能 graceful 清 sidecar，消除绝大多数孤儿场景。spawn→insert 竞态：信号钩子在 cleanup 时 take state，与 RunEvent::Exit 共用同一清理函数，竞态窗口由 `Mutex` 序列化。

### 2.4 M1 清理

- `supervisor::build_launch`：移除注入的 `INKOS_PROJECT_ROOT` env（死代码——`inkos studio` 用 cwd；架构 §6.1/§14 Q4 已修正）。保留 cwd=project_root（真正机制）。相应更新单测。
- `main.rs`：error eval 的 JS 字符串改 `serde_json::to_string(&msg)` 规范转义（defense-in-depth，消解最终审查 LOW）。

## 3. 数据流

```
启动（M1 已有）→ spawn sidecar → health_probe → WebView 加载 SPA
   → observer 开始旁路订阅 :{port}/api/v1/events
   → inkos 写作 → SSE 推 write:complete 等
   → observer Router → NativeNotifier（失焦时弹通知）/ TrayBadge（角标+1）
用户关窗 → hide 到托盘（daemon 保活）
托盘退出 / SIGTERM / Ctrl+C → cleanup_sidecar(kill_tree) → exit
```

## 4. 错误处理与降级

| 故障 | 处置 |
|---|---|
| SSE 连不上/断线 | 指数退避重连（上限 30s）；不影响 SPA |
| observer 任务 panic/结束 | watcher 任务重启 observer；inkos 不受影响（SseClient 内部对单次断线已退避重连） |
| 通知权限被拒 | 降级：仅托盘角标，日志提示 |
| 托盘不可用（无 DE） | 降级：普通窗口模式，信号钩子仍生效 |
| 信号清理时 sidecar 已退 | kill_tree 容忍 ESRCH（M1 已实现） |

无静默吞错：所有 fallible 路径显式 log。

## 5. 测试

- `Router` 路由表单测：mock `SseEvent` 每种事件名 → 断言命中 NativeNotifier/TrayBadge（用 spy handler 记录调用）。
- `SseClient` 重连：mock 断线 → 断言退避重试（用 fake stream）。
- 信号钩子：mock 信号 → 断言 cleanup_sidecar 被调（用可注入的 cleanup 闭包）。
- lifecycle：托盘菜单构建、角标计数 round-trip（不需真 GUI，测逻辑）。
- 清理回归：build_launch 不再含 INKOS_PROJECT_ROOT；JS 转义用 serde_json。
- 覆盖率 ≥80%（路由/清理纯逻辑全覆盖；SSE/通知/托盘 IO 路径尽力覆盖，余下记 deferred）。

## 6. 接口契约（供 writing-plans）

- `observer::SseClient::new(url) -> Self`；`async run(router: Arc<Router>) -> anyhow::Result<()>`（含重连）
- `observer::Router::default() -> Self`（内置默认路由表）；`fn dispatch(&self, ev: &SseEvent)`
- `observer::EventHandler` trait（NativeNotifier/TrayBadge 实现）
- `lifecycle::TrayController`：建托盘、隐藏窗口、更新角标、退出清理（复用 M1 `cleanup_sidecar`）
- `lifecycle::install_signal_hooks(cleanup: impl Fn() + Send + 'static)` —— observer 与 main 接入
- 复用 M1：`supervisor::{spawn, kill_tree}`、`lifecycle::{SidecarState, cleanup_sidecar}`

## 7. 与架构 v1.2 对齐

- §4 observer/lifecycle 模块 ✓
- §6.2 SSE 旁路容错（忽略未知事件、断线重连）✓
- §11 observer 处理器可插拔（EventHandler 责任链）✓
- §1.1 "最小化到托盘保活后台写作" ✓
- M1 parked 消解：SIGKILL/SIGTERM 孤儿、spawn→insert 竞态、死 env、JS 转义 ✓
