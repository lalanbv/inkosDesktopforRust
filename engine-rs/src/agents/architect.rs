//! 架构师（architect）——基础设定生成域核心（57 号）。
//!
//! 移植自 `packages/core/src/agents/architect.ts`（1433 行）的主链：
//! - [`generate_foundation`] / [`generate_foundation_from_import`]（双语
//!   提示词逐字）
//! - SECTION 解析（`=== SECTION: ===` 标记 / `#` 标题双模式 + legacy 段名
//!   回退）+ `---ROLE---` 角色卡解析
//! - [`parse_sections_with_repair`]（缺段 LLM 修复环 →
//!   [`ArchitectIncompleteFoundationError`〕双语兜底文案）
//! - [`normalize_pending_hooks_section`]（13 列 Phase 7 规范化 + 种子预晋升
//!   四规则：core_hook / depends_on / advanced_count / cross_volume）
//! - [`write_foundation_files`]（Phase 5 落盘 contract：outline 双 prose 文件
//!   + 一人一卡 roles/ + 兼容 shim + 运行时种子；revise 模式清空重建）
//!
//! 暂缓件（58 号）：`generateFanficFoundation`、`generateAndReviewFoundation`
//! 审核环（FoundationReviewerAgent）、books/create 等端点挂载。
//!
//! ## 与 TS 的差异
//! - chat 端口由调用方注入（`ArchitectChat`），经 AgentRouter 装配；
//! - `normalizeSectionName` 的 NFKC 归一化简化为 ASCII 语义处理
//!   （section 名为英文 snake_case，全角边角差异备案）。

use regex::Regex;
use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

use crate::agents::continuity::ChatOutcome;
use crate::agents::rules_reader::read_genre_profile;
use crate::llm::provider::{LLMMessage, LLMRole};
use crate::models::book::BookConfig;
use crate::models::runtime_state::{HookPayoffTiming, HookRecord, HookStatus};
use crate::utils::language::WritingLanguage;
use crate::utils::story_markdown::render_hook_snapshot;

/// 架构师 chat 端口（TS `BaseAgent.chat` 的最小面）。
#[async_trait::async_trait]
pub trait ArchitectChat: Send + Sync {
    async fn chat(&self, messages: Vec<LLMMessage>, temperature: f64) -> Result<ChatOutcome, String>;
}

/// 架构师上下文（I/O 注入）。
pub struct ArchitectCtx<'a> {
    pub project_root: &'a Path,
    pub builtin_genres_dir: &'a Path,
}

/// 角色卡。对齐 TS `ArchitectRole`。
#[derive(Debug, Clone, PartialEq)]
pub struct ArchitectRole {
    pub tier: ArchitectRoleTier,
    pub name: String,
    pub content: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchitectRoleTier {
    Major,
    Minor,
}

/// 架构师产出。对齐 TS `ArchitectOutput`（legacy 面 + Phase 5 新面）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ArchitectOutput {
    pub story_bible: String,
    pub volume_outline: String,
    pub book_rules: String,
    pub current_state: String,
    pub pending_hooks: String,
    pub story_frame: String,
    pub volume_map: String,
    pub rhythm_principles: String,
    pub roles: Vec<ArchitectRole>,
}

/// 基础设定不完整（修复环后仍缺段）。双语兜底文案由 TS 逐字移植。
#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct ArchitectIncompleteFoundationError {
    pub missing: Vec<String>,
    pub partial_content: String,
    pub message: String,
}

/// 内部缺段信号（对齐 TS `MissingArchitectSectionsError`）。
#[derive(Debug)]
struct MissingSectionsError {
    missing: Vec<String>,
    content: String,
}

// ── 生成主链 ─────────────────────────────────────────────────────

/// 生成基础设定（temp 0.8；双语提示词逐字）。对齐 TS `generateFoundation`。
pub async fn generate_foundation(
    ctx: &ArchitectCtx<'_>,
    chat: &dyn ArchitectChat,
    book: &BookConfig,
    external_context: Option<&str>,
    review_feedback: Option<&str>,
) -> Result<ArchitectOutput, ArchitectFlowError> {
    generate_foundation_inner(ctx, chat, book, external_context, review_feedback, None).await
}

/// 带修订模式（reviseFoundation 消费面）：旧四文 + 用户反馈注入系统提示词尾。
pub async fn generate_foundation_inner(
    ctx: &ArchitectCtx<'_>,
    chat: &dyn ArchitectChat,
    book: &BookConfig,
    external_context: Option<&str>,
    review_feedback: Option<&str>,
    revise_prompt: Option<&str>,
) -> Result<ArchitectOutput, ArchitectFlowError> {
    let parsed = read_genre_profile(ctx.project_root, &book.genre, ctx.builtin_genres_dir)
        .await
        .map_err(|e| ArchitectFlowError::Io(e.to_string()))?;
    let gp = &parsed.profile;
    let genre_body = &parsed.body;
    let language = match book.language.as_deref() {
        Some("en") => WritingLanguage::En,
        Some(_) => WritingLanguage::Zh,
        None if gp.language == "en" => WritingLanguage::En,
        None => WritingLanguage::Zh,
    };

    let context_block = external_context
        .filter(|c| !c.is_empty())
        .map(|c| format!("\n\n## 外部指令\n以下是来自外部系统的创作指令，请将其融入设定中：\n\n{c}\n"))
        .unwrap_or_default();
    let review_feedback_block = build_review_feedback_block(review_feedback, language);
    let numerical_block = if gp.numerical_system {
        "- 有明确的数值/资源体系可追踪\n- 在 book_rules 中写清核心资源、硬上限和不可突破规则"
    } else {
        "- 本题材无数值系统，不需要资源账本"
    };
    let power_block = if gp.power_scaling { "- 有明确的战力等级体系" } else { "" };
    let era_block = if gp.era_research {
        "- 需要年代考据支撑（在 story_frame 中织入时代锚，在 book_rules 中写清不可违背的年代限制）"
    } else {
        ""
    };

    let system_prompt = if language == WritingLanguage::En {
        build_english_foundation_prompt(book, gp, genre_body, &context_block, &review_feedback_block, numerical_block, power_block, era_block)
    } else {
        build_chinese_foundation_prompt(book, gp, genre_body, &context_block, &review_feedback_block, numerical_block, power_block, era_block)
    };
    let lang_prefix = if language == WritingLanguage::En {
        "【LANGUAGE OVERRIDE】ALL output (story_frame, volume_map, roles, book_rules, pending_hooks) MUST be written in English. Character names, place names, and all prose must be in English. The === SECTION: === tags remain unchanged. Do NOT emit rhythm_principles or current_state sections — rhythm principles live inside the last paragraph of volume_map; environment/era anchors (when relevant) are woven into story_frame's world-tonal-ground paragraph.\n\n"
    } else {
        ""
    };
    let user_message = if language == WritingLanguage::En {
        format!("Generate the complete foundation for a {} novel titled \"{}\". Write everything in English.", gp.name, book.title)
    } else {
        format!("请为标题为\"{}\"的{}小说生成完整基础设定。", book.title, gp.name)
    };

    let response = chat
        .chat(
            vec![
                LLMMessage { role: LLMRole::System, content: format!("{lang_prefix}{system_prompt}{}", revise_prompt.unwrap_or("")), tool_calls: None, tool_call_id: None },
                LLMMessage { role: LLMRole::User, content: user_message, tool_calls: None, tool_call_id: None },
            ],
            0.8,
        )
        .await
        .map_err(ArchitectFlowError::Chat)?;
    parse_sections_with_repair(chat, &response.content, language).await
}

/// `buildRevisePrompt`（architect.ts 逐字）：既有架构稿修订模式。
pub fn build_revise_prompt(
    story_bible: &str,
    volume_outline: &str,
    book_rules: &str,
    character_matrix: &str,
    user_feedback: &str,
) -> String {
    fn or_none(text: &str) -> &str {
        if text.is_empty() { "（无）" } else { text }
    }
    format!(
        "\n\n## 既有架构稿修订模式\n你在把一本已有书的架构稿从条目式升级为当前的段落式架构稿 + 一人一卡角色目录；如果它已经是 Phase 5 结构，则按用户反馈二次重写。\n\n原书信息（这是权威内容，必须完整保留其中的世界观、角色、主线、伏笔和语气）：\n\n【story_bible / story_frame 全文】\n{}\n\n【volume_outline / volume_map 全文】\n{}\n\n【book_rules 全文】\n{}\n\n【character_matrix / roles 全文】\n{}\n\n你的任务：\n1. 把现有内容重新组织成当前 5 段 SECTION：story_frame / volume_map / roles / book_rules / pending_hooks\n2. story_frame 使用段落式世界观与核心冲突，不要退回条目表格\n3. volume_map 使用段落式卷/章级方向，并把节奏原则放进末段\n4. roles 必须按一人一卡输出，主要/次要角色判断沿用原内容，缺失才按主线重要性推断\n5. pending_hooks 必须保留原有未回收伏笔，不要因为重写架构稿而清空\n6. 不要改动已写章节的运行时事实，不要重置 current_state / pending_hooks 之外的运行时日志\n\n用户额外要求：\n{}\n",
        or_none(story_bible),
        or_none(volume_outline),
        or_none(book_rules),
        or_none(character_matrix),
        or_none(user_feedback),
    )
}

/// 从已有章节反向推导基础设定（temp 0.5）。对齐 TS `generateFoundationFromImport`。
pub async fn generate_foundation_from_import(
    ctx: &ArchitectCtx<'_>,
    chat: &dyn ArchitectChat,
    book: &BookConfig,
    chapters_text: &str,
    external_context: Option<&str>,
    review_feedback: Option<&str>,
    import_mode: ImportMode,
) -> Result<ArchitectOutput, ArchitectFlowError> {
    let parsed = read_genre_profile(ctx.project_root, &book.genre, ctx.builtin_genres_dir)
        .await
        .map_err(|e| ArchitectFlowError::Io(e.to_string()))?;
    let gp = &parsed.profile;
    let genre_body = &parsed.body;
    let language = match book.language.as_deref() {
        Some("en") => WritingLanguage::En,
        Some(_) => WritingLanguage::Zh,
        None if gp.language == "en" => WritingLanguage::En,
        None => WritingLanguage::Zh,
    };
    let review_feedback_block = build_review_feedback_block(review_feedback, language);
    let context_block = external_context
        .filter(|c| !c.is_empty())
        .map(|c| {
            if language == WritingLanguage::En {
                format!("\n\n## External Instructions\n{c}\n")
            } else {
                format!("\n\n## 外部指令\n{c}\n")
            }
        })
        .unwrap_or_default();
    let numerical_block = match (gp.numerical_system, language) {
        (true, WritingLanguage::En) => "- The story uses a trackable numerical/resource system",
        (true, _) => "- 有明确的数值/资源体系可追踪",
        (false, WritingLanguage::En) => "- No explicit numerical system",
        (false, _) => "- 本题材无数值系统",
    };
    let is_series = import_mode == ImportMode::Series;
    let continuation_directive = if language == WritingLanguage::En {
        if is_series {
            "## Continuation Direction Requirements\nThe continuation portion must open up new narrative space — new conflict vector, new location, new time horizon. Ignite within 5 chapters; at least 50% fresh scenes."
        } else {
            "## Continuation Direction\nNaturally extend the existing arc. Advance existing conflicts, pay off planted hooks, introduce new complications organically."
        }
    } else if is_series {
        "## 续写方向要求\n续写必须引入新叙事空间——新冲突、新地点、新时间。5章内引爆，50%以上场景新鲜。"
    } else {
        "## 续写方向\n自然延续已有叙事弧线。推进现有冲突、兑现已埋伏笔、引入有机新变数。"
    };

    let system_prompt = if language == WritingLanguage::En {
        format!(
            "You are a professional novel architect. Reverse-engineer a prose-density foundation from the source chapters and write the continuation path.{context_block}{review_feedback_block}\n\n## Book metadata\n- Title: {title}\n- Platform: {platform}\n- Genre: {genre_name} ({genre})\n- Target chapters: {target}\n- Chapter length: {length}\n\n## Genre body\n{genre_body}\n\n{numerical_block}\n\n{continuation_directive}\n\n## Output contract\nFollow the consolidated 5-section === SECTION: === layout: story_frame, volume_map, roles, book_rules, pending_hooks. Do NOT emit rhythm_principles or current_state — rhythm principles live in the last paragraph of volume_map; character initial status lives in roles.Current_State; initial hooks live in pending_hooks start_chapter=0 rows; era / setting anchors (only when the genre pins to a real year) are woven into story_frame's world-tonal-ground paragraph.\n\nAll prose must be derived from the source package. Do not invent settings. If the package says it is compressed, treat chapter catalog + excerpts as evidence for the foundation; the full chapters will be replayed later for detailed truth files. For volume_map, treat existing chapters as \"review\" (one paragraph) and continuation as prose chapter-level planning. Hook extraction must be complete for the evidence provided.\n\nAll output MUST be written in English.",
            title = book.title,
            platform = book.platform.as_str(),
            genre_name = gp.name,
            genre = book.genre,
            target = book.target_chapters,
            length = book.chapter_word_count,
        )
    } else {
        format!(
            "你是专业的网络小说架构师。从已有章节中反向推导散文密度的基础设定，同时设计续写路径。{context_block}{review_feedback_block}\n\n## 书籍元信息\n- 标题：{title}\n- 平台：{platform}\n- 题材：{genre_name}（{genre}）\n- 目标章数：{target}章\n\n## 题材底色\n{genre_body}\n\n{numerical_block}\n\n{continuation_directive}\n\n## 输出契约\n合并后的 5 段 === SECTION: === 结构：story_frame / volume_map / roles / book_rules / pending_hooks。**不要输出 rhythm_principles 或 current_state 两个 section**——节奏原则合并进 volume_map 尾段，角色初始状态合并进 roles.当前现状，初始钩子写在 pending_hooks startChapter=0 行；环境/时代锚（只有年代文 / 历史同人 / 都市重生等真实年份题材需要）织进 story_frame.世界观底色，其他题材直接省略。\n\n所有 prose 必须从资料包中推导，不得臆造。若资料包声明为压缩包，把章节目录和正文摘录当作基础设定证据；完整章节会在后续回放阶段逐章进入 truth files。volume_map 中，已有章节作为\"回顾段\"（一段散文），续写部分写到章级 prose。伏笔识别以资料包提供的证据为准，尽量完整。",
            title = book.title,
            platform = book.platform.as_str(),
            genre_name = gp.name,
            genre = book.genre,
            target = book.target_chapters,
        )
    };
    let user_message = if language == WritingLanguage::En {
        format!(
            "Generate the complete foundation for an imported {} novel titled \"{}\". Write everything in English.\n\n{chapters_text}",
            gp.name, book.title
        )
    } else {
        format!("以下是《{}》的已有正文资料包，请从中反向推导完整基础设定：\n\n{chapters_text}", book.title)
    };

    let response = chat
        .chat(
            vec![
                LLMMessage { role: LLMRole::System, content: system_prompt, tool_calls: None, tool_call_id: None },
                LLMMessage { role: LLMRole::User, content: user_message, tool_calls: None, tool_call_id: None },
            ],
            0.5,
        )
        .await
        .map_err(ArchitectFlowError::Chat)?;
    parse_sections_with_repair(chat, &response.content, language).await
}

/// 同人基础设定生成（temp 0.7）。对齐 TS `generateFanficFoundation`
/// （提示词逐字；reviewFeedbackBlock 语言用 `book.language ?? "zh"`）。
pub async fn generate_fanfic_foundation(
    ctx: &ArchitectCtx<'_>,
    chat: &dyn ArchitectChat,
    book: &BookConfig,
    fanfic_canon: &str,
    fanfic_mode: crate::models::book::FanficMode,
    review_feedback: Option<&str>,
) -> Result<ArchitectOutput, ArchitectFlowError> {
    let parsed = read_genre_profile(ctx.project_root, &book.genre, ctx.builtin_genres_dir)
        .await
        .map_err(|e| ArchitectFlowError::Io(e.to_string()))?;
    let genre_body = &parsed.body;
    let language = if book.language.as_deref().unwrap_or("zh") == "en" {
        WritingLanguage::En
    } else {
        WritingLanguage::Zh
    };
    let review_feedback_block = build_review_feedback_block(review_feedback, language);

    let mode = match fanfic_mode {
        crate::models::book::FanficMode::Canon => "canon",
        crate::models::book::FanficMode::Au => "au",
        crate::models::book::FanficMode::Ooc => "ooc",
        crate::models::book::FanficMode::Cp => "cp",
    };
    let mode_instruction = match fanfic_mode {
        crate::models::book::FanficMode::Canon => "剧情发生在原作空白期或未详述的角度。不可改变原作已确立的事实。",
        crate::models::book::FanficMode::Au => "标注AU设定与原作的关键分歧点，分歧后的世界线自由发展。保留角色核心性格。",
        crate::models::book::FanficMode::Ooc => "标注角色性格偏离的起点和驱动事件。偏离必须有逻辑驱动。",
        crate::models::book::FanficMode::Cp => "以配对角色的关系线为主线规划卷纲。每卷必须有关系推进节点。",
    };
    let system_prompt = format!(
        "你是专业同人架构师。基于原作正典为同人生成散文密度的基础设定。\n\n## 同人模式：{mode}\n{mode_instruction}\n\n## 新时空要求\n必须为这本同人设计原创叙事空间，不是复述原作剧情：\n1. 明确分岔点——story_frame 必须标注本作从原作的哪个节点分岔\n2. 独立核心冲突——volume_map 的核心冲突必须是原创的\n3. 5章内引爆\n4. 场景新鲜度 ≥ 50%\n{review_feedback_block}\n\n## 原作正典\n{fanfic_canon}\n\n## 题材底色\n{genre_body}\n\n## 输出契约\n严格按合并后的 5 段 === SECTION: === 块输出：story_frame / volume_map / roles / book_rules / pending_hooks。**不要输出 rhythm_principles 或 current_state**：节奏原则合并进 volume_map 尾段；角色初始状态写在 roles.当前现状，初始钩子写在 pending_hooks startChapter=0 行；环境/时代锚（仅当同人的原作/本作锚定真实年份时）织进 story_frame.世界观底色，其他情况省略。\n\n- 主要角色必须来自原作正典\n- 可添加原创配角，标注\"原创\"\n- book_rules 用普通 Markdown 规则卡；必须写清同人模式：{mode}\n- 长篇散文规则写进 story_frame.世界观底色，book_rules 只保留主角、题材锁、同人模式、禁止事项等可执行规则\n- 主角弧线只写在 roles/主要角色/<主角>.md，不在 story_frame 重复\n- 所有 outline 必须是散文密度"
    );
    let user_message = format!(
        "请为标题为\"{}\"的{mode}模式同人小说生成基础设定。目标{}章，每章{}字。",
        book.title, book.target_chapters, book.chapter_word_count
    );
    let response = chat
        .chat(
            vec![
                LLMMessage { role: LLMRole::System, content: system_prompt, tool_calls: None, tool_call_id: None },
                LLMMessage { role: LLMRole::User, content: user_message, tool_calls: None, tool_call_id: None },
            ],
            0.7,
        )
        .await
        .map_err(ArchitectFlowError::Chat)?;
    parse_sections_with_repair(chat, &response.content, language).await
}

/// 导入模式。对齐 TS `importMode: "continuation" | "series"`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ImportMode {
    #[default]
    Continuation,
    Series,
}

/// 架构师流程错误。
#[derive(Debug, thiserror::Error)]
pub enum ArchitectFlowError {
    #[error("{0}")]
    Io(String),
    #[error("{0}")]
    Chat(String),
    #[error(transparent)]
    Incomplete(#[from] ArchitectIncompleteFoundationError),
}

// ── 解析链 ───────────────────────────────────────────────────────

/// 解析 + 缺段修复环。对齐 TS `parseSectionsWithRepair`。
async fn parse_sections_with_repair(
    chat: &dyn ArchitectChat,
    content: &str,
    language: WritingLanguage,
) -> Result<ArchitectOutput, ArchitectFlowError> {
    match parse_sections(content, language) {
        Ok(output) => Ok(output),
        Err(MissingSectionsError { missing, content }) => {
            let repaired = repair_missing_sections(chat, &missing, &content, language)
                .await
                .map_err(ArchitectFlowError::Chat)?;
            match parse_sections(&repaired, language) {
                Ok(output) => Ok(output),
                Err(repair_error) => {
                    let missing_list = repair_error.missing.join("、");
                    let message = if language == WritingLanguage::En {
                        format!(
                            "The story foundation came back incomplete (missing: {}). This usually means the model didn't write every section in one pass — it's not a problem with your input. Try again, or switch to a stronger model (e.g. deepseek-v4-pro / gpt-5.5) and regenerate.",
                            repair_error.missing.join(", ")
                        )
                    } else {
                        format!("基础设定没有生成完整(缺少:{missing_list})。这通常是模型一次没把所有部分写全,不是你的输入有问题。点重试,或换更强的模型(如 deepseek-v4-pro / gpt-5.5)再生成一次,通常就能解决。")
                    };
                    Err(ArchitectFlowError::Incomplete(ArchitectIncompleteFoundationError {
                        missing: repair_error.missing,
                        partial_content: repair_error.content,
                        message,
                    }))
                }
            }
        }
    }
}

/// 缺段修复（temp 0.2，保留已有内容只补缺失）。对齐 TS `repairMissingSections`。
async fn repair_missing_sections(
    chat: &dyn ArchitectChat,
    missing: &[String],
    content: &str,
    language: WritingLanguage,
) -> Result<String, String> {
    let missing_list = missing.join(", ");
    let system = if language == WritingLanguage::En {
        [
            "You repair InkOS architect output formatting.",
            "The previous draft is partially useful but is missing required SECTION blocks.",
            "Do not invent a new book. Preserve usable existing content and add the missing parts.",
            "Return the complete output with exactly these 5 SECTION blocks in order: story_frame, volume_map, roles, book_rules, pending_hooks.",
            "book_rules must be ordinary Markdown, not YAML. pending_hooks must be a Markdown table.",
            "Do not explain the repair.",
        ]
        .join("\n")
    } else {
        [
            "你负责修复 InkOS architect 的输出格式。",
            "上一轮草稿有可用内容，但缺少必需的 SECTION 块。",
            "不要重新发明一本书；保留已有可用内容，只补齐缺失部分并整理成完整输出。",
            "必须按顺序返回完整 5 段 SECTION：story_frame、volume_map、roles、book_rules、pending_hooks。",
            "book_rules 必须是普通 Markdown，不要 YAML；pending_hooks 必须是 Markdown 表格。",
            "不要解释修复过程。",
        ]
        .join("\n")
    };
    let user = if language == WritingLanguage::En {
        format!("Missing sections: {missing_list}\n\nOriginal partial output:\n\n{content}")
    } else {
        format!("缺失 section：{missing_list}\n\n原始不完整输出如下：\n\n{content}")
    };
    let response = chat
        .chat(
            vec![
                LLMMessage { role: LLMRole::System, content: system, tool_calls: None, tool_call_id: None },
                LLMMessage { role: LLMRole::User, content: user, tool_calls: None, tool_call_id: None },
            ],
            0.2,
        )
        .await?;
    Ok(response.content)
}

/// SECTION 解析（标记/标题双模式 + legacy 段名回退 + 5 段必需契约）。
/// 对齐 TS `parseSections`。
fn parse_sections(content: &str, language: WritingLanguage) -> Result<ArchitectOutput, MissingSectionsError> {
    let sections = parse_architect_section_map(content);

    let story_frame_raw = sections.get("story_frame").cloned().unwrap_or_default();
    let volume_map_raw = sections.get("volume_map").cloned().unwrap_or_default();
    let rhythm_principles = sections.get("rhythm_principles").cloned().unwrap_or_default();
    let roles_raw = sections.get("roles").cloned().unwrap_or_default();
    let legacy_story_bible = sections.get("story_bible").cloned().unwrap_or_default();
    let legacy_volume_outline = sections.get("volume_outline").cloned().unwrap_or_default();
    let book_rules = sections.get("book_rules").cloned();
    let current_state_legacy = sections.get("current_state").cloned().unwrap_or_default();
    let pending_hooks_raw = sections.get("pending_hooks").cloned();

    // 仅 legacy 段名命中 → roles 允许为空（v12 回退，读侧落 character_matrix shim）。
    let using_legacy_outline_names = story_frame_raw.is_empty()
        && volume_map_raw.is_empty()
        && (!legacy_story_bible.is_empty() || !legacy_volume_outline.is_empty());

    let effective_story_frame = if !story_frame_raw.is_empty() { story_frame_raw } else { legacy_story_bible.clone() };
    let effective_volume_map = if !volume_map_raw.is_empty() { volume_map_raw } else { legacy_volume_outline.clone() };

    let mut missing: Vec<String> = Vec::new();
    if effective_story_frame.is_empty() {
        missing.push("story_frame".to_string());
    }
    if effective_volume_map.is_empty() {
        missing.push("volume_map".to_string());
    }
    if roles_raw.trim().is_empty() && !using_legacy_outline_names {
        missing.push("roles".to_string());
    }
    if book_rules.is_none() {
        missing.push("book_rules".to_string());
    }
    if pending_hooks_raw.is_none() {
        missing.push("pending_hooks".to_string());
    }
    if !missing.is_empty() {
        return Err(MissingSectionsError { missing, content: content.to_string() });
    }

    let roles = parse_roles(&roles_raw);
    let pending_hooks = normalize_pending_hooks_section(
        &strip_trailing_assistant_coda(pending_hooks_raw.as_deref().unwrap_or("")),
        &effective_volume_map,
    );

    let story_bible = if !legacy_story_bible.is_empty() {
        legacy_story_bible
    } else {
        build_story_bible_shim(&effective_story_frame, language)
    };
    let volume_outline = if !legacy_volume_outline.is_empty() { legacy_volume_outline } else { effective_volume_map.clone() };

    Ok(ArchitectOutput {
        story_bible,
        volume_outline,
        book_rules: book_rules.unwrap_or_default(),
        current_state: current_state_legacy,
        pending_hooks,
        story_frame: effective_story_frame,
        volume_map: effective_volume_map,
        rhythm_principles,
        roles,
    })
}

/// SECTION 标记正则：`=== SECTION: name ===`（可带 # 前后缀）。
fn section_marker_re() -> &'static Regex {
    // TS: /^\s{0,3}(?:#{1,6}\s*)?===\s*SECTION\s*[：:]\s*([^\n=]+?)\s*===\s*(?:#+\s*)?$/gim
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"(?im)^[\t ]{0,3}(?:#{1,6}[\t ]*)?===[\t ]*SECTION[\t ]*[：:][\t ]*([^\n=]+?)[\t ]*===[\t ]*(?:#+[\t ]*)?$")
            .expect("section marker regex")
    })
}

/// `#` 标题回退正则（h1-h3）。
fn heading_re() -> &'static Regex {
    // TS: /^\s{0,3}#{1,3}\s+(.+?)\s*$/gim
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?im)^[\t ]{0,3}#{1,3}[\t ]+(.+?)[\t ]*$").expect("heading regex"))
}

/// 双模式 SECTION 切片。对齐 TS `parseArchitectSectionMap` + `sliceArchitectSections`。
fn parse_architect_section_map(content: &str) -> HashMap<String, String> {
    let markers: Vec<(String, usize, usize)> = section_marker_re()
        .captures_iter(content)
        .map(|caps| {
            let whole = caps.get(0).unwrap();
            (normalize_section_name(&caps[1]), whole.start(), whole.as_str().len())
        })
        .collect();
    if !markers.is_empty() {
        return slice_sections(content, &markers);
    }

    let headings: Vec<(String, usize, usize)> = heading_re()
        .captures_iter(content)
        .map(|caps| {
            let whole = caps.get(0).unwrap();
            let name = canonical_section_name_from_heading(&caps[1]);
            (name, whole.start(), whole.as_str().len())
        })
        .filter(|(name, _, _)| !name.is_empty())
        .collect();
    slice_sections(content, &headings)
}

fn slice_sections(content: &str, matches: &[(String, usize, usize)]) -> HashMap<String, String> {
    let mut out: HashMap<String, String> = HashMap::new();
    for (i, (name, index, marker_len)) in matches.iter().enumerate() {
        let start = index + marker_len;
        let end = matches.get(i + 1).map(|(_, next, _)| *next).unwrap_or(content.len());
        out.insert(name.clone(), content[start..end.min(content.len())].trim().to_string());
    }
    out
}

/// 段名归一（lower + 符号折叠为 `_`）。对齐 TS `normalizeSectionName`（NFKC
/// 简化为 ASCII 语义，偏差备案）。
fn normalize_section_name(name: &str) -> String {
    let mut out = String::new();
    let mut last_underscore = false;
    for c in name.trim().to_lowercase().chars() {
        let mapped = match c {
            '`' | '"' | '\'' | '*' | '_' | ' ' => '_',
            c if c.is_ascii_lowercase() || c.is_ascii_digit() => c,
            _ => '_',
        };
        if mapped == '_' {
            if !last_underscore && !out.is_empty() {
                out.push('_');
                last_underscore = true;
            }
        } else {
            out.push(mapped);
            last_underscore = false;
        }
    }
    while out.ends_with('_') {
        out.pop();
    }
    out
}

/// `#` 标题 → 规范段名（英文关键词 + 中文标题词）。对齐 TS `canonicalSectionNameFromHeading`。
fn canonical_section_name_from_heading(heading: &str) -> String {
    let normalized = normalize_section_name(heading);
    let contains_any = |names: &[&str]| names.iter().any(|n| normalized.contains(n));
    // 冷路径（SECTION 标记缺失时的标题回退）——直接编译，不做缓存。
    let zh = |pattern: &str| Regex::new(pattern).expect("heading keyword regex").is_match(heading);
    if contains_any(&["story_frame", "story_bible", "story_foundation", "foundation"])
        || zh("故事框架|故事圣经|基础设定|世界框架|故事底座")
    {
        return "story_frame".to_string();
    }
    if contains_any(&["volume_map", "volume_outline", "outline", "plot_map"])
        || zh("分卷地图|卷纲|分卷大纲|章节地图|故事大纲")
    {
        return "volume_map".to_string();
    }
    if contains_any(&["roles", "characters", "character_cards"])
        || zh("角色设定|人物设定|角色卡|主要角色|角色|人物")
    {
        return "roles".to_string();
    }
    if contains_any(&["book_rules", "rules", "writing_rules"])
        || zh("本书规则|写作规则|运行规则|创作规则|规则卡")
    {
        return "book_rules".to_string();
    }
    if contains_any(&["pending_hooks", "hooks", "hook_ledger"])
        || zh("待回收钩子|待回收伏笔|伏笔表|钩子表|钩子|伏笔")
    {
        return "pending_hooks".to_string();
    }
    if contains_any(&["rhythm_principles", "rhythm"]) || zh("节奏原则|节奏") {
        return "rhythm_principles".to_string();
    }
    if contains_any(&["current_state", "initial_state"]) || zh("当前状态|初始状态") {
        return "current_state".to_string();
    }
    String::new()
}

/// `---ROLE--- / ---CONTENT---` 角色卡解析（坏块静默丢弃）。对齐 TS `parseRoles`。
fn parse_roles(raw: &str) -> Vec<ArchitectRole> {
    if raw.trim().is_empty() {
        return Vec::new();
    }
    let role_re = role_delimiter_re();
    let content_re = content_delimiter_re();
    let tier_re = tier_cell_re();
    let name_re = name_cell_re();

    role_re
        .split(raw)
        .map(str::trim)
        .filter(|chunk| !chunk.is_empty())
        .filter_map(|block| {
            let mut parts: Vec<&str> = content_re.splitn(block, 2).collect();
            if parts.len() < 2 {
                return None;
            }
            let header_raw = parts.remove(0).trim();
            let content = parts.join("\n---CONTENT---\n").trim().to_string();
            let tier = tier_re.captures(header_raw).map(|c| match &c[1] {
                t if t.eq_ignore_ascii_case("major") || t == "主要" => ArchitectRoleTier::Major,
                _ => ArchitectRoleTier::Minor,
            })?;
            let name = name_re.captures(header_raw)?.get(1)?.as_str().trim().to_string();
            if name.is_empty() || content.is_empty() {
                return None;
            }
            Some(ArchitectRole { tier, name, content })
        })
        .collect()
}

fn role_delimiter_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?m)^---ROLE---$").expect("role delimiter"))
}

fn content_delimiter_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?m)^---CONTENT---$").expect("content delimiter"))
}

fn tier_cell_re() -> &'static Regex {
    // TS: /tier\s*[:：]\s*(major|minor|主要|次要)/i
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)tier\s*[:：]\s*(major|minor|主要|次要)").expect("tier cell"))
}

fn name_cell_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)name\s*[:：]\s*(.+)").expect("name cell"))
}

/// 剥离尾部助手客套（"如果你愿意…" / "If you'd like…"）。对齐 TS `stripTrailingAssistantCoda`。
fn strip_trailing_assistant_coda(section: &str) -> String {
    let coda_re = assistant_coda_re();
    let lines: Vec<&str> = section.split('\n').collect();
    let cutoff = lines
        .iter()
        .position(|line| {
            let trimmed = line.trim();
            !trimmed.is_empty() && coda_re.is_match(trimmed)
        });
    match cutoff {
        None => section.to_string(),
        Some(index) => lines[..index].join("\n").trim_end().to_string(),
    }
}

fn assistant_coda_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"(?i)^(如果(你愿意|需要|想要|希望)|If (you('d)? like|you want|needed)|I can (continue|next))")
            .expect("assistant coda regex")
    })
}

// ── 兼容 shim ────────────────────────────────────────────────────

/// story_bible.md 兼容指针（摘录前 2000 字符，UTF-16 码元）。对齐 TS `buildStoryBibleShim`。
fn build_story_bible_shim(story_frame: &str, language: WritingLanguage) -> String {
    let excerpt = utf16_take(story_frame, 2000);
    if language == WritingLanguage::En {
        format!(
            "# Story Bible (compat pointer — deprecated)\n\n> This file is kept for external readers only. The authoritative source is now:\n> - outline/story_frame.md (theme / tonal ground / core conflict / world rules / endgame)\n> - outline/volume_map.md (chapter-granular plot map)\n> - roles/ directory (one-file-per-character sheets)\n\n## Excerpt from story_frame\n\n{excerpt}\n"
        )
    } else {
        format!(
            "# 故事圣经（兼容指针——已废弃）\n\n> 本文件仅为外部读取保留。权威来源已迁移至：\n> - outline/story_frame.md（主题 / 基调 / 核心冲突 / 世界铁律 / 终局）\n> - outline/volume_map.md（章级别的分卷地图）\n> - roles/ 文件夹（一人一卡角色档案）\n\n## story_frame 摘录\n\n{excerpt}\n"
        )
    }
}

/// character_matrix.md 兼容指针（roles/ 文件清单）。对齐 TS `buildCharacterMatrixShim`。
fn build_character_matrix_shim(roles: &[ArchitectRole], language: WritingLanguage) -> String {
    let majors: Vec<String> = roles
        .iter()
        .filter(|r| r.tier == ArchitectRoleTier::Major)
        .map(|r| format!("- roles/主要角色/{}.md", r.name))
        .collect();
    let minors: Vec<String> = roles
        .iter()
        .filter(|r| r.tier == ArchitectRoleTier::Minor)
        .map(|r| format!("- roles/次要角色/{}.md", r.name))
        .collect();
    if language == WritingLanguage::En {
        let major = if majors.is_empty() { "(none)".to_string() } else { majors.join("\n") };
        let minor = if minors.is_empty() { "(none)".to_string() } else { minors.join("\n") };
        format!(
            "# Character Matrix (compat pointer — deprecated)\n\n> This file is kept for external readers only. Authoritative source is now the roles/ directory (one-file-per-character).\n\n## Major characters\n\n{major}\n\n## Minor characters\n\n{minor}\n"
        )
    } else {
        let major = if majors.is_empty() { "（无）".to_string() } else { majors.join("\n") };
        let minor = if minors.is_empty() { "（无）".to_string() } else { minors.join("\n") };
        format!(
            "# 角色矩阵（兼容指针——已废弃）\n\n> 本文件仅为外部读取保留。权威来源已迁移至 roles/ 文件夹（一人一卡）。\n\n## 主要角色\n\n{major}\n\n## 次要角色\n\n{minor}\n"
        )
    }
}

/// UTF-16 码元截取（TS `String.slice(0, n)`）。
fn utf16_take(text: &str, limit: usize) -> String {
    if text.encode_utf16().count() <= limit {
        return text.to_string();
    }
    let mut out = String::new();
    let mut count = 0usize;
    for c in text.chars() {
        let units = c.len_utf16();
        if count + units > limit {
            break;
        }
        out.push(c);
        count += units;
    }
    out
}

// ── pending_hooks 规范化（Phase 7） ──────────────────────────────

/// 13 列伏笔表规范化 + 种子预晋升。非表格输入原样返回。
/// 对齐 TS `normalizePendingHooksSection`。
pub fn normalize_pending_hooks_section(section: &str, volume_map_raw: &str) -> String {
    let rows: Vec<Vec<String>> = section
        .lines()
        .map(|line| line.trim())
        .filter(|line| line.starts_with('|'))
        .filter(|line| !line.contains("---"))
        .map(|line| {
            line.split('|')
                .skip(1)
                .take(line.split('|').count().saturating_sub(2))
                .map(|cell| cell.trim().to_string())
                .collect::<Vec<String>>()
        })
        .filter(|cells| cells.iter().any(|c| !c.is_empty()))
        .collect();
    if rows.is_empty() {
        return section.to_string();
    }
    let data_rows: Vec<&Vec<String>> = rows
        .iter()
        .filter(|row| !row.first().map(|c| c.to_lowercase() == "hook_id").unwrap_or(false))
        .collect();
    if data_rows.is_empty() {
        return section.to_string();
    }

    let has_chinese = section.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c));
    let language = if has_chinese { WritingLanguage::Zh } else { WritingLanguage::En };

    let mut hooks: Vec<HookRecord> = data_rows
        .iter()
        .enumerate()
        .map(|(index, row)| {
            let cell = |idx: usize| row.get(idx).cloned().unwrap_or_default();
            let raw_progress = cell(4);
            let normalized_progress = parse_hook_chapter_number(&raw_progress);
            let seed_note = if normalized_progress == 0 && has_narrative_progress(&raw_progress) {
                if language == WritingLanguage::En {
                    format!("initial signal: {raw_progress}")
                } else {
                    format!("初始线索：{raw_progress}")
                }
            } else {
                String::new()
            };

            let phase7 = row.len() >= 12;
            let phase6 = row.len() >= 8;
            let note_cell_index = if phase7 { 11 } else if phase6 { 7 } else { 6 };
            let notes = merge_hook_notes(&cell(note_cell_index), &seed_note, language);

            let status_cell = cell(3);
            HookRecord {
                hook_id: {
                    let id = cell(0);
                    if id.is_empty() { format!("hook-{}", index + 1) } else { id }
                },
                start_chapter: parse_hook_chapter_number(&cell(1)),
                hook_type: cell(2),
                status: HookStatus::Open,
                status_raw: if status_cell.is_empty() { "open".to_string() } else { status_cell },
                last_advanced_chapter: normalized_progress,
                expected_payoff: cell(5),
                payoff_timing: if phase6 { normalize_payoff_timing(&cell(6)) } else { None },
                notes,
                depends_on: if phase7 { Some(parse_depends_on_cell(&cell(7))) } else { None },
                pays_off_in_arc: if phase7 { Some(cell(8).trim().to_string()) } else { None },
                core_hook: if phase7 { Some(parse_boolean_cell(&cell(9))) } else { None },
                half_life_chapters: if phase7 { parse_optional_int(&cell(10)) } else { None },
                advanced_count: None,
                promoted: None,
            }
        })
        .collect();

    // 种子预晋升（三结构规则；advanced_count 运行期由 consolidator 处理）。
    let boundaries = parse_volume_boundaries_for_promotion(volume_map_raw);
    let all_seed_start: HashMap<String, u32> =
        hooks.iter().map(|h| (h.hook_id.clone(), h.start_chapter)).collect();
    for hook in hooks.iter_mut() {
        let decision = should_promote_hook(hook, &boundaries, &all_seed_start);
        hook.promoted = Some(decision);
        if !decision && hook.last_advanced_chapter == 0 {
            let status = normalize_dormant_seed_status(&hook.status_raw, language);
            hook.status_raw = status;
        }
    }

    render_hook_snapshot(&hooks, language)
}

/// 回收节奏单元格 → 枚举（中英双语单元格）。TS 透传原串，
/// HookRecord 的枚举面做双语映射（"立即"→Immediate 等）。
fn normalize_payoff_timing(cell: &str) -> Option<HookPayoffTiming> {
    use crate::utils::hook_lifecycle::normalize_hook_payoff_timing;
    normalize_hook_payoff_timing(Some(cell))
}

fn parse_hook_chapter_number(value: &str) -> u32 {
    let first_digits: String = value.chars().skip_while(|c| !c.is_ascii_digit()).take_while(|c| c.is_ascii_digit()).collect();
    first_digits.parse().unwrap_or(0)
}

fn parse_depends_on_cell(value: &str) -> Vec<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let lower = trimmed.to_lowercase();
    if lower == "none" || lower == "n/a" || trimmed == "-" || trimmed == "无" {
        return Vec::new();
    }
    let stripped = trimmed
        .trim_start_matches(['[', '('])
        .trim_start()
        .trim_end_matches([']', ')'])
        .trim_end();
    let bold_re = bold_marker_re();
    stripped
        .split([',', '，', '、', '/'])
        .map(|item| {
            let item = item.trim();
            bold_re.replace(item, "${1}").trim().to_string()
        })
        .filter(|item| !item.is_empty())
        .collect()
}

fn bold_marker_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^\*\*(.+)\*\*$").expect("bold marker"))
}

fn parse_boolean_cell(value: &str) -> bool {
    let normalized = value.trim().to_lowercase();
    if normalized.is_empty() {
        return false;
    }
    matches!(normalized.as_str(), "true" | "yes" | "y" | "是" | "核心" | "core" | "1" | "✓" | "✔")
}

fn parse_optional_int(value: &str) -> Option<u32> {
    let normalized = value.trim();
    if normalized.is_empty() {
        return None;
    }
    let digits: String = normalized.chars().skip_while(|c| !c.is_ascii_digit()).take_while(|c| c.is_ascii_digit()).collect();
    let parsed = digits.parse().ok()?;
    (parsed > 0).then_some(parsed)
}

fn has_narrative_progress(value: &str) -> bool {
    let normalized = value.trim().to_lowercase();
    if normalized.is_empty() {
        return false;
    }
    !matches!(normalized.as_str(), "0" | "none" | "n/a" | "na" | "-" | "无" | "未推进")
}

fn merge_hook_notes(notes: &str, seed_note: &str, language: WritingLanguage) -> String {
    let trimmed_notes = notes.trim();
    let trimmed_seed = seed_note.trim();
    if trimmed_seed.is_empty() {
        return trimmed_notes.to_string();
    }
    if trimmed_notes.is_empty() {
        return trimmed_seed.to_string();
    }
    if language == WritingLanguage::Zh {
        format!("{trimmed_notes}（{trimmed_seed}）")
    } else {
        format!("{trimmed_notes} ({trimmed_seed})")
    }
}

/// 休眠种子状态：open/active 系 → 「暂缓 / deferred」；其余保留原值。
fn normalize_dormant_seed_status(status: &str, language: WritingLanguage) -> String {
    let normalized = status.trim().to_lowercase();
    if normalized.is_empty() || matches!(normalized.as_str(), "open" | "opened" | "active") {
        return if language == WritingLanguage::Zh { "暂缓".to_string() } else { "deferred".to_string() };
    }
    let original = status.trim();
    if original.is_empty() {
        if language == WritingLanguage::Zh { "暂缓".to_string() } else { "deferred".to_string() }
    } else {
        original.to_string()
    }
}

// ── 种子预晋升（hook-promotion 三结构规则） ──────────────────────

/// 卷边界。对齐 TS `VolumeBoundary`。
#[derive(Debug, Clone, PartialEq)]
pub struct PromotionVolumeBoundary {
    pub name: String,
    pub start_ch: u32,
    pub end_ch: u32,
}

/// 单钩晋升判定（core_hook / depends_on / cross_volume；建书期
/// advanced_count 恒 0）。对齐 TS `shouldPromoteHook`。
fn should_promote_hook(
    hook: &HookRecord,
    boundaries: &[PromotionVolumeBoundary],
    all_seed_start: &HashMap<String, u32>,
) -> bool {
    if hook.core_hook == Some(true) {
        return true;
    }
    if hook.depends_on.as_ref().is_some_and(|d| !d.is_empty()) {
        return true;
    }
    is_cross_volume(hook, boundaries, all_seed_start)
}

fn is_cross_volume(
    hook: &HookRecord,
    boundaries: &[PromotionVolumeBoundary],
    all_seed_start: &HashMap<String, u32>,
) -> bool {
    if boundaries.len() < 2 {
        return false;
    }
    let seed_volume = find_volume_index(boundaries, hook.start_chapter);
    if seed_volume < 0 {
        return false;
    }
    // Case A：上游依赖声明在后卷。
    for upstream in hook.depends_on.as_deref().unwrap_or(&[]) {
        if let Some(&upstream_start) = all_seed_start.get(upstream) {
            let upstream_volume = find_volume_index(boundaries, upstream_start);
            if upstream_volume > seed_volume {
                return true;
            }
        }
    }
    // Case B：回收卷提及另一卷。
    if let Some(arc) = hook.pays_off_in_arc.as_deref() {
        if let Some(arc_volume) = extract_volume_index_from_arc(arc) {
            if arc_volume as i32 != seed_volume {
                return true;
            }
        }
    }
    // Case C：endgame / slow-burn 回收节奏 + 早卷种子。
    if matches!(hook.payoff_timing, Some(HookPayoffTiming::Endgame) | Some(HookPayoffTiming::SlowBurn))
        && (seed_volume as usize) < boundaries.len() - 1
    {
        return true;
    }
    false
}

fn find_volume_index(boundaries: &[PromotionVolumeBoundary], chapter: u32) -> i32 {
    for (i, vol) in boundaries.iter().enumerate() {
        if chapter >= vol.start_ch && chapter <= vol.end_ch {
            return i as i32;
        }
    }
    if chapter == 0 && !boundaries.is_empty() {
        return 0;
    }
    -1
}

/// 回收卷描述 → 卷序（0 基）。对齐 TS `extractVolumeIndexFromArc`。
fn extract_volume_index_from_arc(arc: &str) -> Option<usize> {
    let trimmed = arc.trim();
    if trimmed.is_empty() {
        return None;
    }
    for pattern in volume_pattern_res().iter() {
        if let Some(caps) = pattern.captures(trimmed) {
            let token = caps.get(1)?.as_str();
            let n = parse_volume_number(token)?;
            return Some((n - 1) as usize);
        }
    }
    None
}

fn volume_pattern_res() -> &'static [Regex] {
    static R: OnceLock<Vec<Regex>> = OnceLock::new();
    R.get_or_init(|| {
        vec![
            Regex::new(r"第\s*([一二三四五六七八九十百千0-9]+)\s*卷").unwrap(),
            Regex::new(r"(?i)volume\s+(\d+)").unwrap(),
            Regex::new(r"(?i)vol\.?\s*(\d+)").unwrap(),
        ]
    })
}

fn parse_volume_number(token: &str) -> Option<u32> {
    if !token.is_empty() && token.chars().all(|c| c.is_ascii_digit()) {
        return token.parse().ok();
    }
    match token {
        "一" => Some(1),
        "二" => Some(2),
        "三" => Some(3),
        "四" => Some(4),
        "五" => Some(5),
        "六" => Some(6),
        "七" => Some(7),
        "八" => Some(8),
        "九" => Some(9),
        "十" => Some(10),
        _ => None,
    }
}

/// 从 volume_map 散文解析卷头行（`第N卷 (A-B章)` / `Volume N (chapters A-B)`）。
/// 对齐 TS `parseVolumeBoundariesForPromotion`。
fn parse_volume_boundaries_for_promotion(raw: &str) -> Vec<PromotionVolumeBoundary> {
    if raw.is_empty() {
        return Vec::new();
    }
    let header_re = volume_header_re();
    let range_re = volume_range_re();
    let mut volumes = Vec::new();
    for raw_line in raw.lines() {
        let line = raw_line.trim_start_matches('#').trim();
        if !header_re.is_match(line) {
            continue;
        }
        let Some(caps) = range_re.captures(line) else { continue };
        // TS 双分支正则：组 1/2 或组 3/4。
        let start_raw = caps.get(1).or_else(|| caps.get(3)).map(|m| m.as_str()).unwrap_or("0");
        let end_raw = caps.get(2).or_else(|| caps.get(4)).map(|m| m.as_str()).unwrap_or("0");
        let (Ok(start_ch), Ok(end_ch)) = (start_raw.parse::<u32>(), end_raw.parse::<u32>()) else {
            continue;
        };
        if start_ch == 0 || end_ch == 0 {
            continue;
        }
        let range_index = caps.get(0).unwrap().start();
        let name = line[..range_index].trim_end_matches(['（', '(']).trim().to_string();
        if !name.is_empty() {
            volumes.push(PromotionVolumeBoundary { name, start_ch, end_ch });
        }
    }
    volumes
}

fn volume_header_re() -> &'static Regex {
    // TS: /^(第[一二三四五六七八九十百千万零〇\d]+卷|Volume\s+\d+)/i
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"(?i)^(第[一二三四五六七八九十百千万零〇0-9]+卷|Volume\s+[0-9]+)").expect("volume header")
    })
}

fn volume_range_re() -> &'static Regex {
    // TS 双分支：括号内区间 或 无括号区间。
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"(?i)[（(]\s*(?:第|[Cc]hapters?[[:space:]]+)?([0-9]+)\s*[-–~～—]\s*([0-9]+)\s*(?:章)?\s*[）)]|(?:第|[Cc]hapters?[[:space:]]+)([0-9]+)\s*[-–~～—]\s*([0-9]+)\s*(?:章)?")
            .expect("volume range")
    })
}

// ── 审核反馈块 ───────────────────────────────────────────────────

/// 对齐 TS `buildReviewFeedbackBlock`（双语逐字）。
fn build_review_feedback_block(review_feedback: Option<&str>, language: WritingLanguage) -> String {
    let trimmed = review_feedback.map(str::trim).filter(|f| !f.is_empty());
    let Some(trimmed) = trimmed else {
        return String::new();
    };
    if language == WritingLanguage::En {
        format!("\n\n## Previous Review Feedback\nThe previous foundation draft was rejected. You must explicitly fix the following issues in this regeneration instead of paraphrasing the same design:\n\n{trimmed}\n")
    } else {
        format!("\n\n## 上一轮审核反馈\n上一轮基础设定未通过审核。你必须在这次重生中明确修复以下问题，不能只换措辞重写同一套方案：\n\n{trimmed}\n")
    }
}

// ── 落盘（write_foundation_files） ──────────────────────────────

/// 基础设定落盘模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FoundationWriteMode {
    #[default]
    Init,
    Revise,
}

/// Phase 5 落盘 contract。对齐 TS `writeFoundationFiles`（revise 模式要求
/// Phase 5 产物 + 清空重建 roles 目录）。
pub async fn write_foundation_files(
    book_dir: &Path,
    output: &ArchitectOutput,
    language: WritingLanguage,
    mode: FoundationWriteMode,
) -> Result<(), String> {
    let story_dir = book_dir.join("story");
    let outline_dir = story_dir.join("outline");
    let roles_dir = story_dir.join("roles");
    let roles_major_dir = roles_dir.join("主要角色");
    let roles_minor_dir = roles_dir.join("次要角色");
    for dir in [&story_dir, &outline_dir, &roles_major_dir, &roles_minor_dir] {
        tokio::fs::create_dir_all(dir).await.map_err(|e| e.to_string())?;
    }

    let story_frame_body = if output.story_frame.is_empty() { output.story_bible.clone() } else { output.story_frame.clone() };
    let volume_map = if output.volume_map.is_empty() { output.volume_outline.clone() } else { output.volume_map.clone() };
    let is_phase5_output = !output.story_frame.trim().is_empty();

    if mode == FoundationWriteMode::Revise && !is_phase5_output {
        return Err(
            "Architect revise mode produced legacy-format output (storyFrame empty). The book's architecture files have NOT been modified."
                .to_string(),
        );
    }
    if mode == FoundationWriteMode::Revise {
        let _ = tokio::fs::remove_dir_all(&roles_major_dir).await;
        let _ = tokio::fs::remove_dir_all(&roles_minor_dir).await;
        tokio::fs::create_dir_all(&roles_major_dir).await.map_err(|e| e.to_string())?;
        tokio::fs::create_dir_all(&roles_minor_dir).await.map_err(|e| e.to_string())?;
    }

    if !is_phase5_output {
        // Legacy 产物面（v12 前）。
        let writes: Vec<(std::path::PathBuf, String)> = vec![
            (story_dir.join("story_bible.md"), output.story_bible.clone()),
            (story_dir.join("volume_outline.md"), output.volume_outline.clone()),
            (story_dir.join("book_rules.md"), output.book_rules.clone()),
            (
                story_dir.join("character_matrix.md"),
                if language == WritingLanguage::En {
                    "# Character Matrix\n\n<!-- One ## section per character. Add new characters as new ## blocks. -->\n".to_string()
                } else {
                    "# 角色矩阵\n\n<!-- 每个角色一个 ## 块，新角色追加新 ## 即可。 -->\n".to_string()
                },
            ),
        ];
        for (path, content) in writes {
            tokio::fs::write(&path, content).await.map_err(|e| e.to_string())?;
        }
        if mode == FoundationWriteMode::Init {
            let current_state_seed = if !output.current_state.trim().is_empty() {
                output.current_state.clone()
            } else if language == WritingLanguage::En {
                "# Current State\n\n> Seeded at book creation. Runtime state is appended by the consolidator after each chapter.\n".to_string()
            } else {
                "# 当前状态\n\n> 建书时占位。运行时每章之后由 consolidator 追加最新状态。\n".to_string()
            };
            tokio::fs::write(story_dir.join("current_state.md"), current_state_seed).await.map_err(|e| e.to_string())?;
            tokio::fs::write(story_dir.join("pending_hooks.md"), &output.pending_hooks).await.map_err(|e| e.to_string())?;
            let emotional_seed = if language == WritingLanguage::En {
                "# Emotional Arcs\n\n| Character | Chapter | Emotional State | Trigger Event | Intensity (1-10) | Arc Direction |\n| --- | --- | --- | --- | --- | --- |\n"
            } else {
                "# 情感弧线\n\n| 角色 | 章节 | 情绪状态 | 触发事件 | 强度(1-10) | 弧线方向 |\n|------|------|----------|----------|------------|----------|\n"
            };
            tokio::fs::write(story_dir.join("emotional_arcs.md"), emotional_seed).await.map_err(|e| e.to_string())?;
        }
        return Ok(());
    }

    let story_frame = story_frame_body.trim().to_string();

    // Phase 5 主产物。
    tokio::fs::write(outline_dir.join("story_frame.md"), &story_frame).await.map_err(|e| e.to_string())?;
    tokio::fs::write(outline_dir.join("volume_map.md"), &volume_map).await.map_err(|e| e.to_string())?;
    if !output.rhythm_principles.trim().is_empty() {
        let rhythm_file = if language == WritingLanguage::En { "rhythm_principles.md" } else { "节奏原则.md" };
        tokio::fs::write(outline_dir.join(rhythm_file), &output.rhythm_principles).await.map_err(|e| e.to_string())?;
    }

    // 一人一卡。
    let unsafe_chars = unsafe_filename_re();
    for role in &output.roles {
        let safe_name = unsafe_chars.replace_all(&role.name, "_").trim().to_string();
        if safe_name.is_empty() {
            continue;
        }
        let target = if role.tier == ArchitectRoleTier::Major { &roles_major_dir } else { &roles_minor_dir };
        tokio::fs::write(target.join(format!("{safe_name}.md")), &role.content).await.map_err(|e| e.to_string())?;
    }

    // 兼容 shim。
    tokio::fs::write(story_dir.join("story_bible.md"), build_story_bible_shim(&story_frame, language))
        .await
        .map_err(|e| e.to_string())?;
    tokio::fs::write(story_dir.join("character_matrix.md"), build_character_matrix_shim(&output.roles, language))
        .await
        .map_err(|e| e.to_string())?;
    tokio::fs::write(story_dir.join("book_rules.md"), format!("{}\n", output.book_rules.trim()))
        .await
        .map_err(|e| e.to_string())?;

    if mode == FoundationWriteMode::Init {
        let current_state_seed = if !output.current_state.trim().is_empty() {
            output.current_state.clone()
        } else if language == WritingLanguage::En {
            "# Current State\n\n> Seeded at book creation. Runtime state is appended by the consolidator after each chapter. Initial per-character state lives in roles/*.Current_State; load-bearing initial world facts live in pending_hooks rows with start_chapter=0.\n".to_string()
        } else {
            "# 当前状态\n\n> 建书时占位。运行时每章之后由 consolidator 追加最新状态。每个角色的初始状态详见 roles/*.当前现状；承重的初始世界设定见 pending_hooks 里 startChapter=0 的行。\n".to_string()
        };
        tokio::fs::write(story_dir.join("current_state.md"), current_state_seed).await.map_err(|e| e.to_string())?;
        tokio::fs::write(story_dir.join("pending_hooks.md"), &output.pending_hooks).await.map_err(|e| e.to_string())?;
        let emotional_seed = if language == WritingLanguage::En {
            "# Emotional Arcs\n\n| Character | Chapter | Emotional State | Trigger Event | Intensity (1-10) | Arc Direction |\n| --- | --- | --- | --- | --- | --- |\n"
        } else {
            "# 情感弧线\n\n| 角色 | 章节 | 情绪状态 | 触发事件 | 强度(1-10) | 弧线方向 |\n|------|------|----------|----------|------------|----------|\n"
        };
        tokio::fs::write(story_dir.join("emotional_arcs.md"), emotional_seed).await.map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn unsafe_filename_re() -> &'static Regex {
    // TS: /[/\\:*?"<>|]/g
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r#"[/\\:*?"<>|]"#).expect("unsafe filename"))
}

// 提示词模板在独立模块（体积控制）。
pub use crate::agents::foundation_prompts::{
    build_chinese_foundation_prompt, build_english_foundation_prompt,
};

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_OUTPUT: &str = r#"=== SECTION: story_frame ===
## 主题与基调
少年于微末中抬起头。

=== SECTION: volume_map ===
### 第一卷（1-30章）觉醒
主角入宗门。

=== SECTION: roles ===
---ROLE---
tier: major
name: 林动
---CONTENT---
## 核心标签
坚韧、藏拙。

---ROLE---
tier: minor
name: 药老
---CONTENT---
## 核心标签
神秘导师。

=== SECTION: book_rules ===
## 主角
- 名字：林动

=== SECTION: pending_hooks ===
| hook_id | 起始章节 | 类型 | 状态 | 最近推进 | 预期回收 | 回收节奏 | 上游依赖 | 回收卷 | 核心 | 半衰期 | 备注 |
|---|---|---|---|---|---|---|---|---|---|---|---|
| H01 | 0 | 身世 | open | 0 | 第2卷 | 慢烧 | 无 | 第2卷中段 | true |  | 祖符来历 |
| H02 | 0 | 资源 | open | 0 | 第1卷 | 近期 | 无 | 第1卷末 | false |  | 药园执事 |
"#;

    #[test]
    fn parse_sections_full_five_blocks() {
        let output = parse_sections(SAMPLE_OUTPUT, WritingLanguage::Zh).unwrap();
        assert!(output.story_frame.contains("主题与基调"));
        assert!(output.volume_map.contains("第一卷"));
        assert_eq!(output.roles.len(), 2);
        assert_eq!(output.roles[0].tier, ArchitectRoleTier::Major);
        assert_eq!(output.roles[0].name, "林动");
        assert!(output.book_rules.contains("林动"));
        // pending_hooks 规范化：H01 core=true 预晋升、H02 普通种子休眠（暂缓）。
        assert!(output.pending_hooks.contains("H01"));
        assert!(output.pending_hooks.contains("暂缓"), "hooks: {}", output.pending_hooks);
        // legacy 面由 shim 填充。
        assert!(output.story_bible.contains("兼容指针"));
    }

    #[test]
    fn parse_sections_missing_blocks_reported() {
        let err = parse_sections("=== SECTION: story_frame ===\n只有一段。", WritingLanguage::Zh).unwrap_err();
        assert_eq!(err.missing, vec!["volume_map", "roles", "book_rules", "pending_hooks"]);
    }

    #[test]
    fn heading_fallback_maps_chinese_titles_to_new_section_names() {
        // 无 SECTION 标记 → # 标题回退（中文标题词 → 新段名）。
        // 新段名命中 → roles 必需（TS 同款：此输入缺 roles 报缺失，经修复环补）。
        let content = r#"# 故事框架
框架内容。

# 分卷地图
卷内容。

# 角色设定
---ROLE---
tier: major
name: 主角
---CONTENT---
角色卡。

# 本书规则
规则。

# 伏笔表
| hook_id | 起始章节 | 类型 | 状态 | 最近推进 | 预期回收 |
|---|---|---|---|---|---|
| H01 | 1 | 线索 | open | 0 | 第3卷 |
"#;
        let output = parse_sections(content, WritingLanguage::Zh).unwrap();
        assert!(output.story_frame.contains("框架内容"));
        assert!(output.volume_map.contains("卷内容"));
        assert_eq!(output.roles.len(), 1);
    }

    #[test]
    fn legacy_outline_section_names_allow_empty_roles() {
        // 仅 legacy 段名（story_bible/volume_outline SECTION 标记）→ roles 可缺省（v12 回退）。
        let content = r#"=== SECTION: story_bible ===
旧圣经。

=== SECTION: volume_outline ===
旧卷图。

=== SECTION: book_rules ===
规则。

=== SECTION: pending_hooks ===
| H01 | 1 |
"#;
        let output = parse_sections(content, WritingLanguage::Zh).unwrap();
        assert!(output.roles.is_empty());
        assert!(output.story_bible.contains("旧圣经"));
        assert!(!output.volume_map.is_empty());
    }

    #[test]
    fn parse_roles_drops_malformed_blocks() {
        let raw = r#"---ROLE---
tier: major
name: 完整卡
---CONTENT---
内容。

---ROLE---
缺 CONTENT 分隔。

---ROLE---
tier: 主要
name: 中文tier
---CONTENT---
中文tier内容。
"#;
        let roles = parse_roles(raw);
        assert_eq!(roles.len(), 2);
        assert_eq!(roles[1].name, "中文tier");
        assert_eq!(roles[1].tier, ArchitectRoleTier::Major);
    }

    #[test]
    fn normalize_hooks_seed_promotion_and_dormant() {
        let volume_map = "第一卷（1-30章）觉醒\n第二卷（31-60章）风起";
        let table = "\
| hook_id | 起始章节 | 类型 | 状态 | 最近推进 | 预期回收 | 回收节奏 | 上游依赖 | 回收卷 | 核心 | 半衰期 | 备注 |
|---|---|---|---|---|---|---|---|---|---|---|---|
| H01 | 0 | 身世 | open | 0 | 第3卷 | 终局 | 无 | 第3卷 | true |  | 主谜团 |
| H02 | 0 | 小事 | open | 0 | 第1卷 | 近期 | 无 | 第1卷末 | false |  | 无关紧要 |
| H03 | 0 | 跨卷 | open | 0 | 第2卷 | 慢烧 | 无 | 第2卷 | false |  | 跨卷线 |
";
        let normalized = normalize_pending_hooks_section(table, volume_map);
        // H01 core → 晋升；H02 普通种子未晋升且未推进 → 暂缓；H03 慢烧 + 卷1种子 + 后卷存在 → 跨卷晋升。
        assert!(normalized.contains("H01"));
        assert!(normalized.contains("暂缓"), "{normalized}");
        assert!(normalized.contains("H03"));
    }

    #[test]
    fn volume_boundary_parsing() {
        let raw = "### 第一卷（1-30章）觉醒\n正文。\n## 第二卷（第31-60章）风起";
        let volumes = parse_volume_boundaries_for_promotion(raw);
        assert_eq!(volumes.len(), 2);
        assert_eq!(volumes[0].name, "第一卷");
        assert_eq!(volumes[1].name, "第二卷");
        assert_eq!((volumes[0].start_ch, volumes[0].end_ch), (1, 30));
        assert_eq!((volumes[1].start_ch, volumes[1].end_ch), (31, 60));
    }

    #[test]
    fn normalize_section_name_folding() {
        assert_eq!(normalize_section_name("Story Frame"), "story_frame");
        assert_eq!(normalize_section_name("  BOOK--Rules "), "book_rules");
        assert_eq!(normalize_section_name("Pending Hooks!"), "pending_hooks");
    }

    #[test]
    fn coda_stripped_from_hooks() {
        assert_eq!(
            strip_trailing_assistant_coda("正文表格。\n\n如果你愿意，我可以继续。"),
            "正文表格。"
        );
    }

    #[tokio::test]
    async fn write_foundation_files_phase5_layout() {
        let dir = tempfile::tempdir().unwrap();
        let book = dir.path().join("b1");
        let output = parse_sections(SAMPLE_OUTPUT, WritingLanguage::Zh).unwrap();
        write_foundation_files(&book, &output, WritingLanguage::Zh, FoundationWriteMode::Init)
            .await
            .unwrap();
        let story = book.join("story");
        assert!(story.join("outline").join("story_frame.md").exists());
        assert!(story.join("outline").join("volume_map.md").exists());
        assert!(story.join("roles").join("主要角色").join("林动.md").exists());
        assert!(story.join("roles").join("次要角色").join("药老.md").exists());
        assert!(story.join("story_bible.md").exists());
        assert!(story.join("character_matrix.md").exists());
        assert!(story.join("book_rules.md").exists());
        assert!(story.join("current_state.md").exists());
        assert!(story.join("pending_hooks.md").exists());
        assert!(story.join("emotional_arcs.md").exists());
        // revise 模式 + legacy 产物 → 明确错误。
        let legacy = ArchitectOutput {
            story_bible: "旧圣经".into(),
            volume_outline: "旧卷图".into(),
            book_rules: "旧规则".into(),
            ..Default::default()
        };
        let err = write_foundation_files(&book, &legacy, WritingLanguage::Zh, FoundationWriteMode::Revise)
            .await
            .unwrap_err();
        assert!(err.contains("legacy-format output"));
    }
}

#[cfg(test)]
mod revise_prompt_tests {
    use super::build_revise_prompt;

    #[test]
    fn revise_prompt_embeds_documents_and_feedback() {
        let prompt = build_revise_prompt("旧圣经", "旧卷纲", "旧规则", "旧人物", "加强群像");
        assert!(prompt.starts_with("\n\n## 既有架构稿修订模式"));
        assert!(prompt.contains("【story_bible / story_frame 全文】\n旧圣经"));
        assert!(prompt.contains("【volume_outline / volume_map 全文】\n旧卷纲"));
        assert!(prompt.contains("【book_rules 全文】\n旧规则"));
        assert!(prompt.contains("【character_matrix / roles 全文】\n旧人物"));
        assert!(prompt.contains("用户额外要求：\n加强群像"));
        assert!(prompt.contains("5. pending_hooks 必须保留原有未回收伏笔"));

        let empty = build_revise_prompt("", "", "", "", "");
        assert!(empty.contains("【story_bible / story_frame 全文】\n（无）"));
        assert!(empty.contains("用户额外要求：\n（无）"));
    }
}
