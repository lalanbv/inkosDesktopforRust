# 217 号：session-transcript 深度双端对照——聊天轮工具消息落盘 + 失败/中止轮持久化对齐

- 日期：2026-09-09
- 分支：develop
- 关联：65 号（聊天轮持久化初版——本批修复其简化面）、66/102 号（中止契约——按 TS 真实行为修订）、68 号（agent 重放面）、210 号（pi-agent 对照先例）
- 推送核验：origin/develop 仍停 6bc65564——**201–216 十六提交均未推送**；本批次后本地领先 17。两项默认值无新答复。

## 一、对照范围与结论

对照 core `session-transcript{,-schema,-restore,-legacy}.ts`（1293 行）↔ engine `session_transcript.rs` + `session_restore.rs`（1596 行）：

| 面 | 结论 |
|---|---|
| 持久层（6 事件 schema / JSONL / 坏行跳过 / seq 排序 / per-session 串行队列） | ✓ 对齐（68 号已备案 version≠1 宽松性差异） |
| restore/derive（committedMessageEvents kind 过滤 + legacy 放行、工具卡聚合、thinking 合并、play 独立卡、skill 抑制、标签表、首消息标题 20 码元截断） | ✓ 对齐 |
| agent 重放（历史工具摘要 8 条/180 截断/use_skill 过期、text-only 对话 12 条上限、边界文案 zh/en 逐字） | ✓ 对齐 |
| legacy 迁移（`.json` → session_created + 事件流） | ✓ 对齐 |
| cleanRestored/adaptRestored | 有意偏差（68 号备案：Rust loop 为 OpenAI 文本形态，无需 pi-agent 块适配层） |
| **事件写路径** | **三处真缺口（本批修复）** |

## 二、修复的三处写路径缺口

1. **聊天轮工具消息不落盘（高）**：TS `persistAgentEvent` 逐消息持久化（assistant toolCall 轮 + 每条 toolResult）；Rust `append_chat_turn` 仅写 user + 纯文本 assistant 两消息——后果：①derive 恢复后 UI 工具执行卡全丢（会话历史跨刷新不完整）；②`buildHistoricalToolSummary` 的 `[历史状态摘要]` 恒空，恢复后 LLM 缺历史工具状态（Node 端有）。修复：每个 LoopToolExecution 写 assistant(toolCall) + toolResult 消息对（toolCallId/sourceToolAssistantUuid 链、arguments 对象、isError/details 面逐字），还原逻辑（pending attach 链）零改动即可消费。
2. **失败轮零持久化（中）**：Rust LLM 错误时只 broadcast agent:error——transcript 无任何事件，刷新后用户消息消失。TS 写 request_started → user → request_failed。修复：`append_failed_chat_turn`（同事件序）。
3. **中止轮写 committed + 占位文本（中）**：Rust abort 轮原写 user + assistant「（无回复内容）」+ request_committed——刷新后出现假回复。TS abort → pi-agent stopReason "aborted" + errorMessage → request_failed + failure 500（`formatAgentFailure` unknown 类 → `AGENT_ERROR`，无工具卡响应）。修复：aborted → `append_failed_chat_turn("aborted")` + 500 AGENT_ERROR + agent:error 广播；66/102 号「200 + 空回复占位 + 工具卡」契约按 TS 真实行为修订（e2e 断言更新，章粒度保留核心断言不变）。

## 三、验证

- engine：lib **1304**（+2：工具轮落盘→derive 卡/摘要断言、失败轮持久化断言）、集成 **196**（含修订后的 102 号 abort e2e——500 AGENT_ERROR + 已落章保留）、clippy 零告警、INKOS_DUEL=1 duel **10/10 真跑**。
- core 1917 / studio 790 / 双 typecheck ✓（TS 零改动复核）。
- 单测关键断言：工具轮转写 → derive 恢复出 `patch_chapter_text` 执行卡（label/status/result/args 逐项）+ restore 产生 `[历史状态摘要]`（`- patch_chapter_text completed — 已替换 12 处。`）且带 kind 工具轮不回放原文（TS 同构：仅 legacy 轮保留 user 原话）。

## 四、遗留备案

1. Rust 聊天 LLM 错误响应 500 `AGENT_SESSION_FAILED` vs TS 502 `AGENT_LLM_ERROR`（`classifyAgentFailure` llm 类）——既有响应面差异，非本批 transcript 范围，待响应面对照批次统一。
2. `piTurnIndex` 在 Rust message 事件恒缺省（schema optional、derive 不消费）——无行为差，不补。
