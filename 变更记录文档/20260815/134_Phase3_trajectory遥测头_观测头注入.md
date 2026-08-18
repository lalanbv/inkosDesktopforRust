# 134 号（Phase3）：trajectory 遥测头——LLM 请求观测头注入

日期：2026-08-18 · 前置：130/133 号备案项

## 背景

TS `agent-trajectory.ts`（114 行）为 kkaiapi.com 端点的 LLM 请求注入
`X-InkOS-*` 观测头（会话/运行/调用标识 + 重试 attempt + 思考档位），
以 AsyncLocalStorage 传播回合作域。Rust 此前缺失。

## 实现

### A. 核心（TS 逐字，`src/llm/agent_trajectory.rs` 新模块）

- `opaque_conversation_id`：`inkos-` + sha256(sessionId) 十六进制前 32 位
 （会话标识不透明化，不把原始 id 发给外部端点；sha2 复用）。
- `is_kkaiapi_endpoint`：hostname 等于 kkaiapi.com 或 `.kkaiapi.com` 后缀。
- `AgentTrajectoryScope`：TS ALS store 的显式传递对应物——main 回合作用域
  + pi_turn 原子递增（`begin_model_call`：main 先增再取 max(·,1)，非 main
  只读）；subagent 嵌套形态（role/parent_tool_call_id）结构已备、接线备案。
- `agent_trajectory_headers`：仅 kkaiapi 端点 9-11 头（Trace-Version/Scaffold/
  Conversation-ID/Run-ID/Model-Call-ID/Agent-Role/Pi-Turn-Index/Client-Attempt/
  Thinking-Effort + 条件 Budget-Tokens/Parent-Tool-Call-ID）；其它端点零头。

### B. 接入（三面）

- `ChatCompletionParams.trajectory: Option<Arc<AgentTrajectoryScope>>`；
  streaming_client 两传输（chat completions + responses）请求构造处
  `inject_trajectory_headers`（TS 两传输同传 traceHeaders）。
- **聊天面**（RouterLoopChat）：每回合构造 main 作用域
 （conversation=opaque(sessionId)、runId=uuid）——回合内多次 LLM 调用共享、
  pi_turn 递增，对齐 TS runWithAgentTrajectory 回合语义。
- **管线面**（AgentRouter::chat）与探测面：None——TS 管线 agent 在 ALS 作用
  域外（beginAgentModelCall undefined → 零头），等价。
- **差异备案**：Rust 无 withTransientLLMRetry 重试环 → Client-Attempt 恒 1；
  端点配置无 thinkingBudget → Effort 恒 disabled、Budget 头省略。

### C. 顺带去重（133 号遗留）

发现 think 剥离器在 Phase 3 起点已有独立模块（cd1b8b0b
`src/llm/think_tag_stripper.rs`，注册在册但从未接线）——133 号在
streaming_client 内嵌了重复实现。本轮去重：streaming_client 改引用既有模块
（语义一致且更精确：UTF-16 码元计数），删内嵌副本；接线与集成测试保留、
纯函数单测归既有模块（8 例）。

### D. 测试（+5）

- trajectory 单测 4：opaque id 形态/确定性、kkaiapi 判定（含 notkkaiapi.com
  负例）、头集逐字形态（条件头省略/非 kkaiapi 零头/无 trace 零头/budget 条件）、
  pi_turn 递增（main 递增 / 非 main 只读 / parentToolCallId 透传）。
- 真 HTTP mock 集成 1：记录请求头断言——带作用域的非 kkaiapi 端点（127.0.0.1
  mock）**零 X-InkOS 头**（kkaiapi 端点的头集形态由纯函数断言覆盖）。

## 验收基线（终态）

| 项 | 结果 |
|---|---|
| `cargo test --lib` | **1184** 过 |
| `cargo test --test e2e_write_next_contract` | **190** 过 |
| `cargo test --test golden_leaf` | **70** 过 |
| `cargo test --features export-bindings --lib` | **1342** 过 |
| `cargo clippy --all-targets` | 零警告 |
| duel 对跑（INKOS_DUEL=1） | **8/8** |

## 备案（后续轮）

1. runner harness 类重构（agentCtxFor/worker-agent/production harness）与
   skills 生产模式绑定——合并 e7c04465 剩余主体。
2. writeProductionRunSnapshot（生产运行快照持久化）。
3. subagent 嵌套作用域接线（agent-tools 的 runWithAgentTrajectoryRole 对应面）。
