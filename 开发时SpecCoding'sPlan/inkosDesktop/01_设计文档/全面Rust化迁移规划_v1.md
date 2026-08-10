# inkos 全面 Rust 化迁移规划 v1

> 从「Tauri 壳 + Node sidecar 引擎」混合架构 → 原生纯 Rust 桌面应用
> 的渐进式迁移权威路线图。
>
> | 项 | 值 |
> |---|---|
> | 文档版本 | v1（初版，待评审） |
> | 创建日期 | 2026-08-11 |
> | 作者 | 审查 + 架构设计 |
> | 文档根目录 | /Users/lalanbv/GitProject/inkosDesktopforRust |
> | 上游设计 | `01_架构设计/inkosDesktop架构设计.md` v1.2（γ 旁路增强派） |
> | 关系 | **战略反转**：本规划推翻上游 §1.2 非目标「❌ 重写 inkos 核心逻辑」 |

---

## 0. 执行摘要（TL;DR）

当前 inkosDesktop 是 **Rust 外壳 + Node TS 引擎 sidecar** 的混合体（γ 旁路增强派）：
Rust 壳负责窗口/托盘/沙箱/插件/配置/项目索引；真正的业务逻辑（AI 故事创作引擎）
跑在 Node sidecar（`packages/core` ≈107k 行 + cli + studio）。

**全面 Rust 化 = 把 107k 行 TS 业务引擎搬到 Rust，最终消除 Node sidecar。**

推荐路线：**Strangler-Fig（绞杀者模式）+ HTTP 边界不动 + 自下而上逐域移植**——

1. **准备期**：抽 trait 边界、固化 golden 测试向量、搭 Rust crate 骨架、类型同步通道
2. **Phase 1–3**：按依赖底向上逐域移植（utils→models→…→llm→agent），Rust HTTP 服务与
   Node sidecar 并存，端点逐个切换，Node 侧逐步萎缩直至移除
3. **Phase 4–5**（可选）：前端从 fetch 改 Tauri invoke（去 HTTP）；或前端原生化的终极形态

**核心原则**：
- 🟢 **零功能丢失**：每一步对外行为等价，前端无感知
- 🟢 **可验证**：每个移植域配差分测试（TS vs Rust 同输入同输出）
- 🟢 **可回滚**：移植态与原生态并存，坏一块切回 Node 即可
- 🔴 **明确放弃**：零修改同步上游（fork-and-own，自主维护引擎）

预期总工期：**6–12 个月**（单人）/ 3–6 个月（2–3 人并行），可随时停在任一阶段交付。

---

## 1. 现状基线（已核查）

### 1.1 代码规模与边界

| 层 | 语言 | 规模 | 角色 | Rust 化目标 |
|---|---|---|---|---|
| `src-tauri/` | Rust | 21.8k 行 | Tauri 壳：窗口/托盘/沙箱/插件/配置/索引/observer | ✅ 已是 Rust，保留 |
| `packages/core` | TS | 107k 行 / 16 域 | **业务引擎**（agent/llm/pipeline/…） | ⭐ **主迁移对象** |
| `packages/cli` | TS | 13.7k 行 | CLI + TUI（ink/React） | 迁移（TUI→ratatui） |
| `packages/studio` | TS | 46.2k 行 | Web UI（React SPA + Hono API + SSE） | 前端保留；Hono API→axum |
| `src-ui/` | TSX | 小 | Tauri webview 内置面板（配置/工作区选择） | 保留 webview 或并入 studio |
| `src-tauri/engine/dist` | JS | 预编译 | sidecar 引擎产物（零修改上游） | 移除（被 Rust 引擎取代） |

### 1.2 当前调用链

```
用户 → Tauri 窗口(webview)
     → http://127.0.0.1:{port}        ← loopback
     → Node sidecar (studio Hono API) ← 单端口 SPA + REST + SSE + daemon
     → packages/core (业务逻辑)
     → LLM / 文件系统 / SQLite(node:sqlite)
```

Rust 壳仅做：启动 sidecar、loopback 加固、SSE 旁路观察（→通知/托盘角标）、
密钥同步、配置/项目索引、插件系统（WASM+进程）、自动更新。**不参与业务。**

### 1.3 16 个业务域（迁移单元）

```
agent        — AI Agent（工具调用循环）          ⚠️ 最难，依赖 llm/pipeline/state
agents       — Agent 变体（架构师/作曲家/审核）  ⚠️ 依赖 agent
forecast     — 预测/规划                          依赖 pipeline/models
interaction  — 交互元素                           依赖 models
interactive-film — 互动电影（图模式）             依赖 models/play
llm          — LLM 集成（流式/工具/重试）         ⚠️ 依赖 reqwest→已有
materials    — 素材管理                           依赖 models
models       — 数据模型（纯类型）                 ✅ 叶子，先迁
notify       — 通知                               依赖 models
pipeline     — 处理流水线（编排）                 依赖 多数域
play         — 游玩模式（状态机/reducer）         依赖 state/models
prompts      — 提示词模板                         依赖 models
skills       — 技能系统                           依赖 prompts
state        — 状态管理（持久化/恢复）            依赖 models
translation  — i18n                              ✅ 叶子，先迁
utils        — 工具函数                           ✅ 叶子，先迁
```

依赖图底向上：`utils/translation/models` → `notify/materials/state/prompts` →
`skills/pipeline/play/interaction` → `forecast/interactive-film` → `llm` → `agent/agents`。

---

## 2. 战略决策：为何与如何

### 2.1 战略反转的代价与收益

原架构 §1.2 把「重写核心逻辑」列为非目标，理由是「完美同步上游未来更新」。
全面 Rust 化意味着**主动放弃这个特性**：

| 维度 | 损失 | 收益 |
|---|---|---|
| 上游同步 | ❌ 不再零摩擦跟随 inkos 上游 release | — |
| 分发体积 | — | ✅ 无需打包 Node 22 运行时（~40–60 MB） |
| 启动延迟 | — | ✅ 无 Node 启动 + engine bootstrap（冷启动 -数秒） |
| 内存占用 | — | ✅ 无 V8 堆（常驻 -100–300 MB） |
| 安全面 | — | ✅ 无 Node 进程 = 无 Node 依赖供应链 + 无 `node:sqlite` 双实现 |
| 一致性 | — | ✅ 单一语言、单一类型系统、单一测试栈 |
| 维护 | ❌ 自主维护引擎演进 | ✅ 无 TS/JS 工具链漂移 |
| 性能 | — | ✅ 热路径 0GC（与壳层已实现的规范一致） |

**判定**：用户已显式选择全面 Rust 化，本规划据此制定。建议明确建档为
**「inkosDesktop fork-and-own」决策**（见 §8 ADR），后续不再追求上游同步。

### 2.2 为何选 Strangler-Fig 而非 Big-Bang

| 方案 | 工期 | 风险 | 可中途交付 | 选用 |
|---|---|---|---|---|
| Big-Bang 重写 | 长 | 🔴 极高（全量切换，一处坏全盘崩） | ❌ | ✗ |
| **Strangler-Fig 逐域** | 中 | 🟢 低（每域独立切换+回滚） | ✅ | ⭐ |
| 包装层（Node 调 Rust N-API） | 短 | 🟡 中（仍需 Node，未消除运行时） | ✅ | ✗（治标） |

绞杀者模式核心：**Node sidecar 与 Rust 引擎并存**，端点逐个从 Node 切到 Rust，
Node 侧单调递减，直至为空移除。任何时刻系统都可工作、可发布。

### 2.3 为何保持 HTTP 边界（Phase 1–3）

前端 Studio SPA 全部基于 `fetch('/api/v1/...')` + SSE。若 Phase 1 就改 Tauri invoke，
会牵动整个前端，违背「零功能丢失、前端无感知」。

**保持 HTTP 边界** = 用 Rust axum 服务替换 Node Hono 服务，**端点 URL 与响应契约不变**。
前端零改动，迁移对前端不可见。HTTP 边界是天然的 strangler 切换面。

（Phase 4 再决定是否去掉 HTTP，见 §4.4。）

---

## 3. 目标架构（终态 + 过渡态）

### 3.1 终态（Phase 5 完成后）

```
╔═══════════════════════════════════════════════════════════════╗
║  inkosDesktop（纯 Rust，Tauri 2.x）                            ║
║  ┌─ WebView 层 ──────────────────────────────────────────────╗ ║
║  │  选项 A：保留 React SPA（webview 加载本地 bundle）          │ ║
║  │  选项 B：原生 UI（egui/iced/slint）—— 仅若追求极致原生体验  │ ║
║  ╞════════════════════════════════════════════════════════════╡ ║
║  │  Rust 业务引擎（原 packages/core 全部移植）                 │ ║
║  │  axum HTTP（可选）→ /api/v1/* + SSE                         │ ║
║  │  agent │ llm │ pipeline │ state │ models │ ...              │ ║
║  │  LLM 调用：reqwest（已有）流式；SQLite：rusqlite（已有）     │ ║
║  ╞════════════════════════════════════════════════════════════╡ ║
║  │  既有 Rust 壳模块（不变）                                    │ ║
║  │  plugin(WASM+进程) │ isolation │ observer │ updater │ ...   │ ║
║  ╚════════════════════════════════════════════════════════════╝ ║
╚════════════════════════════════════════════════════════════════╝
（无 Node 进程，无 engine/dist，无 Node bootstrap，无 loopback HTTP——除非保留 SPA）
```

### 3.2 过渡态（Phase 2–3，strangler）

```
        ┌─── webview fetch /api/v1/X ───┐
        │                                │
        ▼                                ▼
  ┌─────────────┐  ← 路由分流（前端无感） →  ┌─────────────┐
  │ Rust axum   │                          │ Node sidecar │
  │ 已迁端点    │                          │ 未迁端点      │
  │ utils/llm/… │                          │ agent/…/…     │
  └──────┬──────┘                          └──────┬───────┘
         │                                        │
    Rust crates                              packages/core
  (业务库形态)                               (原 TS 引擎)
         │                                        │
         ▼                                        ▼
   rusqlite / reqwest                     node:sqlite / fetch
```

分流策略（二选一，推荐 B）：
- **A. 反向代理**：Rust axum 做前台，未迁端点 `proxy` 到 Node；统一入口在 Rust。
- **B. 端点契约迁移表**：Tauri 壳启动时按迁移表决定把 `{port}` 指向 Rust 还是 Node；
  前端始终打 `http://127.0.0.1:{port}`，由壳决定后端。

B 更简单且复用现有 loopback 加固，推荐。

---

## 4. 分阶段路线图

### Phase 0：准备期（2–3 周）— 不动业务，搭脚手架

| # | 任务 | 产出 | 验证 |
|---|---|---|---|
| 0.1 | 在 `packages/` 同级建 Rust workspace 成员 `engine-rs/` | Cargo workspace + 空 crate | `cargo build` 通过 |
| 0.2 | 选定并集成类型同步：`ts-rs`（推荐，TS→Rust 单向生成也可用 `typify`） | 构建脚本生成 `.ts` | 生成的类型与 core 现有类型 diff=0 |
| 0.3 | 抽取 TS 现有接口为 trait-like 抽象（为 Rust 镜像） | `core/src/__ports__/` 接口清单 | 接口覆盖所有域入口 |
| 0.4 | 固化 golden 测试向量：跑现有 core 测试，dump 输入/输出 corpus | `engine-rs/tests/golden/*.json` | 每域 ≥10 向量 |
| 0.5 | 选定 HTTP 框架：**axum**（与 tokio/reqwest 同生态）+ `tower-http` | axum hello 端点 | curl 通过 |
| 0.6 | 选定日志/错误：复用壳的 `tracing` + `thiserror`（已就位） | 错误类型骨架 | 与壳一致 |
| 0.7 | CI：加 Rust workspace 的 `cargo test --workspace` + clippy + tarpaulin 覆盖率 | workflow | 门禁通过 |

**Phase 0 不改任何运行时行为**，纯脚手架。完成即可发布（无变化）。

### Phase 1：叶子域移植（4–6 周）

目标域（无内部依赖，最易）：
- `utils` → `engine-rs-utils`
- `translation` → `engine-rs-translation`（i18n 资源搬运 + rustyline/i18n）
- `models` → `engine-rs-models`（数据类型 + serde + `ts-rs` 派生）

每个域：TDD 移植 → golden 差分测试 100% 通过 → 接入 axum（若该域有 HTTP 端点）→
端点契约测试（与 TS 版响应 diff=0）→ 切流量。

**退出标准**：3 个叶子域全部上线 Rust 版，Node 侧对应端点停服，前端无 bug。

### Phase 2：中层域（6–10 周）

目标域：
- `notify` / `materials` / `prompts` / `state` / `skills`

`state` 是关键：含持久化与崩溃恢复。需保证 **SQLite schema 与 Node 版兼容**
（或一次性迁移：dump→rusqlite 重建）。`rusqlite` 已在壳里用，经验可复用。

### Phase 3：高层域 + 引擎核心（10–16 周）

目标域：
- `pipeline` / `play` / `interaction` / `forecast` / `interactive-film`
- `llm`：流式 SSE 解析、工具调用、重试退避。`reqwest` 已是壳依赖，复用。
- `agent` / `agents`：最难。工具调用循环、上下文治理（inputGovernanceMode v2）、
  多 Agent 编排。**必须 1:1 复刻提示词模板**（逐字搬运 `prompts/`，防漂移）。

**退出标准**：Node sidecar 内全部端点已迁，`engine/dist` 不再被引用 → **移除 Node sidecar**。
此时已是「纯 Rust + React SPA(webview)」形态。

### Phase 4（可选）：去 HTTP，前端改 Tauri invoke（3–5 周）

收益：去掉 loopback HTTP 服务、端口绑定、SSE 中转；前端直接 `invoke()` 调 Rust。
- 用 `ts-rs` 生成 invoke 参数类型
- SSE 改 Tauri event（`app_handle.emit`）
- 移除 `supervisor`（无 sidecar）、loopback guard 降级为可选

代价：前端 fetch 调用全改。**仅在不再需要「Studio 是独立 Web 应用」时做**。

### Phase 5（可选）：前端原生化（开放式，8–16 周）

若追求极致原生体验：React SPA → egui/iced/slint。
此阶段独立、可逆、非必须。webview 方案本身已是「桌面级」。

---

## 5. 横切关注点（Cross-Cutting）

### 5.1 类型系统单一真源

- **canonical types 在 Rust**（`engine-rs-models`）
- `ts-rs` 派生 → 生成 `.ts` → 前端 + Node 侧 import
- 过渡期 Node 侧逐步从生成文件读类型，替换手写类型
- 禁止手改生成文件（CI 校验 `generated/*` 与 build 输出一致）

### 5.2 测试策略（parity 是硬指标）

三层防线：
1. **单元差分**：每个移植函数配 golden 向量，`assert_eq!(rust_out, ts_out)`
2. **端点契约**：同一 HTTP 请求打 Rust 与 Node，响应 JSON 结构化 diff=0
3. **E2E 回归**：复用 `packages/studio/e2e/*.spec.ts`（已有），切换后端重跑

覆盖率门禁：每个域 ≥80%（与现有规范一致），关键域（agent/llm/state）≥90%。

### 5.3 LLM 调用确定性

LLM 流式响应在 TS 与 Rust 间的差异：
- 流式分块边界不同 → 前端 SSE 消费须按 event 语义而非字节
- 重试/退避算法须 1:1 搬运（指数退避参数、抖动种子）
- 工具调用 JSON 解析容错须对齐（TS 侧的宽松解析行为）

建议：`llm` 域移植时，先用**录制的真实 LLM 响应**做回放测试，不依赖网络。

### 5.4 SQLite 兼容

现状：Node 用 `node:sqlite`（Node 22 内置），壳用 `rusqlite`（bundled）。
两者读写同一 `.db` 文件无问题（标准 SQLite 格式），但：
- 并发模型不同（Node 同步 vs rusqlite 连接池）→ 须定义单一写入者
- 迁移脚本（schema 版本）须双向兼容

建议：过渡期 Rust 与 Node **不共享同一 .db**，各管各的；终态统一到 rusqlite。

### 5.5 提示词与配置资产

`prompts/`、`genres/*.md`、i18n 资源 = 纯文本资产，**逐字搬运**到 Rust
`include_str!` 或运行时加载。禁止「顺手优化」措辞——任何措辞改动单独立项评审。

### 5.6 0GC / 性能规范（延续壳层规范）

- 热路径（agent 循环、流式解析、SSE 转发）禁堆分配：`&str`/`Cow`/预分配 `Vec`
- LLM 流式：`Bytes` 复用 + 零拷贝直写 SSE
- 复用壳层已验证模式：`Read::take` 限量、`with_capacity` 预分配、`&[&str]` 白名单

### 5.7 安全（继承壳层 41 项加固）

Rust 引擎仍在壳的沙箱/loopback 加固之内，额外注意：
- LLM 输出是不可信内容（提示注入）：渲染前净化，复用 `display_field_validation` 模式
- 文件操作复用壳的 `host_api` 路径校验（符号链接/TOCTOU/配额）
- 不引入新的 `unsafe`；`unsafe` 须有安全说明（同壳规范）

---

## 6. 风险登记册

| # | 风险 | 概率 | 影响 | 缓解 |
|---|---|---|---|---|
| R1 | agent/llm 移植行为漂移（输出质量下降） | 中 | 🔴 高 | golden 向量 + 真实 LLM 回放 + 人工抽检 |
| R2 | 提示词模板搬运误差 | 低 | 🔴 高 | 逐字 include_str!，CI 字节级 diff |
| R3 | SQLite schema 不兼容 | 中 | 🟡 中 | 过渡期分库；终态 rusqlite 统一 + 一次性迁移脚本 |
| R4 | 工期失控（107k 行） | 高 | 🟡 中 | Strangler 可随时停；按域优先级排序 |
| R5 | 上游 inkos 新功能无法同步 | 中 | 🟡 中 | fork-and-own 已接受；定期人工 cherry-pick 高价值特性 |
| R6 | 类型同步脱钩（ts-rs 生成偏差） | 低 | 🟡 中 | CI 校验生成文件 + 契约测试 |
| R7 | 性能回归（Rust 移植初版不如成熟 TS） | 低 | 🟢 低 | 基准对比（已有 criterion benches 框架） |
| R8 | Node sidecar 与 Rust 并存期端口/状态竞争 | 中 | 🟡 中 | 单一写入者原则；Tauri 壳统一编排 |

---

## 7. 立即可做的准备工作（本周）

不阻塞、低风险、为后续省大量返工：

1. **冻结 golden 向量**：写脚本跑现有 `packages/core` 全部测试，捕获输入/输出 →
   `engine-rs/tests/golden/`（TS 版若改动，向量随之更新，是活文档）
2. **建 `engine-rs/` workspace 成员**：空 crate + CI 接入（Phase 0.1/0.7）
3. **集成 ts-rs 试跑 `models` 域**：验证类型生成链路通（Phase 0.2 PoC）
4. **梳理 HTTP 端点清单**：从 `packages/studio/src/api/` 提取全部路由 →
   `迁移端点登记.md`，标注每端点依赖的 core 域，作为切换排期依据
5. **建档 ADR-001：fork-and-own 决策**（见 §8）

---

## 8. 架构决策记录（ADR）

### ADR-001：inkosDesktop fork-and-own（放弃上游零修改同步）

- **状态**：提议（待用户确认）
- **背景**：原架构 γ 旁路增强派以「零修改同步上游」为硬约束；全面 Rust 化与此互斥
- **决策**：inkosDesktop 转为 inkos 的独立 Rust 分支，自主演进引擎；不再追求自动同步上游 release
- **代价**：失去上游新功能的免费获取；获得完全自主 + 性能 + 单语言栈
- **复核点**：每次上游 major release 时人工评估是否 cherry-pick 高价值特性

### ADR-002：Strangler-Fig + HTTP 边界 + 自下而上

- **状态**：提议
- **决策**：采用绞杀者模式，保持 HTTP `/api/v1/*` 契约不变，按 core 域依赖图底向上移植
- **替代方案**：Big-Bang（否决：风险过高）、N-API 包装（否决：未消除运行时）

### ADR-003：类型真源在 Rust，ts-rs 单向生成

- **状态**：提议
- **决策**：canonical 类型定义在 `engine-rs-models`，`ts-rs` 派生生成 `.ts` 供前端/Node 消费
- **替代方案**：typify（TS→Rust，但 TS 类型表达力弱于 Rust，反向生成丢信息）

---

## 9. 度量与验收

每个 Phase 退出标准（硬指标）：

| 指标 | Phase 1 | Phase 2 | Phase 3 | 终态 |
|---|---|---|---|---|
| Rust 域覆盖率 | 3 叶子域 | +5 中层 | +全部高层+agent | 16/16 |
| Node sidecar 端点数 | 原量-已迁 | 继续减 | **0** | N/A（已移除） |
| golden 差分测试通过 | 100% | 100% | 100% | 100% |
| E2E (`studio/e2e/*.spec`) | 全绿 | 全绿 | 全绿 | 全绿 |
| clippy / tsc | 0 警告 | 0 警告 | 0 警告 | 0 警告 |
| 分发包体积 | 持平 | 持平 | 下降（去部分 Node） | **-Node runtime** |
| 冷启动时间 | 持平 | 持平 | 下降 | **最低** |

---

## 10. 开放问题（待评审决议）

1. **Phase 4/5 是否执行？** 去 HTTP / 前端原生化是非必须的「完美主义」阶段，
   取决于是否仍需「Studio 作为独立 Web 应用」属性。建议到达 Phase 3 终态后评估。
2. **CLI/TUI 去向**：`packages/cli`（13.7k 行，含 ink TUI）是否需要 Rust 化？
   桌面用户用不到 CLI；若保留 CLI 产物，可暂不迁（Node CLI 独立分发）。
3. **studio SPA 长期形态**：保留 React on webview vs 迁 Tauri 原生组件？
   React 生态成熟，建议保留 webview 至 Phase 5 再议。
4. **是否保留 WASM 插件运行时**：已是 Rust（wasmtime），保留，不动。

---

## 附录 A：域 → 依赖 → 优先级矩阵

| 域 | 依赖 | 难度 | 优先级 | Phase |
|---|---|---|---|---|
| utils | — | 🟢 | P0 | 1 |
| translation | — | 🟢 | P0 | 1 |
| models | — | 🟢 | P0 | 1 |
| notify | models | 🟢 | P1 | 2 |
| materials | models | 🟢 | P1 | 2 |
| prompts | models | 🟡 | P1 | 2 |
| state | models | 🟡 | P1 | 2 |
| skills | prompts | 🟡 | P2 | 2 |
| pipeline | 多域 | 🟡 | P2 | 3 |
| play | state/models | 🟡 | P2 | 3 |
| interaction | models | 🟡 | P2 | 3 |
| forecast | pipeline/models | 🟡 | P2 | 3 |
| interactive-film | models/play | 🔴 | P3 | 3 |
| llm | utils | 🔴 | P3 | 3 |
| agent | llm/pipeline/state | 🔴 | P3 | 3 |
| agents | agent | 🔴 | P3 | 3 |

## 附录 B：技术选型清单

| 关注点 | 选型 | 理由 |
|---|---|---|
| HTTP | axum | tokio 原生，与 reqwest/tower 同生态 |
| 序列化 | serde + serde_json | 已是壳依赖 |
| 类型同步 | ts-rs | Rust→TS 单向，Rust 为真源 |
| SQLite | rusqlite (bundled) | 壳已用，经验复用 |
| HTTP 客户端 | reqwest (streaming) | 壳已用 |
| 日志 | tracing | 壳已用 |
| 错误 | thiserror + anyhow | 壳已用 |
| 异步 | tokio | 壳已用 |
| 测试 | cargo-test + criterion + assert_cmd | 壳已用 |
| TUI（若迁 CLI） | ratatui | ink 的 Rust 对应 |

## 附录 C：本规划与现有壳层规范的衔接

- 性能/0GC：延续 `开发时SpecCoding'sPlan/性能优化/01-02_*.md` roadmap
- 安全：延续 `变更记录文档/20260807/27-41_*.md` 41 项加固
- 错误处理：延续壳层 `error.rs`（AppError + with_details/with_suggestion 模式）
- 测试规范：延续 80% 覆盖率 + 反向验证（见 00_SUMMARY）
- 文档归档：每域迁移完成按 `变更记录文档/{YYYYMMDD}/` 归档
