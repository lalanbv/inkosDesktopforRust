# 29 — Phase 3 agents 域：writer 系统提示词三件套（writer-prompts 全量）

> 日期：2026-08-14
> 范围：engine-rs（Rust 化迁移，Strangler-Fig 阶段）
> 关联：`28_Phase3_agents域_outline-paths与ContinuityAuditor主体.md`
> 会话背景：延续 ContinuityAuditor 收官后的 writer 链推进。本次移植 writer
> agent 的 system prompt 全套构造——writer.ts 编排的 prompt 依赖层就绪。

## 移植内容

### `agents/fanfic_prompt_sections.rs`（新文件，110 行 TS 全量）
移植 `fanfic-prompt-sections.ts`——同人三段构造：
- [`build_fanfic_canon_section`]：模式前言（canon/au/ooc/cp）+ 正典文本
- [`build_character_voice_profiles`]：从 fanfic_canon.md 的 `## 角色档案` 表格
  提取 [姓名/口头禅/说话风格/典型行为]，「（素材未提及）」跳过
- [`build_fanfic_mode_instructions`]：模式自检清单 + 允许偏离块

### `agents/en_prompt_sections.rs`（新文件，143 行 TS 全量）
移植 `en-prompt-sections.ts`——英文段构造（5 函数）：
- [`build_english_core_rules`]：通用写作规则（含 TS 原文编号重复 bug 逐字保留）
- [`build_english_anti_ai_rules`]：去 AI 味七铁律 + 对照表
- [`build_english_character_method`]：人物心理五步法
- [`build_english_pre_write_checklist`]：动笔前自检（powerScaling/
  numericalSystem 条件项）
- [`build_english_genre_intro`]：题材介绍（chapterWordCount/targetChapters 插值）

### `agents/writer_prompts.rs`（新文件，1057 行 TS 全量）
移植 `writer-prompts.ts`——writer system prompt 总装：
- [`build_writer_system_prompt`]：按 zh（21 段，含黄金三章 + 全员追踪）/
  en（19 段）序列拼装，空段过滤后 `\n\n` 连接
- 20+ 私有段构造器：核心规则 / 写作铁律卡 / 创作宪法 / 代入感六支柱 /
  黄金三章纪律 / 黄金开篇规则（zh 3 章 / en 5 章）/ 题材规范 / 主角铁律 /
  叙事人称 / 输入治理契约 / 章节备忘对齐 / 文笔执行 / 全员追踪 /
  zh+en 各两套输出格式（creative / full，numericalSystem 条件 ledger 段）
- [`build_golden_opening_discipline`]（pub，chapter ≤ 3 时追加）
- 配套类型：`FanficContext` / `WriterPromptMode` / `InputProfile` /
  `WriterSystemPromptInput`（TS 14 位置参数 → struct）

### 基础设施
- `models/book.rs`：[`Platform::as_str`] 辅助（`${book.platform}` 字符串插值）

## 关键技术点

- **不移植 TS 死代码**：`buildAntiAIExamples` / `buildCharacterPsychologyMethod` 等
  7 个函数在 TS 中定义但 `buildWriterSystemPrompt` 未调用（v10 精简为 Writing
  Craft Card 后的遗留），外部亦无引用——Rust 侧不引入死代码（clippy 零警告纪律）。
  若 TS 侧恢复调用再移植。`buildEnglishCoreRules/AntiAIRules/CharacterMethod`
  同为 en 段未引用，但作为 en-prompt-sections 公开 API 一并移植。
- **位置参数 → 参数 struct**：TS `buildWriterSystemPrompt` 14 个位置参数在 Rust
  用 `WriterSystemPromptInput<'a>` 承载（book/genre_profile 为 `Option<&T>`：
  调用方保证非空，内部 `expect`），与项目 `LoadPromptPackPromptInput` 同模式。
- **zh 分支内的死语言判定**：`buildGoldenChaptersRules(chapterNumber,
  isEnglish ? "en" : "zh")` 在 zh 分支内恒传 "zh"（外层已 !isEnglish）——
  TS 原文如此，逐字保留形状（注释说明）。
- **文案字节级 parity**：golden 差分对 `buildGoldenOpeningDiscipline`（zh/en ×
  多章号）与 `buildFanficCanonSection`（4 模式）做字节级 diff，零分歧——证明
  1057 行文案逐字搬运无误（含编号 bug、emoji、表格对齐）。
- **`\n\n\n` 是合法结构**：fanfic 段开头 `\n` + join `\n\n` 产生三个换行——
  TS 原始行为如此（filter 只过滤空段，不规范化内部换行），逐字保留。
- **mode/condition 双维度输出格式**：zh/en × creative/full 四变体 +
  numericalSystem 条件 ledger/UPDATED_LEDGER 段，逐字对齐 TS 模板。

## 验证

| 维度 | 结果 |
|------|------|
| 新增单测 | **16 项**（fanfic_prompt_sections 4 + en_prompt_sections 5 +
  writer_prompts 7）全绿 |
| 全量测试 | **669 passed**（上轮 651 + 18），0 回归 |
| golden 差分 | **29 passed**（+2 域：buildGoldenOpeningDiscipline 7 向量、
  buildFanficCanonSection 4 模式），字节级零分歧 |
| clippy（lib+tests） | 零警告 |

## 解锁进度（writer 链）

- ✅ writer-parser（25 号，章节输出解析）
- ✅ writer 系统提示词三件套（本次）
- ⬜ writer.ts 编排（58.7k，文件加载 + prompt 注入 + LLM chat + 解析）——
  AuditorChat 模式可直接复用，下次目标

## 会话累计（28 个里程碑）

lib 测试 624 → **651**（+27 单测：fanfic 4 + en 5 + writer_prompts 7 +
continuity 11 已含上轮），golden 27 → **29 域**全绿，clippy 双模式零警告。
