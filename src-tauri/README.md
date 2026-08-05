# inkos-desktop（Tauri 桌面壳）

inkos 的 Tauri / Rust 桌面客户端骨架。当前里程碑 **M2a**（observer + lifecycle + 信号钩子），覆盖：
开窗 → 拉起 sidecar → 健康探测 → WebView 加载 SPA → loopback 加固占位 → **observer 旁路 SSE 路由到原生通知 + 托盘角标** → **托盘保活（关窗隐藏）** → **SIGINT/SIGTERM 安全退出清理**。

## 模块速览

| 模块 | 职责 |
|------|------|
| `src/config.rs` | 跨任务共享常量（默认 Studio 端口、健康探针超时/间隔、CLI 入口相对路径） |
| `src/paths.rs` | 跨平台目录解析（`PathResolver` trait + `AppPaths` 实现） |
| `src/supervisor.rs` | sidecar 监督：`pick_free_port` + `build_launch` + `spawn`（进程组）+ `kill_tree` + `health_probe` |
| `src/lifecycle.rs` | `SidecarState`（Tauri managed state）+ `cleanup_sidecar`（退出清理）+ `BadgeCounter` + `TrayController` + `ExitingFlag` + `install_signal_hooks` |
| `src/isolation/` | loopback 加固：`LoopbackGuard` trait + macOS pf / Linux iptables / Windows netsh 占位实现 |
| `src/observer/` | SSE 旁路：`sse`（frame parser + 重连 client）→ `router`（默认路由表）→ `notifier`（原生通知）+ `tray_badge`（托盘角标） |
| `src/main.rs` | Tauri 二进制入口：setup 拉起 sidecar、Exit 清理、loopback guard 接入、observer/lifecycle 接线、关窗隐藏保活、信号钩子 |
| `src/lib.rs` | 库入口，导出公开模块（供集成测与 doctest 使用） |

所有模块均 < 500 行（单一职责约束）。

## M2a 能力一览

### 1. 系统通知（背景写作不打扰）

observer 旁路订阅 `http://127.0.0.1:<port>/api/v1/events`（与 SPA 同端口），按事件前缀路由：

| SSE 事件 | 行为 |
|---|---|
| `book:` / `write:` / `draft:` / `agent:` / `tool:` / `import:` / `audit:` / `revise:` | 系统通知 + 托盘角标 +1 |
| `daemon:chapter:*` | 仅托盘角标 +1（后台章节生成静默） |
| 其他（`log:` / `llm:` / 未知） | 静默忽略 |

通知仅在主窗口**未聚焦**时发送（隐藏/失焦/取不到 → 视为未在前台，保守多发不漏发）。

### 2. 托盘保活

关窗不退出：用户点窗口 X / Cmd+W / Alt+F4 → `prevent_close + hide()`，窗口入托盘。
退出途径（任一保证 cleanup）：托盘"退出"菜单 / SIGINT(Ctrl+C) / SIGTERM(Unix) / Cmd+Q(macOS) → set `ExitingFlag` → cleanup sidecar + release loopback + shutdown observer → exit。

托盘菜单三态：角标项（动态显示 `未读 N` / `无未读`）、显示、退出。

### 3. SSE 重连韧性（双层）

- 连接级：`SseClient::run` 内指数退避（500ms→1s→2s，封顶 5s），shutdown 预置即返回。
- 任务级：watcher 循环包裹 `SseClient::run`，异常返回 → 500ms 分段 sleep 重启。

### 4. 信号钩子（幂等 cleanup）

SIGINT/SIGTERM → cleanup 闭包（与 `RunEvent::Exit` 路径冗余执行同一组 take/store/release，全部幂等）：
set `ExitingFlag` + shutdown observer + take SidecarState + cleanup_sidecar（kill 整个进程组）+ release loopback（best-effort）→ `process::exit(0)`。

## 运行（M2a）

> 仓库是 **mono-repo**：`src-tauri/` 是桌面壳，仓库根是 inkos 上游（`packages/`、根 `package.json` 等）。下文命令默认从仓库根执行。

### 环境要求

| 工具 | 版本 | 备注 |
|------|------|------|
| Rust | stable | `rustup` 安装 |
| Node.js | 22+ | inkos CLI 入口 `packages/cli/dist/index.js` 需要 |
| **pnpm** | **10.x**（必须） | ⚠️ **pnpm 11 不兼容**：不再读 `package.json` 的 `pnpm.overrides`，`--frozen-lockfile` 必败。锁版本：`corepack enable && corepack prepare pnpm@10.34.5 --activate` |
| macOS | Xcode CLT | dev 模式签名（M1 不做公证，留 M3） |

### 步骤

```bash
# 1. 安装并构建 inkos（产物落到 packages/cli/dist/ 与 packages/studio/dist/）
./scripts/desktop-build-inkos.sh

# 2. 拉起桌面壳
cd src-tauri && cargo run
```

预期：窗口打开显示「启动中…」stub 页，几秒内自动跳转到 `http://127.0.0.1:<port>/`（默认从 `:4567` 起探测首个空闲端口）。**M2a 起追加**：sidecar 健康后自动启用托盘图标、observer 旁路 SSE、信号钩子；关窗变隐藏（点托盘"显示"恢复），托盘"退出"或 Ctrl+C / SIGTERM 触发 cleanup 并退出进程。

### 何处会降级

- **loopback 加固**：macOS `pfctl` / Linux `iptables` / Windows `netsh advfirewall` 均需特权。非特权启动时 guard.lock 失败，stderr 输出 `loopback guard: lock port=<p> 失败 ... 降级继续`，sidecar 仍绑 `0.0.0.0`（同网段可访问）。完整运行时强制待 M2（macOS SMJob/launchd）/ M3（Linux setuid、Windows WFP）。
- **端口选择**：`pick_free_port` 探测瞬间空闲，不保证 spawn 时仍空闲（TOCTOU）。若被抢，`health_probe` 30s 内拿不到 2xx，窗口 title 显示「inkos 启动超时 (30s)，见日志」。

## 测试

```bash
cd src-tauri && cargo test                  # 单测 + 集成测（mock）+ doctest，全绿
cd src-tauri && cargo test -- --ignored     # 包含真实 inkos sidecar 集成测（需先跑构建脚本）
```

**稳定性**：单测/集成测/doctest 共 69 个（61 lib + 1 bin + 3 observer integration + 2 supervisor integration mock + 2 doctest），连续 3 次全量跑全绿，无 flaky。1 个 `#[ignore]`（`real_inkos_sidecar_serves_spa`）沿用 M1 选项，需先跑 `desktop-build-inkos.sh` 才有意义。

## 覆盖率

```bash
cd src-tauri && cargo llvm-cov --workspace --html --output-dir target/llvm-cov/html
open target/llvm-cov/html/index.html
```

工具：`cargo-llvm-cov 0.8.7`（macOS 原生支持，比 tarpaulin 更稳）+ `llvm-tools-aarch64-apple-darwin`。

| 范围 | 行覆盖率 |
|------|---------|
| **observer 纯逻辑**（notifier + router + sse + tray_badge） | **95.29% / 96.88% / 97.30% / 100%** |
| **lifecycle 纯逻辑子集**（BadgeCounter + ExitingFlag + SidecarState + cleanup_sidecar） | **≈100%** |
| **M1 纯函数核心**（config + paths + supervisor） | 92.91%–100% |
| 全量（含 main.rs 二进制入口 + isolation 三平台 lock/release I/O 路径 + TrayController GUI 路径） | 59.07% |

差距说明：`main.rs` 需 Tauri 运行时无法单测（由 `real_inkos_sidecar_serves_spa` 集成测端到端覆盖）；`isolation/*/lock|release` 需 root/管理员权限；`lifecycle.rs` 的 `TrayController::build/refresh`、`build_menu`、`handle_menu_event`、`install_signal_hooks` 需 Tauri 运行时 + 真机 GUI + 信号送达——均不计入 spec 的 ≥80% 纯逻辑目标。详见 `变更记录文档/20260806/M2a冒烟验证.md` §6。

## 零修改纪律

仓库根是 inkos 上游内容（`packages/`、根 `package.json`、根 `README*`、根 `.gitignore`、既有 `.github/`）。**桌面壳的所有产物只进 `src-tauri/`、`scripts/desktop-*`、新增 `.github/workflows/desktop-*.yml`**，永不修改 inkos 根文件——这样 `git merge upstream/master` 近零冲突。Rust 构建忽略用 `src-tauri/.gitignore`（不追加到根 `.gitignore`）。

## 协议

AGPL-3.0，与 inkos 主仓一致。
