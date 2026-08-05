# inkos-desktop（Tauri 桌面壳）

inkos 的 Tauri / Rust 桌面客户端骨架。当前里程碑 **M2b**（secrets keychain Route A：同步 + 回写 + 防回环），覆盖：
开窗 → 拉起 sidecar → 健康探测 → WebView 加载 SPA → loopback 加固占位 → observer 旁路 SSE 路由到原生通知 + 托盘角标 → 托盘保活（关窗隐藏）→ SIGINT/SIGTERM 安全退出清理 → **启动期 keychain ↔ `secrets.json` 同步 + 文件监听回写 + 防回环**。

## 模块速览

| 模块 | 职责 |
|------|------|
| `src/config.rs` | 跨任务共享常量（默认 Studio 端口、健康探针超时/间隔、CLI 入口相对路径） |
| `src/paths.rs` | 跨平台目录解析（`PathResolver` trait + `AppPaths` 实现） |
| `src/supervisor.rs` | sidecar 监督：`pick_free_port` + `build_launch` + `spawn`（进程组）+ `kill_tree` + `health_probe` |
| `src/lifecycle.rs` | `SidecarState`（Tauri managed state）+ `cleanup_sidecar`（退出清理）+ `BadgeCounter` + `TrayController` + `ExitingFlag` + `install_signal_hooks` |
| `src/isolation/` | loopback 加固：`LoopbackGuard` trait + macOS pf / Linux iptables / Windows netsh 占位实现 |
| `src/observer/` | SSE 旁路：`sse`（frame parser + 重连 client）→ `router`（默认路由表）→ `notifier`（原生通知）+ `tray_badge`（托盘角标） |
| `src/secrets/` | keychain 同步：`store`（`SecretStore` trait + `MockStore` + `KeyringStore` keyring v3 封装）+ `jsonio`（`.inkos/secrets.json` 纯函数读写，atomic NamedTempFile persist + 创建即 0600）+ `sync`（启动期 sync_on_startup + 首迁 + `spawn_writeback` watcher + 防回环 + debounce + **SPA 删除传播**） |
| `src/main.rs` | Tauri 二进制入口：setup 拉起 sidecar、Exit 清理、loopback guard 接入、observer/lifecycle 接线、关窗隐藏保活、信号钩子、**secrets 同步与 writeback 接线** |
| `src/lib.rs` | 库入口，导出公开模块（供集成测与 doctest 使用） |

所有模块均 < 500 行（单一职责约束；secrets/sync.rs 含测试约 900 行，纯逻辑约 320 行）。

## M2a 能力一览

### 1. 系统通知（背景写作不打扰）

observer 旁路订阅 `http://127.0.0.1:<port>/api/v1/events`（与 SPA 同端口），按事件名**精确匹配**路由（非前缀匹配，与 spec §2.1 一致）：

| SSE 事件 | 原生通知 | 托盘角标 |
|---|---|---|
| `write:complete` / `draft:complete` | ✅（仅窗口失焦时） | +1 |
| `book:created` | ✅（失焦时） | +1 |
| `agent:complete` | ✅（失焦时） | +1 |
| `daemon:chapter` | ❌ | +1 |
| `log` / `tool:*` / `llm:progress` / `context:*` / `ping` / 其他未知 | ❌ | ❌（静默忽略，容错） |

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

## M2b 能力一览

### 1. keychain 密钥同步（启动期，三路径）

启动期 `sync_on_startup`（spawn sidecar **之前**调用，确保 SPA 启动前 `secrets.json` 就绪）按 keychain / json 状态决策：

| 路径 | 触发条件 | 行为 |
|---|---|---|
| **首迁（json→keychain）** | keychain 空 且 json 非空 | json 全部 key 灌入 keychain（初次安装场景）；`syncing` flag 不动（防触发回写反向链路） |
| **覆盖（keychain→json）** | keychain 非空 | keychain 为 source of truth：原子写 json（merge 保留顶层其他字段，仅覆盖 `services[*].apiKey`）；置 `syncing=true` 防回环，写完 reset |
| **noop** | 双空 或 json 损坏 | 不报错 |

### 2. 文件监听回写 + 防回环（spawn sidecar 之后）

`spawn_writeback` 启动 `notify::RecommendedWatcher` 监听 `.inkos/secrets.json` 改动 → debounce 500ms → `process_writeback` 读 json → `compute_writeback_diff`（返回 `WritebackDiff{upserts, deletes}`）→ keychain `upsert` + `delete`（**M2b 修复：传播 SPA 删除**）→ 恢复 secrets.json 权限 0600（**M2b 修复**：inkos `saveSecrets` 用默认 umask 写常 0644，桌面壳作为特权方每次 SPA 写后恢复 0600）。

**防回环三层**：
- L1 `syncing` flag（SeqCst）：sync_on_startup 写 json 期间置 true，`process_writeback` 入口检查 true 直接整体跳过（不删不写——**关键**：sync 写期间不会因 json 暂时不完整而误删 keychain）
- L2 debounce 500ms：合并连发写；分段检查 shutdown
- L3 diff 只返回真正变更（upserts = json 中"缺失或值不同"；deletes = store 中"json 缺失"，传播 SPA 删除）；**灾难兜底**：json 完全空 + store 非空 → 跳过删除（防 secrets.json 损坏误删全部 key）

**原子写（M2b 加固）**：`atomic_write_0600` 改用 `tempfile::NamedTempFile::new_in(parent)` + `.persist(path)`：
- 随机文件名：消除固定 `secrets.json.tmp` 的符号链接预创建攻击面
- 创建即 0600（Unix）：消除 `fs::write` 默认权限（受 umask 影响常 0644）的短暂可读窗口
- `.persist`：原子 rename（同 filesystem 保证）

### 3. 降级（keychain 不可用不阻塞）

- keychain 完全不可用（headless CI / Linux 无 Secret Service / 拒绝授权）→ `sync_on_startup` 返回 Err → setup 仅 `eprintln!` 警告"密钥未加密存储" → SPA 仍启动（读 `secrets.json`，inkos 仍可用）
- writeback 失败：错误 log，外层 syncing flag 始终 reset，watcher 任务继续运行
- keychain 中途恢复：writeback 下次触发时 upsert 成功

### 4. KeyringStore 的 `__index__` 设计

`keyring` crate **无枚举 API**。KeyringStore 在同一 service（`inkosDesktop`）下维护名为 `__index__` 的元 entry，value 是 JSON 数组记录所有已写入的 key 名单（不是 secret 值）。`read_all` 先读 index、再按 index 逐个 `get_password`；index 与 entry 不一致时容错跳过（log），不报错。攻击者拿到 keychain 只能知道"有哪些 service id"，不能拿到 key。

### 5. schema 兼容（与 inkos 上游一致）

直读 inkos `packages/core/src/llm/secrets.ts:5` 证实 schema：`{ "services": { "<serviceId>": { "apiKey": "<key>" } } }`（**camelCase `apiKey`**）。jsonio `write_secrets` 严格 camelCase 输出，零 snake_case 风险。

## 运行（M2b）

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

预期：窗口打开显示「启动中…」stub 页，几秒内自动跳转到 `http://127.0.0.1:<port>/`（默认从 `:4567` 起探测首个空闲端口）。**M2a 起追加**：sidecar 健康后自动启用托盘图标、observer 旁路 SSE、信号钩子；关窗变隐藏（点托盘"显示"恢复），托盘"退出"或 Ctrl+C / SIGTERM 触发 cleanup 并退出进程。**M2b 起追加**：setup 阶段先做 keychain↔json 同步（keychain 不可用 → 仅警告，不阻塞），sidecar spawn 后启动 writeback watcher 监听 `secrets.json` 改动写回 keychain。

### 何处会降级

- **loopback 加固**：macOS `pfctl` / Linux `iptables` / Windows `netsh advfirewall` 均需特权。非特权启动时 guard.lock 失败，stderr 输出 `loopback guard: lock port=<p> 失败 ... 降级继续`，sidecar 仍绑 `0.0.0.0`（同网段可访问）。完整运行时强制待 M2（macOS SMJob/launchd）/ M3（Linux setuid、Windows WFP）。
- **端口选择**：`pick_free_port` 探测瞬间空闲，不保证 spawn 时仍空闲（TOCTOU）。若被抢，`health_probe` 30s 内拿不到 2xx，窗口 title 显示「inkos 启动超时 (30s)，见日志」。
- **keychain 不可用**：headless CI / Linux 无 Secret Service（D-Bus 未启动）/ 用户拒绝授权 → `sync_on_startup` 返回 Err → setup 仅 log 警告"密钥未加密存储"，不阻塞；SPA 仍启动，inkos 读 `.inkos/secrets.json`（若存在）仍可用。完整三平台 keychain 端到端验证待 M3 真机冒烟。

## 测试

```bash
cd src-tauri && cargo test                  # 单测 + 集成测（mock）+ doctest，全绿
cd src-tauri && cargo test -- --ignored     # 包含真实 inkos sidecar 集成测与 keychain 真实路径（需先跑构建脚本与可达 OS keychain）
```

**稳定性**：单测/集成测/doctest 共 128 个（113 lib + 1 bin + 3 observer integration + 2 supervisor integration mock + 2 doctest + 7 ignored），连续 3 次全量跑全绿，无 flaky。M2b 修复后新增 6 个测试（删除传播 + 0600 恢复 + tempfile 创建模式）。7 个 `#[ignore]` 中：3 个 KeyringStore 真实 keychain + 1 watcher SPA IO timing + 1 real_inkos_sidecar_serves_spa + 2 secrets 内部 IO timing——均需真机或真实 keychain 才有意义。

## 覆盖率

```bash
cd src-tauri && cargo llvm-cov --workspace --html --output-dir target/llvm-cov/html
open target/llvm-cov/html/index.html
```

工具：`cargo-llvm-cov 0.8.7`（macOS 原生支持，比 tarpaulin 更稳）+ `llvm-tools-aarch64-apple-darwin`。

| 范围 | 行覆盖率 |
|------|---------|
| **M2b secrets 纯逻辑**（jsonio + sync 纯函数 + store MockStore） | **96.69% / 89.04% / ≈100%**（keyring 真实 keychain 路径 + notify IO 不计） |
| **observer 纯逻辑**（notifier + router + sse + tray_badge） | **95.29% / 96.88% / 97.30% / 100%** |
| **lifecycle 纯逻辑子集**（BadgeCounter + ExitingFlag + SidecarState + cleanup_sidecar） | **≈100%** |
| **M1 纯函数核心**（config + paths + supervisor） | 92.91%–100% |
| 全量（含 main.rs 二进制入口 + isolation 三平台 lock/release I/O 路径 + TrayController GUI 路径 + KeyringStore 真实 keychain 路径 + notify IO 路径） | 60.92% |

差距说明：`main.rs` 需 Tauri 运行时无法单测（由 `real_inkos_sidecar_serves_spa` 集成测端到端覆盖）；`isolation/*/lock|release` 需 root/管理员权限；`lifecycle.rs` 的 `TrayController::build/refresh`、`build_menu`、`handle_menu_event`、`install_signal_hooks` 需 Tauri 运行时 + 真机 GUI + 信号送达；`secrets/store.rs` 的 KeyringStore 部分（line 100-244，3 个 `#[ignore]` 真实 keychain 测试）需 OS keychain 可达；`secrets/sync.rs` 的 `spawn_writeback` / `run_writeback_loop` / `debounce_and_process` 是 notify + spawn IO 边界——均不计入 spec 的 ≥80% 纯逻辑目标。详见 `变更记录文档/20260806/M2b冒烟验证.md` §6。

## 零修改纪律

仓库根是 inkos 上游内容（`packages/`、根 `package.json`、根 `README*`、根 `.gitignore`、既有 `.github/`）。**桌面壳的所有产物只进 `src-tauri/`、`scripts/desktop-*`、新增 `.github/workflows/desktop-*.yml`**，永不修改 inkos 根文件——这样 `git merge upstream/master` 近零冲突。Rust 构建忽略用 `src-tauri/.gitignore`（不追加到根 `.gitignore`）。

## 协议

AGPL-3.0，与 inkos 主仓一致。
