# 86 号变更记录：聊天面卡 details 外露（LoopToolExecution 结构化透传）

- 日期：2026-08-14
- 阶段：Phase3 strangler 迁移（engine-rs）
- 契约源：`packages/studio/src/api/server.ts` L1330-1344（CollectedToolExec：`details?: unknown` 原样携带，未提供时序列化省略键）、L5380-5505（聊天面 `details.toolExecutions` 返回形态）

## 一、背景

66 号聊天面定型时，工具执行卡只透出 result 文本（`LoopToolExecution` 无 details 字段）——80-85 号陆续交付的 play 三件、propose_action、research_web、import_chapters 的结构化 details（确认卡 actionPayload、回合卡 graph、研究报告 sources 等）全部滞留在执行器内部，前端确认卡没有消费面。本轮补上：`LoopToolExecution` 增 details 字段 → loop 收集 → `tool_execution_cards` 原样透出（对齐 TS CollectedToolExec 的 `details?: unknown` 可选键形态）。

## 二、交付

1. **`engine-rs/src/interaction/agent_loop.rs`**：`LoopToolExecution` 增 `details: Option<Value>`；执行收集时 `(!is_error).then(|| result.details.clone()).flatten()`——错误卡不带 details（TS pi-agent 错误路径同样无 details）
2. **`engine-rs/src/server/agent_route.rs`**：`tool_execution_cards` 在 error 键之后插入 `details`（有值才插键，对齐 JSON.stringify 省略 undefined）
3. **E2E `details86_e2e`**（+2）：
   - `propose_card_details_surface_structured_payload`：聊天建书 → 卡 `details` 断言（kind=proposed_action / action=create_book / targetSessionKind=book-create / sameSession / instruction / **actionPayload.createBook 三字段**）
   - `play_step_card_details_surface_graph_and_state`：play 会话推进 → 卡 `details` 断言（kind=play_turn_advanced / worldId/runId/title / sceneText / suggestedActions / currentState.turn / **graph.entities 含 location_hall**——graph 为读取面数组形态）

## 三、parity 要点

- **details 原样不截断**：result 文本仍截 2000 字符（66 号既定），details 整体透出（graph 可能很大——TS 同样原样，前端自管渲染）
- **可选键语义**：无 details 的卡（read/ls/grep/错误卡）不出现 `details` 键——对齐 TS `details?: unknown`
- **错误卡零 details**：is_error 时不收集（TS 工具 throw 路径无 AgentToolResult 可言）

## 四、偏差备案

1. **SSE tool:end 仍只带文本**：TS pi-agent 的 tool:end 事件含结构化 result（content + details）；Rust 66 号面只传 result 文本——实时事件流的结构化透出随 SSE 事件面轮次（如需）再补
2. **transcript 工具轮落盘不含 details**：65/68 号 transcript 写入面只记文本——TS legacyDisplay 同样只在确认面落 details，聊天面 transcript 不落

## 五、暂缓件

- 书会话工具集（sub_agent / generate_cover / 写工具，agent-session edit/book 分支）
- SSE tool:end 结构化事件（如前端需要）
- 既有备案延续：PDF 抽取（83 号）、zh 提示词逐字（82 号）、resumeFrom 增量续放与 importMode=series（85 号）
- 散件：单章写作中途截断、/agent model 校验、resumeFrom REST、fetchWithProxy、attachments 归一化、模型四层解析

## 六、验证基线（2026-08-14）

- `cargo test --lib`：**1110**
- `cargo test --test golden_leaf`：76
- `cargo test --test e2e_write_next_contract`：**146**（+2：details86_e2e）
- `cargo test --features export-bindings --lib`：**1269**
- `cargo clippy --lib --tests --bins`：零警告
- TS：`packages/core` vitest 185 文件 / **1798** 测试全过

## 七、影响面与下一步

- 影响面：`LoopToolExecution` 增字段（唯一构造点 agent_loop 已同步；外部无手工构造方）；80-85 号全部聊天工具的结构化 details 自此可供前端消费——84 号"propose 卡消费面"备案正式关闭
- 下一步（87 号候选）：**首选书会话工具集**（sub_agent 五代理 + generate_cover + read/write 编辑工具族，agent-session 注册面最后的 book/edit 分支）；其次散件收尾（单章截断/model 校验/resumeFrom REST/attachments/模型四层解析）；或既有备案落地（PDF 抽取/zh 提示词逐字/resumeFrom 增量）
