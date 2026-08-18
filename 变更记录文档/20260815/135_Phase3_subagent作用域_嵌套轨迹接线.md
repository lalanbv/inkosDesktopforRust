# 135 号（Phase3）：subagent 嵌套作用域——轨迹链路完整接线

日期：2026-08-18 · 前置：134 号（main 回合作用域；subagent 接线备案）

## 背景

TS `runWithAgentTrajectoryRole("subagent", task)`（agent-tools.ts L947）把
sub_agent 工具执行体包进**继承外层**（同 conversationId/runId/计数器）的
subagent 作用域——仅切 role，未传 parentToolCallId（头省略）。134 号的
显式传递只覆盖聊天面直调；sub_agent 工具链跨装配闭包（runner）无法直传。

## 实现

### A. 作用域 v2（`agent_trajectory.rs`）

- `pi_turn: Arc<AtomicU32>`（原内联 AtomicU32）——`derive_subagent()`
  共享**同一计数器**（TS `...current` 展开的同一 counter 对象语义）。
- **task-local 通道**（`tokio::task_local! TRAJECTORY_SCOPE`，Rust 的 ALS
  等价物——同一 async 链内自动传播）：`current_scope()` 读取（无 set →
  None，TS storage.getStore() undefined 语义）；`with_subagent_scope(task)`
  ——外层无作用域原样执行（`if (!current) return task()` 逐字），有则
  派生 subagent 包住执行体。

### B. 三点接线

1. **回合体**（agent_route run）：main 作用域（opaque(sessionId) + uuid
   runId）经 `TRAJECTORY_SCOPE.scope` 包住 run_agent_loop 与消费段——
   回合内所有 LLM 调用（聊天增量 + 工具链）同源。
2. **RouterLoopChat / AgentRouter::chat**：params.trajectory 改读
   `current_scope()`（删 134 号直传字段——管线直调链无 set → None 零头
   等价；sub_agent 链内读到 subagent 作用域）。
3. **tool_sub_agent**：执行体整体 `with_subagent_scope` 包装（内部 runner →
   router.chat 自动读到 role=subagent；pi_turn 读取不递增、计数与父回合
   共享）。

### C. 测试（+2，净额 +1）

- `subagent_scope_shares_counter_and_never_increments`：派生同
  conversation/run、parent 缺省（TS 未传逐字）、subagent 读当前值不递增、
  每调用新 uuid、回 main 后递增跨 subagent 继续。
- `task_local_channel_propagates_and_subagent_wraps`：set 内读 main →
  包装内读 subagent（同 conversation）→ 包装外回 main → 无外层原样执行。
 （134 号的 role 透传断言旧测试语义被新测覆盖，删除冗余。）

## 验收基线（终态）

| 项 | 结果 |
|---|---|
| `cargo test --lib` | **1185** 过 |
| `cargo test --test e2e_write_next_contract` | **190** 过 |
| `cargo test --test golden_leaf` | **70** 过 |
| `cargo test --features export-bindings --lib` | **1343** 过 |
| `cargo clippy --all-targets` | 零警告 |
| duel 对跑（INKOS_DUEL=1） | **8/8** |

## 备案（后续轮）

1. runner harness 类重构（agentCtxFor/worker-agent/production harness）与
   skills 生产模式绑定——合并 e7c04465 剩余主体。
2. writeProductionRunSnapshot（生产运行快照持久化）。
