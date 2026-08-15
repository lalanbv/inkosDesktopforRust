# 57 号变更记录：Phase3 agents 域——architect 基础设定生成核心（大件主链）

- 日期：2026-08-15
- 范围：engine-rs（新增 `agents/architect.rs`、`agents/foundation_prompts.rs`；扩展 `agents/mod.rs`、`llm/agent_router.rs`）
- 契约源：`packages/core/src/agents/architect.ts`（1433 行）主链 + `packages/core/src/utils/hook-promotion.ts`（种子晋升三规则 + isCrossVolume + extractVolumeIndexFromArc）
- 验证：`cargo test --lib`（935，+10）+ `cargo test --test golden_leaf`（76）+ `cargo test --test e2e_write_next_contract`（48）+ `cargo test --features export-bindings --lib`（1094）+ `cargo clippy --lib --tests --bins`（零警告）+ TS vitest（185 文件/1798 测试）

## 一、背景

architect 域是迁移主线剩余最大块（books/create、import/chapters、fanfic/init、spinoff/init 四端点的共同依赖）。本轮交付其**核心层**（生成/解析/规范化/落盘/修复环），端点装配（books/create 等）复用本轮件于 58 号交付。

## 二、交付内容

### 1. `agents/architect.rs`（新建，~1250 行）

**生成主链**：
- `generate_foundation(ctx, chat, book, external_context, review_feedback)`：temp 0.8；`generate_foundation_from_import(...)`（temp 0.5，continuation/series 双模式续写指令）——中英系统/用户提示逐字、`language ?? genre.language` 解析、`【LANGUAGE OVERRIDE】` 前缀
- chat 端口 `ArchitectChat`（已入 AgentRouter 的 `impl_simple_chat!` 装配面）

**解析链**：
- `parse_architect_section_map`：`=== SECTION: name ===` 标记正则（可带 # 前后缀）优先，`#` 标题（h1-h3）回退（中文标题词表映射到规范段名）
- `parse_sections`：Phase 5 新段名优先 + legacy 段名（story_bible/volume_outline）回退；**5 段必需契约**（story_frame/volume_map/roles/book_rules/pending_hooks；仅 legacy 段名命中时 roles 可缺省——v12 兼容）；legacy 面由 shim 合成
- `parse_roles`：`---ROLE---/---CONTENT---` 块解析（tier major/minor/主要/次要 + name；坏块静默丢弃）
- `strip_trailing_assistant_coda`（"如果你愿意…" / "If you'd like…" 尾巴剥离）
- `parse_sections_with_repair`：缺段 → LLM 修复环（temp 0.2，双语提示逐字，"不要重新发明一本书"）→ 仍缺 → `ArchitectIncompleteFoundationError`（双语兜底文案逐字："点重试,或换更强的模型(如 deepseek-v4-pro / gpt-5.5)…"）

**pending_hooks Phase 7 规范化**（`normalize_pending_hooks_section`）：
- 13 列表格解析（phase7/phase6/legacy 三形态的 noteCellIndex、seedNote「初始线索：」合并、hook-N 缺省 id）
- **种子预晋升三规则**：core_hook=true / depends_on 非空 / cross_volume（Case A 上游后卷声明、Case B 回收卷提及他卷【第N卷中文数字解析】、Case C endgame/slow-burn + 早卷种子）
- 未晋升且未推进的普通种子 → 「暂缓/deferred」休眠态
- 卷边界解析（`第N卷 (A-B章)` / `Volume N (chapters A-B)` 双语正则）
- 产出经 `render_hook_snapshot` 渲染（复用既有件）

**落盘**（`write_foundation_files`，Phase 5 contract）：
- Phase 5 产物：`outline/story_frame.md` + `outline/volume_map.md`（+可选 节奏原则.md）+ `roles/{主要,次要}角色/<name>.md` 一人一卡（文件名非法字符清洗）+ story_bible/character_matrix 兼容 shim + book_rules（trim+\n）+ current_state 种子占位（双语，指向 roles/当前现状 与 startChapter=0 行）+ pending_hooks + emotional_arcs 表头种子
- Legacy 产物面（story_bible/volume_outline/character_matrix 直写）
- **revise 模式**：要求 Phase 5 产物（否则明确报错"文件未被修改"）+ 清空重建 roles 目录

### 2. `agents/foundation_prompts.rs`（新建，~460 行）

中文/英文基础设定提示词**逐字**移植（architect.ts L202-L604）：去重铁律、预算表、story_frame 4 段（主题/双层冲突/世界观底色/终局 Objective）、volume_map 5 段+节奏尾段（OKR 分解/卷间钩子双层）、roles 一人一卡模板（9 小节）、book_rules 规则卡（数值/年代条件块）、pending_hooks 13 列规则、硬性完结检查。数值/年代内嵌条件块按 genre profile 开关。

### 3. 单测 10 例

五段全解析（含 hooks 晋升/休眠断言）/缺段报告/标题回退（中文标题→新段名）/legacy 段名 roles 缺省/坏角色块丢弃/种子晋升矩阵（core 晋升+普通休眠+跨卷晋升）/卷边界双语解析/段名折叠/coda 剥离/Phase 5 落盘全文件面 + revise legacy 拒绝。

## 三、parity 要点

1. **heading 回退产出新段名**：中文标题"故事框架"映射 story_frame（非 legacy 名）→ roles 仍必需——TS 同款（此输入走修复环）
2. `shim` 摘录按 UTF-16 码元截 2000（TS slice）
3. pending_hooks 表无数据行时**原样返回**（不规范化非表格输入）
4. revise 模式的 legacy 产物是显式失败（保护既有文件不被半改）
5. 晋升判定建书期 advanced_count 恒 0（运行期由 consolidator 补）

## 四、偏差备案

1. `normalizeSectionName` 的 NFKC 归一化简化为 ASCII 语义处理（段名为英文 snake_case；全角字母边角差异）
2. LLM 调用无 abort signal（全端点一致暂缓）
3. `parseVolumeNumber` 中文数字仅支持一至十（TS 同款单字/十映射）

## 五、暂缓件（58 号）

- `generateFanficFoundation`（fanfic 维度注入变体）
- `generateAndReviewFoundation` 审核环 + FoundationReviewerAgent（210 行）
- `buildRevisePrompt`（修订模式提示——依赖 reviseFrom 四文件装配，随 architect-revise 端点）
- **books/create 端点**（bookCreateStatus 内存状态机 + SSE book:creating/created/error + buildStudioBookConfig + completeBookExists——复用本轮 generate_foundation 即可装配）
- **import/chapters 端点**（splitChapters + 复用本轮 generate_foundation_from_import + ChapterAnalyzer 逐章回放——`resetImportReplayTruthFiles` 待勘测）
- fanfic/init、spinoff/init 端点

## 六、下一步（58 号候选）

1. books/create 端点（architect 已就绪，装配 + 状态机 + SSE）
2. import/chapters 端点（fromImport 已就绪 + chapter_analyzer 逐章回放）
3. 审核环（FoundationReviewer + generateAndReviewFoundation）
4. fanfic/spinoff 变体

## 七、影响面

- Rust 业务端点累计：67（56 号后，本轮无新端点——纯核心层）
- 新增测试：lib +10（architect）；端点数不变
- `ArchitectChat` 已入 AgentRouter 装配面（`impl_simple_chat!`），58 号端点直接 `RoutedAgent{agent:"architect"}` 装配
