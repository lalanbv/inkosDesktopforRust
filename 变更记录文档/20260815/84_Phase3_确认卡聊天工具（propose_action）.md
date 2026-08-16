# 84 号变更记录：确认卡聊天工具（propose_action）

- 日期：2026-08-14
- 阶段：Phase3 strangler 迁移（engine-rs）
- 契约源：`packages/core/src/agent/agent-tools.ts` L126-551（ProposeActionParams 15 action 枚举 + 9 结构化子域、createProposeActionTool、compactObject、compactPlayStartPayload、proposedActionPayload、assertExecutableProposedAction、proposedActionSessionKind/TargetRoute/FallbackTitle/FallbackSummary、normalizeProposedSkillIds、normalizeSuggestedActions）、`packages/core/src/agent/agent-session.ts` L827-911（注册矩阵与 sameSession）、`packages/core/src/interaction/action-envelope.ts` L232-242（isUsablePlayInitialScene + INCOMPLETE_PLAY_SCENE_SUFFIX）

## 一、背景

agent-session 注册面的最后一大块：聊天里把生产意图升格为**确认卡**（用户未点击确认前不执行）。propose_action 是 chat/short/script/storyboard/film/book-create 与 play-无世界会话的入口工具，确认后的执行面（67-78 号确认意图执行器）早已在位——本轮补上生成确认卡的前半段：instruction 自包含 + 结构化 actionPayload 填充 + 必填断言 + targetSessionKind/targetRoute 路由信息。

## 二、交付

1. **`engine-rs/src/interaction/propose_action_tool.rs`**（新建 ~700 行，7 单测）：
   - `proposed_action_session_kind`：15 action → 目标会话 kind 映射（create_book→book-create、play_start→play、draft_structure/connect_choice/remove_node→interactive-film-authoring、四辅助入口→chat、short_run→short …）
   - `proposed_action_target_route`：fanfic_init/continuation_import/spinoff_create/style_imitation → import:* 路由
   - `proposed_action_fallback_title/summary`：15 种 action 双语回退（辅助入口的 summary 是"确认后只会打开现有 Studio 工具，不会直接生成成品。"）
   - `compact_object`：子域清洗（字符串 trim 非空、数组滤空 trim、正数保留、其余非空保留；空 → None）
   - `compact_play_start_payload`：playStart 专用——initialScene 过 `is_usable_play_initial_scene`（非空 + ≥12 码元 + 句末不悬垂：单字连词/标点集合 + 因为/如果/——多字后缀）；suggestedActions 字符串/对象（action/label/text/title/description）归一 ≤4
   - `proposed_action_payload`：按 action 拼对应子域；**shortRun 注入会话语言**（模型未填 language 时兜底）
   - strict 校验**复用 75 号 `validate_action_payload_strict`**（zod ActionPayloadSchema 等价，含 shortRun 语言×字数 superRefine）→ 失败 "Invalid proposed action payload: …"
   - `assert_executable_proposed_action`：7 种 action 必填断言（create_book.title、play_start 三件、generate_cover/script/storyboard/film.title、translation_create 三件）→ "propose_action is missing X; retry with that field in the structured payload, not only in summary or instruction."
   - 执行器：文本四行（title\nsummary\n\nInstruction: …）+ details {kind:"proposed_action", action, targetSessionKind, targetRoute?, sameSession, title, summary, instruction, requestedSkills?, actionPayload?}（可选项缺省不出现，对齐 JSON.stringify）
   - `propose_action_schema`：ProposeActionParams 逐字（15 枚举 + 9 子域全字段描述）
2. **`engine-rs/src/server/agent_route.rs`**：
   - `ChatToolRouter` 组合执行器（propose_action → play 工具 → 项目文件工具）取代 80 号的 PlayChatToolExecutor（已从 play_tools.rs 移除）
   - 注册条件对齐 TS 矩阵：**play 会话且有世界时不注册**（此时只挂 play 三件 + material）；其余全部会话注册；`same_session = sessionKind !== "chat"`；requestedSkills 复用 67 号请求归一面
   - `validate_action_payload_strict` 提为 pub(crate)（propose 工具复用）
3. **E2E `propose84_e2e`**（2 测试）：
   - `propose_action_confirmation_card_from_chat`：chat 会话 → mock 调 propose_action（create_book + 完整 createBook 子域）→ 卡 completed + 四行文本逐字断言；注册面 propose_action 与 material 同现
   - `propose_action_missing_title_error_and_play_world_exclusion`：缺 createBook.title → error 卡（固定文案逐字）；play 会话有世界 → tools 不含 propose_action（含 play_edit）+ 模型越权调用 → "Unknown tool: propose_action" error 卡

## 三、parity 要点

- **确认卡不执行**：propose_action 只生成卡（details 驱动前端确认面），确认后由确认意图执行器（67-78 号）真正执行——工具描述逐字"Use this before creating books…when the user has not clicked a confirmation"
- **instruction 自包含**：schema 描述逐字强调确认后跨会话执行不能依赖聊天上下文
- **shortRun 语言注入**：`payload.shortRun = { language, ...shortRun }` 语义——模型显式填的 language 胜出（entry or_insert）
- **注册矩阵**：TS 中 play 有世界的会话不挂 proposalTool（玩主循环不需要确认卡），其余会话全挂
- **strict 校验单一来源**：propose 卡与确认执行共用 75 号校验（ActionPayloadSchema 逐字），卡上被拒的载荷确认面也必被拒

## 四、偏差备案

1. **参数层枚举校验文案**：TS 由 zod 产生；Rust error_result 自拟（"Invalid propose_action.action: …（expected one of …）"/"requires an instruction"），语义等价
2. **details 在聊天面不外露**：Rust 聊天卡（66 号形态）只含 result 文本；确认卡的 actionPayload/targetSessionKind 等经 details 供前端消费的面随聊天面 details 外露轮次（66 号范围）补
3. **normalizeSuggestedActions 对象字段优先级**：action → label → text → title → description（TS 逐字）；非字符串/非对象项按空串处理跳过
4. **`sameSession` 恒由会话类型推导**：TS options.sameSession 由 agent-session 传入（sessionKind !== "chat"）；Rust 同口径，无外部覆盖口

## 五、暂缓件

- research_web / import_chapters 聊天工具（agent-session 注册面余下两件）
- 聊天面卡 details 外露（66 号范围）
- PDF 文本抽取（83 号备案延续）
- 散件：单章写作中途截断、/agent model 校验、resumeFrom、fetchWithProxy、attachments 归一化、模型四层解析

## 六、验证基线（2026-08-14）

- `cargo test --lib`：**1101**（+7：propose_action_tool 单测）
- `cargo test --test golden_leaf`：76
- `cargo test --test e2e_write_next_contract`：**142**（+2：propose84_e2e）
- `cargo test --features export-bindings --lib`：**1260**（+7）
- `cargo clippy --lib --tests --bins`：零警告
- TS：`packages/core` vitest 185 文件 / **1798** 测试全过

## 七、影响面与下一步

- 影响面：`agent_route.rs` 执行器改为 ChatToolRouter（propose→play→文件三级分发）；`play_tools.rs` 移除 PlayChatToolExecutor（无外部消费方）；`validate_action_payload_strict` 可见性 pub(crate)；聊天工具面达 6 件（read/ls/grep + material 双件 + propose_action），play 有世界时 8 件（play 三件替换 propose_action）
- 下一步（85 号候选）：**首选 research_web / import_chapters 聊天工具**（agent-session 注册面收尾）；其次聊天面卡 details 外露（66 号范围，propose 卡消费面）；或散件收尾（单章截断/model 校验/resumeFrom/attachments/模型四层解析）
