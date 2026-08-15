# 67 号：interaction 域 —— 确认式生产任务分支与建书迁移

**日期**：2026-08-15
**阶段**：Phase3 strangler 迁移（agent 端点收官段：65 校验装配 → 66 工具循环 → 67 生产任务分支）
**契约源**：`packages/studio/src/api/server.ts` L5075-L5254（生产分支主链）、L1587-L1869（executeConfirmedProductionAction）、L1403-L1585（确认判定 + write_next 工具）、`packages/core/src/interaction/action-envelope.ts`（意图枚举 + 写章启发式）、`packages/core/src/interaction/book-session-store.ts` L207-L221（migrateBookSession）、`packages/studio/src/api/task-store.ts`（快照，39 号已移植）

---

## 一、背景

65/66 号交付了 POST /agent 的校验装配与聊天工具循环，但 button/slash 确认的生产动作（写章、建书等）仍缺口：这类请求需走"任务分支"——有 taskId、可中止、有磁盘快照（刷新恢复任务卡）、受单任务闸门约束。67 号补齐该分支的首批两个意图执行器（write_next / create_book——两者分别接线 58 号 writer 编排链与 architect 建书链），并完成建书后会话绑定迁移。

## 二、交付

### 新文件 `engine-rs/src/server/agent_production.rs`（~1000 行）

- **枚举与归一**：`ActionSource`（4 值）/ `RequestedIntent`（18 值）逐值对齐 zod；`normalize_action_source` / `normalize_requested_intent`（空回退 + 非法 400 `INVALID_ACTION_SOURCE` / `INVALID_REQUESTED_INTENT`）。
- **写章启发式**（action-envelope.ts 正则逐字移植，lib 单测固化正反例）：
  - `is_write_next_instruction`（全词匹配，不含 `/write` 形态）
  - `is_explicit_write_chapter_command`（zh 前缀单词 + 章节词 + 分隔/结尾；en 词边界）
  - `is_write_next_production_request` 三来源归一（显式 intent / free-text 明确命令 / 其它来源写作指令）
  - `resolve_confirmed_intent`：写章三来源 → `write_next`；button/slash 的 11 个确认 intent 透传；其余 None 走聊天分支。
- **注册表（进程级）**：`reserved_production_sessions`（sessionId→taskId，**任何 await 前同步预留**，并发第二确认请求直接 409 `PRODUCTION_TASK_ALREADY_RUNNING`）、`active_confirmed_tasks`（taskId→AbortHandle）。
- **快照对账升级**（`load_reconciled_task_snapshot` 从 session_routes 迁入并升级）：running 快照 + 本进程注册表无该任务 id → 改写 error 终态落盘（64 号版"一律视为遗留"升级为真注册表判定，与 TS 对齐）；`find_active_running_task` = 对账后 running 且本进程持有句柄。
- **主流程 `run_confirmed_production`**：同步预留 → 快照检查（409）→ 建书预备广播（`book:creating` + create-status 内存态）→ **user 消息先写 transcript**（刷新恢复用户气泡）→ 执行卡装配（tool/agent/label/stages/args）→ `tool:start`（`background:true` + sourceRequestId）→ 执行器 → 进度 logs 快照（80 条上限）→ `tool:end` → 终态快照 → 成功路径建书迁移 + `book:created` / 失败路径 `book:error` + 502 响应 → 收尾助手工具消息（manualToolAssistantMessage：零 usage + `stopReason:toolUse` + legacyDisplay 工具卡）→ finally 释放注册表。
- **执行器**：
  - `execute_write_next`（createWriteNextChapterTool）：单章 / 多章连写（chapterCount 1-20 校验 + 每轮 status≠`ready-for-review` 停止 + onChapterComplete 进度文案）；`needsReview` → 卡按 error 态但 200 返回（isError 语义）；响应文案中英双语逐字（buildWriteNextResponseText）；接线 58 号 `write_next_chapter` + `build_write_next_agents/ctx`（BooksRuntime 真实 router）。
  - `execute_create_book`（createSubAgentTool 的 architect 分支）：payload 缺 title → 统一错误面；`build_studio_book_config_pub` 派生（id 派生失败回退 `book-{毫秒 base36}`）→ `init_book` 同步执行（58 号 staging 原子落盘链）→ `Book "{title}" ({id}) initialised successfully...` + `details:{kind:"book_created"}`。
  - 其余 9 个确认意图 → `Unsupported confirmed action: {intent}` 错误路径（域未迁移，见偏差备案）。
- **错误统一面**（TS 行为逐字考证）：执行器内一切失败（含 `CONFIRMED_ACTION_PAYLOAD_INCOMPLETE` 类 ApiError）经分支 catch 统一 `formatAgentActionFailure`——busy → 409 `BOOK_BUSY`；其余一律 502 `AGENT_ACTION_FAILED`（消息原文）。**TS 端 ApiError 的原 code 不透出响应**（被 catch 转写），Rust 对齐此行为。
- **进度持久化竞态防护**：进度回调经 spawn 保持同步签名，句柄入 `ProgressSink` 队列，**终态持久化前 drain 等待**——否则滞后的进度快照（running）会覆盖终态快照，被对账逻辑误判改写 error（E2E 开发中实际捕获并修复的竞态）。
- **双语文案**：PIPELINE_STAGES（writer 7 / architect 5 / reviser 4 / auditor 1 阶段）、AGENT_LABELS / TOOL_LABELS（18 工具）、formatTaskElapsed、`buildRunningTaskContextBlock`（后台任务状态块，中英全文）。
- **语言解析**：`current_project_language`（raw config `language`，默认 zh）对齐 TS `currentProjectLanguage`。

### 修改文件

- **`agent_route.rs`**：参数面补齐（actionSource/requestedIntent/actionPayload 结构守卫/clientRequestId→sourceRequestId 128 码元截断/requestedSkills·disabledSkills 校验）；`agent:start` 广播真实值；确认意图判定 → `run_confirmed_production` 分支路由（200 响应 `{response, details.toolExecutions[卡], session}` / agent:complete / agent:error）；聊天分支 system prompt 注入后台任务状态块（suppressProductionTools 硬剔除面：read/ls/grep 聊天工具集天然不含生产工具）。
- **`book_session_store.rs`**：+`migrate_book_session`（migrateBookSession：未绑定会话 → 绑新书 + kind 升 book；已绑定 → Err 上层静默忽略，TS SessionAlreadyMigratedError 语义）。
- **`session_routes.rs`**：`deleted_session_sessions` → `deleted_session_ids` pub(crate)（生产分支 persist/append 守卫复用）；本地对账函数删除，改用 agent_production 升级版。
- **`books_routes.rs`**：`build_write_next_agents` / `build_write_next_ctx` → pub(crate)（67 号生产分支复用）。
- **`server/mod.rs`**：注册 agent_production 模块。

### 测试

- **E2E `agent67_e2e`（5 个）**：write_next 成功全链（响应卡 + stages 7 全 completed + logs 进度 + 快照（sourceRequestId 透传）+ transcript（user 先写 + toolUse 收尾）+ SSE 四事件序）；预留闸门 409；create_book 成功（书落盘 + 会话迁移 GET 断言 bookId + SSE `agent:creating→book:creating→tool:start→tool:end→book:created→agent:complete` 序）；未支持意图 / 缺 title 502；聊天分支背景任务注入（mock 捕获 system prompt 断言状态块）。各测试独立 sessionId（进程级注册表/快照路径防并行污染）；测试持有同一 BroadcastHub 实例（app 与 subscribe 共享 runtime）。
- **lib 单测（7 个）**：启发式正反例（含"请帮我"双前缀不命中的 TS 语义固化）、意图归一三来源、失败分类、枚举解析、标签/阶段表、运行任务块渲染、base36。
- **49 号既有 flaky 修复**：`put_chapter_replaces_archives_and_marks_review` 的归档 `find(_manual_)` 依赖 read_dir 顺序（APFS 哈希序不稳定），改为排除预置 VERSION_ID 精确定位——全套并行时偶发失败的根因（与 67 号无关的固有脆弱断言，8 次连跑验证修复）。

## 三、parity 要点

1. **单任务闸门同步预留**：TS 注释明确"名额必须在任何 await 之前同步预留"（check-then-act 竞态），Rust 以 std Mutex insert 在首个 await 前完成，语义逐字。
2. **user 消息先写**：任务运行期间刷新页面用户气泡从 transcript 恢复；完成/失败只追加助手工具消息（instruction 不写第二遍）。
3. **错误码不透出**：TS 确认分支 catch 把一切非 ConfirmedActionExecutionError（含 payload 缺失 ApiError）转写为 `AGENT_ACTION_FAILED` 502（busy 例外 409 BOOK_BUSY）——Rust 相同。
4. **isError 语义**：写章完成但审稿未通过 → 任务卡 error 态 + 响应 200 结果文本（前端按错误态展示）。
5. **快照对账**：running 快照 + 本进程无句柄 = 旧进程遗留 → 改写 error（前端不再恢复出永远运行中的任务卡）。
6. **建书迁移**：architect 成功 → details.kind=book_created → migrateBookSession 绑定会话 → `book:created` 广播（sessionId + book summary）。

## 四、偏差备案

1. **9 个确认意图未接线**（short_run / generate_cover / script_create / storyboard_create / interactive_film_create / translation_create / play_start / draft_structure / connect_choice / remove_node）：依赖域在 62 号缺口清单（interactive-films/projects 10 条、translations 6 条、play 4 条），当前落 `Unsupported confirmed action` 502 兜底，随域迁移逐个接线。
2. **单章写作中途不可截断**：TS `runWithAbortSignal` 在 pipeline 内部检查点抛中止；Rust 66 号写作链无内建中止信号，67 号多章轮间轮询 abort，单章中途截断随写作链中止信号专项补。
3. **actionPayload 非 strict 校验**：TS zod strict（未知字段 400 `INVALID_ACTION_PAYLOAD`）；Rust 67 号仅结构守卫（必须 object）+ 消费面子集字段（createBook/writeNext），完整 strict 随 payload 域接线补。
4. **model 校验缺失**：TS `/agent` 对 reqModel 做 isTextChatModelId 校验（非文本模型 400），65 号起备案未移植（涉及模型表）。
5. **abort 端点未接确认任务注册表**：TS `findRunningTaskController` 经 reserved→active 链找 controller；Rust 64 号 abort 端点只接聊天轮注册表，确认任务的停止入口随后续号接线（注册表已就绪）。
6. **manualToolAssistantMessage 的 provider/model**：TS 用请求级 service/model 回退配置链（configuredEntry 深链）；Rust 取 payload.service/model 回退 raw config（configuredEntry 解析随 service 域深化）。
7. **agent:start 广播时序**：TS 在会话装配前，Rust（65 号起）在装配后——既有偏差，67 号保持。

## 五、暂缓件（沿 66 号清单滚动）

- restoreAgentMessages 历史回放（restore.ts 后半）+ stream_chat 逐 delta 回调
- 生产工具面其余执行器（依赖域迁移）
- resumeFrom 断点续导、/test chatCompletion 深链、fetchWithProxy、resolveEffectiveLLMConfig cli/daemon 分支、attachments 归一化、模型四层解析
- 62 号清单剩余缺口：interactive-films/projects 域 10 条、translations 6 条、play 4 条、daemon/doctor/logs/radar 7 条、foundation/revise 1 条

## 六、验证基线

| 套件 | 结果 |
| --- | --- |
| `cargo test --lib` | **982** 通过（975 + 7 新增） |
| `cargo test --test golden_leaf` | 76 通过 |
| `cargo test --test e2e_write_next_contract` | **95** 通过（90 + 5 新增；连跑 8 次稳定） |
| `cargo test --features export-bindings --lib` | 1141 通过 |
| `cargo clippy --lib --tests --bins` | 零警告 |
| `packages/core vitest run` | 185 文件 / 1798 测试全绿 |

## 七、影响面与下一步

- **影响面**：POST /agent 生产动作路径（前端确认卡按钮 / 写章 quick-action / free-text 写章命令）；sessions 任务卡快照对账升级（64 号端点行为不变、语义更准）；58 号 books/create 状态机被 create_book 意图共享。
- **下一步（68 号候选）**：abort 端点接确认任务注册表（findRunningTaskController 链）+ restoreAgentMessages 历史回放；或按 62 号清单清缺口（interactive-films/projects 域 10 条为最大块）。
