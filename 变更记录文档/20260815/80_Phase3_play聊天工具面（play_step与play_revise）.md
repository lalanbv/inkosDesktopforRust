# 80 号变更记录：play 聊天工具面（play_step / play_revise）

- 日期：2026-08-14
- 阶段：Phase3 strangler 迁移（engine-rs）
- 契约源：`packages/core/src/agent/agent-tools.ts` L2297-2523（`createPlayStepTool` / `createPlayReviseTool` / `PlayStepParams` / `PlayReviseParams` / `safePlayId`）、`packages/core/src/agent/agent-session.ts` L898-912（注册条件）、`packages/core/src/agent/agent-system-prompt.ts` L407-455（`buildPlayPrompt` playWorldExists 分支）、`packages/studio/src/api/server.ts` L5380-5505（聊天面返回契约）

## 一、背景

79 号交付了 play 域的 reconcile/regenerate/restore 本体（PlayRunner trait 面），但其唯一消费方是 play 会话的聊天工具 play_step / play_revise——Rust 侧此前只有工具名注册（agent_production 清单）无执行器。本轮把两个聊天工具接入 Rust chat 循环：世界与会话绑定（worldId === sessionId）、step 失败优雅降级、revise 三分支（重做 / 改输入 / 恢复变体）。至此 play 会话的"玩"主循环（推进 + 重做 + 恢复）在 Rust 端闭环，仅剩 play_edit（世界契约编辑工具）未接。

## 二、交付

1. **`engine-rs/src/interaction/play_tools.rs`**（新建 ~490 行，5 单测）
   - `safe_play_id`：TS `safePlayId` 逐字（trim || fallback → 80 UTF-16 码元截断 → 拒 `.`/`..`/`/`/`\`/NUL）
   - `session_world_exists`：注册条件探测（sessionKind=play 且会话绑定世界可载入）
   - `tool_play_step`：空输入 → `"Play input is empty."`；无世界 → 双语优雅文案（表面语言）；step 失败 → 双语固定降级文案 + details `play_step_failed`（不把原始错误交给外层 agent 即兴发挥）；成功 → sceneText 为文本 + details `play_turn_advanced`（title/sceneText/suggestedActions/action/mutation/currentState/graph）
   - `tool_play_revise`：三分支逐字——`restore_variant`（缺 turn/variantId → "恢复版本需要 turn 和 variantId。"；成功 → details `play_variant_restored`；**错误透传不吞**，TS 该分支无 catch）；`edit_last_input`（缺 replacement → "编辑上一条玩家动作需要提供新的 input。"）；`regenerate_last`（失败 → 双语降级 + details `play_revise_failed`；成功 → details `play_turn_revised` 含 replayedInput/previousVariantId/variantId）
   - `play_tool_schemas`：两工具 OpenAI function schema（描述/参数逐字）
   - `play_chat_system_prompt(is_en)`：`buildPlayPrompt` playWorldExists 分支逐字（含"【铁律】/[HARD RULE]"与 commonOutputRules）
   - `PlayChatToolExecutor`：play 工具优先、回落项目文件工具的回环执行器
2. **`engine-rs/src/interaction/agent_loop.rs`**：新增 `LoopToolExecutor` trait，`run_agent_loop` 的 `root: &Path` 参数泛化为 `&dyn LoopToolExecutor`（80 号起 chat 工具面需要会话/路由上下文）；is_error 判定增加显式 `result.is_error` 短路（原有文本启发式保留兼容）
3. **`engine-rs/src/interaction/project_tools.rs`**：`ToolResult` 增加 `is_error: bool`（error_result 置 true）；新增 `ProjectToolExecutor`（文件工具的 trait 适配）；error_result 改 `impl Into<String>`
4. **`engine-rs/src/server/agent_route.rs`** 聊天分支装配：表面语言（项目语言）→ play 世界探测 → play 系统提示词替换通用提示词 → tools payload 追加 play_step/play_revise schema → `PlayChatToolExecutor` 注入 loop
5. **E2E `play80_e2e`**（2 测试）：
   - `play_step_chat_tool_advances_session_world`：play 会话聊天指令 → mock studio-agent 首轮发 play_step 工具调用 → 执行卡 completed + result=场景甲；注册面断言（tools 含 play_step/play_revise/read、system 含【铁律】）；落盘断言（events evt-1 / state turn=1 / scene.md / play-graph.json location_hall）
   - `play_revise_chat_tool_regenerates_and_restore_error_surfaces`：直调 runner 先走一回合 → 聊天"重来上一回合" → play_revise regenerate → 场景乙 + 变体对 + 事件不涨；再聊天恢复不存在变体 → error 执行卡（"Play variant not found: turn 1 / v-none"）

## 三、parity 要点

- **注册条件**：`sessionKind === "play" && playWorldExists`（TS agent-session L898-912）——无世界时 play 工具不进 tools payload（模型看不见），聊天回退通用助手行为
- **世界绑定**：worldId = safePlayId(sessionId, sessionId)、runId 恒 "main"（世界与会话一对一）
- **失败语义不对称是故意的**：step/regenerate 失败 → 固定优雅文案（外层模型拿到可恢复的提示，不会编造"服务不可用请重载存档"）；restore 失败 → 原始错误透传（TS 只有 regenerate 包在 try/catch 里）
- **details 形态**：currentState 用 `load_current_state` 的 Option（缺失 → null，对齐 TS `.catch(() => null)`）；title 缺失 → null
- **isZh 判定**：`(world.language ?? "zh") !== "en"`——工具内部文案随**世界**语言；无世界文案随**表面**语言（项目语言）

## 四、偏差备案

1. **非法 action 的拒绝层**：TS 由 zod 枚举在参数校验层拒绝（工具不执行）；Rust 在执行器内 `error_result("Invalid play_revise action: ...")`——语义等价，错误文案非逐字
2. **onUpdate 进度回调**：TS 工具执行中 `onUpdate?.(textResult("Advancing ..."))` 推中间进度；Rust 由 loop 的 tool:start/tool:end SSE 事件覆盖，无逐条进度
3. **聊天面响应形态**：Rust 聊天面恒带 `details.toolExecutions` 且卡内只含 result 文本（66 号已定偏差，本轮沿用）；TS 文本成功路径不带 details、空文本 + 工具成功路径带
4. **restore 错误的执行卡状态**：TS 工具 throw → pi-agent 标 error；Rust 靠新增 `ToolResult.is_error` 显式标记达成同等状态（文本启发式无法覆盖该文案）
5. **play_edit 未接**：系统提示词逐字保留 play_edit 段落（TS 契约源），但工具本体未注册——用户要求改世界规则时模型可能尝试调用并得到 Unknown tool；下一轮接入后自然收敛

## 五、暂缓件

- play_edit 工具本体（世界契约/视觉契约/persona/图谱实体持久编辑，`createPlayEditTool` ~200 行）
- material / material_retrieval / propose_action 等聊天工具（play 会话无世界时的工具集）
- 聊天面空文本 + 工具成功 → `response: ""` 的 TS 分支形态（66 号范围，非本轮）
- 单章写作中途截断、/agent model 校验、resumeFrom、fetchWithProxy、attachments 归一化、模型四层解析

## 六、验证基线（2026-08-14）

- `cargo test --lib`：**1079**（+5：play_tools 5 单测）
- `cargo test --test golden_leaf`：76
- `cargo test --test e2e_write_next_contract`：**135**（+2：play80_e2e）
- `cargo test --features export-bindings --lib`：**1238**（+5）
- `cargo clippy --lib --tests --bins`：零警告
- TS：`packages/core` vitest 185 文件 / **1798** 测试全过

## 七、影响面与下一步

- 影响面：`agent_loop.rs` 的 `run_agent_loop` 签名变更（root → trait 执行器）——调用方 agent_route 与 agent_loop 测试已同步；`ToolResult` 增字段（仅 project_tools/agent_loop 消费）；chat 工具执行链首次具备会话/路由上下文注入能力
- 下一步（81 号候选）：**首选 play_edit 聊天工具**（世界契约/视觉契约/premise 替换+追加+实体编辑，play 聊天面最后一块）；其次 play en 提示词补齐（interpreter/mutator/renderer/context brief 标签）；或散件收尾（单章截断/model 校验/resumeFrom/attachments/模型四层解析）
