# 79 号：play 对账代理与重做变体面

**日期**：2026-08-15
**阶段**：Phase3 深度收尾（78 号"下一步"首选：sceneReconciler + regenerate 变体/checkpoint 面——73 号 play 域的最后一块暂缓件）
**契约源**：`packages/core/src/play/play-runner.ts`（L54-66 reconciler 端口 + L229-256 step 对账/checkpoint 段 + L293-360 regenerateLastTurn/restoreVariant + L413-439 buildReplayContext + L486-548 merge 辅助族）、`play/play-agents.ts`（L240-360 PlaySceneReconcilerAgent 双语提示词 + L537-572 renderer 重写约束注入）、`play/play-store.ts`（L44-54 PlayRunSnapshot + L255-343 capture/save/load checkpoint/variant/restore）、`play/play-graph` 的 replaceWithSnapshot；`agent/agent-tools.ts` L2088-2523（play_revise 工具——唯一消费方，聊天面）

---

## 一、背景

73 号交付 play 域时备案了三件暂缓：sceneReconciler 对账代理、regenerateLastTurn 变体/检查点面、en 提示词。本轮补齐前两件（reconciler 双语齐备；replayContext 双语齐备；其余三代理 en 提示词维持备案）。TS 侧 regenerate/restore 无 REST 端点——唯一消费方是 play_revise 聊天工具（agent 聊天循环内调用），故本轮为**域本体交付**（trait 注入测试 + 真实 PlayAgents 的 E2E），agent_production 不新增接线（play_revise/play_step 均非确认意图）。

## 二、交付

### 1. 图后端整体替换 `play_graph.rs::replace_with_snapshot`（79 号）

清空四表（File 内存重置 / Sqlite DELETE）后按快照重灌（复用既有 upsert 路径），全程 `with_transaction`（File 内存克隆回滚 / Sqlite BEGIN-COMMIT）。双后端往返测试覆盖（含落盘重开持久性）。

### 2. 快照/检查点/变体存储 `play.rs`（六函数 + PlayRunSnapshot）

- **PlayRunSnapshot**（camelCase 磁盘形态）：id/turn/createdAt + 五路原文（events.jsonl / transcript.jsonl / state/current.json / projections/scene.md / projections/state.md）+ graph 四集合快照。
- **capture_run_snapshot**：读齐五路原文 + 注入图快照；**save/load_checkpoint**：`checkpoints/{id}.json`（段校验防逃逸，损坏/缺失 → None）；**save_variant**：`variants/turn-{N}/v-{uuid}.json`（id 改写为变体 id 并返回）；**load_variant**；**restore_run_snapshot**：`replace_with_snapshot` + 五路原文写回。

### 3. reconciler 代理与 merge 链 `play_runner.rs`

- **PlaySceneReconciler trait** + PlayAgents 实现：agent 键 `play-scene-reconciler`（0.1 / 2048），system/user 提示词**双语逐字**（"对齐正文与图谱/只补缺失事实/holding 边 physical 语义"）；fail-open——LLM/解析失败返回 empty_reconciliation；输出 normalize + eventId/turn/actionKind 补齐。
- **merge 辅助族**：merge_play_mutations（upsert 按 id 去重**后写胜**、expire/transitions/notes 拼接、timeAdvance **base 优先**、summary 合成）、merge_mutation_summary（归一相等/互含取宽侧，否则全角分号拼接）、normalize_summary_for_dedupe（TS 标点字符类逐字 + lowercase）、is_empty_mutation_supplement（全空数组 + 空 summary + 未 blocked 才算空）。
- **build_replay_context**（双语逐字）：重写非新回合 / 原动作保持 / Time 权威不倒退 / 禁新事实；替换输入与原输入相同 → 不出现替换行。
- **renderer 重写约束注入**：PlaySceneRenderer::render 增加 `replay_context: Option<&str>`，user prompt 追加"重写约束："节（TS 逐字）。

### 4. step 集成 + regenerate/restore（PlayRunner 新面）

- **step()**：签名加 reconciler（Option<&dyn>）与 replay_context；render 后**非 blocked 回合对账**——空补充不动原 mutation，非空合成 final_mutation + 重算 state brief；**checkpoint `before-turn-{N}` 先于 apply 落盘**（capture + save，regenerate 的回滚点）；提交段改用 final_mutation（事件/current state lastSummary/state 投影）。另补 language 读取（world.language ?? zh）：en 世界的空场景 brief 默认文案双语化（沿 TS）。
- **regenerate_last_turn(input?)**：末事件缺失 → "No Play turn to regenerate."；重放输入 = 替换 trim || 原输入；当前态存变体（previousVariantId）→ 回滚 `before-turn-{N}` 检查点（缺失 → "Missing checkpoint before turn N; cannot regenerate safely."）→ step 重放（注入 replayContext；restore 后事件数 N-1 → 重算 turn=N）→ 新态存变体（variantId）。事件/回合不增长，两个变体文件成对。
- **restore_variant(turn, variantId)**：变体缺失 → "Play variant not found: turn N / v-..."；restore_run_snapshot 整体恢复 + 返回 scene_projection（trim）。

### 测试

- **lib 单测 7 个（+7）**：play_graph 1（双后端替换往返）；play_runner 6——summary 合成分支（空侧/归一相等/互含/拼接）、merge 去重与拼接 + timeAdvance 优先 + 空补充直返、空补充门（summary/blocked 破空）、replayContext zh/en 形态（同替换不出现）、renderer 约束注入、**全链集成**（seed → step 补充入图 + holding 边 + checkpoint + state 投影 → regenerate 变体对/事件不涨/turn 复位 → restore previous 恢复第一版 + 错误面）。
- **E2E `play79_e2e` 1 个（+1）**：真实 PlayAgents + mock LLM 五路分流（含 reconciler 关键词）——补充实体/边入图断言、重写约束注入原子标志断言、场景甲→乙重放、变体文件对、恢复回场景甲、"No Play turn"/"variant not found" 错误面。
- 73 号 E2E 的 step 两处调用适配新签名（reconciler=None 保持原契约面）。

## 三、parity 要点

1. **对账语义**：reconciler 只补正文里出现但图谱缺失的事实（具名物件/线索/持有边）；空补充不动原 mutation——summary/entity 去重后写胜防止同一实体双写。
2. **checkpoint 先于 apply**：before-turn-{N} 在任何回合提交前落盘——regenerate 的安全回滚点；回合 all-or-nothing 语义不变（render 先行 + checkpoint + 提交）。
3. **重放约束**：重写是"同一动作的另一版"——时间权威（Time 段不倒退/不另写钟点）、禁新增玩家动作、新事实必须已在已应用变化中。
4. **变体对**：regenerate 保存 current-turn-{N}（回滚前现场）与 regenerated-turn-{N}（重放后）两个变体——restore_variant 可在任意版本间切换。
5. **mergeById 后写胜 + 首现顺序**：Map 语义逐字（order 数组首现序 + 后写覆盖）。

## 四、偏差备案

1. **agent_production 不接线**：TS 中 play_revise 是聊天工具（非确认意图，isConfirmedProductionAction 名单外）；Rust 聊天循环的 play 工具面（play_step/play_revise）仍是暂缓件，本轮交付域本体 + trait 面，聊天工具接入后即可直接消费。
2. **interpreter/mutator/renderer 三代理 en 提示词维持 73 号备案**（en 世界回退 zh）；reconciler 与 replayContext 本轮已双语。
3. **renderer user prompt 标签沿用 73 号形态**（"玩家输入/动作理解/已应用变化摘要"），TS 现行版为"玩家原话/动作/已应用的本回合变化"——79 号仅追加"重写约束："节（TS 逐字），整体标签措辞不作为本轮偏差修复项（行为等价）。
4. **context brief 标签 zh-only**（"当前实体名册/当前场景/当前状态"）沿 73 号；TS en 标签随三代理 en 提示词项一并处理。

## 五、暂缓件（沿 78 号清单滚动）

- play_step / play_revise 聊天工具面（消费本轮 trait 面）；play en 提示词（interpreter/mutator/renderer + context brief 标签）
- 单章写作中途截断、/agent model 校验、resumeFrom、fetchWithProxy、attachments 归一化、模型四层解析

## 六、验证基线

| 套件 | 结果 |
| --- | --- |
| `cargo test --lib` | **1074** 通过（+7） |
| `cargo test --test golden_leaf` | 76 通过 |
| `cargo test --test e2e_write_next_contract` | **133** 通过（+1） |
| `cargo test --features export-bindings --lib` | **1233** 通过（+7） |
| `cargo clippy --lib --tests --bins` | 零警告 |
| `packages/core vitest run` | 185 文件 / 1798 测试全绿 |

## 七、影响面与下一步

- **影响面**：play 域具备完整重做能力——对账防正文实体丢失、检查点/变体支持回合重写与版本切换；PlayRunner 面（step/regenerate/restore）全部就绪，聊天工具接入即通。
- **下一步（80 号候选）**：play_step / play_revise 聊天工具面（消费本轮交付）；或 play en 提示词补齐；或散件收尾（单章截断/model 校验/resumeFrom/attachments/模型四层解析）。
