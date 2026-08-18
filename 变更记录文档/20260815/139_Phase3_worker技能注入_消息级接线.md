# 139 号（Phase3）：worker 技能指导消息级注入——生产链接线

日期：2026-08-18 · 前置：138 号（production_bindings 积木）/ 135 号（task-local 先例）

## 勘测（TS 注入链）

TS：sub_agent 工具 `merge(workerSkills(agent), activeSkills())` →
`pipeline.runWithAgentContext({activatedSkills})`（runner 的 ALS
operationContext）→ `agentCtxFor` 装配 AgentContext → **BaseAgent.chat 出口**
`appendTaskSkillGuidance`（hydrate 引用检索 + append）注入 system 消息。

## 实现（Rust 映射）

- **OPERATION_SKILLS task-local**（production_bindings.rs，135 号
  TRAJECTORY_SCOPE 同款模式）+ `current_operation_skills()` 读取。
- **AgentRouter::chat 出口注入**：Rust 各 pipeline agent 统一经 router.chat
  出口——即 Rust 的 "BaseAgent.chat" 等价物；非空激活集时
  `append_activated_skill_guidance` 拼进 system。hydrate 引用检索属
  local-search 面（未移植）——activations 的 resources 恒空，无需 hydrate。
- **tool_sub_agent 注入点**：可用技能装载（130 号 load_available——builtin
  + configured）→ `worker_skills_for_agent(available, agent)`（138 号绑定）
  → OPERATION_SKILLS.scope 包住执行体（会话激活集为空——Rust 无 use-skill
  工具，merge 结果即 worker 绑定，等价）。skill_routes 的 env/home 装载
  出口 pub 化复用。
- **聊天面零注入**：RouterLoopChat 直连不经 router.chat；会话激活为空
 （TS 聊天面经 use-skill 工具激活，Rust 未移植该工具——等价空集）。

## 测试（+1 e2e）

sub139：OPERATION_SKILLS 作用域包 write-next 全链，捕获 mock 收到的全部
system——断言 writer 系消息含 "## Activated professional skills" 指导块 +
`### inkos-long-writing` 条目 + 技能正文；且**不含** story-review（writer
只绑 longWriting——绑定隔离正确性）。既有全量测试不设作用域 → 零注入
零影响。

## 验收基线（终态）

| 项 | 结果 |
|---|---|
| `cargo test --lib` | **1195** 过 |
| `cargo test --test e2e_write_next_contract` | **192** 过（+1） |
| `cargo test --test golden_leaf` | **70** 过 |
| `cargo test --features export-bindings --lib` | **1353** 过 |
| `cargo clippy --all-targets` | 零警告 |
| duel 对跑（INKOS_DUEL=1） | **8/8** |

## 备案（后续轮）

1. runner harness 类形态结构对齐（agentCtxFor/worker-agent 类化——Rust
   已等价承载其行为面：task-local 作用域 + router 出口注入；类形态对齐
   为纯结构重构，收益待评估）。
2. script-storyboard / translation runner 移植（接入 137 号同事务面 +
   138 号绑定积木）。
