# 202 号：启动/内存稳定性审计——Box::leak 静态端口量化评估与 write-next 双链重构

- 日期：2026-09-07
- 分支：develop
- 关联：189 号（节拍沉淀端口引入泄漏面增量）、200 号（Box::leak 记录为已知权衡）、164 号（Rust 引擎为默认后端——泄漏在默认路径上）
- 推送核验：origin/develop 停 6bc65564（97dfa56f 尚未推送，本地领先）；本批次后领先 2 提交。

## 一、量化评估（全景）

全仓 `Box::leak` 共 **66 处 / 6 文件**（改前）：books_routes 40、book_create_routes 15、fanfic_routes 5、ops_routes 4、interactive_film_routes 2、agent_router.rs 0（tests 另计）。

### write-next 热路径（每章自动执行，泄漏主源）

两条装配链逐章泄漏：

| 链 | 泄漏对象 | 构成 |
|---|---|---|
| `build_write_next_agents`（已删） | 8×`RoutedAgent`（各含一份 `AgentRouter` **深拷贝**）+ 1×`RoutedSettler`（内含 `WriterCtx` 的 2 PathBuf + 2 FsStateStore） | ~13 次分配 |
| `build_write_next_ctx`（已删） | FsStateStore + RouterTimelineBeatsChat（含 Arc）+ 2 PathBuf | 4 次分配 |

**修正旧估计**：200 号记录的「~1KB/章」只算了 ctx 链；agents 链每章另有 **9 份 AgentRouter 深拷贝**（每份含 4 String + HashMap，约 300-600B）——合计每章 **~4-6KB 泄漏 + 9 次深拷贝 CPU 浪费**。1000 章长跑 ≈ 4-6MB（绝对量小，但语义性债务：无界、不可回收，且 189 号把 beats 端口加进了泄漏面）。

其余 58 处泄漏在**低频手动端点**（plan/revise/audit/book_create/fanfic 等，用户单次触发）——每触发一次泄漏几百字节，量级可忽略。

## 二、重构（write-next 双链 owned 化）

### 结构改造（llm/agent_router.rs）

- `RoutedAgent.router: AgentRouter` → **`Arc<AgentRouter>`**：chat 实现经 Deref 无感；`FullCycleAuditor.router` 同改（`for_chapter` 的 clone 由深拷贝变计数递增）；`RoutedSettler` 的 `ctx: WriterCtx<'static>` → **`WriterCtx<'a>`**（结构泛型化，`impl SettlePort for RoutedSettler<'_>`）——低频端点的既有 `Box::leak` 装配传 `'static` 值仍兼容（`'static: 'a`），零破坏。
- 全仓 39 处 `RoutedAgent` 构造点适配：`(*x).clone()`（deref+深拷贝）→ Arc 传递/浅 clone。

### 装配改造（server/books_routes.rs）

- 新增 **`WriteNextAgentPorts`**（owned 持有者）：router 共享单一 Arc；7 个 RoutedAgent owned 字段；settle 的 WriterCtx 依赖（2 PathBuf + 2 store）以 owned 字段持有——**结构体不能自引用，依赖与端口分置两层**：`settler()` 借用 self 构造端口、`agents(&settler)` 聚合视图。
- 新增 **`WriteNextPorts`**（owned 持有者）：project_root/genres_dir/store/189 号 beats 端口 owned 持有，`ctx()` 返回借用视图。
- 新增 **`write_next_assembly!(runtime, agents, ctx)` 宏**（`#[macro_export]`）：展开为持有者局部变量 + 同帧借用——调用方一行替换旧两行泄漏装配。
- 删除 `build_write_next_ctx` / `build_write_next_agents` 两个泄漏装配函数；7 个调用点改造：books_routes run_draft（宏）、ops_routes、agent_production（宏，多章循环外装配循环内借用——owned 形态同样成立）、sub_agent_tool（宏）、book_create_routes（手写展开：import 回放需 agents+ctx 长命持有）、bin runner（手写展开：需填 full_auditor）、189 号 wiring 测试（WriteNextPorts 形态）。

### 效果

- write-next 主链泄漏 **13+4 → 0**（全仓 66 → 58，剩余全在低频端点，backlog 同模式迁移）。
- 每章消除 9 份 AgentRouter 深拷贝（CPU + 内存双收）。
- 装配值随调用帧整体 drop——泄漏语义归零，编译器全程护航（无 unsafe、无 Pin）。

## 三、验证

- 全门禁：engine lib **1277** / 集成 **194+70+10**、clippy --all-targets -D warnings 零告警、bench 门禁 ✓（title_dedup 两基准反快 5-9%）、INKOS_DUEL=1 strangler_duel **10/10**（契约面含 write-next 装配真实跑）。
- 真机冒烟（生产形态 bin + INKOS_PROJECT_ROOT fixture）：health 200；`POST /books/b1/write-next` → `{"status":"writing"}` 任务接受（无 LLM mock 下管线在 LLM 阶段失败不落章，符合预期）——**owned 装配路径执行无 panic，进程存活**。
- 本批次纯 Rust 端改动（core/studio 未动），Node 回退端无对应泄漏面（TS GC 自回收）。

## 四、遗留

- 剩余 58 处 `Box::leak`（低频手动端点，每触发数百字节）：backlog——按 202 号同模式（owned 持有者 + 同帧借用）逐端点迁移，无紧迫性。
- `WriteNextAgents.full_auditor` 填充仍每章重建 FullCycleAuditor（owned 值，正常 drop，非泄漏）。
