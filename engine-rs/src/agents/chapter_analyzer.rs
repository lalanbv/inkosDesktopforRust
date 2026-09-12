//! chapter-analyzer —— 已完成正文的连续性重分析（修订/重写后的结算重跑）。
//!
//! 移植自 `packages/core/src/agents/chapter-analyzer.ts`（634 行）。
//! `buildPersistenceOutput` 在正文被修订改变后调用本 agent 重跑结算：读取与
//! writer 相同的真相文件集 + 记忆选集 + 治理工作集，LLM(0.3) 产出全套
//! === TAG === 追踪文件更新，经 `parse_writer_output` 解析；正文与字数以
//! **传入的 canonical 内容为准**（分析器不重写正文）。
//!
//! ## 移植纪律
//! - 系统提示词（zh/en）与用户提示词逐字移植
//! - `defaultChapterTitle` 按 language（非 countingMode）判定；标题回退条件
//!   = 解析标题等于默认标题或 `第N章` 两种形态
//! - `renderSummarySnapshot` 是 analyzer 私有版本：空表 → 占位文案、单元格
//!   `\n` → `<br>`（区别于 story_markdown 的 render_summary_snapshot）
//! - `findOutlineNode` 是简化版（标题行正则 + 下一行非标题即节点），与
//!   planner 的四级匹配**不同源**，勿合并

use std::path::Path;

use async_trait::async_trait;
use regex::Regex;
use std::sync::OnceLock;

use crate::agents::continuity::ChatOutcome;
use crate::agents::rules_reader::{read_book_rules, read_genre_profile};
use crate::agents::writer_parser::{parse_writer_output, ParsedWriterOutput};
use crate::llm::provider::{LLMMessage, LLMRole};
use crate::models::book::BookConfig;
use crate::models::input_governance::{ContextPackage, RuleStack};
use crate::utils::context_filter::{filter_emotional_arcs, filter_subplots};
use crate::utils::governed_context::build_governed_memory_evidence_blocks;
use crate::utils::governed_working_set::{
    build_governed_character_matrix_working_set, build_governed_hook_working_set,
    GovernedHookWorkingSetInput, GovernedMatrixWorkingSetInput,
};
use crate::utils::language::WritingLanguage;
use crate::utils::length_metrics::{count_chapter_length, resolve_length_counting_mode};
use crate::utils::memory_retrieval::{retrieve_memory_selection, RetrieveMemoryParams};
use crate::utils::outline_paths::{
    read_character_context, read_current_state_with_fallback, read_story_frame, read_volume_map,
};
use crate::utils::story_markdown::parse_pending_hooks_markdown;

/// agent 名。对齐 TS `ChapterAnalyzerAgent.name`。
pub const CHAPTER_ANALYZER_NAME: &str = "chapter-analyzer";

/// LLM 聊天端口（temperature 0.3）。
#[async_trait]
pub trait ChapterAnalyzerChat: Send + Sync {
    async fn chat(
        &self,
        messages: Vec<LLMMessage>,
        temperature: f64,
    ) -> Result<ChatOutcome, String>;
}

/// analyzer 环境依赖。
pub struct ChapterAnalyzerCtx<'a> {
    pub project_root: &'a Path,
    pub builtin_genres_dir: &'a Path,
}

/// analyze 入参。对齐 TS `AnalyzeChapterInput`。
pub struct AnalyzeChapterInput<'a> {
    pub book: &'a BookConfig,
    pub book_dir: &'a Path,
    pub chapter_number: u32,
    pub chapter_content: &'a str,
    pub chapter_title: Option<&'a str>,
    pub chapter_intent: Option<&'a str>,
    pub context_package: Option<&'a ContextPackage>,
    pub rule_stack: Option<&'a RuleStack>,
}

#[derive(Debug, thiserror::Error)]
pub enum AnalyzeChapterError {
    #[error("LLM chat failed: {0}")]
    Chat(String),
}

fn missing_file_placeholder(language: WritingLanguage) -> &'static str {
    if language == WritingLanguage::En {
        "(file not created yet)"
    } else {
        "(文件尚未创建)"
    }
}

fn default_chapter_title(chapter_number: u32, language: WritingLanguage) -> String {
    if language == WritingLanguage::En {
        format!("Chapter {chapter_number}")
    } else {
        format!("第{chapter_number}章")
    }
}

/// 分析主入口：真相文件装配 → prompt → LLM → 解析 → canonical 正文回填。
pub async fn analyze_chapter(
    chat: &dyn ChapterAnalyzerChat,
    ctx: &ChapterAnalyzerCtx<'_>,
    input: &AnalyzeChapterInput<'_>,
) -> Result<ParsedWriterOutput, AnalyzeChapterError> {
    let book = input.book;
    let book_dir = input.book_dir;
    let story = book_dir.join("story");

    let parsed_genre = read_genre_profile(ctx.project_root, &book.genre, ctx.builtin_genres_dir)
        .await
        .map_err(|e| AnalyzeChapterError::Chat(e.to_string()))?;
    let genre_profile = parsed_genre.profile;
    let genre_body = parsed_genre.body;
    let resolved_language = match book.language.as_deref() {
        Some("en") => WritingLanguage::En,
        _ => WritingLanguage::Zh,
    };
    let placeholder = missing_file_placeholder(resolved_language);

    let ledger_path = story.join("particle_ledger.md");
    let hooks_path = story.join("pending_hooks.md");
    let subplot_path = story.join("subplot_board.md");
    let emotional_path = story.join("emotional_arcs.md");
    let (
        current_state,
        ledger,
        hooks,
        subplot_board,
        emotional_arcs,
        character_matrix,
        story_bible,
        volume_outline,
    ) = tokio::join!(
        // Phase 5 整合：current_state.md 还是架构师占位时从 roles + 种子 hook 推导。
        read_current_state_with_fallback(book_dir, placeholder),
        read_file_or_default(&ledger_path, placeholder),
        read_file_or_default(&hooks_path, placeholder),
        read_file_or_default(&subplot_path, placeholder),
        read_file_or_default(&emotional_path, placeholder),
        read_character_context(book_dir, placeholder),
        read_story_frame(book_dir, placeholder),
        read_volume_map(book_dir, placeholder),
    );

    let parsed_book_rules = read_book_rules(book_dir).await;
    let book_rules_body = parsed_book_rules
        .as_ref()
        .map(|parsed| parsed.body.clone())
        .unwrap_or_default();
    let book_rules = parsed_book_rules.as_ref().map(|parsed| &parsed.rules);
    let governed_mode = input.chapter_intent.is_some()
        && input.context_package.is_some()
        && input.rule_stack.is_some();

    let memory_selection = retrieve_memory_selection(&RetrieveMemoryParams {
        book_dir,
        chapter_number: input.chapter_number,
        goal: &build_memory_goal(input.chapter_title, input.chapter_content),
        outline_node: find_outline_node(&volume_outline, input.chapter_number).as_deref(),
        must_keep: &[],
        semantic_selector: None,
    })
    .await;
    let chapter_summaries =
        render_summary_snapshot(&memory_selection.summaries, resolved_language);

    let governed_memory_blocks = input
        .context_package
        .map(|package| build_governed_memory_evidence_blocks(package, Some(resolved_language)));

    let hooks_working_set = match (governed_mode, input.context_package, input.chapter_intent) {
        (true, Some(package), Some(intent)) => build_governed_hook_working_set(
            &GovernedHookWorkingSetInput {
                hooks_markdown: &hooks,
                context_package: package,
                chapter_intent: Some(intent),
                chapter_number: input.chapter_number,
                language: resolved_language,
                keep_recent: None,
            },
        ),
        _ => hooks.clone(),
    };
    let subplot_working_set = if governed_mode {
        filter_subplots(&subplot_board)
    } else {
        subplot_board.clone()
    };
    let emotional_working_set = if governed_mode {
        filter_emotional_arcs(&emotional_arcs, input.chapter_number, None)
    } else {
        emotional_arcs.clone()
    };
    let matrix_working_set = match (
        governed_mode,
        input.chapter_intent,
        input.context_package,
    ) {
        (true, Some(intent), Some(package)) => build_governed_character_matrix_working_set(
            &GovernedMatrixWorkingSetInput {
                matrix_markdown: &character_matrix,
                chapter_intent: intent,
                context_package: package,
                protagonist_name: book_rules
                    .and_then(|rules| rules.protagonist.as_ref())
                    .map(|protagonist| protagonist.name.as_str()),
            },
        ),
        _ => character_matrix.clone(),
    };
    let reduced_control_block = match (
        governed_mode,
        input.chapter_intent,
        input.context_package,
        input.rule_stack,
    ) {
        (true, Some(intent), Some(package), Some(stack)) => {
            build_reduced_control_block(intent, package, stack, resolved_language)
        }
        _ => String::new(),
    };

    let system_prompt = build_system_prompt(
        book,
        &genre_profile,
        &genre_body,
        &book_rules_body,
        resolved_language,
    );

    let bible_block = if !governed_mode && story_bible != placeholder {
        if resolved_language == WritingLanguage::En {
            format!("\n## Story Bible\n{story_bible}\n")
        } else {
            format!("\n## 世界观设定\n{story_bible}\n")
        }
    } else {
        String::new()
    };
    let outline_or_control_block = if !reduced_control_block.is_empty() {
        reduced_control_block
    } else if volume_outline != placeholder {
        if resolved_language == WritingLanguage::En {
            format!("\n## Volume Outline\n{volume_outline}\n")
        } else {
            format!("\n## 卷纲\n{volume_outline}\n")
        }
    } else {
        String::new()
    };
    let hooks_block = governed_memory_blocks
        .as_ref()
        .and_then(|blocks| blocks.hooks_block.clone())
        .unwrap_or_else(|| {
            if hooks_working_set != placeholder {
                if resolved_language == WritingLanguage::En {
                    format!("\n## Current Hooks\n{hooks_working_set}\n")
                } else {
                    format!("\n## 当前伏笔池\n{hooks_working_set}\n")
                }
            } else {
                String::new()
            }
        });
    let summaries_block = governed_memory_blocks
        .as_ref()
        .and_then(|blocks| blocks.summaries_block.clone())
        .unwrap_or_else(|| {
            if chapter_summaries != placeholder {
                if resolved_language == WritingLanguage::En {
                    format!("\n## Existing Chapter Summaries\n{chapter_summaries}\n")
                } else {
                    format!("\n## 已有章节摘要\n{chapter_summaries}\n")
                }
            } else {
                String::new()
            }
        });
    let volume_summaries_block = governed_memory_blocks
        .as_ref()
        .and_then(|blocks| blocks.volume_summaries_block.clone())
        .unwrap_or_default();
    let subplot_block = if subplot_working_set != placeholder {
        if resolved_language == WritingLanguage::En {
            format!("\n## Current Subplot Board\n{subplot_working_set}\n")
        } else {
            format!("\n## 当前支线进度板\n{subplot_working_set}\n")
        }
    } else {
        String::new()
    };
    let emotional_block = if emotional_working_set != placeholder {
        if resolved_language == WritingLanguage::En {
            format!("\n## Current Emotional Arcs\n{emotional_working_set}\n")
        } else {
            format!("\n## 当前情感弧线\n{emotional_working_set}\n")
        }
    } else {
        String::new()
    };
    let matrix_block = if matrix_working_set != placeholder {
        if resolved_language == WritingLanguage::En {
            format!("\n## Current Character Matrix\n{matrix_working_set}\n")
        } else {
            format!("\n## 当前角色交互矩阵\n{matrix_working_set}\n")
        }
    } else {
        String::new()
    };

    let user_prompt = build_user_prompt(
        resolved_language,
        input.chapter_number,
        input.chapter_content,
        input.chapter_title,
        &current_state,
        if genre_profile.numerical_system { ledger.as_str() } else { "" },
        &hooks_block,
        &summaries_block,
        &volume_summaries_block,
        &subplot_block,
        &emotional_block,
        &matrix_block,
        &bible_block,
        &outline_or_control_block,
    );

    let response = chat
        .chat(
            vec![
                LLMMessage { role: LLMRole::System, content: system_prompt, tool_calls: None, tool_call_id: None },
                LLMMessage { role: LLMRole::User, content: user_prompt, tool_calls: None, tool_call_id: None },
            ],
            0.3,
        )
        .await
        .map_err(AnalyzeChapterError::Chat)?;

    let counting_mode = resolve_length_counting_mode(resolved_language);
    let mut output = parse_writer_output(
        input.chapter_number,
        &response.content,
        &genre_profile,
        counting_mode,
    );
    // canonical 正文回填：分析器不重写正文。
    output.content = input.chapter_content.to_string();
    output.word_count = count_chapter_length(input.chapter_content, counting_mode);

    // LLM 没给出有效标题（等于默认形态）→ 用入参标题。
    if let Some(chapter_title) = input.chapter_title {
        let is_default = output.title == default_chapter_title(input.chapter_number, resolved_language)
            || output.title == format!("第{}章", input.chapter_number);
        if is_default {
            output.title = chapter_title.to_string();
        }
    }

    Ok(output)
}

/// 系统提示词（zh/en 逐字）。golden 守门。
pub fn build_system_prompt(
    book: &BookConfig,
    genre_profile: &crate::models::genre_profile::GenreProfile,
    genre_body: &str,
    book_rules_body: &str,
    language: WritingLanguage,
) -> String {
    if language == WritingLanguage::En {
        let numerical_block = if genre_profile.numerical_system {
            "\n- This genre tracks numerical/resources systems; UPDATED_LEDGER must capture every resource change shown in the chapter."
        } else {
            "\n- This genre has no numerical system; leave UPDATED_LEDGER empty."
        };
        let book_rules_section = if book_rules_body.is_empty() {
            String::new()
        } else {
            format!("## Book Rules\n\n{book_rules_body}")
        };
        return format!(
            "【LANGUAGE OVERRIDE】ALL output MUST be in English. The === TAG === markers remain unchanged.\n\nYou are a fiction continuity analyst. Analyze a finished chapter, extract every state change, and update the tracking files.\n\n## Working Mode\n\nYou are not writing new prose. You are reading completed chapter text and updating the book's truth files.\n1. Read the chapter carefully and extract all important facts.\n2. Update the existing tracking files incrementally rather than rebuilding them from scratch.\n3. Keep the output contract identical to the writer pipeline.\n\n## What To Extract\n\n- Character entrances, exits, injuries, breakthroughs, deaths, and other status changes\n- Location movement and scene transitions\n- Item or resource gains and losses\n- Hook setup, advancement, and payoff\n- Emotional arc movement\n- Subplot progress\n- Relationship changes and information-boundary changes\n\n## Book Information\n\n- Title: {title}\n- Genre: {genre_name} ({genre_id})\n- Platform: {platform}\n{numerical_block}\n\n## Genre Guidance\n\n{genre_body}\n\n{book_rules_section}\n\n## Output Format\n\nUse === TAG === delimiters exactly as shown:\n\n=== CHAPTER_TITLE ===\n(Extract or infer the chapter title. Output title text only.)\n\n=== CHAPTER_CONTENT ===\n(Repeat the original chapter content exactly. Do not rewrite.)\n\n=== PRE_WRITE_CHECK ===\n(Leave empty in analysis mode.)\n\n=== POST_SETTLEMENT ===\n(Leave empty in analysis mode.)\n\n=== UPDATED_STATE ===\nUpdated state card as a Markdown table reflecting the end-of-chapter state:\n| Field | Value |\n| --- | --- |\n| Current Chapter | {{chapter_number}} |\n| Current Location | ... |\n| Protagonist State | ... |\n| Current Goal | ... |\n| Current Constraint | ... |\n| Current Alliances | ... |\n| Current Conflict | ... |\n\n=== UPDATED_LEDGER ===\n(If the genre has a numerical system: output the fully updated resource ledger table. Otherwise leave empty.)\n\n=== UPDATED_HOOKS ===\nUpdated hooks pool as a Markdown table with the latest status of every known hook:\n| hook_id | start_chapter | type | status | last_advanced_chapter | expected_payoff | payoff_timing | notes |\n\n=== CHAPTER_SUMMARY ===\nSingle Markdown table row:\n| Chapter | Title | Characters | Key Events | State Changes | Hook Activity | Mood | Chapter Type |\n\n=== UPDATED_SUBPLOTS ===\nUpdated subplot board (Markdown table)\n\n=== UPDATED_EMOTIONAL_ARCS ===\nUpdated emotional arcs (Markdown table)\n\n=== UPDATED_CHARACTER_MATRIX ===\nUpdated character matrix (one ## section per character, bullet-list fields):\n\n## Character Name\n- **Role**: protagonist / antagonist / ally / minor / mentioned\n- **Tags**: core identity tags\n- **Contrast**: distinctive details that defy expectations\n- **Speech**: speaking style summary\n- **Personality**: core personality traits\n- **Motivation**: fundamental driving force\n- **Current**: immediate goal this chapter\n- **Relationships**: OtherChar(type/Ch#) | ...\n- **Known**: what this character knows (only witnessed or told)\n- **Unknown**: what this character does not know\n\n(Repeat for each character. Add new characters; keep existing ones updated.)\n\n## Rules\n\n1. UPDATED_STATE and UPDATED_HOOKS must be incremental updates based on the current tracking files.\n2. Every factual change in the chapter must appear in the corresponding tracking file.\n3. Do not miss resource changes, movement, relationship changes, or information changes.\n4. Information boundaries in the character matrix must stay exact: each character only knows what they directly witnessed or learned.",
            title = book.title,
            genre_name = genre_profile.name,
            genre_id = book.genre,
            platform = book.platform.as_str(),
        );
    }

    let numerical_block = if genre_profile.numerical_system {
        "\n- 本题材有数值/资源体系，你必须在 UPDATED_LEDGER 中追踪正文中出现的所有资源变动"
    } else {
        "\n- 本题材无数值系统，UPDATED_LEDGER 留空"
    };
    let book_rules_section = if book_rules_body.is_empty() {
        String::new()
    } else {
        format!("## 本书规则\n\n{book_rules_body}")
    };
    format!(
        "你是小说连续性分析师。你的任务是分析一章已完成的小说正文，从中提取所有状态变化并更新追踪文件。\n\n## 工作模式\n\n你不是在写作，而是在分析已有正文。你需要：\n1. 仔细阅读正文，提取所有关键信息\n2. 基于\"当前追踪文件\"做增量更新\n3. 输出格式与写作模块完全一致\n\n## 分析维度\n\n从正文中提取以下信息：\n- 角色出场、退场、状态变化（受伤/突破/死亡等）\n- 位置移动、场景转换\n- 物品/资源的获得与消耗\n- 伏笔的埋设、推进、回收\n- 情感弧线变化\n- 支线进展\n- 角色间关系变化、新的信息边界\n\n## 书籍信息\n\n- 标题：{title}\n- 题材：{genre_name}（{genre_id}）\n- 平台：{platform}\n{numerical_block}\n\n## 题材特征\n\n{genre_body}\n\n{book_rules_section}\n\n## 输出格式（必须严格遵循）\n\n使用 === TAG === 分隔各部分，与写作模块完全一致：\n\n=== CHAPTER_TITLE ===\n（从正文标题行提取或推断章节标题，只输出标题文字）\n\n=== CHAPTER_CONTENT ===\n（原样输出正文内容，不做任何修改）\n\n=== PRE_WRITE_CHECK ===\n（留空，分析模式不需要写作自检）\n\n=== POST_SETTLEMENT ===\n（留空，分析模式不需要写后结算）\n\n=== UPDATED_STATE ===\n更新后的状态卡（Markdown表格），反映本章结束时的最新状态：\n| 字段 | 值 |\n|------|-----|\n| 当前章节 | {{章节号}} |\n| 当前位置 | ... |\n| 主角状态 | ... |\n| 当前目标 | ... |\n| 当前限制 | ... |\n| 当前敌我 | ... |\n| 当前冲突 | ... |\n\n=== UPDATED_LEDGER ===\n（如有数值系统：更新后的完整资源账本表格；无则留空）\n\n=== UPDATED_HOOKS ===\n更新后的伏笔池（Markdown表格），包含所有已知伏笔的最新状态：\n| hook_id | 起始章节 | 类型 | 状态 | 最近推进 | 预期回收 | 回收节奏 | 备注 |\n\n=== CHAPTER_SUMMARY ===\n本章摘要（Markdown表格行）：\n| 章节 | 标题 | 出场人物 | 关键事件 | 状态变化 | 伏笔动态 | 情绪基调 | 章节类型 |\n\n=== UPDATED_SUBPLOTS ===\n更新后的支线进度板（Markdown表格）\n\n=== UPDATED_EMOTIONAL_ARCS ===\n更新后的情感弧线（Markdown表格）\n\n=== UPDATED_CHARACTER_MATRIX ===\n更新后的角色矩阵（每个角色一个 ## 块，字段用 bullet list）：\n\n## 角色名\n- **定位**: 主角 / 反派 / 盟友 / 配角 / 提及\n- **标签**: 核心身份标签\n- **反差**: 打破刻板印象的独特细节\n- **说话**: 说话风格概述\n- **性格**: 性格底色\n- **动机**: 根本驱动力\n- **当前**: 本章即时目标\n- **关系**: 某角色(关系性质/Ch#) | ...\n- **已知**: 该角色已知的信息（仅限亲历或被告知）\n- **未知**: 该角色不知道的信息\n\n（每个角色重复以上格式。新角色追加新 ## 块，已有角色做增量更新。）\n\n## 关键规则\n\n1. 状态卡和伏笔池必须基于\"当前追踪文件\"做增量更新，不是从零开始\n2. 正文中的每一个事实性变化都必须反映在对应的追踪文件中\n3. 不要遗漏细节：数值变化、位置变化、关系变化、信息变化都要记录\n4. 角色矩阵中的\"已知/未知\"要准确——角色只知道他在场时发生的事",
        title = book.title,
        genre_name = genre_profile.name,
        genre_id = book.genre,
        platform = book.platform.as_str(),
    )
}

/// 用户提示词（zh/en 逐字）。golden 守门。
#[allow(clippy::too_many_arguments)]
pub fn build_user_prompt(
    language: WritingLanguage,
    chapter_number: u32,
    chapter_content: &str,
    chapter_title: Option<&str>,
    current_state: &str,
    ledger: &str,
    hooks_block: &str,
    summaries_block: &str,
    volume_summaries_block: &str,
    subplot_block: &str,
    emotional_block: &str,
    matrix_block: &str,
    bible_block: &str,
    outline_or_control_block: &str,
) -> String {
    if language == WritingLanguage::En {
        let title_line = chapter_title
            .map(|title| format!("Chapter Title: {title}\n"))
            .unwrap_or_default();
        let ledger_block = if ledger.is_empty() {
            String::new()
        } else {
            format!("\n## Current Resource Ledger\n{ledger}\n")
        };
        return format!(
            "Analyze chapter {chapter_number} and update all tracking files.\n{title_line}\n## Chapter Content\n\n{chapter_content}\n\n## Current State\n{current_state}\n{ledger_block}\n{hooks_block}{volume_summaries_block}{subplot_block}{emotional_block}{matrix_block}{summaries_block}{outline_or_control_block}{bible_block}\n\nPlease return the result strictly in the === TAG === format."
        );
    }

    let title_line = chapter_title
        .map(|title| format!("章节标题：{title}\n"))
        .unwrap_or_default();
    let ledger_block = if ledger.is_empty() {
        String::new()
    } else {
        format!("\n## 当前资源账本\n{ledger}\n")
    };
    format!(
        "请分析第{chapter_number}章正文，更新所有追踪文件。\n{title_line}\n## 正文内容\n\n{chapter_content}\n\n## 当前状态卡\n{current_state}\n{ledger_block}\n{hooks_block}{volume_summaries_block}{subplot_block}{emotional_block}{matrix_block}{summaries_block}{outline_or_control_block}{bible_block}\n\n请严格按照 === TAG === 格式输出分析结果。"
    )
}

/// 缩减控制块（analyzer 版：已选上下文带 excerpt 的 bullet 形态）。
pub fn build_reduced_control_block(
    chapter_intent: &str,
    context_package: &ContextPackage,
    rule_stack: &RuleStack,
    language: WritingLanguage,
) -> String {
    let selected_context = context_package
        .selected_context
        .iter()
        .map(|entry| {
            let excerpt = entry
                .excerpt
                .as_deref()
                .filter(|excerpt| !excerpt.is_empty())
                .map(|excerpt| format!(" | {excerpt}"))
                .unwrap_or_default();
            format!("- {}: {}{}", entry.source, entry.reason, excerpt)
        })
        .collect::<Vec<_>>()
        .join("\n");
    let overrides = if !rule_stack.active_overrides.is_empty() {
        rule_stack
            .active_overrides
            .iter()
            .map(|override_| {
                format!(
                    "- {} -> {}: {} ({})",
                    override_.from, override_.to, override_.reason, override_.target
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        "- none".to_string()
    };
    let selected = if selected_context.is_empty() {
        "- none".to_string()
    } else {
        selected_context
    };

    if language == WritingLanguage::En {
        let join_or_none = |items: &[String], none: &str| -> String {
            if items.is_empty() {
                none.to_string()
            } else {
                items.join(", ")
            }
        };
        return format!(
            "\n## Chapter Control Inputs (compiled by Planner/Composer)\n{chapter_intent}\n\n### Selected Context\n{selected}\n\n### Rule Stack\n- Hard guardrails: {}\n- Soft constraints: {}\n- Diagnostic rules: {}\n\n### Active Overrides\n{overrides}\n",
            join_or_none(&rule_stack.sections.hard, "(none)"),
            join_or_none(&rule_stack.sections.soft, "(none)"),
            join_or_none(&rule_stack.sections.diagnostic, "(none)"),
        );
    }

    let join_or_none = |items: &[String]| -> String {
        if items.is_empty() {
            "(无)".to_string()
        } else {
            items.join("、")
        }
    };
    format!(
        "\n## 本章控制输入（由 Planner/Composer 编译）\n{chapter_intent}\n\n### 已选上下文\n{selected}\n\n### 规则栈\n- 硬护栏：{}\n- 软约束：{}\n- 诊断规则：{}\n\n### 当前覆盖\n{overrides}\n",
        join_or_none(&rule_stack.sections.hard),
        join_or_none(&rule_stack.sections.soft),
        join_or_none(&rule_stack.sections.diagnostic),
    )
}

/// 记忆检索目标：标题 + 正文（trim 后非空段以空行连接）。
pub fn build_memory_goal(chapter_title: Option<&str>, chapter_content: &str) -> String {
    [chapter_title.unwrap_or(""), chapter_content]
        .into_iter()
        .filter(|part| !part.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// 简化版大纲节点定位：标题行（`# Chapter N` / `# 第 N 章` 前缀形态）命中后
/// 取下一非标题行；无则剥 `#` 前缀用标题本身。占位/空 → None。
pub fn find_outline_node(volume_outline: &str, chapter_number: u32) -> Option<String> {
    if volume_outline.is_empty()
        || volume_outline == missing_file_placeholder(WritingLanguage::Zh)
        || volume_outline == missing_file_placeholder(WritingLanguage::En)
    {
        return None;
    }

    let lines: Vec<&str> = volume_outline
        .split('\n')
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .collect();

    let en_pattern = format!(r"(?i)^#+\s*Chapter\s*{chapter_number}\b");
    let zh_pattern = format!(r"^#+\s*第\s*{chapter_number}\s*章");
    let en_re = Regex::new(&en_pattern).ok()?;
    let zh_re = Regex::new(&zh_pattern).ok()?;

    let heading_index = lines
        .iter()
        .position(|line| en_re.is_match(line) || zh_re.is_match(line))?;
    let heading = lines[heading_index];
    let next_line = lines.get(heading_index + 1).copied();
    match next_line {
        Some(next) if !next.starts_with('#') => Some(next.to_string()),
        _ => Some(
            heading_prefix_re()
                .replace(heading, "")
                .to_string(),
        ),
    }
}

/// analyzer 私有摘要快照：空表 → 占位；单元格 `|` 转义 + `\n` → `<br>`。
pub fn render_summary_snapshot(
    summaries: &[crate::state::memory_db::StoredSummary],
    language: WritingLanguage,
) -> String {
    if summaries.is_empty() {
        return missing_file_placeholder(language).to_string();
    }

    let header: [&str; 2] = if language == WritingLanguage::En {
        [
            "| Chapter | Title | Characters | Key Events | State Changes | Hook Activity | Mood | Chapter Type | Conflict | Reveal |",
            "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |",
        ]
    } else {
        [
            "| 章节 | 标题 | 出场人物 | 关键事件 | 状态变化 | 伏笔动态 | 情绪基调 | 章节类型 | 冲突强度 | 揭示强度 |",
            "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |",
        ]
    };

    let mut lines: Vec<String> = header.iter().map(|s| s.to_string()).collect();
    for summary in summaries {
        let cells = [
            summary.chapter.to_string(),
            summary.title.clone(),
            summary.characters.clone(),
            summary.events.clone(),
            summary.state_changes.clone(),
            summary.hook_activity.clone(),
            summary.mood.clone(),
            summary.chapter_type.clone(),
            summary.conflict_level.map(|v| v.to_string()).unwrap_or_default(),
            summary.reveal_level.map(|v| v.to_string()).unwrap_or_default(),
        ];
        let escaped: Vec<String> = cells.iter().map(|c| escape_table_cell(c)).collect();
        lines.push(format!("| {} |", escaped.join(" | ")));
    }
    lines.join("\n")
}

fn escape_table_cell(value: &str) -> String {
    value.replace('|', "\\|").replace('\n', "<br>")
}

fn heading_prefix_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^#+\s*").unwrap())
}

async fn read_file_or_default(path: &Path, placeholder: &str) -> String {
    tokio::fs::read_to_string(path)
        .await
        .unwrap_or_else(|_| placeholder.to_string())
}

/// 挂给 parse_pending_hooks_markdown 的消费锚（analyzer 不直接解析 hook 表，
/// 但保留与 TS 相同的读取面；供未来诊断扩展）。
#[allow(dead_code)]
fn _hooks_parse_surface(raw: &str) -> usize {
    parse_pending_hooks_markdown(raw).len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_goal_joins_title_and_content() {
        assert_eq!(
            build_memory_goal(Some("夜探"), "正文。"),
            "夜探\n\n正文。"
        );
        assert_eq!(build_memory_goal(None, "正文。"), "正文。");
        assert_eq!(build_memory_goal(Some("  "), "正文。"), "正文。");
        assert_eq!(build_memory_goal(None, ""), "");
    }

    #[test]
    fn find_outline_node_simplified_semantics() {
        let md = "## Chapter 3 大比\n林动登场。\n## 其他\n无关";
        assert_eq!(find_outline_node(md, 3).as_deref(), Some("林动登场。"));
        // 下一行是标题 → 剥前缀用标题文本。
        let md = "## 第 3 章\n## 后续";
        assert_eq!(find_outline_node(md, 3).as_deref(), Some("第 3 章"));
        assert_eq!(find_outline_node("(文件尚未创建)", 3), None);
        assert_eq!(find_outline_node("(file not created yet)", 3), None);
        assert_eq!(find_outline_node("", 3), None);
        // \b 语义：Chapter 3 不匹配 Chapter 34。
        let md = "## Chapter 34 后章\n内容";
        assert_eq!(find_outline_node(md, 3), None);
    }

    #[test]
    fn summary_snapshot_escapes_and_placeholder() {
        let summaries = vec![crate::state::memory_db::StoredSummary {
            chapter: 2,
            title: "含|竖线".into(),
            characters: String::new(),
            events: "多行\n事件".into(),
            state_changes: String::new(),
            hook_activity: String::new(),
            mood: String::new(),
            chapter_type: String::new(),
            conflict_level: None,
            reveal_level: None,
        }];
        let out = render_summary_snapshot(&summaries, WritingLanguage::Zh);
        assert!(out.starts_with("| 章节 | 标题 |"));
        assert!(out.contains("含\\|竖线"));
        assert!(out.contains("多行<br>事件"));
        assert_eq!(
            render_summary_snapshot(&[], WritingLanguage::En),
            "(file not created yet)"
        );
    }

    #[test]
    fn reduced_control_block_variants() {
        let package = ContextPackage {
            chapter: 3,
            selected_context: vec![crate::models::input_governance::ContextSource {
                source: "story/current_focus.md".into(),
                reason: "焦点".into(),
                excerpt: Some("聚焦夺符".into()),
                rank: None,
            }],
        };
        let rule_stack = crate::utils::context_assembly::build_governed_rule_stack(
            &["禁止降智".into()],
            &[],
            3,
        );
        let zh = build_reduced_control_block("意图文本", &package, &rule_stack, WritingLanguage::Zh);
        assert!(zh.contains("## 本章控制输入（由 Planner/Composer 编译）\n意图文本"));
        assert!(zh.contains("- story/current_focus.md: 焦点 | 聚焦夺符"));
        assert!(zh.contains("- 硬护栏："));
        assert!(zh.contains("- L4 -> L3: 禁止降智 (chapter:3/mustAvoid)"));

        let en = build_reduced_control_block("intent", &package, &rule_stack, WritingLanguage::En);
        assert!(en.contains("## Chapter Control Inputs (compiled by Planner/Composer)"));
        assert!(en.contains("- Hard guardrails: "));
    }

    #[test]
    fn user_prompt_shape() {
        let zh = build_user_prompt(
            WritingLanguage::Zh,
            3,
            "正文内容。",
            Some("夜探"),
            "| 状态 |",
            "账本",
            "\n## 当前伏笔池\nhooks\n",
            "",
            "",
            "",
            "",
            "",
            "",
            "\n## 卷纲\n卷\n",
        );
        assert!(zh.starts_with("请分析第3章正文，更新所有追踪文件。\n章节标题：夜探\n"));
        assert!(zh.contains("## 当前状态卡\n| 状态 |"));
        assert!(zh.contains("## 当前资源账本\n账本"));
        assert!(zh.ends_with("请严格按照 === TAG === 格式输出分析结果。"));

        let en = build_user_prompt(
            WritingLanguage::En,
            3,
            "Body.",
            None,
            "| state |",
            "",
            "",
            "",
            "",
            "",
            "",
            "",
            "",
            "",
        );
        assert!(en.starts_with("Analyze chapter 3 and update all tracking files.\n\n## Chapter Content"));
        assert!(en.ends_with("Please return the result strictly in the === TAG === format."));
    }

    #[test]
    fn system_prompt_bilingual_skeleton() {
        let book = BookConfig {
            id: "test-book".to_string(),
            title: "测试书".to_string(),
            platform: crate::models::book::Platform::Other,
            genre: "other".to_string(),
            status: crate::models::book::BookStatus::Active,
            target_chapters: 100,
            chapter_word_count: 3000,
            language: None,
            created_at: String::new(),
            updated_at: String::new(),
            parent_book_id: None,
            fanfic_mode: None,
            series: None,
            writing: None,
        };
        let gp = crate::models::genre_profile::GenreProfile {
            name: "都市".into(),
            language: "zh".into(),
            numerical_system: true,
            ..Default::default()
        };
        let zh = build_system_prompt(&book, &gp, "题材正文", "规则正文", WritingLanguage::Zh);
        assert!(zh.contains("你是小说连续性分析师。"));
        assert!(zh.contains("- 标题："));
        assert!(zh.contains("## 本书规则\n\n规则正文"));
        assert!(zh.contains("本题材有数值/资源体系"));
        let en = build_system_prompt(&book, &gp, "", "", WritingLanguage::En);
        assert!(en.starts_with("【LANGUAGE OVERRIDE】ALL output MUST be in English."));
        assert!(!en.contains("## Book Rules"));
    }
}
