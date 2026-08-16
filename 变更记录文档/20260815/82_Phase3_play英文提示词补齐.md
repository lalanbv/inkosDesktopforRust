# 82 号变更记录：play 英文提示词补齐（三代理 + context 标签 + 开场播种）

- 日期：2026-08-14
- 阶段：Phase3 strangler 迁移（engine-rs）
- 契约源：`packages/core/src/play/play-agents.ts` L358-484（interpreter/mutator 的 system+user prompt en 分支）、L486-571（renderer system+user en 分支）、L222-234（renderer fail-open 兜底与严格 JSON 追问）、`packages/core/src/play/play-runner.ts` L140-190（seedOpening 语言流转 + intent 双语）、L196-238（step 语言流转 + 空场景 brief 双语）、L362-375（buildContextBrief 标签）、L441-457（renderPlayWorldContext）、L550-570（renderEntityRoster）、L388-423（buildOpeningSeedInput / buildReplayContext）

## 一、背景

73 号移植 play 回合执行流时，interpreter/mutator/renderer 三代理提示词与 context brief 标签只落了 zh 分支（en 世界此前也无法走通端到端），73 号偏差备案遗留；79 号已补 reconciler 与 replayContext 双语。本轮补齐最后一批：en 世界的完整回合链（seed → step → reconcile → regenerate 重放）全部走 en 系统提示词与 en 标签，en 分支文案对 TS 逐字。

## 二、交付

1. **trait 签名扩展**（`engine-rs/src/play_runner.rs`）：`PlayActionInterpreter::interpret` / `PlayWorldMutator::propose_mutation` / `PlaySceneRenderer::render` 各加 `language: &str` 参数（对齐 TS input 对象携带 language 的流转）；PlayAgents 实现与 step/seed_opening 调用点、测试替身同步适配
2. **三代理提示词 en 分支（TS 逐字）**：
   - interpreter system 5 行 + user 标签（Current scene / Player input / Output fields）
   - mutator system 24 行（含 en JSON 范例行——sample clue/sample key/actor_player 保留字说明）+ user 标签（Player's words / Action interpretation / Current context / "Requirement: use eventId evt-N; …"）
   - renderer system 12 行 base（含"Respect negative player intent"、"不催不逼"、生死例句 zombie/axe）+ guided/open actionsRule + "Output strict JSON: sceneText, suggestedActions."；user 标签（World setting (always obey) / Player's words / Action / Applied changes this turn / Current state summary / Replay constraints）
3. **renderer 双语兜底**：严格 JSON 追问 en（"That was not strict JSON. Output ONLY one JSON object …"）+ 空输出兜底 en（"(The moment holds, unresolved.)"）
4. **context 辅助双语**：`render_world_context`（World setting / World contract (high priority; obey before genre defaults) / Visual contract…）、`render_entity_roster`（en 头部 + "status: " 标签 + "; " 分隔）、`build_context_brief`（Current scene / Current state / "No persisted state yet." 兜底）、`build_opening_seed_input`（en 三行指令 + Premise / Opening scene / Suggested player actions 标签）
5. **seed_opening 语言贯通**：world.language 推导 + 播种 intent 双语（"Seed the opening state for the first playable scene."）+ 三辅助调用带语言
6. **测试**：
   - 单测 +2：`en_agent_prompts_and_labels`（三代理 system/user en 逐项断言 + zh 分支回归）、`en_context_labels_and_labels_and_seed_input`（世界上下文/名册/播种 en 标签 + zh 回归）
   - E2E +2（`play82_e2e`）：`en_world_full_chain_uses_english_agent_prompts`（en 世界 seed+step+reconcile 全链——mock 按 en system 关键词分流并捕获 system/user 文本，逐项断言 en 提示词与标签；落盘 events/scene/graph 含 reconcile 补充）、`renderer_en_empty_output_falls_back_to_english_placeholder`（renderer 三轮空输出 → en 兜底文案，回合仍提交）

## 三、parity 要点

- **en 分支逐字**：三代理 system/user、兜底、追问、context 标签、播种指令全部对齐 TS en 文案（含标点与转义）；mutator en 的 JSON 范例行原样保留
- **zh 分支维持 73 号形态**：本轮零改动（renderer user 的 zh 紧凑形态/标签措辞为 73 号已备案偏差，不在本轮范围）
- **语言流转对齐 TS**：语言唯一来源是 world.language（缺省 zh），由 runner 推导后随 trait 参数下发；PlayAgents 无自持语言状态
- **mutator fail-open 文案 zh-only 是 TS 原样**：TS 的 blockedReason 无 en 分支，Rust 保持一致

## 四、偏差备案

1. **zh 提示词仍为 73 号自由组织形态**（非 TS zh 逐字）：TS zh 的范例 JSON 行、否定动作行等未补——en 已逐字，zh 若需逐字对齐属独立轮次
2. **appendPromptPackGuidance 未移植**：TS 在 mutator/renderer system prompt 后追加项目级提示词包（promptPack），Rust 无该机制——提示词包体系整体未入迁移范围
3. **logDroppedMutationItems 未移植**：TS mutator 解析丢弃实体/边/槽时 console.warn 可观测性日志，Rust 静默（73 号已有备案，维持）

## 五、暂缓件

- material / retrieve_material / propose_action 聊天工具面（play 会话无世界时的工具集）
- 聊天面空文本 + 工具成功 → `response: ""` 的 TS 分支形态（66 号范围）
- zh 提示词逐字对齐 TS zh（独立轮次，如需）
- 散件：单章写作中途截断、/agent model 校验、resumeFrom、fetchWithProxy、attachments 归一化、模型四层解析

## 六、验证基线（2026-08-14）

- `cargo test --lib`：**1086**（+2：en 提示词/标签单测）
- `cargo test --test golden_leaf`：76
- `cargo test --test e2e_write_next_contract`：**138**（+2：play82_e2e）
- `cargo test --features export-bindings --lib`：**1245**（+2）
- `cargo clippy --lib --tests --bins`：零警告
- TS：`packages/core` vitest 185 文件 / **1798** 测试全过

## 七、影响面与下一步

- 影响面：`play_runner.rs` 三 trait 签名扩展（外部实现方仅测试替身与 PlayAgents，已同步）；en 世界自此端到端可用（73 号备案遗留关闭）
- 下一步（83 号候选）：**首选 material / retrieve_material 聊天工具面**（play 无世界工具集 + 书会话材料工具，agent-session 注册面的最大缺口）；其次散件收尾（单章截断/model 校验/resumeFrom/attachments/模型四层解析）或 zh 提示词逐字对齐
