# M2a 冒烟验证（observer + lifecycle + 信号钩子）

| 项 | 值 |
|---|---|
| 日期 | 2026-08-06 |
| 分支 | `feat/desktop-m2a` |
| 标签 | `v0.2.0-m2a` |
| 范围 | observer 旁路 SSE 路由 → 原生通知 + 托盘角标；lifecycle 托盘保活 + 信号钩子；M1 清理 |
| 上游计划 | `开发时SpecCoding'sPlan/inkosDesktop/02_实现计划/M2a_observer与lifecycle实现计划.md` |
| 上游设计 | `开发时SpecCoding'sPlan/inkosDesktop/01_架构设计/inkosDesktop架构设计.md` §6.2 / §6.3 / §6.5 |

---

## 1. M2a 交付清单（7 任务，全部完成）

| # | 任务 | 关键 commit | 状态 |
|---|------|------------|------|
| T1 | SSE frame parser + `SseEvent` | `d35f267e feat(observer): SSE frame parser + SseEvent` | ✅ review clean |
| T2 | router + 默认路由表 | `9e530b7f feat(observer): router + default routing table` | ✅ review clean |
| T3 | native notifier + tray badge handlers | `801fd4d4 feat(observer): native notifier + tray badge handlers` | ✅ review clean |
| T4 | `SseClient` 指数退避重连 | `951ad3d1 feat(observer): SseClient with backoff reconnect` | ✅ review clean |
| T5 | `TrayController` + `install_signal_hooks` | `ca74f5c4 feat(lifecycle): tray controller + signal hooks` | ✅ review clean |
| T6 | 接线 + M1 清理（死 env / JS 转义） | `f51ed4b0 feat(app): wire observer/tray/signal hooks + M1 cleanup` | ✅ review MEDIUM/LOW 已在 T7 修 |
| T7 | 冒烟归档 + 覆盖率 + tag（本档） | 见 §7 commit SHA | 本档 |

**M2a HEAD（打 tag 处）**：见 §7 commit SHA。

---

## 2. observer 路由表（默认，源自 T3）

`Router::default_table(notifier, badge)` 按事件名**精确匹配**组装（非前缀匹配，与 spec §2.1 一致）：

| SSE 事件 | NativeNotifier | TrayBadge | 备注 |
|---|---|---|---|
| `write:complete` / `draft:complete` | ✅（仅窗口失焦时） | +1 | 章节写完 |
| `book:created` | ✅（失焦时） | +1 | 新建书 |
| `agent:complete` | ✅（失焦时） | +1 | agent 完成 |
| `daemon:chapter` | ❌ | +1 | daemon 写完一章（静默，仅角标） |
| `log` / `tool:*` / `llm:progress` / `context:*` / `ping` / 其他未知 | ❌ | ❌ | 静默忽略（架构 §6.2 容错，防上游新增） |

设计点：router 内部用 `Arc<dyn EventHandler>`，handler 失败仅 log 不阻断链路（`failing_handler_does_not_break_chain` 单测验证）；未知事件不崩溃（`unknown_event_is_ignored` 单测验证）。

---

## 3. SSE 重连韧性（双层）

| 层 | 实现位置 | 行为 |
|---|---|---|
| **连接级**（SseClient::run 内） | `observer/sse.rs` | 失败 → 指数退避（500ms → 1s → 2s，封顶 5s）+ 每轮检查 `shutdown`；shutdown=true 时立即返回 `Ok(())` |
| **任务级**（watcher 循环外层） | `main.rs::wire_observer_and_lifecycle` | SseClient::run 异常返回 → 500ms 分段 sleep 后重启 client；shutdown=true 时跳出循环 |

集成测（`tests/observer_integration.rs`，3 个全绿）：
- `sse_client_exits_immediately_when_shutdown_pre_set`：shutdown 预置即返回
- `sse_client_handles_connection_errors_without_panic`：mock server 拒连不 panic
- `sse_client_dispatches_frame_and_reconnects`：mock server 发 1 帧 → 断开 → 重连

---

## 4. 托盘保活（lifecycle）

### 4.1 关窗隐藏保活

| 关窗来源 | 行为 |
|---|---|
| 用户点窗口 X / Cmd+W / Alt+F4 | `on_window_event(CloseRequested)` → `ExitingFlag.is_set()==false` → `prevent_close + hide()` |
| 托盘"退出"菜单 | `handle_menu_event` "quit" → set `ExitingFlag` → `app.exit(0)` → `RunEvent::Exit` 走 cleanup |
| SIGINT（Ctrl+C）/ SIGTERM（Unix） | `install_signal_hooks` cleanup 闭包 set `ExitingFlag` + shutdown observer + cleanup sidecar + release loopback → `process::exit(0)` |
| macOS Cmd+Q | 走 NSApplication.terminate，直接 `RunEvent::Exit`（不触发 CloseRequested） |

`ExitingFlag` 与 `ObserverShutdown` 分开（语义不同：前者=退出整个 app，后者=停 observer watcher）。当前两者同时 set，分开是为 M3 reload 场景预留。

### 4.2 托盘菜单三态

| 菜单项 | 行为 |
|---|---|
| 角标项（动态文本） | 显示 `未读 N` 或 `无未读`，每次 `inc_badge` / `clear_badge` 后 refresh |
| 显示 | `window.show() + set_focus()` |
| 退出 | set `ExitingFlag` + `app.exit(0)` |

### 4.3 信号钩子幂等性

`install_signal_hooks` 的 cleanup 闭包与 `RunEvent::Exit` 路径**冗余执行同一组 take/store/release**：
- `SidecarState::take()` → `Option<Child>`（take 幂等，二次返回 None）
- `AtomicBool::store(true, SeqCst)`（幂等）
- `LoopbackGuard::release(port)`（best-effort，失败仅 log）

3 个单测验证 cleanup 闭包签名 + Arc 共享 + 多次调用幂等：
- `cleanup_closure_arc_clone_compiles_and_calls`
- `cleanup_closure_callable_multiple_times`
- `cleanup_sidecar_none_is_noop`

---

## 5. M1 清理（Task 6）+ Task 7 注释措辞修正

### 5.1 `INKOS_PROJECT_ROOT` env 移除

**事实链**（直读上游源码）：
- `packages/studio/src/api/index.ts:9` 解析 root：`process.argv[2] ?? process.env.INKOS_PROJECT_ROOT ?? process.cwd()`
- `packages/cli/src/commands/studio.ts:111` 把 `process.cwd()` 作 `argv[2]` 透传给 studio 服务（`spawn("node", [builtEntry, root], { cwd: root, ... })`）
- supervisor 设 `cwd=project_root` → CLI 的 `process.cwd()`=project_root → argv[2]=project_root

**结论**：env 与 cwd **等价**（三者解析结果相同），supervisor 已注入 cwd=project_root，再叠加 env 冗余、徒增状态面，故移除。

**注释措辞修正（Task 7 Step 1，修 Task 6 审查 MEDIUM）**：
- Task 6 旧注释："已确认为死代码" — 措辞不准（env 在 `api/index.ts` 端确有读取，非死代码）
- Task 7 新注释（`src-tauri/src/supervisor.rs::build_launch` doc）：明确"该 env 与 cwd 在本路径下**等价**……env 端确有读取，**非死代码**……supervisor 已通过 cwd 注入相同值，再叠加 env 冗余、徒增状态面，故移除"

测试 `build_launch_sets_studio_port_env_only` 同步更新注释。零行为变更（断言不变）。

### 5.2 用 `serde_json::to_string` 替代手写 JS 转义

`src-tauri/src/main.rs::to_js_string_literal`：用 `serde_json::to_string` 做规范 JSON 字符串转义（自动处理引号/反斜杠/控制字符），替代 Task 1 的手写 `.replace('\n',..).replace('\'',...)` 链——后者漏掉反斜杠与控制字符。

**Task 7 Step 2 修 Task 6 审查 LOW**：新增单测 `tests::to_js_string_literal_escapes_quotes_backslash_and_control`（main.rs `#[cfg(test)] mod tests`），断言：普通 ASCII 包裹、双引号转 `\"`、反斜杠转 `\\`、换行转 `\n`、空串/中文直传、组合消息产生合法 JS 字面量。serde_json 失败兜底分支触发条件极端（非 UTF-8 边界），不可达，不测。

---

## 6. 测试稳定性 + 覆盖率

### 6.1 全量测试稳定性（连跑 3 次，2026-08-06）

```
Run 1/2/3 一致：
  lib unittests:           61 passed; 0 failed; 0 ignored
  bin unittests (main.rs):  1 passed; 0 failed; 0 ignored   ← T7 新增 to_js_string_literal
  observer_integration:     3 passed; 0 failed; 0 ignored
  supervisor_integration:   2 passed; 0 failed; 1 ignored   ← real_inkos_sidecar_serves_spa（沿用 M1 ignore）
  doctest:                  2 passed; 0 failed; 0 ignored
  -------------------------------
  合计：                    69 passed; 0 failed; 1 ignored
```

3 次结果完全一致，无 flaky。M1 的 TOCTOU（`pick_free_port` 并发竞争）已通过单测断言契约（不复核 bind）规避；observer 集成测用 mock HTTP server（无真实端口竞争）。

### 6.2 覆盖率（`cargo llvm-cov --workspace --summary-only`，2026-08-06）

工具：`cargo-llvm-cov 0.8.7` + `llvm-tools-aarch64-apple-darwin`（rustup component）。可用，无需 tarpaulin。

| 文件 | 行覆盖率 | 类别 |
|---|---|---|
| `observer/notifier.rs` | **95.29%** | 纯逻辑（spec 要求 ≥80%）✅ |
| `observer/router.rs` | **96.88%** | 纯逻辑 ✅ |
| `observer/sse.rs` | **97.30%** | 纯逻辑（重连/退避全覆盖）✅ |
| `observer/tray_badge.rs` | **100.00%** | 纯逻辑 ✅ |
| `supervisor.rs` | **92.91%** | 纯逻辑 ✅（M1 已达标） |
| `paths.rs` | **94.74%** | 纯逻辑 ✅（M1 已达标） |
| `config.rs` | **100.00%** | 纯逻辑 ✅（M1 已达标） |
| `lifecycle.rs` | 58.06% 全量 / **BadgeCounter + ExitingFlag + SidecarState + cleanup_sidecar 纯逻辑子集 ≈ 100%** | 0% 覆盖部分：TrayController::build/inc_badge/clear_badge/badge/refresh、build_menu、handle_menu_event、install_signal_hooks（全部为 Tauri 运行时 / GUI / 信号送达路径） |
| `isolation/{linux,macos,windows}.rs` | 38.89%–60.42% | 规则生成纯函数 100%；`lock` / `release` 需 root/管理员权限，CI 与本机不可达 |
| `main.rs` | 6.99% | Tauri 二进制入口；新增 `to_js_string_literal` 单测已覆盖该函数；setup/RunEvent/on_window_event 需 Tauri 运行时 |
| **TOTAL** | **59.07%** | 与 M1 持平；observer 全部 ≥95% 拉不动全量平均（main.rs+isolation 占比大） |

**Spec 达标结论**：observer 纯逻辑（notifier/router/sse/tray_badge）全部 ≥95%，远超 ≥80% 目标；lifecycle BadgeCounter 子集 100%。IO/GUI 路径不计（brief 明示）。

### 6.3 覆盖率复现命令

```bash
cd src-tauri && cargo llvm-cov --workspace --summary-only           # 表格
cd src-tauri && cargo llvm-cov --workspace --text                   # 逐行
cd src-tauri && cargo llvm-cov --workspace --html --output-dir target/llvm-cov/html
open target/llvm-cov/html/index.html
```

---

## 7. Commit / Tag

| 项 | 值 |
|---|---|
| M2a HEAD（含 T7） | `<见 git log，T7 commit SHA>` |
| tag | `v0.2.0-m2a`（annotated，message: `M2a: observer + lifecycle + signal hooks`） |
| baseline | `81913dce`（M1 close-out merge） → 6 个 M2a feature commit + 1 个 T7 收尾 commit |

---

## 8. Parked 项（不在 M2a 范围，留给真机 / M3）

| 项 | 原因 | 归宿 |
|---|---|---|
| **托盘图标实际渲染** | 本环境（headless / no GUI）无法可视核验；`TrayController::build` 已实现 + Send+Sync 编译期断言，但图标位图加载、菜单弹出席位需真机 | 真机冒烟（macOS / Windows / Linux） |
| **关窗隐藏动画** | `window.hide()` 调用已实现；窗口是否真的不可见、有无过渡动画需真机 | 真机冒烟 |
| **系统通知弹窗可见性** | `tauri-plugin-notification` 调用已接线；通知是否出现在 macOS 通知中心 / Windows Action Center / Linux libnotify 需真机（含权限授予路径） | 真机冒烟 + M3 通知配置（静默时段、重要性级别） |
| **SPA 在 webview 内的实际加载** | health_probe 通过后 `window.location.replace` 已实现；SPA 真正渲染 inkos UI 需真机 | 真机冒烟 |
| **pfctl / iptables / netsh loopback 强制** | M1 已知降级：非 root 下 guard.lock 失败 → 仅 log 警告 → sidecar 仍绑 0.0.0.0 | M3 特权 helper：macOS SMJob/launchd、Linux setuid、Windows WFP |
| **observer 真实事件流验证** | mock server 集成测已验证 SSE 解析/路由/重连；真实 inkos sidecar 启动后 observer 是否真的收到 ~25 种事件需真机 | 真机冒烟（启动 sidecar → 触发一次写作 → 观察托盘角标 + 系统通知） |

---

## 9. 零交叉自检

- 改动文件全部在 `src-tauri/` 与 `变更记录文档/`：
  - `src-tauri/src/supervisor.rs`（注释 + 测试注释）
  - `src-tauri/src/main.rs`（新增 `#[cfg(test)] mod tests`）
  - `src-tauri/README.md`（追加 M2a 能力）
  - `变更记录文档/20260806/M2a冒烟验证.md`（本档）
- `inkos/` 子模块、`packages/` 子模块、根 `Cargo.*`、根 `package.json`、根 `README*`、既有 `.github/` 均未触碰。
- `git diff --stat` 仅上述 4 文件，符合"零交叉（仅 src-tauri/ + 变更记录文档/）"约束。

---

## 10. 约束符合性

| 约束 | 状态 | 证据 |
|---|---|---|
| 零修改 inkos | ✅ | git diff 仅 src-tauri/ + 变更记录文档/ |
| Rust 无静默吞错 | ✅ | notify/badge/hide/release 失败均 `eprintln!`；cleanup 闭包冗余执行全部幂等且 log |
| 覆盖率 ≥80%（纯逻辑） | ✅ | observer 全部 ≥95%；lifecycle BadgeCounter 子集 ≈100%（见 §6.2） |
| 单一职责 < 800 行 | ✅ | main.rs=478（+42 行 T7 单测）、lifecycle.rs=476、supervisor.rs=283、observer/* 全部 < 250 |
| 命名禁空格 | ✅ | `to_js_string_literal`、`wire_observer_and_lifecycle`、`ExitingFlag`、`ObserverShutdown` |
| T1-5 公开签名不变 | ✅ | build/inc_badge/clear_badge/install_signal_hooks/SseClient::new/run 等签名零改动 |
| 关窗隐藏保活 | ✅ | `on_window_event(CloseRequested)` + `ExitingFlag` 双状态门 |
| watcher 任务级韧性 | ✅ | 外层 loop 包裹 SseClient::run，异常重启 |

---

## 11. 后续

- **M2b**（secrets Route A）：keyring + keychain→secrets.json + 文件监听回写 + 防回环 + 首迁。独立 spec→plan。
- **M3**：loopback 强制（特权 helper）、Windows 打包、updater、CI、签名、全局快捷键、通知配置、真机冒烟回归。
