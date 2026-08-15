# 66 号变更记录：agent 工具循环——tool-calls 协议 + read/ls/grep 工具 + SSE 增量 + abort 截断（Phase3 · interaction 域）

- 日期：2026-08-14
- 范围：engine-rs（`src/llm/provider.rs` LLMRole/LLMMessage 扩展、
  `src/llm/streaming_client.rs` tool_calls 聚合 + tools 参数、
  `src/llm/agent_router.rs` client 公开包装、`src/interaction/project_tools.rs` 新建、
  `src/interaction/agent_loop.rs` 新建、`src/server/agent_route.rs` loop 接线、E2E）
- 契约源：`server.ts` L5268-L5382（runAgentSession 调用面事件）+
  `core/src/interaction/project-tools.ts`（文件工具）+ runtime.ts loop 骨架

## 背景

strangler 迁移 66 号（65 号备案的交互运行时工具面专项第一层）。POST /agent 从
直通单轮升级为多轮 tool-use 循环：LLM 发起工具调用 → 执行 → 结果回填 → 直至
最终文本。协议层（OpenAI tool-calls 流式增量）、工具执行、SSE 增量事件、
abort 截断在本轮打通。

## 交付

### 协议层（llm 域）

- **LLMRole 加 `Tool`** 变体（serde "tool"；ts 类型同步）；**LLMMessage 加
  `tool_calls`（assistant 轮 OpenAI 数组透传）与 `tool_call_id`**（tool 轮），
  全库 19 处构造批量迁移
- **streaming_client**：`ChatCompletionParams.tools`（schema 透传，保留字面扩
  "tools"）；`StreamedCompletion.tool_calls`（聚合结果）；`aggregate_tool_calls`
  纯函数（按 index 合并 id/name/arguments 分片——sse_parser 的 ToolCallDelta
  从"暂不累积"升级为完整聚合）；请求体消息含 tool_calls/tool_call_id

### 工具面（`interaction/project_tools.rs`）

- **read / ls / grep 三工具**：OpenAI function schema + 执行器。read（1MB 上限、
  逃逸拒绝）、ls（目录 + `/` 标记）、grep（文本扩展名浅层递归 ≤3 深度、
  200 行上限、`rel:line:text` 形态）
- `safe_join`：canonicalize 前缀比较（符号链接归一——macOS tempdir /var→
  /private/var 场景）+ 词法回退 + 逃逸拒绝
- `tools_payload` / `execute_tool` 分发（未知工具 → 错误文本）

### agent 循环（`interaction/agent_loop.rs`）

- `run_agent_loop`：12 轮上限；每轮前 **abort 句柄轮询**（截断即返）；LLM 返回
  tool_calls → `tool:start` → 执行 → `tool:end` → tool 消息回填继续；无工具
  → 最终文本
- `LoopChat` trait（LLM 调用抽象，测试可注入 ScriptedChat）+ `LoopEvents`
  trait（delta/tool_start/tool_end 回调）
- 工具执行卡收集（id/tool/args/status/result/error/时间戳）
- 单测 3 件：工具轮→完成、abort 前置截断（零 LLM 调用）、未知工具错误卡

### 端点接线（`server/agent_route.rs`）

- 直通路径升级为 `run_agent_loop`：RouterLoopChat（resolve + client_for_public +
  tools 参数透传）+ SseBridge（draft:delta / tool:start / tool:end → BroadcastHub）
- 响应附加 `details.toolExecutions` 执行卡
- 65 号 abort 注册表条目与 loop 句柄并存（统一为同句柄随 67 号生产任务）

## parity 要点

- SSE 事件名与负载形态对齐 server.ts L5297-L5377（draft:delta{text}、
  tool:start{id,tool,args,stages}、tool:end{result,isError}）
- transcript 的 assistant 轮文本为最终回复（工具结果经会话详情 derive 可见）
- E2E 固化：mock LLM 首轮发 tool_calls(read) 次轮文本 → 200 响应带执行卡 +
  会话详情两消息

## 偏差备案

- **draft:delta 为每轮聚合文本**（非逐 token 增量）——stream_chat 返回聚合
  结果，逐 delta 回调需流式回调扩展（stream_chat 增加 on_delta 闭包），随 67 号
- **生产工具暂缓**：write/edit/propose_action/sub_agent/play_*/short_fiction_run/
  generate_cover 等（依赖确认式生产任务分支与既有 pipeline 链接线）
- **确认式生产任务分支暂缓（67 号）**：reservedProductionSessions 闸门 +
  executeConfirmedProductionAction + task 快照 + 建书迁移
- **restoreAgentMessages 历史回放暂缓**：loop 起始消息仅 system+instruction
  （历史上下文注入随回放面）
- **工具 schema 未逐字对齐 TS**（read/ls/grep 的 description 文案以中文简述——
  功能面一致，E2E 以行为断言）
- grep 的结果显示为 `相对路径:行号:文本`（root 前缀剥离）

## 下一步（67 号候选）

- 确认式生产任务分支（write_next/create_book 等意图执行器接线既有 42/58 号链）
- restoreAgentMessages 历史回放 + stream_chat 逐 delta 回调
- foundation/revise、interactive-films/projects、translations 域

## 影响面

- lib 970 → 975（project_tools 2 + agent_loop 3）；e2e 88 → 90；
  export-bindings 1129 → 1134；TS 基线不变
- 全量基线：`cargo test --lib`（975）/ `golden_leaf`（76）/
  `e2e_write_next_contract`（90）/ `--features export-bindings --lib`（1134）/
  `clippy --lib --tests --bins`（零警告）/ TS vitest（185 文件 1798 测试）
