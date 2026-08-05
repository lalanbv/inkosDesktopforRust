# inkosDesktop 架构设计

> inkos 多平台桌面客户端（Rust/Tauri）权威设计规格
> 对应上游：[Narcooo/inkos](https://github.com/Narcooo/inkos) v1.6.3，AGPL-3.0

| 项 | 值 |
|---|---|
| 文档版本 | 1.2 |
| 创建日期 | 2026-08-05 |
| 修订日期 | 2026-08-06（v1.2：仓库组织改 mono-repo；v1.1：承重假设核查修正） |
| 状态 | 二次审阅完成 + 仓库组织定案（mono-repo），进入实现（M1 执行中） |
| 设计阶段 | brainstorming 产出 → writing-plans 输入 |
| 文档根目录 | /Users/lalanbv/GitProject/inkosDesktopforRust |
| 上游锚定 | inkos v1.6.3（跟随上游 release 自动同步） |
| 适用范围 | 整体架构 + Phase 1 MVP；Phase 2/3 另出子 spec |

---

## 1. 项目概述

### 1.1 目标

为 inkos 提供 Mac/Windows/Linux 三平台**原生桌面客户端**，满足：

- **完美同步**：与 inkos 所有功能（含未来更新）完全一致——通过零修改运行上游原版实现
- **原生体验**：系统窗口、托盘、全局快捷键、系统通知、最小化到托盘保活后台写作
- **一键安装**：双击即用，免装 Node/CLI，自动更新
- **离线本地**：全本地运行，数据不出本机（LLM 调用除外）
- **安全可靠**：进程隔离、密钥不落盘、崩溃自恢复、更新可回滚
- **高性能**：Rust 壳层接近零分配；Node 引擎 GC 非性能瓶颈

### 1.2 非目标（明确排除）

- ❌ 移动端（iOS/Android）原生客户端——Node 无法跑于 iOS
- ❌ 重写 inkos 任何核心逻辑（Agent/流水线/状态机/提示词）
- ❌ 云端服务/账号体系（保持本地优先）
- ❌ 修改 inkos 上游源码（零修改原则，见 §7）

### 1.3 核心权衡决策（已确认）

| 决策点 | 选择 | 理由 |
|---|---|---|
| 整体路径 | B（Rust 壳 + inkos sidecar） | 唯一能兑现"完美同步未来更新" |
| 分发定位 | 开源免费 | AGPL-3.0 合规路径最干净 |
| 桌面诉求 | 原生体验 + 一键安装 + 离线本地 | 路径 B 强项 |
| 多设备范围 | 仅桌面三平台 | 移动端 Node 不可行 |
| 引擎更新 | 跟随上游 release 自动同步 | 零滞后 |
| UI/进程架构 | **γ 旁路增强派** | 兼容性（零摩擦）+ 扩展性（旁路+注入）兼得 |

---

## 2. 背景与关键事实（已核查）

### 2.1 inkos 技术现状

| 维度 | 事实 | 来源 |
|---|---|---|
| 形态 | 本地优先：Node 22+ 进程 + CLI/TUI/Studio 三界面 | README + package.json |
| 包结构 | monorepo：`core` + `cli` + `studio`（pnpm workspace） | 仓库结构 |
| 原生依赖 | **无任何 native addon**（core 全纯 JS） | packages/core/package.json |
| SQLite | 用 Node 内置 `node:sqlite`；缺失时自动降级为纯文件实现 | play-reducer.ts / memory-retrieval.ts |
| Studio 架构 | React 19 SPA + Hono API；**生产 = 单端口统一服务**（默认 :4567，`INKOS_STUDIO_PORT` 可配）：Hono 同端口 serve SPA 静态产物 + REST + SSE（`/api/v1/events`）；仅 dev 模式才分 Vite:4567 + Hono:4569 | `api/index.ts`、`server.ts:6192-6220` SPA fallback |
| daemon | **Studio 进程内 Scheduler**（非独立守护进程），生命周期 = studio 进程；SSE 事件 `daemon:started/stopped/chapter/error` | `server.ts:3849-3878` |
| 网络绑定 | **默认绑所有接口**（`::`/`0.0.0.0`），**非 localhost**——`inkos` 经 `@hono/node-server` 调 `serve({fetch,port})` 未传 hostname → Node `listen(port, undefined)` | `server.ts:6231`；`@hono/node-server/src/server.ts` |
| 协议 | AGPL-3.0-only | package.json license |
| 引擎要求 | Node >=20.0.0（22+ 启用 SQLite） | package.json engines |

### 2.2 关键技术结论

1. **打包底座极简**：纯 JS + 一个跨平台 Node 22 二进制，无 native 交叉编译负担。
2. **SQLite 零额外依赖**：编进 Node 二进制，且有文件降级兜底，鲁棒。
3. **Studio 可原样复用**：生产单端口（SPA + Hono + SSE 同源），SPA 用同源相对 baseURL（`/api/v1`），换端口零摩擦。
4. **daemon 天然适配桌面**：daemon 是 studio 进程内 Scheduler，托盘保活 sidecar 即保活 daemon；桌面壳增值于"托盘保活 + 原生通知"。
5. **⚠️ 绑定需壳层加固**：inkos 默认绑所有接口（非 localhost），零修改原则下须由 Rust 壳用 OS 防火墙/沙箱锁回 loopback（见 §8）。

---

## 3. 架构决策：γ 旁路增强派

### 3.1 决策原理

α（透传）与 β（掌控）的矛盾是伪二选一。γ 用经典模式组合破解：

- **Sidecar 模式**：inkos 作为独立子进程，进程隔离
- **观察者模式**：Tauri 旁路订阅 inkos SSE，转原生增强，不侵入 inkos
- **依赖注入**：Tauri 通过 initialScript 向 SPA 注入渐进增强能力（可选使用）
- **零修改不变量（Zero-Patch Invariant）**：inkos 上游源码一行不改

### 3.2 总览架构

```
╔═══════════════════════════════════════════════════════════════╗
║  inkosDesktop（Tauri 2.x 壳，Rust，稳态接近零堆分配）           ║
║  ┌─ WebView 层 ──────────────────────────────────────────────╗ ║
║  │  加载 http://127.0.0.1:{port}（inkos Studio，原版零改；port=INKOS_STUDIO_PORT）  │ ║
║  │  + Tauri initialScript：注入 window.__inkosDesktop__ 桥    │ ║
║  │    （渐进增强：原生通知/文件对话框/深度链接，SPA 可选使用） │ ║
║  ╞════════════════════════════════════════════════════════════╡ ║
║  │  Tauri 后端（Rust 模块，单一职责，每个 <400 行）           │ ║
║  │  supervisor │ secrets │ observer │ updater │ lifecycle    │ ║
║  │  ipc │ isolation(含 loopback 防火墙) │ telemetry(可选)                        │ ║
║  ╚════════════════════════════╤═══════════════════════════════╝ ║
╚═══════════════════════════════╪═════════════════════════════════╝
                                │ 启动 + env 注入（INKOS_PROJECT_ROOT + 密钥）
╔═══════════════════════════════▼═════════════════════════════════╗
║  inkos 引擎 sidecar（Node 22 便携 + 预构建 dist/，零修改）     ║
║  inkos studio → 单端口 :{port} 统一服务（SPA + REST + SSE + daemon）     ║
║  ★ 上游原版 ★ 未来更新自动生效 ★ AGPL 源码随附                  ║
║  数据 → INKOS_PROJECT_ROOT 指向的用户项目目录（权限 0700）                        ║
╚══════════════════════════════════════════════════════════════════╝
```

> ⚠️ **绑定说明**：`{port}` = `INKOS_STUDIO_PORT`（默认 4567，被占时壳层另选并下发同值）。inkos 默认绑 `0.0.0.0`（所有接口，非 localhost），由 `isolation` 模块用 OS 防火墙/沙箱规则锁回 `127.0.0.1`（见 §8）。SPA 用同源相对 baseURL `/api/v1`，换端口自动跟随，无需重写。

### 3.3 与 α/β 的对比（决策依据）

| 维度 | α 透传 | β 掌控 | **γ（采用）** |
|---|---|---|---|
| 同步摩擦 | 零 | 低（CI 提取 SPA） | 零 |
| 原生增强 | 弱 | 强 | 强 |
| 扩展性 | 弱 | 强 | 强 |
| 实现风险 | 低 | 中 | 低（降级即 α） |
| 模式纯度 | — | — | 观察+注入+Sidecar |

---

## 4. 核心组件（Rust 模块划分）

每个模块单一职责、可独立测试、文件 <400 行（遵循不可变与小文件规范）。

| 模块 | 职责 | 关键 trait/接口 | 设计模式 |
|---|---|---|---|
| `supervisor` | 拉起/监督/重启 inkos sidecar，看门狗，健康探测 | `ProcessSupervisor` | Supervisor |
| `secrets` | 系统 keychain 读写，启动时注入子进程 env | `SecretStore`（每平台一实现） | Strategy |
| `observer` | 旁路订阅 inkos SSE（`/api/v1/events`，与 SPA 同端口 :{port}），事件路由到原生通知/托盘 | `EventListener`、`EventHandler` | Observer + 责任链 |
| `updater` | 双通道更新（Tauri 壳 + inkos 引擎）、签名校验、原子替换、回滚 | `UpdateChannel` | Strategy + Template Method |
| `lifecycle` | 窗口/托盘/快捷键，最小化到托盘保活 daemon | — | — |
| `ipc` | WebView ↔ Rust 命令桥，零拷贝序列化（rkyv/bincode） | `Command` | Command |
| `isolation` | OS 防火墙/沙箱把 sidecar :{port} 锁回 127.0.0.1（inkos 默认绑 0.0.0.0，见 §8）、CSP、capability 白名单 | — | 防御默认 |
| `paths` | 跨平台 app data 目录解析、引擎目录、日志目录 | `PathResolver` | Adapter |
| `telemetry`（可选） | 本地崩溃日志、匿名用量（默认关闭） | — | — |

---

## 5. 数据流

### 5.1 启动流

```
用户双击 inkosDesktop
  → Tauri 主进程启动
  → paths 解析各平台 app data 目录（用于日志/引擎目录；非项目数据目录）
  → isolation 预置 OS 防火墙规则：把将用的 :{port} 锁回 127.0.0.1
  → secrets 从 keychain 读取 LLM API key（见 §6.4 优先级注意事项）
  → supervisor 选定空闲 port（默认 4567，被占则递增），构造子进程环境：
       cwd = 用户选定的项目目录（含 inkos.json）
       env = { INKOS_PROJECT_ROOT=<项目目录>, INKOS_STUDIO_PORT=<port>, INKOS_LLM_*（可选，见 §6.4） }
  → supervisor 启动 inkos（便携 node + 预构建 dist/），命令 = `inkos studio --port <port>`
  → supervisor 轮询探测 http://127.0.0.1:<port> 就绪（最长 30s 超时）
  → WebView 加载 http://127.0.0.1:<port> + 注入 initialScript（暴露 __inkosDesktop__ 桥）
  → observer 开始旁路订阅 http://127.0.0.1:<port>/api/v1/events（与 SPA 同端口）
  → 进入工作台（首屏显示启动进度，就绪后切主界面）
```

### 5.2 写作流

```
用户在 SPA 操作
  → SPA 调 inkos Hono API（同源 :{port}/api/v1/*，REST）
  → inkos 跑 Agent 流水线（plan→compose→draft→audit→revise）
  → inkos SSE 推送进度到 SPA（:{port}/api/v1/events）
  → SPA 更新 UI（实时流式）
  ‖并行‖
  observer 旁路订阅同一 SSE 流
  → 命中 write:complete / draft:complete / daemon:chapter / agent:complete 等事件
  → 路由到 EventHandler：
       • 系统原生通知（聚焦窗口/最小化时）
       • 托盘角标计数
       • 可选：Webhook/ Telegram（复用 inkos 内置 --notify，不重复）
```

### 5.3 更新流（双通道）

```
updater 定时（默认每 24h）查询：
  通道 A（Rust 壳）：Tauri updater manifest（Ed25519 签名）
  通道 B（inkos 引擎）：inkos 上游 GitHub release tag
  → 检测到新版本
  → 校验：SHA256 + AGPL 源码归档完整性 + 签名
  → 下载到 staging
  → 健康预检（可选：在临时进程跑 `inkos doctor`）
  → 备份当前 engine 目录 → 原子替换（rename，POSIX 原子语义）
  → 重启 sidecar → 健康探测 :{port}
  → 失败 → 自动回滚备份 + 通知用户
```

---

## 6. 关键工程机制

### 6.1 零修改原则（完美同步的基石）

所有桌面化适配**全部发生在 inkos 外部**，永不改源码。违反此原则即产生 fork，同步承诺瓦解。

| 适配需求 | 外部手段（不改 inkos） |
|---|---|
| 数据写入规范目录 | `INKOS_PROJECT_ROOT` env（或 argv/cwd）指向用户项目目录；`~/.inkos` 全局配置是否随 `HOME` 重定向另行评估（重定向 HOME 会同时挪走全局密钥，建议不动 HOME） |
| API key 安全 | keychain 管理；注入策略见 §6.4（⚠️ Studio 模式 `.inkos/secrets.json` 优先于 env） |
| 系统通知 | observer 旁路 SSE → 原生通知（不改 inkos --notify） |
| 全局快捷键 | Tauri 注册 → WebView focus 命令 |
| 引擎升级 | 整体替换 engine 目录（原子 rename） |
| 端口冲突 | 壳层下发 `INKOS_STUDIO_PORT` 选备用端口；SPA 同源相对 baseURL 自动跟随，**无需重写** |
| loopback 加固 | inkos 默认绑 0.0.0.0 → OS 防火墙/沙箱锁回 127.0.0.1（见 §8） |

### 6.2 SSE 旁路容错

observer 订阅 inkos SSE 端点 `http://127.0.0.1:{port}/api/v1/events`（与 SPA 同端口，经 loopback 加固后只走本机），属"只读观察"，对 inkos 零影响。容错策略：

- 未知事件类型：忽略不崩溃（防上游新增事件——`broadcast(event: string)` 是动态字符串，无联合类型约束）
- 已知事件 schema 微变：尽力解析，失败降级为"通用进度通知"
- SSE 连接断开：自动重连（指数退避，上限 30s；端点每 30s 发 `ping` 心跳）
- observer 整体崩溃：supervisor 重启 observer，inkos 不受影响

契约测试锁定当前已知事件集合。v1.6.3 实际事件（源自 `server.ts` 的 `broadcast()`，约 25 个，上游可自由增删）：`tool:start/update/end`、`log`、`context:compression`、`llm:progress`、`book:creating/created/error`、`write:start/complete/error`、`draft:start/complete/error`、`consolidate:*`、`repair-state:*`、`foundation:*`、`daemon:started/stopped/chapter/error`、`agent:start/complete/aborted`、`session:title`、`ping`。**不存在** `audit:*`/`revise:*`/`import:*`（那是 CLI 概念，非 SSE 事件）。契约测试须基于 `grep broadcast(` 自动生成，上游变更时 CI 告警。observer 路由到原生通知的合理事件：`write:complete`/`draft:complete`/`daemon:chapter`/`agent:complete`/`book:created`。

### 6.3 双通道更新与回滚

- **通道分离**：Tauri 壳更新（原生二进制）与 inkos 引擎更新（JS bundle + Node）独立，避免耦合
- **原子性**：引擎替换用目录 rename（POSIX 原子）；Windows 用 MoveFileEx(MOVEFILE_REPLACE_EXISTING)
- **回滚**：保留前一版本 engine 目录（`engine.bak`），健康探测失败自动切回
- **频率**：跟随上游 release（默认自动），用户可在设置改为"提醒确认"或"手动"

### 6.4 密钥注入

**inkos 密钥解析优先级（Studio 模式，已核查）**：

```
项目 .inkos/secrets.json  →  项目 .env  →  全局 ~/.inkos/.env  →  inkos.json llm 块  →  CLI flags
```

- **`.inkos/secrets.json` 优先级最高**：Studio UI 保存的 key 写入此处，env 无法覆盖它。
- 规范 env 变量名：`INKOS_LLM_PROVIDER` / `INKOS_LLM_BASE_URL` / `INKOS_LLM_API_KEY` / `INKOS_LLM_MODEL`（对应 `~/.inkos/.env`）；另有 per-service 约定如 `DEEPSEEK_API_KEY` / `MOONSHOT_API_KEY`。

**⚠️ 故"keychain → env 注入"并非权威路径**：若用户曾在 Studio 存 key，env 注入会被忽略。桌面壳两条可行路线，writing-plans 二选一：

| 路线 | 做法 | 取舍 |
|---|---|---|
| **A（推荐）：壳管 secrets.json** | keychain 读 key → 壳层写项目 `.inkos/secrets.json`（权限 0600）→ inkos 读取 | 权威源清晰；与 Studio UI 共存；keychain 为主存储，secrets.json 为派生 |
| B：纯 env 注入 | keychain → 子进程 env（`INKOS_LLM_*`）→ 仅当无 secrets.json 时生效 | 实现最简；但要求项目永不出现 secrets.json，与 Studio 自带密钥 UI 冲突 |

无论哪条路线，密钥不落盘明文（keychain 为唯一持久存储），主进程退出即销毁 env 句柄。无需改 inkos。

---

## 7. 错误处理与降级

| 故障 | 检测 | 处置 |
|---|---|---|
| sidecar 启动失败 | 健康探测 :4567 超时（30s） | 重启（最多 5 次，指数退避）；超限弹诊断 + 日志导出 |
| sidecar 运行中崩溃 | 进程退出码 / 信号 | supervisor 自动重启；连续崩溃 >5 次进入"安全模式"（仅诊断 UI） |
| observer 崩溃 | panic 捕获 | 重启 observer；inkos/SPA 不受影响（降级为纯 α） |
| 端口 :{port} 被占 | 启动前探测 | 壳层递增选备用 port 并下发 `INKOS_STUDIO_PORT`；SPA 同源相对 baseURL 自动跟随，无需重写 |
| 引擎更新失败 | 健康探测失败 | 自动回滚 engine.bak + 通知 |
| keychain 不可用 | secrets 读失败 | 降级：首次引导用户粘贴 key（明文暂存内存，提示风险） |
| 数据目录权限错误 | paths 校验 | 引导修复权限或选新目录 |
| node:sqlite 不可用 | inkos 自降级 | 透传 inkos 的文件降级，UI 提示"记忆库降级模式" |

**核心韧性**：inkos 所有状态本就在文件系统（JSON+Markdown+SQLite），sidecar 崩溃不丢数据，重启即恢复。

---

## 8. 安全设计

| 层 | 措施 |
|---|---|
| 进程隔离 | inkos 为受限子进程；Tauri 主进程监督；最小权限 |
| 密钥 | 系统 keychain 存储，永不落盘明文，env 注入即用即销 |
| 网络 | inkos **默认绑 0.0.0.0（所有接口，非 localhost）**——`isolation` 用 OS 防火墙（macOS `pf` / Windows Firewall / Linux `iptables`）或 Tauri App Sandbox 把 :{port} 锁回 127.0.0.1（writing-plans 实测确认）；inkos 仅出站到用户配置的 LLM provider |
| WebView | CSP 严格策略；Tauri capability 最小权限白名单；禁用 remote content |
| 更新 | Ed25519 签名验证；SHA256 校验；AGPL 源码归档完整性 |
| 数据 | 数据目录权限 0700；可选项目级 AES-GCM 加密（Phase 2） |
| 审计 | 无静默错误；崩溃日志本地化（默认不上报） |

---

## 9. 性能设计

### 9.1 "0GC" 的定位（诚实）

inkos 跑在 V8（带 GC），**系统级 0GC 物理不可能**。但性能瓶颈分布为：LLM 网络等待 ~85% > I/O ~8% > V8 GC ~2%。GC 非性能瓶颈。

| 层 | 0GC 可行性 | 做法 |
|---|---|---|
| Rust 壳 | ✅ 接近零分配 | 对象池、`&str`/`Cow` 借用、零拷贝 IPC（rkyv）、避免 `clone`、arena |
| IPC | ✅ 零拷贝 | rkyv/bincode 替代 JSON |
| Node 引擎 | ❌ 物理不可能 | V8 tracing GC，非性能瓶颈 |

### 9.2 真实性能杠杆（优先级）

1. sidecar 预热常驻（应用启动即拉起，首次写作无冷启动）
2. SQLite WAL 模式（inkos 侧配置或 env 注入）
3. LLM 流式 + 多 Agent 并发（inkos 原生支持）
4. WebView 本地协议加载（γ 下仍走 :4567；Phase 2 可评估 β 的 tauri:// 加载提速）
5. Rust 壳稳态零分配（长文档渲染节流由 SPA 负责）

---

## 10. 测试策略

| 层次 | 工具 | 覆盖目标 | 门槛 |
|---|---|---|---|
| Rust 单测 | `cargo test` | 各模块（trait mock：supervisor/secrets/updater/observer） | ≥80% 行覆盖 |
| 集成测 | `cargo test` + 临时 sidecar | 启动/降级/回滚/端口冲突/密钥注入 | 关键路径全覆盖 |
| 同步回归 | CI 脚本 | 每次上游 release：拉新 engine → `inkos doctor` + 写一章冒烟 | 必须 |
| 契约测 | 锁定 SSE 事件 schema | 防上游 SSE 改动静默打断 observer | 必须 |
| E2E | Tauri WebDriver | 启动→配置→写作→导出全流程 | 关键流程 |
| 分平台烟测 | macOS/Win/Linux CI | 三平台构建+启动 | 每次发版 |

---

## 11. 扩展性设计

| 扩展点 | 机制 | 责任方 |
|---|---|---|
| Agent 技能 | 文件系统桥接 inkos `INKOS_SKILL_DIRS`，UI 管理技能 | inkos 原生 + 桌面 UI |
| LLM Provider | inkos 原生多 provider，桌面提供配置 UI + keychain | inkos 原生 |
| 通知渠道 | inkos 内置 Telegram/飞书/企微/Webhook + 桌面原生通知 | inkos + Rust observer |
| 导出格式 | inkos EPUB/Markdown + Rust 插件槽（PDF/DOCX 经 Rust 库） | 混合 |
| observer 处理器 | 可插拔 `EventHandler`（新自动化、新通知形态） | Rust 壳 |
| Tauri 插件 | Phase 3：WASM/dylib 插件，沙箱隔离 | 桌面端 |
| initialScript 桥 | SPA 可选调用 `window.__inkosDesktop__` 渐进增强 | 桌面端 |

**原则**：inkos 能做的绝不重造；桌面端只补 inkos 没有的（系统集成、原生 UI、分发、安全）。

---

## 12. 分阶段路线

> 本 spec 聚焦整体架构 + Phase 1 MVP。每阶段独立 spec → plan → 实现。

### Phase 1：MVP（核心可用）

- Tauri 2.x 壳 + 三平台窗口/托盘/快捷键
- inkos sidecar 打包（便携 Node 22 + **预构建 dist/** + 资源 copy）
- supervisor 启动/监督/重启
- WebView 加载 :4567（`INKOS_STUDIO_PORT`，冲突递增）+ initialScript 桥
- secrets keychain → `.inkos/secrets.json` 或 env（三平台，路线见 §6.4）
- **isolation loopback 加固（OS 防火墙/沙箱锁回 127.0.0.1，安全前置，见 §8）**
- observer 旁路 SSE（`/api/v1/events`，与 SPA 同端口）→ 系统通知 + 托盘角标
- updater 引擎通道（跟随上游 release，原子替换 + 回滚）
- `INKOS_PROJECT_ROOT` 数据目录重定向
- 基础测试（单测 + 集成 + 同步回归 CI + SSE 契约测）
- **macOS 代码签名/公证**（必要，否则用户无法运行）

### Phase 2：体验与性能

- Tauri 壳自更新通道（Ed25519）
- 项目级加密（AES-GCM）
- SEA 单文件打包（体积优化，验证 node:sqlite 在 SEA 可用后）
- 评估 β 式 tauri:// 加载提速
- E2E 测试完善

### Phase 3：扩展生态

- 插件系统（WASM/dylib，沙箱）
- 技能管理 UI、provider 配置 UI 增强
- 多项目工作区、云盘同步（加密后）
- 可选遥测/崩溃上报（opt-in）

---

## 13. 风险矩阵

| 风险 | 概率 | 影响 | 缓解 |
|---|---|---|---|
| inkos 默认绑 0.0.0.0，公共 WiFi 下 :{port} 暴露 LLM key 与数据到 LAN | 确定 | 高 | `isolation` OS 防火墙/沙箱锁回 loopback（见 §8、§14.1 实测）；SPA 同源 baseURL 已确认无需重写 |
| 上游 SSE schema 变更打断 observer | 中 | 中 | 契约测 + observer 容错（忽略未知事件） |
| macOS 签名/公证成本 | 高 | 中 | Apple Developer ID（$99/年）纳入 Phase 1 |
| 体积 ~60-80MB | 确定 | 低 | 桌面可接受；SEA 优化 Phase 2 |
| node:sqlite 在便携 Node 可用性 | 低 | 低 | Node 22 官方内置 + 文件降级兜底 |
| Windows SmartScreen 警告 | 中 | 低 | EV 证书（Phase 2 评估）或用户引导 |
| 上游 breaking change 致桥接失效 | 低 | 中 | 桥接走稳定接口（env/stdio/HTTP/SSE）；CI 同步回归 |

---

## 14. 核查结论（原开放问题，v1.1 已用源码消解）

| # | 原问题 | 核查结论（证据） |
|---|---|---|
| 1 | 生产启动方式 | **单端口统一服务**。`inkos studio` 默认 :4567（`INKOS_STUDIO_PORT`/`--port`），Hono 同端口 serve SPA 静态 + REST + SSE（`api/index.ts`、`server.ts:6192-6220`）。:4569 仅 dev |
| 2 | 能否仅起 Hono 不起 SPA | server 始终 serve `staticDir`；理论上可指向空目录，γ 非必需，不深入 |
| 3 | API key env 变量名 | `INKOS_LLM_PROVIDER/BASE_URL/API_KEY/MODEL` + per-service `*_API_KEY`；但 **`.inkos/secrets.json` 优先于 env**（见 §6.4） |
| 4 | 数据目录 env 重定向 | **`INKOS_PROJECT_ROOT`**（`argv[2]→env→cwd()`），非 `HOME`/`INKOS_HOME`；`HOME` 仅影响 `~/.inkos/.env` |
| 5 | SPA 是否硬编码 baseURL | **否**——`use-api.ts` 用同源相对 `/api/v1`，换端口自动跟随，无需重写 |
| 6 | daemon 托盘保活生命周期 | daemon 是 **studio 进程内 Scheduler**，托盘保活 sidecar 即保活 daemon（`server.ts:3849-3878`） |
| 7 | bundle 运行时资源保留 | sidecar **预构建并随包分发 dist/**（含 SPA 静态 + genres/skills/提示词）；禁止依赖运行时 `npx vite build`（`api/index.ts` 的自动构建仅兜底） |

### 14.1 仍需 writing-plans 实测确认（不阻塞架构审批）

- **loopback 实测**：`nc -zv <本机 LAN-IP> 4567` 确认默认 0.0.0.0 暴露，并验证 OS 防火墙/沙箱规则把端口锁回 127.0.0.1 生效
- **node:sqlite 便携可用性**：便携 Node 22 下 `node:sqlite`（实验性）可用 + inkos 文件降级兜底验证
- **pi-ai / epub-gen-memory 纯 JS 复核**：打包前确认无隐藏 native 模块（当前判断为纯 JS，依据各 `package.json` 无 native 依赖）
- **upstream release tag 格式**：决定 updater 通道 B 的版本探测方式
- **macOS App Sandbox 网络 SANDBOX 补强**：确认 Tauri sandbox 能限制子进程监听地址（否则退回 pf 防火墙方案）

---

## 15. AGPL-3.0 合规说明

- inkos 上游协议：AGPL-3.0-only
- 本项目：开源免费，分发时**保留协议声明 + 随附 inkos 源码指向**（链接上游仓库即满足）
- γ 架构下本项目**不修改、不衍生** inkos 源码（聚合/链接关系），合规风险最低
- 本项目自身代码建议采用兼容协议（如 MIT/Apache-2.0 或同样 AGPL-3.0），由项目所有者最终决定
- 正式发布前建议复核 AGPL 条款（尤其若未来引入网络服务形态）

---

## 16. 仓库组织策略（mono-repo：本仓 = inkos fork + 桌面壳）

> **v1.2 修订（2026-08-06）**：原"方案 B：fresh 壳仓 + inkos submodule"改为 **mono-repo**——`lalanbv/inkosDesktopforRust` 本身即用户为此 fork 的 inkos 仓，桌面壳直接建在仓库根。决策由用户确认。

### 16.1 单仓库模型

| 角色 | 位置 | 修改策略 |
|---|---|---|
| inkos fork 本体 | 仓库根 `packages/`（inkos 工作区） | **零修改**：永不改 `packages/` 源码、根 `package.json`、`README*`、既有 `.gitignore`、既有 `.github/` |
| 桌面壳（Rust/Tauri） | `src-tauri/`、`scripts/desktop-*`、新增 `.github/workflows/desktop-*.yml` | 自由开发 |

### 16.2 零交叉纪律（同步近零冲突的保障）

桌面壳产物**只进新文件/新目录**，与 inkos 既有文件零重叠：
- ✅ `src-tauri/`（新）、`src-tauri/.gitignore`（新，放 `/target/`）、`scripts/desktop-*`（新）、`.github/workflows/desktop-*.yml`（新）
- ❌ 永不修改根 `package.json`（不加 tauri script）、根 `README*`、既有 `.gitignore`、既有 `.github/workflows/`

如此 `git merge upstream/master` 仅带入 inkos 自身演进，近零冲突。

### 16.3 上游同步（merge 模式，非 fast-forward）

- 配置 `upstream` remote 指向 `Narcooo/inkos`
- 同步：`git fetch upstream && git merge upstream/master`（`packages/` 改动自动合并；偶发根文件冲突人工解决）
- 可选 GitHub Actions 定时尝试 merge：成功推送，冲突则开 issue 告警
- 引擎升级：同步回归 CI 验证新 upstream HEAD → merge → 重新打包 → 发版

> 与 v1.0/v1.1 的 "fast-forward" 不同：mono-repo 含本地壳提交，无法纯 fast-forward；改 merge，**功能等价兑现"完美同步"承诺**（inkos 源码随上游演进，壳代码独立）。

### 16.4 bug fix 回流

发现 inkos bug：在本仓 `packages/` 修复 → 提 PR 回 `Narcooo/inkos` → 上游合入后，下次 merge 时该修复从 upstream 回流。**不在本仓维护与上游分歧的长期 inkos 补丁**。

### 16.5 仓库目录结构（mono-repo）

```
inkosDesktopforRust/                (= inkos fork + 桌面壳，单仓)
├── packages/               inkos 本体（cli/core/studio），零修改
├── assets/ skills/ ...     inkos 既有目录，零修改
├── src-tauri/              Rust 壳（supervisor/observer/secrets/updater/isolation/...）+ src-tauri/.gitignore
├── scripts/desktop-*.sh    桌面壳脚本（构建/打包/同步），新增
├── engine/                 预构建产物（M3 发布打包：便携 node22 + inkos dist/）
├── .github/workflows/desktop-*.yml   桌面壳 CI（三平台构建 + 同步回归），新增
├── 开发时SpecCoding'sPlan/ 设计文档（本文件所在）
└── 变更记录文档/            变更归档
```

### 16.6 协议与 AGPL 合规

- 本仓含 inkos 源码（AGPL-3.0）+ 桌面壳代码；整体维持 AGPL-3.0（与 inkos 兼容，copyleft 一致）
- 分发时本仓即含 inkos 源码，满足 AGPL "分发即提供源码" 义务（见 §15）
- 正式发布前由项目所有者确认最终协议

### 16.7 落地清单（部分已执行）

1. ✅ fork `Narcooo/inkos` → `lalanbv/inkosDesktopforRust`（用户已完成，已拉本地）
2. 配置 `upstream` remote 指向 `Narcooo/inkos`
3. 建特性分支 `feat/desktop-m1` 开发桌面壳
4. 桌面壳产物只进 `src-tauri/`、`scripts/desktop-*`、`.github/workflows/desktop-*.yml`（零交叉，见 §16.2）
5. 添加桌面壳 CI（三平台构建 + 同步回归 + SSE 契约测）
6. 可选：定时 merge upstream 的 GitHub Action

---

## 17. 设计自检清单

- [x] 不可变优先：所有 Rust 模块返回新对象，无就地突变（coding-style 规范）
- [x] 小文件高内聚：每模块 <400 行
- [x] 错误显式处理：无静默吞错（observer 降级亦有 UI 提示）
- [x] 输入校验：在系统边界（更新包签名、SSE 事件、IPC 命令）校验
- [x] 无硬编码值：端口/路径/超时均为常量或配置
- [x] 枚举规范：Rust enum 占位遵循项目规范（移植到代码时落实）
- [x] 测试覆盖 ≥80%
- [x] 零修改不变量贯穿全设计
- [x] 承重假设源码核查（v1.1）：端口模型/绑定/数据目录/密钥/SSE 均经上游代码证实
- [x] localhost 绑定缺口已识别并由壳层兜底（见 §8、§6.1）

---

## 18. 修订历史

### v1.2（2026-08-06）— 仓库组织定案：mono-repo

基于用户确认（`lalanbv/inkosDesktopforRust` 本仓即其 inkos fork，已拉本地），§16 从 "fresh 壳仓 + inkos submodule + fast-forward" 改为 **mono-repo**：桌面壳直接建在 fork 仓库根（`src-tauri/`），inkos 留 `packages/` 零修改，上游同步改 `git merge upstream/master`。"完美同步"承诺以"零文件交叉纪律 + merge 近零冲突"兑现。M1 计划 Task 1/3 相应改写（无 `git init` / 无 submodule）。

### v1.1（2026-08-06）— 二次审阅修正

基于对 inkos v1.6.3 上游源码（`cli/src/commands/studio.ts` / `studio/src/api/index.ts` / `studio/src/api/server.ts` / `studio/src/hooks/use-api.ts` / 各 `package.json` / `@hono/node-server`）的逐条核查，修正以下承重假设：

| 编号 | 级别 | 修正内容 | 证据 |
|---|---|---|---|
| H1 | HIGH | 生产端口模型：~~双端口 :4567(SPA)/:4569(API)~~ → **单端口统一服务**（默认 :4567，`INKOS_STUDIO_PORT` 可配），Hono 同端口 serve API+SPA 静态产物；:4569 仅 dev | `studio.ts` CLI 默认 4567；`api/index.ts` startStudioServer；`server.ts:6192-6220` SPA fallback |
| H2 | HIGH | 安全：inkos **默认绑所有接口**（非 localhost）。`@hono/node-server` `serve()` 不传 hostname → Node `listen(port, undefined)` → `::`/`0.0.0.0`。壳层必须用 OS 防火墙/沙箱锁 loopback | `server.ts:6231`；`@hono/node-server/src/server.ts` |
| M1 | MED | 数据目录：~~`HOME`/`INKOS_HOME`~~ → **`INKOS_PROJECT_ROOT`**（`argv[2]→env→cwd()`）；`HOME` 仅影响 `~/.inkos/.env` 全局配置 | `api/index.ts` root 解析 |
| M2 | MED | 密钥：Studio 模式 `.inkos/secrets.json` **优先于 env**；keychain→env 可行但非权威；给出取舍（见 §6.4） | `server.ts` loadSecrets；密钥解析链 |
| M3 | MED | SPA baseURL：**同源相对 `/api/v1`**，换端口自动跟随，~~initialScript 重写 baseURL~~ 删除 | `use-api.ts` |
| M4 | MED | SSE 事件集合：~~`audit:*/revise:*/import:*`~~ → 实际 ~25 个（`tool/log/llm/book/write/draft/daemon/agent/...`）；`broadcast(event:string)` 动态字符串 | `server.ts` broadcast() |
| M5 | MED | daemon：~~独立守护进程~~ → **Studio 进程内 Scheduler**，生命周期随 studio 进程 | `server.ts:3849-3878` |
| L1 | LOW | sidecar 必须随包**预构建 dist/**（`api/index.ts` 缺 dist 会运行时 `npx vite build`） | `api/index.ts` |
| L3 | LOW | 路径修正 `inkosforRust` → `inkosDesktopforRust`（元数据 + §16.1） | — |

### v1.0（2026-08-05）— 初版

brainstorming 产出，γ 旁路增强派整体架构。承重假设未核查，存在 H1/H2 等偏差（见上行）。
