# 68 号：interaction 域 —— abort 注册表接线与历史回放

**日期**：2026-08-15
**阶段**：Phase3 strangler 迁移（agent 端点段：65 校验装配 → 66 工具循环 → 67 生产任务分支 → 68 abort 接线 + 历史回放）
**契约源**：`packages/studio/src/api/server.ts` L4788-L4802（abort 端点）+ L2869-L2878（findRunningTaskController）、`packages/core/src/agent/agent-session.ts` L1356-L1366（abortAgentSession）、`packages/core/src/interaction/session-transcript-restore.ts` L343-L524（restoreAgentMessagesFromTranscript 本体：buildHistoricalToolSummary / textOnlyAgentMessage / requestIdsWithToolActivity / dialogue 过滤）+ L125-L143（appendRestoredHistoryBoundary）

---

## 一、背景

两个收尾缺口：其一，64 号交付的 `POST /sessions/:sessionId/abort` 是空壳（`aborted` 恒 false），67 号的确认任务注册表（reserved/active）与 66 号的聊天轮注册表都没有消费面；其二，Rust agent 循环每轮从空上下文开始（system + 本轮 user 两轮消息），会话历史不进 LLM 上下文——用户追问上一轮内容时模型失忆。68 号补齐两件：abort 端点接线（TS scope 语义 + controller 链）与 transcript 历史回放（工具摘要折叠 + 对话重放 + 边界标记）。

## 二、交付

### 1. abort 端点接线（session_routes.rs + agent_production.rs + agent_route.rs）

- **`find_running_task_controller`**（agent_production，TS L2872 逐字）：**内存优先**（预留表 sessionId → taskId → 句柄——controller 注册与首次快照落盘之间有多个 await 间隙，磁盘快照会漏掉刚启动的任务），磁盘快照只作回退（对账快照 → execution.id → 注册表）。
- **`abort_session` 主体**（TS L4788 逐字）：`scope=chat` 只停聊天轮（不动生产任务控制器）；默认 `all` 两者一起停。任务侧：controller 句柄置位（TS `controller.abort()` 的等价物——67 号 AbortHandle 轮询语义）；聊天侧 `abort_agent_session`。
- **聊天轮注册表与 loop 连通**（66 号遗留）：`AgentSessionHandle` 的 `abort_requested: bool` 占位升级为持 `AbortHandle`，且**与 agent loop 轮询的是同一个 `Arc<Mutex<bool>>`**——abort 端点置位即在下一轮检查点截断（此前注册表与 loop 各持一个孤立 flag，中止永远不生效）。
- 响应 `{ok:true, aborted}` + `agent:aborted` 广播（aborted = 聊天轮或任务任一命中）。

### 2. restoreAgentMessages 历史回放（session_restore.rs 追加，68 号本体）

- **`build_historical_tool_summary`**：全部已提交轮的 toolCall/toolResult 折叠为一条 system 摘要——行格式 `- {tool}:{agent} {status} — {文本}`（`​\s+`→空格折叠 + 180 UTF-16 码元截断补 `...`；`use_skill` 结果固定占位文案"expired; instructions are not active for later turns"——技能指令对后续轮不生效）；最近 8 条；`[历史状态摘要]` 头部两行说明。kind 过滤与 committed 放行语义**相反**（摘要循环对 kind 不匹配含 legacy 一律跳过）——TS 双函数行为差异逐字保留。
- **dialogue 重放**：无工具活动的请求轮 → text-only 消息（user/system 取纯文本，assistant 取非空文本块）；带工具活动的轮次不回放（语义已折叠进摘要），**legacy 无 kind 轮的 user 原话保留**为对话记忆；截最近 **12 条（消息级，非轮级）**。
- **`append_restored_history_boundary`**：历史尾部追加边界 system 消息（中英双语文案逐字），提示模型"以上是已完成的历史，本轮优先最新指令，不要因历史工具结果跳过判断"。
- **agent_loop 注入**：`run_agent_loop` 新增 `initial_history` 参数，插在 system 与本轮 user 之间（pi-agent `initialState.messages` 的等价位置）；agent_route 聊天分支装配时 restore + boundary（项目语言选文案）。

### 3. tokio 缓冲 flush 竞态修复（session_transcript.rs，开发中实测捕获）

**现象**：全套 E2E 并行时约 1/7 概率，transcript 追加"成功"（`write_all().await` 返回）但紧随的同进程读取缺行；inode 级探针显示每次 append 的数据"延迟一个 append"才可见，最旧一次的写入最终丢失。

**根因**：`tokio::fs::File` 的写经**内部缓冲**——`write_all().await` 返回只代表数据进入 tokio 缓冲，drop 时才异步刷到 OS。同进程内紧随的 `std::fs::read` / `metadata` 跑在刷出之前。64 号交付以来潜伏（生产场景：GET /sessions 的 derive 读、restore 读、以及任何"写后即读"路径），68 号历史回放让"写后即读"成为每轮聊天的必经路径而暴露。

**修复**：append 后显式 `file.flush().await`（把缓冲推到 OS 即返回，不到磁盘——比 sync_all 轻）。高并发（16 线程）20 连跑稳定。

### 测试

- **E2E `agent68_e2e`（4 个）**：abort all 停运行中生产任务（注册表插桩 + 句柄置位断言 + `agent:aborted` 广播）；scope=chat 不动任务 + 停聊天轮（聊天注册表与 loop 连通断言——**同一 Arc 置位验证**）；无活动 `{ok:true,aborted:false}`；历史回放注入序（手工构造含工具轮 + 对话轮的 transcript → mock 捕获 LLM messages → 断言 `[system, summary, user, assistant, boundary, 本轮指令]` 六条完整顺序与内容）。各测试独立 sessionId 防进程级注册表并行互扰。
- **lib 单测（8 个，restore_agent_tests）**：工具摘要折叠（行格式/截断/空白折叠）、dialogue 过滤（工具轮排除/legacy user 保留/kind 不匹配排除）、12 条与 8 条上限（消息级语义固化）、use_skill 占位、boundary 中英文与空列表。

## 三、parity 要点

1. **scope=chat / all 双语义**：chat 只截聊天轮；all 生产任务与聊天轮一起停——前端停止按钮与"仅停止本轮回答"分流。
2. **controller 链内存优先**：磁盘快照在 controller 注册与首次持久化之间的 await 窗口内会漏掉刚启动的任务，必须先走内存（TS 注释逐字对应）。
3. **历史回放的双层折叠**：工具活动折叠为摘要（防重放旧工具语义）+ 对话文本重放（保记忆）；boundary 防模型把历史当本轮已执行动作。
4. **12 条 / 8 条上限是消息级**（TS slice 语义，非轮级）。
5. **摘要与 committed 的 kind 过滤相反**：摘要循环跳过 legacy 无 kind 轮，committed 放行——TS 两函数的真实行为差异，非笔误。

## 四、偏差备案

1. **adaptRestoredAgentMessagesForModel / cleanRestored 未移植**：pi-agent 块模型（text/toolCall/thinking blocks）的模型身份适配层——Rust agent loop 为 OpenAI 文本形态，历史回放本就 text-only（工具已折叠进摘要），无块模型可适配；toolResult bridge（"I have processed the tool results."）同理不适用（无裸 toolResult 回放）。
2. **agentCache 未移植**：TS 按 model/root/book/kind/… 十四项缓存键复用 agent 实例并检测失效重建；Rust loop 每轮轻量重建（restore 是纯 transcript 读），无缓存失效问题。
3. **abort 无 clearAllQueues**：TS abortAgentSession 还清 pi-agent 队列；Rust loop 无队列概念，轮询截断即等价。
4. **boundary 语言取项目语言**：TS 传 agent 会话 language（与 currentProjectLanguage 同源）；Rust 每次聊天请求时读项目配置，等价。

## 五、暂缓件（沿 67 号清单滚动）

- 生产工具面其余 9 个确认意图执行器（依赖域迁移）
- 单章写作中途截断（写作链内建中止信号）
- actionPayload strict 校验、/agent model 校验、configuredEntry 深链
- resumeFrom 断点续导、fetchWithProxy、attachments 归一化、模型四层解析
- 62 号清单剩余缺口：interactive-films/projects 域 10 条、translations 6 条、play 4 条、daemon/doctor/logs/radar 7 条、foundation/revise 1 条

## 六、验证基线

| 套件 | 结果 |
| --- | --- |
| `cargo test --lib` | **990** 通过（+8） |
| `cargo test --test golden_leaf` | 76 通过 |
| `cargo test --test e2e_write_next_contract` | **99** 通过（+4；16 线程高并发 20 连跑稳定） |
| `cargo test --features export-bindings --lib` | 1149 通过 |
| `cargo clippy --lib --tests --bins` | 零警告 |
| `packages/core vitest run` | 185 文件 / 1798 测试全绿 |

## 七、影响面与下一步

- **影响面**：POST /sessions/:sessionId/abort（前端停止按钮）；POST /agent 聊天分支（多轮记忆）；所有 transcript 写路径（flush 修复——GET /sessions 与 restore 的"写后即读"一致性）。
- **下一步（69 号候选）**：62 号清单清缺口首选 interactive-films/projects 域 10 条（最大块，解锁 draft_structure/connect_choice/remove_node 三个确认意图执行器）；或先做 translations 6 条 / play 4 条（较小域）。
