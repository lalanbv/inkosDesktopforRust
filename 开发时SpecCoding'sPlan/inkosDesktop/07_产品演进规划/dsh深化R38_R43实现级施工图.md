# dsh 对标 v7 深化：R38–R43 实现级施工图

> 日期：2026-09-22 ｜ 编号：544 号 ｜ 前序：541 号 v7 路线（R38–R46）→ 本篇为其 P0（R38/R39/R40）**实现级施工图** + P1（R41/R42/R43）深化设计。
> 证据升级声明：v7 建立在「dsh 官方文档 12 篇全文 + 本地勘察级事实」之上；本篇把双侧证据升级到**源码级**——dsh 侧 sparse-clone 实读 `packages/core/tools/src/index.ts`（1955 行，注册表+三段管线本体）、`packages/spill/spill-policy/src/index.ts`（227 行全文）、`packages/core/scope/src/index.ts`（204 行全文）、`schema.ts`（defineTool :545）、`ts-types.ts`（schema→TS 投影器）；本地侧对工具分发面、agent_loop、skills、SSE、router、write_next、streaming_client、agent-tools.ts、agent-session.ts 做了 file:line 级二次提取。
> 行号口径：本地行号以 **73c83f41（543 号 HEAD）** 为快照；并行会话正在修改 write_next.rs 等文件，施工时以**函数名锚点为准**、行号仅作导航。

## 1. 源码级关键发现（对 v7 路线的三点修正/强化）

### 1.1 现状比 v7 判断更好：抽象已存在，缺的是「同位化」

v7 判定「无注册表」。源码级复查修正为：**注册表的原材料已齐备，散的是装配**——

- Rust 侧已有 schema 结构体 `InteractionTool { name: &'static str, description: &'static str, parameters: Value }`（interaction/project_tools.rs:164，注释自述「工具注册表项」）与执行器 trait `LoopToolExecutor::execute(&self, name: &str, args: &Value) -> ToolResult`（interaction/agent_loop.rs:88），主循环 `run_agent_loop`（:97）已经只依赖 trait（`tool_exec: &dyn LoopToolExecutor`，:99；调用点 :176；轮上限 `MAX_ROUNDS = 12`，:107）。
- **缺的三块**：①schema 与执行器不同位（schema 在 project_tools 等处的 `interaction_tools()` 类函数，执行在六个 family 分发函数）；②可用性门控是隐式的三重机制——`ChatToolRouter::execute`（server/agent_route.rs:1407）里 `if let Some(deps) = &self.xxx_deps` 的在场判断 + `PRODUCTION_MUTATION_TOOL_NAMES` 静态抑制集 + family 分发函数内部各自的 `match name`；③双端工具面（名称/描述/schema）零机械对照。
- TS 侧 32 个 `createXxxTool` 工厂（agent-tools.ts，返回 pi-agent-core `AgentTool<typeof Params>`）+ `createModeTools(params)`（agent-session.ts:799）等临时组装点——形态与 dsh `ToolDefinition` 已同构其子集。

因此 **R38 是收拢而非新造**，迁移风险低于 v7 预估；`agent.rs` 2 行占位与 agent 域移植仍是独立悬案，不因本项自动解决。

### 1.2 dsh 三态调度器 = R39 管线的实现形态答案

`ToolRuntime::execute`（tools/src/index.ts:1348）= `prepareExecution` → `completeScheduledExecution`，以三态枚举推进：`dispatch`（有序 pre+守卫 → 工具体）→ `post-result`（有序 post）→ `final-result`（finalizeContent+finish）。这个「同步 prepare / 异步 dispatch / 有序 post」的分解使并行调度器可以在 dispatch 段并发、在 pre/post 段保序。**本仓不需要其并行批执行**（聊天面单调用为主，写作链不经 LoopToolExecutor），但三态形态 + 单调守卫（只 deny/abstain、无 allow、不可重排，:706-749）+ restriction 编译缓存（:682-701）直接作为 R39 的结构蓝本，Rust 实现为枚举状态机即可，无需事件运行时。

`ToolDefinition`（:216）的字段集是 R38 trait 设计的直接参照：`execute(args, exec)` + `output{schema, render}` + 可选 `finalizeContent` / `timeoutMs`（协作式超时预算，由独立 policy 包装 :244）/ `isConcurrencySafe`（默认互斥，显式 opt-in，:263）/ `presentCall/presentResult`（UI 投影，纯函数可回放）。**注意**：`schemas()` 白名单只透出 name/description/parameters——`timeoutMs` 等元数据永不对模型可见（:248-249），这条纪律进 R40 目录生成。

### 1.3 spill-policy 227 行 = R42 的完整验收规格

`spill-policy` 是一个**纯 post-execute 消费方**（`inject: ['tools']`，不注册服务、不拥有存储），五条边界语义全部可直接搬运为验收向量：

1. **委托先行**：`const decision = await next()` 先让下游（hook 等）落定，只约束被接受的纯文本结果（index.ts:189-192）。
2. **read 豁免防回环**：`read → spill → 再 read` 死循环豁免（:191；durable-log 臂不豁免——日志副本非模型上下文，:214-216）。
3. **通知预留进预算**：replacement = preview + 通知，通知字节数按最坏情形预留进 `maxInlineBytes`，不变量「replacement 永不超 cap」；不满足则放弃替换保留原内联（:158-181）。
4. **尽力而为**：无 session 归属/无 spillStore 后端/保存失败 → 记日志保留原内联，「spill 失败绝不能把成功调用变 isError」（:133-155）。
5. **配置装载期校验**：负数/非整数 cap 在 load 时抛错（fail the deployment, not the tool，:109-113）。
另有纯文本判定：任一非 text block 即不处理（flattenPlainText :81-88）。

### 1.4 schema 双投影 = R40 目录生成的范式印证

dsh 的 `schemas()`（native function calling）与 `renderToolsSdk/renderToolsSdkPy`（PTC SDK 文本，ts-types.ts:297）是**同一工具库的两个生成投影**——TS 类型文本由 `jsonSchemaToTs` 从 schema 机械渲染（:240），非手写。工具目录就是 schema 的第三投影，**必须生成、不可手写**，否则重蹈 plugin-system.md 漂移覆辙。

## 2. R38 工具注册表单点化——实现级设计

### 2.1 Rust 目标形态

```rust
// engine-rs/src/interaction/registry.rs（新增）
pub struct ToolCtx<'a> {           // 会话级依赖注入（借用，非 'static）
    pub root: &'a Path,
    pub chat: Option<&'a ChatToolRouterDeps<'a>>,   // 现 ChatToolRouter 字段组收拢
}
pub trait ToolDef: Sync + Send {
    fn meta(&self) -> &'static InteractionTool;     // 静态 schema，'static 表项零堆分配
    fn available(&self, ctx: &ToolCtx<'_>) -> bool; // 显式化现「deps 在场」门控
    fn execute<'a>(&'a self, ctx: &'a ToolCtx<'a>, args: &Value) -> BoxFuture<'a, ToolResult>;
    fn mutation_kind(&self) -> MutationKind;        // Production/Chat 替代 PRODUCTION_MUTATION_TOOL_NAMES 字符串集
    fn is_concurrency_safe(&self, _args: &Value) -> bool { false }  // dsh :263 同款，先立后用
}
pub struct ToolRegistry { table: HashMap<&'static str, &'static dyn ToolDef> } // OnceLock 全局
```

- 现六个 family 分发函数（project_tools.rs:447 `execute_book_file_tool` / book_edit_tools.rs:446 `execute_book_edit_tool` / film_authoring_tools.rs:212 / play_tools.rs:62 / forecast_tools.rs:232，及 agent_route.rs:1407 内联的 propose/research/import/sub_agent/use_skill/book_reference 单件分支）各自改为「注册模块」：每模块 `pub fn defs() -> Vec<&'static dyn ToolDef>`，注册表启动期汇聚。
- `ChatToolRouter` 保留为 `ToolCtx` 的装配体；其 `execute` 收拢为 `registry.lookup(name)` → `available?` → `mutation_kind 判 suppress` → 管线（R39）→ `def.execute`。
- **迁移分 6 批、每批一个 family、双跑一致性测试护航**（旧 match 与注册表并行执行断言等价，绿后删旧）——禁止一次性 flip。

### 2.2 TS 侧同构

agent-tools.ts 的 32 工厂保持工厂函数形态（闭包携带 deps，与 pi-agent-core `AgentTool` 契约不变），新增**统一出口**：

```ts
export function buildChatToolSet(deps: ChatDeps): ReadonlyMap<string, AgentTool<any>>
```

把 agent-session.ts:799（`createModeTools`）、:819、:981、:986 等散装组装点收拢到该出口；工厂内部继续各自实现（不强行重写 32 个工厂——回退端非性能敏感，避免无谓移植面）。

### 2.3 差分器工具面维度（本项核心收益）

- 双端各暴露 `GET /api/v1/debug/tools`（loopback 限定，沿 health 超集先例 493 号）：`[{name, description, parametersSha256}]`，parameters 先键排序规范化再哈希。
- `scripts/engine-contract-diff.mjs` 新增 tool-catalog 断言组：名称集合相等 + 逐工具 description/parametersHash 一致 + 计数相等。
- **首跑预期产出「差异清单」而非全绿**：Rust 交互面工具与 TS 32 工厂的差集（含 `createInteractionToolsFromDeps` 的 project-tools 族）先全部登记进豁免表并逐项定性（真缺移植/有意裁剪/命名漂移），豁免清零为目标态——沿 487→491 号「先备案后出清」节奏。

### 2.4 验收

差分器工具面维度建立且豁免表有账；6 批迁移各自红绿通过；bench 新增 `tool_dispatch` 基准（lookup+noop execute）；`PRODUCTION_MUTATION_TOOL_NAMES` 删除（被 `MutationKind` 取代）。

## 3. R39 三段执行管线——实现级设计

### 3.1 形态

```rust
pub enum GuardVerdict { Allow, Deny(String), Ask }   // Ask 预留，本轮实现为 Deny("审批未启用")
pub trait ToolGuard: Sync + Send { fn check(&self, name: &str, args: &Value, ctx: &ToolCtx<'_>) -> GuardVerdict; }
pub trait PostHook: Sync + Send {                    // 首消费者=R42 spill
    fn post<'a>(&'a self, name: &'a str, res: ToolResult) -> BoxFuture<'a, ToolResult>;
}
pub async fn run_pipeline(reg: &ToolRegistry, name: &str, ctx: &ToolCtx<'_>, args: &Value) -> ToolResult
// 序：lookup→available→suppress 判定→guards(序=注册序，Deny 短路)→around{超时+重试+计时}→body→post 派链
```

- **pre 段**：loopback_guard（server/loopback_guard.rs）与 segment_guard 的工具面判定归一为两个 `ToolGuard` 实现项；守卫判定表沿 R31 体例 golden 锁定（dsh「单调守卫无 allow、身份保护」语义照搬：守卫只可拒绝，不可放行他人已拒者）。
- **around 段**：①超时 `tokio::time::timeout` + 协作取消（长任务工具转发 CancellationToken——dsh `timeoutMs` 协作语义）；②重试：接 `is_retryable` 分类（206 号 Rust chat_once 既有瞬时分类复用），语义与 R25 接管链协同=工具内重试仅对传输类瞬时错误、不计入模型链 failover 计数；③metrics：耗时/尝试序/error_kind 记入 R26 RunLog——**顺带清偿 streaming_client.rs:199-205 备案**（原文「Rust 无重试环 clientAttempt 恒 1、无 thinkingBudget 恒 disabled」：clientAttempt 从轨迹头注入处开始记真实尝试序；thinkingBudget 另备案不动）。
- **post 段**：`Vec<&'static dyn PostHook>` 注册序派链；spill-policy 五语义（§1.3）逐条转为单元测试向量。
- **defensive-patterns 两条入规范**（v7 §1 已录）：正交结果独立上报——`LoopToolExecution`（agent_loop.rs:67）扩字段 `{took_ms, attempt, timed_out, error_kind}`，超时与业务失败互不吞没；分发器隔离——post/guard 钩子 panic 用 `catch_unwind` 包裹记日志不炸循环。

### 3.2 验收

全工具调用过管线（注册表内无旁路；`debug/tools` 端点加 `pipeline: true` 字段核对）；RunLog 新字段端到端可见；守卫 golden 组绿；超时/重试/钩子异常三行为测。

## 4. R40 生成式目录 + 编号工具化——实现级设计

### 4.1 三目录生成器（scripts/gen-catalog.mjs）

| 目录 | TS 源 | Rust 源 | 校验 |
| --- | --- | --- | --- |
| 工具目录 | `buildChatToolSet` 导出（R38 后）+ debug/tools 端点活体抓取 | debug/tools 端点 | 双端 diff（即 §2.3 维度）|
| 端点目录 | server.ts 路由表静态 import 扫描 | **router 装配点穷举测试**：mod.rs:224/234/249/260 四层工厂各挂 route 的清单在 Rust 侧以常量数组维护 + 测试断言「数组元素数==实际 .route( 调用数」（防漏登记，编译后跑） | 双端 method+path 集合 diff |
| SSE 事件目录 | studio use-sse.ts:14-57 常量数组（单一事实源保持） | sse.rs 事件名常量化（现 broadcast(&str) 散字符串 → `sse_events` 模块 const） | 双端集合 diff |

- 产物入库 `docs/catalog/{tools,endpoints,sse}.md`（docs/ gitignore 外与 TESTING.md 同法）；`verify:catalog`=重生成 diff 为空，挂 gate:ts 第 8 步（沿 509 号 gate:ts 编排）。
- **首跑即抓双端漂移清单**（预期：SSE 事件名 Rust 侧散串 vs studio 常量表已对齐过 416 号口径，但无机械锁）。

### 4.2 编号工具化（scripts/next-change-no.mjs）

输入=当日变更记录目录 glob 取最大号 + `git log --oneline -40` 正则提取 `（(\d{3,4})号）` 最大号 + R 段占用表（`docs/catalog/r-slots.json`：R30–R37=Pi 专项、R38–R46=dsh 专项，R 系继续占用即登记）。输出=下一变更号 + 下一可用 R 号；`--check N` 模式供提交前校验。**写档前双查惯例由脚本承担主责，人只复核输出**。

### 4.3 验收

verify 入门禁全绿+负样本注入被拦（故意改 catalog 文件→gate 红）；next-change-no 对当日目录/git log/R 段三源一致性断言。

## 5. R41–R43 深化设计（P1，次级精度）

### R41 token 计量

- Rust 数据形态：`Mutex<SurfaceFold { nodes: Vec<SurfaceNode{seq, tokens, heuristic_tokens}>, anchor: Option<UsageAnchor> }>`，追加 O(1)、`measure()` 克隆节点表 O(surface) 即取即弃（dsh token-meter `measure(session, requestHeader?)` 语义：usage 锚点仅当最近成功调用的规范 envelope 匹配且总量不低于其路由定价锚点时复用，否则全量重估）。
- 写作链接线点=`prepare_write_input`（write_next.rs，函数名锚点；现被并行会话修改中，行号失效）返回处计量并告警进 ContextLens（先观测不阻断）；聊天面在 run_agent_loop 每轮 derive 后计量。
- `GET /api/v1/context-meter` 只读端点；字段并入 R33 的 `inkos.ai.request` span schema（543 号已裁该 schema，对接点以 R33 落地形态为准）。

### R42 工具结果 spill

- 路径 `{project}/.inkos/spill/{sha256(sessionId)前 16}/{random}-{safeName}`；目录 0700、文件 0600、`wx` 独占创建（dsh spill-local 同款防符号链接竞态）；密钥类 env 清洗仅对涉网工具（research）子进程生效。
- 消费点=R39 post 段 PostHook；五条边界语义（§1.3）=验收向量逐条转测试；豁免 `read`/`ls`/`grep`（防回环）。
- 工件差分器把 `.inkos/spill/` 纳入存在性对照（内容豁免——双端预览字节数可有别，locator 形态一致）。

### R43 会话事件日志 + 投影

- 事件词汇子集（对齐 dsh surface 语义：仅产生消息的事件进模型）：`turn_start/turn_end/step_start/step_end/user_message/assistant_message/tool_call/tool_result/session_title`，JSONL 落盘 `{project}/.inkos/sessions/{sessionId}.jsonl`（新增文件，零迁移）。
- Rust 先行：`interaction/session_log.rs`（append+fold）+ `derive_messages()` 纯函数 + golden 向量（含中断尾部修复用例：未封口 turn 补 turn_end）；studio 只读消费切换随后，Node 回退端对齐半窗口 ≤1 轮（541 号 §7 承诺不变）。
- 「模型可见即已记录」：debug 断言=每次 LLM 请求的 system+messages 必须可由日志重建（重建比对放 debug 编译档）；差分器新增会话面维度（双腿同脚本跑聊天，JSONL 事件名集合+计数比对）。
- RunLog（R26）/ContextLens（R21）逐步改为从日志派生（归一三处重复状态），**写作链持久面不动**（buildPersistenceOutput 体系独立）。

## 6. 七维复核更新（相对 541 号 §4 的增量）

- **最优解**：源码级证据推翻了「从零造注册表」的隐含前提，R38 降险为收拢重构；dsh 三态调度与 spill-policy 五语义成为直接蓝本，设计自由度收敛。
- **兼容性**：debug/tools 与 debug/catalog 为新增只读端点；REST/SSE 现面零改动；`.inkos/spill|sessions` 均新增目录。
- **可扩展性**：`ToolDef.available/mutation_kind` 把三重隐式门控显式化；`PostHook` 使 spill/审批/审计成为注册项。
- **安全可靠**：守卫 golden、spill 0700/wx、分发器隔离、正交上报全部落到字段/类型级。
- **高性能 0GC**：注册表 `&'static` 表项 + `OnceLock` 查找零堆分配；`ToolCtx` 借用避免每工具克隆 deps（沿 202-204 号 Box::leak 清偿的借用纪律）；bench 扩 `tool_dispatch` 基准。
- **可实施执行**：R38 六批双跑迁移、R39 管线单 PR 形态、R40 生成器+穷举测试，均有明确红绿路径。
- **真实需求**：三条主链各得一处——写作链（R41 预算观测）、聊天改稿（R38/R39/R42 工具面）、工程债（R40 撞号与漂移根治）。

## 7. 与 R30–R37 封界再确认

R31 传输层守卫 golden（542 号细化四组）与本文 R39 工具层守卫 golden 是两套向量、两份文件；R33 span schema（543 号裁定的 `inkos.ai.request`）是 R41 计量字段的容器，R41 实施顺序排在 R33 之后（沿 543 号修订序 R30→R31→R33→R32）；R37 自扩展技能链的热载通道在 R44（本文未深化，待 P2 立项时按同法出施工图）。

## 8. 施工批次（M1 内部）

1. **批一（R38a）**：registry.rs 骨架 + project_tools/book_edit 两 family 迁移 + debug/tools 端点 + 差分器工具面维度（豁免表建档）。
2. **批二（R38b）**：余四 family 迁移 + TS buildChatToolSet 出口 + 豁免出清启动 + PRODUCTION_MUTATION_TOOL_NAMES 删除。
3. **批三（R39）**：管线三段 + 守卫归一 + RunLog 扩字段 + streaming_client 备案清偿。
4. **批四（R40）**：三目录生成 + verify 入 gate:ts + next-change-no.mjs。
5. 每批独立编号、门禁 8 项+新维度全绿收口；R41–R43 按 M2 逐项立项（各自再出施工图或随批详设）。
