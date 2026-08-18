//! planner 编排 —— chapter memo 规划 + ChapterIntent 推导 + 意图落盘。
//!
//! 移植自 `packages/core/src/agents/planner.ts`（879 行）。两段产物：
//! - **确定性 ChapterIntent**（goal/outline/mustKeep/mustAvoid/styleEmphasis）——
//!   检索提示与 intent markdown 用
//! - **LLM ChapterMemo**（7 小节 markdown）——严格解析器 + 3 次重试 + 降级 fallback
//!
//! 重试策略：解析失败把错误信息追加进 user message 重新调用；3 次全败则产出
//! 显式警告的合法 fallback memo（不炸整条章节流水线）。
//!
//! ## 移植纪律
//! - TS outline 行匹配正则含**负向先行**（`(?!\d|\s*[-~–—]\s*\d)` 排除范围行/
//!   多位数误匹配），Rust regex 不支持 lookahead → 捕获数字 + 尾部手动校验等价还原
//! - `parseInt` 前缀语义：捕获组必为纯数字，直接 parse 等价
//! - `extractSection` 的标题层级状态机（buffer 重置/同级截断）逐字移植
//! - memo.goal 回写 intent.goal（LLM 产出比大纲推导的兜底 goal 更具体）

use std::path::{Path, PathBuf};

use regex::Regex;
use std::sync::OnceLock;

use crate::agents::continuity::ChatOutcome;
use crate::agents::planner_context::{
    compose_current_arc_prose, extract_collaborator_rows, extract_opponent_rows,
    extract_protagonist_row, extract_relevant_threads, format_recent_summaries,
    format_recyclable_hooks, read_book_rules_block, read_character_matrix,
    read_emotional_arcs, read_pending_hooks, read_subplot_board,
};
use crate::models::length_governance::LengthSpec;
use crate::agents::planner_prompts::{
    build_planner_user_message, get_planner_memo_system_prompt, PlannerUserMessageInput,
};
use crate::agents::rules_reader::read_book_rules;
use crate::llm::provider::{LLMMessage, LLMRole};
use crate::models::input_governance::{ChapterIntent, ChapterMemo};
use crate::models::runtime_state::HookRecord;
use crate::utils::chapter_memo_parser::{parse_memo, PlannerParseError};
use crate::utils::hook_lifecycle::hook_status_text;
use crate::utils::language::WritingLanguage;
use crate::utils::planning_materials::{gather_planning_materials, load_planning_seed_materials};
use crate::utils::story_markdown::{render_hook_snapshot, render_summary_snapshot};

/// agent 名。对齐 TS `PlannerAgent.name`。
pub const PLANNER_NAME: &str = "planner";

/// memo 解析重试上限。对齐 TS `MEMO_RETRY_LIMIT`。
pub const MEMO_RETRY_LIMIT: usize = 3;

/// planChapter 入参。对齐 TS `PlanChapterInput`。
pub struct PlanChapterInput<'a> {
    pub book_language: &'a str,
    pub book_dir: &'a Path,
    pub chapter_number: u32,
    pub external_context: Option<&'a str>,
    /// 131 号：planner 侧篇幅预算基准（TS input.book.chapterWordCount——
    /// write-next 的 wordCountOverride 不作用于此，writer 面独立计算）。
    pub chapter_word_count: u32,
}

/// planChapter 出参。对齐 TS `PlanChapterOutput`。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanChapterOutput {
    pub intent: ChapterIntent,
    pub memo: ChapterMemo,
    pub intent_markdown: String,
    pub planner_inputs: Vec<String>,
    pub runtime_path: PathBuf,
}

/// LLM 聊天端口（复用 AuditorChat/WriterChat 注入模式：测试 mock / 生产实现）。
#[async_trait::async_trait]
pub trait PlannerChat: Send + Sync {
    async fn chat(
        &self,
        messages: Vec<LLMMessage>,
        temperature: f64,
    ) -> Result<ChatOutcome, String>;
}

#[derive(Debug, thiserror::Error)]
pub enum PlanChapterError {
    #[error("LLM chat failed: {0}")]
    Chat(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

fn lang_from_str(value: &str) -> WritingLanguage {
    if value == "en" {
        WritingLanguage::En
    } else {
        WritingLanguage::Zh
    }
}

/// TS `(language ?? "zh").toLowerCase().startsWith("zh")`。
fn is_chinese_language(language: Option<&str>) -> bool {
    language.unwrap_or("zh").to_lowercase().starts_with("zh")
}

/// 规划下一章：种子材料 → 确定性 intent → LLM memo（重试链）→ 意图 markdown 落盘。
pub async fn plan_chapter(
    chat: &dyn PlannerChat,
    input: &PlanChapterInput<'_>,
) -> Result<PlanChapterOutput, PlanChapterError> {
    let story_dir = input.book_dir.join("story");
    let runtime_dir = story_dir.join("runtime");
    tokio::fs::create_dir_all(&runtime_dir).await?;

    let seed = load_planning_seed_materials(input.book_dir, input.chapter_number).await;
    let outline_node = find_outline_node(&seed.volume_outline, input.chapter_number);
    let goal = derive_goal(
        input.external_context,
        &seed.current_focus,
        &seed.author_intent,
        outline_node.as_deref(),
        input.chapter_number,
    );
    // Phase hotfix 5：结构化规则经 Phase 5 权威加载链（story_frame.md 优先 /
    // legacy book_rules.md 回退 / shim 拒绝清零）。
    let parsed_rules = read_book_rules(input.book_dir).await;
    let prohibitions: Vec<String> = parsed_rules
        .map(|parsed| parsed.rules.prohibitions)
        .unwrap_or_default();
    let must_keep = collect_must_keep(&seed.current_state, &seed.story_bible);
    let must_avoid = collect_must_avoid(&seed.current_focus, &prohibitions);
    let style_emphasis = collect_style_emphasis(&seed.author_intent, &seed.current_focus);

    let materials = gather_planning_materials(
        input.book_dir,
        input.chapter_number,
        &goal,
        outline_node.as_deref(),
        &must_keep,
        Some(seed),
    )
    .await;
    let memory_selection = &materials.memory_selection;
    let active_hook_count = memory_selection
        .active_hooks
        .iter()
        // TS 对原始 status 串做大小写敏感全等比较（"resolved"/"deferred"）。
        .filter(|hook| {
            let status = hook_status_text(hook);
            status != "resolved" && status != "deferred"
        })
        .count();

    let arc_context = build_arc_context(
        Some(input.book_language),
        &materials.seed.volume_outline,
        outline_node.as_deref(),
    );

    let mut intent = ChapterIntent {
        chapter: input.chapter_number,
        goal: goal.clone(),
        outline_node: outline_node.clone(),
        arc_context,
        must_keep,
        must_avoid,
        style_emphasis,
    };

    let is_golden_opening =
        is_golden_opening_chapter(Some(input.book_language), input.chapter_number);
    let memo = plan_chapter_memo(
        chat,
        &PlanChapterMemoInput {
            story_dir: &story_dir,
            book_dir: input.book_dir,
            chapter_number: input.chapter_number,
            chapter_word_count: input.chapter_word_count,
            is_golden_opening,
            fallback_goal: &goal,
            chapter_summaries_raw: &materials.seed.chapter_summaries_raw,
            previous_ending_excerpt: materials.seed.previous_ending_excerpt.as_deref(),
            brief: non_empty(&materials.seed.brief),
            chapter_context: input.external_context,
            recyclable_hooks: &memory_selection.recyclable_hooks,
            language: lang_from_str(input.book_language),
        },
    )
    .await?;

    // memo.goal 是 LLM 产出的具体任务句（≤50 字，解析器已校验）。回写 intent.goal
    // 使下游 composer/检索拿到具体任务陈述而非大纲推导的兜底句。
    intent.goal = memo.goal.clone();

    let runtime_path = runtime_dir.join(format!(
        "chapter-{:04}.intent.md",
        input.chapter_number
    ));
    let language = lang_from_str(input.book_language);
    let intent_markdown = render_intent_markdown(
        &intent,
        &memo,
        language,
        &render_hook_snapshot(&memory_selection.hooks, language),
        &render_summary_snapshot(&memory_selection.summaries, language),
        active_hook_count,
    );
    tokio::fs::write(&runtime_path, &intent_markdown).await?;

    Ok(PlanChapterOutput {
        intent,
        memo,
        intent_markdown,
        planner_inputs: materials.planner_inputs,
        runtime_path,
    })
}

/// planChapterMemo 入参。对齐 TS 内联参数对象。
pub struct PlanChapterMemoInput<'a> {
    pub story_dir: &'a Path,
    pub book_dir: &'a Path,
    pub chapter_number: u32,
    pub chapter_word_count: u32,
    pub is_golden_opening: bool,
    pub fallback_goal: &'a str,
    pub chapter_summaries_raw: &'a str,
    pub previous_ending_excerpt: Option<&'a str>,
    pub brief: Option<&'a str>,
    pub chapter_context: Option<&'a str>,
    pub recyclable_hooks: &'a [HookRecord],
    pub language: WritingLanguage,
}

/// 调 LLM 产出 7 小节 memo 并解析。解析失败最多重试 3 次（错误反馈注入 user
/// prompt 让 LLM 自纠）；全败产出显式警告的降级 memo。
pub async fn plan_chapter_memo(
    chat: &dyn PlannerChat,
    input: &PlanChapterMemoInput<'_>,
) -> Result<ChapterMemo, PlanChapterError> {
    let (character_matrix, subplot_board, emotional_arcs, pending_hooks, book_rules_raw) = tokio::join!(
        read_character_matrix(input.story_dir),
        read_subplot_board(input.story_dir),
        read_emotional_arcs(input.story_dir),
        read_pending_hooks(input.story_dir),
        read_book_rules_block(input.story_dir),
    );

    let language = input.language;
    let en = language == WritingLanguage::En;
    let no_prior_chapter = if en {
        "(this is the opening chapter — no prior chapter)"
    } else {
        "（本章为起始章，无前章）"
    };
    let no_book_rules = if en {
        "(no book_rules entries)"
    } else {
        "（暂无 book_rules 条目）"
    };
    let retry_feedback_header = if en {
        "## Error from previous output"
    } else {
        "## 上次输出的错误"
    };
    let retry_feedback_trailer = if en {
        "Fix and re-emit."
    } else {
        "请修正后重新输出。"
    };

    let user_message = build_planner_user_message(&PlannerUserMessageInput {
        chapter_number: input.chapter_number,
        previous_chapter_ending_excerpt: match input.previous_ending_excerpt.map(str::trim) {
            Some(excerpt) if !excerpt.is_empty() => excerpt,
            _ => no_prior_chapter,
        },
        recent_summaries: &format_recent_summaries(
            input.chapter_summaries_raw,
            input.chapter_number,
            3,
        ),
        current_arc_prose: &compose_current_arc_prose(
            &subplot_board,
            &emotional_arcs,
            input.chapter_number,
        ),
        protagonist_matrix_row: &extract_protagonist_row(&character_matrix),
        opponent_rows: &extract_opponent_rows(&character_matrix, 3),
        collaborator_rows: &extract_collaborator_rows(&character_matrix, 3),
        relevant_threads: &extract_relevant_threads(&pending_hooks, &subplot_board),
        recyclable_hooks: &format_recyclable_hooks(
            input.recyclable_hooks,
            input.chapter_number,
            language,
        ),
        is_golden_opening: input.is_golden_opening,
        length_budget: {
            let spec = crate::utils::length_metrics::build_length_spec(
                input.chapter_word_count,
                input.language,
            );
            crate::agents::planner_prompts::PlannerLengthBudget {
                target: spec.target,
                soft_min: spec.soft_min,
                soft_max: spec.soft_max,
                hard_min: spec.hard_min,
                hard_max: spec.hard_max,
                unit: if input.language == WritingLanguage::En { "words" } else { "字" },
            }
        },
        book_rules_relevant: &{
            let trimmed = book_rules_raw.trim();
            if trimmed.is_empty() {
                no_book_rules.to_string()
            } else {
                trimmed.to_string()
            }
        },
        brief: input.brief,
        chapter_context: input.chapter_context,
        language,
    });

    let system_prompt = get_planner_memo_system_prompt(language);

    let mut current_user_message = user_message.clone();
    let mut last_error: Option<PlannerParseError> = None;

    for attempt in 0..MEMO_RETRY_LIMIT {
        let response = chat
            .chat(
                vec![
                    LLMMessage {
                        role: LLMRole::System,
                        content: system_prompt.to_string(),
                        tool_calls: None, tool_call_id: None,
                    },
                    LLMMessage {
                        role: LLMRole::User,
                        content: current_user_message.clone(),
                        tool_calls: None, tool_call_id: None,
                    },
                ],
                0.7,
            )
            .await
            .map_err(PlanChapterError::Chat)?;

        match parse_memo(&response.content, input.chapter_number, input.is_golden_opening) {
            Ok(memo) => return Ok(memo),
            Err(error) => {
                tracing::warn!(
                    "[planner] memo parse failed (attempt {}/{}): {}",
                    attempt + 1,
                    MEMO_RETRY_LIMIT,
                    error.0
                );
                last_error = Some(error.clone());
                current_user_message = format!(
                    "{user_message}\n\n{retry_feedback_header}\n{}\n{retry_feedback_trailer}",
                    error.0
                );
            }
        }
    }

    let fallback_error = last_error
        .unwrap_or_else(|| PlannerParseError(
            "memo planner exhausted retries without a specific error".to_string(),
        ));
    tracing::warn!(
        "[planner] memo planner fell back after {} attempts: {}",
        MEMO_RETRY_LIMIT,
        fallback_error.0
    );
    let fallback_spec = crate::utils::length_metrics::build_length_spec(
        input.chapter_word_count,
        input.language,
    );
    let fallback_markdown = build_fallback_memo_markdown(&FallbackMemoInput {
        chapter_number: input.chapter_number,
        is_golden_opening: input.is_golden_opening,
        fallback_goal: input.fallback_goal,
        error_message: &fallback_error.0,
        language,
        length_spec: &fallback_spec,
    });
    parse_memo(
        &fallback_markdown,
        input.chapter_number,
        input.is_golden_opening,
    )
    .map_err(|error| PlanChapterError::Chat(error.0))
}

struct FallbackMemoInput<'a> {
    chapter_number: u32,
    is_golden_opening: bool,
    fallback_goal: &'a str,
    error_message: &'a str,
    language: WritingLanguage,
    /// 131 号：场景与篇幅预算节的硬区间/目标值来源（TS fallback 同款入参）。
    length_spec: &'a LengthSpec,
}

fn build_fallback_memo_markdown(input: &FallbackMemoInput<'_>) -> String {
    let _ = input.is_golden_opening;
    if input.language == WritingLanguage::En {
        let fallback_goal = if input.fallback_goal.is_empty() {
            format!(
                "Continue chapter {} according to the current outline",
                input.chapter_number
            )
        } else {
            input.fallback_goal.to_string()
        };
        return [
            format!("# Chapter {} memo", input.chapter_number),
            String::new(),
            "## Chapter goal".to_string(),
            fallback_goal,
            String::new(),
            "## Thread refs".to_string(),
            "none".to_string(),
            String::new(),
            "## Scene and length budget".to_string(),
            format!(
                "Plan 2-5 concrete scenes whose combined draft length stays within {}-{} words and aims for {} words. Give each scene a distinct action, consequence, and approximate word budget.",
                input.length_spec.hard_min, input.length_spec.hard_max, input.length_spec.target
            ),
            String::new(),
            "## Current task".to_string(),
            format!(
                "Use the current chapter goal and authoritative book context to continue chapter {} without inventing a new direction.",
                input.chapter_number
            ),
            String::new(),
            "## What the reader is waiting for right now".to_string(),
            "Keep the reader's active expectation from the outline and previous chapter in focus; do not replace it with a generic scene.".to_string(),
            String::new(),
            "## To pay off / to keep buried".to_string(),
            "Pay off only the near-term promises already supported by context; keep larger secrets buried unless the outline explicitly asks for them.".to_string(),
            String::new(),
            "## What the slow / transitional beats carry".to_string(),
            "If a slower beat is needed, make it carry pressure, evidence, relationship movement, or a concrete setup for the next action.".to_string(),
            String::new(),
            "## Three-question check on the key choice".to_string(),
            "The protagonist's main choice must have a reason, match current interest, and stay consistent with the established persona.".to_string(),
            String::new(),
            "## Required end-of-chapter change".to_string(),
            "End with a concrete change in information, pressure, relationship, objective, or risk so the chapter is not only summary.".to_string(),
            String::new(),
            "## Hook ledger for this chapter".to_string(),
            "advance: keep the active promise moving; resolve: only settle what has evidence; defer: preserve larger threads for later chapters.".to_string(),
            String::new(),
            "## Do not".to_string(),
            "Do not contradict established facts, ignore the user's current instruction, or turn the fallback memo into a new outline.".to_string(),
            String::new(),
            "## Planner warning".to_string(),
            format!(
                "The model failed to produce a valid chapter memo after {} attempts. Last parser error: {}",
                MEMO_RETRY_LIMIT, input.error_message
            ),
        ]
        .join("\n");
    }

    let fallback_goal = if input.fallback_goal.is_empty() {
        format!("按当前大纲继续推进第 {} 章", input.chapter_number)
    } else {
        input.fallback_goal.to_string()
    };
    [
        format!("# 第 {} 章 memo", input.chapter_number),
        String::new(),
        "## 本章目标".to_string(),
        fallback_goal,
        String::new(),
        "## 关联线索".to_string(),
        "无".to_string(),
        String::new(),
        "## 场景与篇幅预算".to_string(),
        format!(
            "规划 2-5 个有明确行动与后果的真实场景，总篇幅控制在 {}-{} 字，目标约 {} 字；为每个场景分配动态字数预算，不靠总结和重复内心戏凑字数。",
            input.length_spec.hard_min, input.length_spec.hard_max, input.length_spec.target
        ),
        String::new(),
        "## 当前任务".to_string(),
        format!(
            "沿用当前章节目标和权威设定推进第 {} 章，不临时改方向，也不把章节写成泛泛过渡。",
            input.chapter_number
        ),
        String::new(),
        "## 读者此刻在等什么".to_string(),
        "延续大纲和上一章形成的读者期待，优先回应当前已经建立的压力、证据、关系或目标变化。".to_string(),
        String::new(),
        "## 该兑现的 / 暂不掀的".to_string(),
        "只兑现已有上下文支撑的近端承诺；更大的秘密、身份、幕后主使或终局信息，除非大纲明确要求，否则继续压住。".to_string(),
        String::new(),
        "## 日常/过渡承担什么任务".to_string(),
        "如果需要日常或过渡，它必须承担压力、证据、人物关系、目标变化或下一步行动铺垫，不能只是闲聊和气氛。".to_string(),
        String::new(),
        "## 关键抉择过三连问".to_string(),
        "主角本章的关键选择必须有原因、符合当前利益，并且不背离已经建立的人设和行为逻辑。".to_string(),
        String::new(),
        "## 章尾必须发生的改变".to_string(),
        "章尾至少要在信息、压力、关系、目标或风险上发生一个明确变化，避免只有剧情摘要没有推进。".to_string(),
        String::new(),
        "## 本章 hook 账".to_string(),
        "advance: 推进当前活跃承诺；resolve: 只结清已有证据支撑的线索；defer: 大线继续保留到更合适的位置。".to_string(),
        String::new(),
        "## 不要做".to_string(),
        "不要违背既成事实，不要无视用户当前指令，不要把 fallback memo 当成新大纲重写整本书。".to_string(),
        String::new(),
        "## Planner warning".to_string(),
        format!(
            "模型连续 {} 次没有产出合格章节 memo。最后一次解析错误：{}",
            MEMO_RETRY_LIMIT, input.error_message
        ),
    ]
    .join("\n")
}

/// 黄金三章窗口：zh 书 ≤3 章；非 zh（en）书 ≤5 章。
pub fn is_golden_opening_chapter(language: Option<&str>, chapter_number: u32) -> bool {
    if is_chinese_language(language) {
        chapter_number <= 3
    } else {
        chapter_number <= 5
    }
}

pub fn build_arc_context(
    language: Option<&str>,
    volume_outline: &str,
    outline_node: Option<&str>,
) -> Option<String> {
    let outline_node = outline_node?;
    if volume_outline == "(文件尚未创建)" {
        return None;
    }
    Some(if is_chinese_language(language) {
        format!("卷纲节点：{outline_node}")
    } else {
        format!("Outline node: {outline_node}")
    })
}

/// goal 推导链：本章用户指令首行 → current_focus 局部覆盖 → 大纲节点首行 →
/// current_focus 聚焦目标 → author_intent 首行 → 默认句。
pub fn derive_goal(
    external_context: Option<&str>,
    current_focus: &str,
    author_intent: &str,
    outline_node: Option<&str>,
    chapter_number: u32,
) -> String {
    if let Some(first) = extract_first_directive(external_context) {
        return first;
    }
    if let Some(local_override) = extract_local_override_goal(current_focus) {
        return local_override;
    }
    if let Some(outline) = extract_first_directive(outline_node) {
        return outline;
    }
    if let Some(focus) = extract_focus_goal(current_focus) {
        return focus;
    }
    if let Some(author) = extract_first_directive(Some(author_intent)) {
        return author;
    }
    format!("Advance chapter {chapter_number} with clear narrative focus.")
}

pub fn collect_must_keep(current_state: &str, story_bible: &str) -> Vec<String> {
    let mut merged = extract_list_items(current_state, 2);
    merged.extend(extract_list_items(story_bible, 2));
    unique(&merged).into_iter().take(4).collect()
}

pub fn collect_must_avoid(current_focus: &str, prohibitions: &[String]) -> Vec<String> {
    let avoid_section = extract_section(
        current_focus,
        &["avoid", "must avoid", "禁止", "避免", "避雷"],
    );
    let focus_avoids: Vec<String> = match &avoid_section {
        Some(section) => extract_list_items(section, 10),
        None => current_focus
            .split('\n')
            .map(|line| line.trim())
            .filter(|line| {
                line.starts_with('-') && avoid_directive_re().is_match(line)
            })
            .filter_map(clean_list_item)
            .collect(),
    };

    let mut merged = focus_avoids;
    merged.extend(prohibitions.iter().cloned());
    unique(&merged).into_iter().take(6).collect()
}

pub fn collect_style_emphasis(author_intent: &str, current_focus: &str) -> Vec<String> {
    let mut merged = extract_focus_style_items(current_focus, 3);
    merged.extend(extract_list_items(author_intent, 2));
    unique(&merged).into_iter().take(4).collect()
}

/// 首条指令行：非空、非 `#`/`-` 开头、非模板占位。
pub fn extract_first_directive(content: Option<&str>) -> Option<String> {
    let content = content?;
    content
        .split('\n')
        .map(|line| line.trim())
        .find(|line| {
            !line.is_empty()
                && !line.starts_with('#')
                && !line.starts_with('-')
                && !is_template_placeholder(line)
        })
        .map(String::from)
}

pub fn extract_list_items(content: &str, limit: usize) -> Vec<String> {
    content
        .split('\n')
        .map(|line| line.trim())
        .filter(|line| line.starts_with('-'))
        .filter_map(clean_list_item)
        .take(limit)
        .collect()
}

fn extract_focus_goal(current_focus: &str) -> Option<String> {
    let focus_section = extract_section(
        current_focus,
        &["active focus", "focus", "当前聚焦", "当前焦点", "近期聚焦"],
    )
    .unwrap_or_else(|| current_focus.to_string());
    let directives = extract_list_items(&focus_section, 3);
    if directives.is_empty() {
        return extract_first_directive(Some(&focus_section));
    }
    let separator = if contains_chinese(&focus_section) {
        "；"
    } else {
        "; "
    };
    Some(directives.join(separator))
}

fn extract_local_override_goal(current_focus: &str) -> Option<String> {
    let override_section = extract_section(
        current_focus,
        &[
            "local override",
            "explicit override",
            "chapter override",
            "local task override",
            "局部覆盖",
            "本章覆盖",
            "临时覆盖",
            "当前覆盖",
        ],
    )?;

    let directives = extract_list_items(&override_section, 3);
    if !directives.is_empty() {
        let separator = if contains_chinese(&override_section) {
            "；"
        } else {
            "; "
        };
        return Some(directives.join(separator));
    }

    extract_first_directive(Some(&override_section))
}

fn extract_focus_style_items(current_focus: &str, limit: usize) -> Vec<String> {
    let focus_section = extract_section(
        current_focus,
        &["active focus", "focus", "当前聚焦", "当前焦点", "近期聚焦"],
    )
    .unwrap_or_else(|| current_focus.to_string());
    extract_list_items(&focus_section, limit)
}

/// 伏笔预算行：活跃 <10 简版；≥10 警告版（容量 12，剩余坑位）。
pub fn render_hook_budget(active_count: usize, language: WritingLanguage) -> String {
    const CAP: usize = 12;
    if active_count < 10 {
        return if language == WritingLanguage::En {
            format!("### Hook Budget\n- {active_count} active hooks (capacity: {CAP})")
        } else {
            format!("### 伏笔预算\n- 当前 {active_count} 条活跃伏笔（容量：{CAP}）")
        };
    }
    let remaining = CAP.saturating_sub(active_count);
    if language == WritingLanguage::En {
        format!(
            "### Hook Budget\n- {active_count} active hooks — approaching capacity ({CAP}). Only {remaining} new hook(s) allowed. Prioritize resolving existing debt over opening new threads."
        )
    } else {
        format!(
            "### 伏笔预算\n- 当前 {active_count} 条活跃伏笔——接近容量上限（{CAP}）。仅剩 {remaining} 个新坑位。优先回收旧债，不要轻易开新线。"
        )
    }
}

/// 标题分节提取：命中目标标题（规范化后）收集到同级/更浅标题为止。
/// 更深层的新目标标题会**重置** buffer（重新开始收集）。
pub fn extract_section(content: &str, headings: &[&str]) -> Option<String> {
    let targets: Vec<String> = headings
        .iter()
        .map(|heading| normalize_heading(heading))
        .collect();
    let mut buffer: Option<Vec<String>> = None;
    let mut section_level = 0usize;

    for line in content.split('\n') {
        if let Some((level, heading)) = parse_heading_line(line) {
            let normalized = normalize_heading(&heading);

            if let Some(buf) = &buffer {
                if level <= section_level {
                    let _ = buf; // borrow 提示；break 前 buffer 已收集完毕
                    break;
                }
            }

            if targets.contains(&normalized) {
                buffer = Some(Vec::new());
                section_level = level;
                continue;
            }
        }

        if let Some(buf) = &mut buffer {
            buf.push(line.to_string());
        }
    }

    let section = buffer
        .map(|lines| lines.join("\n").trim().to_string())
        .unwrap_or_default();
    if section.is_empty() {
        None
    } else {
        Some(section)
    }
}

fn parse_heading_line(line: &str) -> Option<(usize, String)> {
    let captures = heading_line_re().captures(line)?;
    let level = captures.get(1)?.as_str().len();
    let heading = captures.get(2)?.as_str().to_string();
    Some((level, heading))
}

fn normalize_heading(heading: &str) -> String {
    let stripped = heading_heading_noise_re().replace_all(heading, "");
    let collapsed = stripped.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.to_lowercase()
}

fn clean_list_item(line: &str) -> Option<String> {
    let cleaned = bullet_prefix_re().replace(line, "").trim().to_string();
    if cleaned.is_empty() {
        return None;
    }
    if table_noise_re().is_match(&cleaned) {
        return None;
    }
    if is_template_placeholder(&cleaned) {
        return None;
    }
    Some(cleaned)
}

fn is_template_placeholder(line: &str) -> bool {
    let normalized = line.trim();
    if normalized.is_empty() {
        return false;
    }
    placeholder_en_re().is_match(normalized) || placeholder_zh_re().is_match(normalized)
}

fn contains_chinese(content: &str) -> bool {
    content.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c))
}

// ---- 卷纲节点匹配（lookahead 手动等价还原） ----

/// 从 volume_map 提取本章大纲节点。四级匹配：精确章行 → 范围行（含节拍编号）→
/// 首个锚点行 → 全文首条指令。
pub fn find_outline_node(volume_outline: &str, chapter_number: u32) -> Option<String> {
    let lines: Vec<&str> = volume_outline
        .split('\n')
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .collect();

    // 1) 精确章行（内联内容或下一行内容）
    for (index, line) in lines.iter().enumerate() {
        let Some(rest) = match_exact_outline_line(line, chapter_number) else {
            continue;
        };
        if let Some(inline) = clean_outline_content(Some(&rest)) {
            return Some(inline);
        }
        if let Some(next) = find_next_outline_content(&lines, index + 1) {
            return Some(next);
        }
    }

    // 2) 范围行（内联内容 / 所属小节 + 章节节拍编号 / 下一行内容）
    for (index, line) in lines.iter().enumerate() {
        let Some((start, _end, rest)) = match_range_outline_line(line, chapter_number) else {
            continue;
        };
        if let Some(inline) = clean_outline_content(Some(&rest)) {
            return Some(inline);
        }
        if let Some(section) = extract_section_around_range(&lines, index) {
            let beat_index = i64::from(chapter_number) - start;
            let specific_beat = extract_numbered_beat(&section, beat_index);
            return specific_beat.or(Some(section));
        }
        if let Some(next) = find_next_outline_content(&lines, index + 1) {
            return Some(next);
        }
    }

    // 3) 首个锚点行（任意精确/范围形态，不校验章节号）
    for (index, line) in lines.iter().enumerate() {
        if !is_outline_anchor_line(line) {
            continue;
        }
        if let Some(rest) = match_any_exact_outline_line(line) {
            if let Some(inline) = clean_outline_content(Some(&rest)) {
                return Some(inline);
            }
        }
        if let Some((_, _, rest)) = match_any_range_outline_line(line) {
            if let Some(inline) = clean_outline_content(Some(&rest)) {
                return Some(inline);
            }
        }
        if let Some(next) = find_next_outline_content(&lines, index + 1) {
            return Some(next);
        }
        break;
    }

    // 4) 全文首条指令兜底
    extract_first_directive(Some(volume_outline))
}

fn clean_outline_content(content: Option<&str>) -> Option<String> {
    let cleaned = content?.trim();
    if cleaned.is_empty() {
        return None;
    }
    if outline_noise_re().is_match(cleaned) {
        return None;
    }
    Some(cleaned.to_string())
}

fn extract_section_around_range(lines: &[&str], range_line_index: usize) -> Option<String> {
    let mut heading_index: Option<usize> = None;
    for i in (0..range_line_index).rev() {
        if lines[i].starts_with('#') {
            heading_index = Some(i);
            break;
        }
        if match_any_range_outline_line(lines[i]).is_some()
            || match_any_exact_outline_line(lines[i]).is_some()
        {
            break;
        }
    }
    let heading_index = heading_index?;
    let heading_line = lines[heading_index];
    let heading_level = heading_level_re()
        .find(heading_line)
        .map(|m| m.as_str().len())
        .unwrap_or(3);

    let mut section_lines: Vec<&str> = Vec::new();
    for (i, line) in lines.iter().enumerate().skip(heading_index) {
        if i > heading_index {
            if let Some(next_heading) = heading_level_re().find(line) {
                if next_heading.as_str().len() <= heading_level {
                    break;
                }
            }
        }
        section_lines.push(line);
    }

    let content = section_lines.join("\n").trim().to_string();
    if content.is_empty() {
        None
    } else {
        Some(content)
    }
}

fn extract_numbered_beat(section: &str, beat_index: i64) -> Option<String> {
    if beat_index < 0 {
        return None;
    }
    let mut beats: Vec<String> = Vec::new();
    for line in section.split('\n') {
        let trimmed = line.trim();
        if let Some(captures) = numbered_beat_re().captures(trimmed) {
            beats.push(numbered_beat_prefix_re().replace(trimmed, "").to_string());
            let _ = captures;
        }
    }
    let index = usize::try_from(beat_index).ok()?;
    beats.get(index).cloned()
}

fn find_next_outline_content(lines: &[&str], start_index: usize) -> Option<String> {
    for line in lines.iter().skip(start_index) {
        if line.is_empty() {
            continue;
        }
        if is_outline_anchor_line(line) {
            return None;
        }
        if line.starts_with('#') {
            continue;
        }
        if let Some(cleaned) = clean_outline_content(Some(line)) {
            return Some(cleaned);
        }
    }
    None
}

/// 精确章行（`Chapter N` / `第 N 章`，N 等于 chapter_number）。
/// 返回 `[:：-]?`/`**`/空白 之后的尾段。
///
/// TS 负向先行 `(?!\d|\s*[-~–—]\s*\d)` 的手动等价：数字后不得紧跟数字
/// （防 12 误配 123）、不得紧跟「空白*破折号空白*数字」（防范围行误配精确行）。
fn match_exact_outline_line(line: &str, chapter_number: u32) -> Option<String> {
    if let Some(captures) = exact_en_re().captures(line) {
        let number: u32 = captures.get(1)?.as_str().parse().ok()?;
        let tail = captures.get(2)?.as_str();
        if number == chapter_number && tail_is_exact_safe(tail) {
            return Some(consume_outline_tail(tail));
        }
    }
    if let Some(captures) = exact_zh_re().captures(line) {
        let number: u32 = captures.get(1)?.as_str().parse().ok()?;
        let tail = captures.get(2)?.as_str();
        if number == chapter_number && tail_is_exact_safe(tail) {
            return Some(consume_outline_tail(tail));
        }
    }
    None
}

/// 任意精确章行（不校验章节号）。返回尾段。
fn match_any_exact_outline_line(line: &str) -> Option<String> {
    if let Some(captures) = exact_en_re().captures(line) {
        let tail = captures.get(2)?.as_str();
        if tail_is_exact_safe(tail) {
            return Some(consume_outline_tail(tail));
        }
    }
    if let Some(captures) = exact_zh_re().captures(line) {
        let tail = captures.get(2)?.as_str();
        if tail_is_exact_safe(tail) {
            return Some(consume_outline_tail(tail));
        }
    }
    None
}

/// 范围行（`Chapter A-B` / `第 A-B 章` / `章节范围：A-B 章` / `Chapter Range: A-B`）。
/// 章节号须落在 [min(A,B), max(A,B)]。返回 (start, end, 尾段)。
fn match_range_outline_line(line: &str, chapter_number: u32) -> Option<(i64, i64, String)> {
    let (start, end, rest) = match_any_range_outline_line(line)?;
    if is_chapter_within_range(start, end, i64::from(chapter_number)) {
        Some((start, end, rest))
    } else {
        None
    }
}

fn match_any_range_outline_line(line: &str) -> Option<(i64, i64, String)> {
    for regex in [range_zh_label_re(), range_en_label_re(), range_zh_re(), range_en_re()] {
        if let Some(captures) = regex.captures(line) {
            let start: i64 = captures.get(1)?.as_str().parse().ok()?;
            let end: i64 = captures.get(2)?.as_str().parse().ok()?;
            let rest = consume_outline_tail(captures.get(3)?.as_str());
            return Some((start, end, rest));
        }
    }
    None
}

/// 精确行尾段安全校验（TS lookahead 等价）。
fn tail_is_exact_safe(tail: &str) -> bool {
    // (?!\d)：首字符不得是数字。
    if tail.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        return false;
    }
    // (?!\s*[-~–—]\s*\d)：不得是「空白* 破折号 空白* 数字」形态（范围行）。
    !range_tail_re().is_match(tail)
}

/// 消费尾段的 `(?:[:：-])?(?:\*\*)?\s*` 前缀，返回剩余（对应 TS 的 `(.*)$`）。
fn consume_outline_tail(tail: &str) -> String {
    outline_tail_re().replace(tail, "").to_string()
}

fn is_outline_anchor_line(line: &str) -> bool {
    match_any_exact_outline_line(line).is_some() || match_any_range_outline_line(line).is_some()
}

fn is_chapter_within_range(start: i64, end: i64, chapter_number: i64) -> bool {
    let lower = start.min(end);
    let upper = start.max(end);
    chapter_number >= lower && chapter_number <= upper
}

/// intent markdown 渲染落盘（runtime/chapter-NNNN.intent.md）。
pub fn render_intent_markdown(
    intent: &ChapterIntent,
    memo: &ChapterMemo,
    language: WritingLanguage,
    pending_hooks: &str,
    chapter_summaries: &str,
    active_hook_count: usize,
) -> String {
    let bullet_list = |items: &[String]| -> String {
        if items.is_empty() {
            "- none".to_string()
        } else {
            items
                .iter()
                .map(|item| format!("- {item}"))
                .collect::<Vec<_>>()
                .join("\n")
        }
    };

    let must_keep = bullet_list(&intent.must_keep);
    let must_avoid = bullet_list(&intent.must_avoid);
    let style_emphasis = bullet_list(&intent.style_emphasis);

    let memo_body = memo.body.trim();
    let thread_refs_line = if memo.thread_refs.is_empty() {
        "- (none)".to_string()
    } else {
        memo.thread_refs
            .iter()
            .map(|id| format!("- {id}"))
            .collect::<Vec<_>>()
            .join("\n")
    };

    [
        "# Chapter Intent".to_string(),
        String::new(),
        "## Goal".to_string(),
        intent.goal.clone(),
        String::new(),
        "## Outline Node".to_string(),
        intent.outline_node.clone().unwrap_or_else(|| "(not found)".to_string()),
        String::new(),
        "## Arc Context".to_string(),
        intent.arc_context.clone().unwrap_or_else(|| "(none)".to_string()),
        String::new(),
        "## Must Keep".to_string(),
        must_keep,
        String::new(),
        "## Must Avoid".to_string(),
        must_avoid,
        String::new(),
        "## Style Emphasis".to_string(),
        style_emphasis,
        String::new(),
        "## Chapter Memo".to_string(),
        format!(
            "- isGoldenOpening: {}",
            if memo.is_golden_opening { "true" } else { "false" }
        ),
        String::new(),
        "### Thread Refs".to_string(),
        thread_refs_line,
        String::new(),
        "### Body".to_string(),
        memo_body.to_string(),
        String::new(),
        render_hook_budget(active_hook_count, language),
        String::new(),
        "## Pending Hooks Snapshot".to_string(),
        pending_hooks.to_string(),
        String::new(),
        "## Chapter Summaries Snapshot".to_string(),
        chapter_summaries.to_string(),
        String::new(),
    ]
    .join("\n")
}

/// trim + 去空 + 保序去重。对齐 TS `unique`。
fn unique(values: &[String]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    values
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .filter(|value| seen.insert(value.to_string()))
        .map(String::from)
        .collect()
}

fn non_empty(value: &str) -> Option<&str> {
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

// ---- 静态正则 ----

fn heading_line_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^(#+)\s*(.+?)\s*$").unwrap())
}

fn heading_level_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^(#+)").unwrap())
}

fn heading_heading_noise_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"[*_`:#]").unwrap())
}

fn bullet_prefix_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^-\s*").unwrap())
}

fn table_noise_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^[-|]+$").unwrap())
}

fn avoid_directive_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)avoid|don't|do not|不要|别|禁止").unwrap())
}

fn placeholder_en_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)^\((describe|briefly describe|write)\b[\s\S]*\)$").unwrap())
}

fn placeholder_zh_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^（(?:在这里描述|描述|填写|写下)[\s\S]*）$").unwrap())
}

fn outline_noise_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^[*_`~:：-]+$").unwrap())
}

/// 精确章行（EN）：数字后的尾段由 captures(2) 承载，lookahead 校验手动完成。
fn exact_en_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"(?i)^(?:#+\s*)?(?:[-*]\s+)?(?:\*\*)?Chapter\s*(\d+)(.*)$").unwrap()
    })
}

/// 精确章行（ZH）：`第 N 章`。
fn exact_zh_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^(?:#+\s*)?(?:[-*]\s+)?(?:\*\*)?第\s*(\d+)\s*章(.*)$").unwrap())
}

fn range_zh_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"^(?:#+\s*)?(?:[-*]\s+)?(?:\*\*)?第\s*(\d+)\s*[-~–—]\s*(\d+)\s*章(.*)$").unwrap()
    })
}

fn range_en_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"(?i)^(?:#+\s*)?(?:[-*]\s+)?(?:\*\*)?Chapter\s*(\d+)\s*[-~–—]\s*(\d+)\b(.*)$")
            .unwrap()
    })
}

fn range_zh_label_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"^(?:[-*]\s+)?(?:\*\*)?章节范围(?:\*\*)?[：:]\s*(\d+)\s*[-~–—]\s*(\d+)\s*章\s*(.*)$")
            .unwrap()
    })
}

fn range_en_label_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"(?i)^(?:[-*]\s+)?(?:\*\*)?Chapter\s*[Rr]ange(?:\*\*)?[：:]\s*(\d+)\s*[-~–—]\s*(\d+)\b\s*(.*)$")
            .unwrap()
    })
}

/// 尾段「空白* 破折号 空白* 数字」形态（TS lookahead 第二分支）。
fn range_tail_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^\s*[-~–—]\s*\d").unwrap())
}

/// 尾段前缀 `(?:[:：-])?(?:\*\*)?\s*`。
fn outline_tail_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^[:：-]?(?:\*\*)?\s*").unwrap())
}

fn numbered_beat_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^\d+[.)]\s").unwrap())
}

fn numbered_beat_prefix_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^\d+[.)]\s*").unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- MockChat：记录调用、按脚本回放 ----

    struct MockChat {
        responses: Vec<String>,
        calls: std::sync::Mutex<Vec<(Vec<LLMMessage>, f64)>>,
    }

    impl MockChat {
        fn new(responses: Vec<String>) -> Self {
            MockChat {
                responses,
                calls: std::sync::Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait::async_trait]
    impl PlannerChat for MockChat {
        async fn chat(
            &self,
            messages: Vec<LLMMessage>,
            temperature: f64,
        ) -> Result<ChatOutcome, String> {
            self.calls.lock().unwrap().push((messages, temperature));
            let index = self.calls.lock().unwrap().len() - 1;
            let content = self
                .responses
                .get(index)
                .cloned()
                .unwrap_or_else(|| self.responses.last().cloned().unwrap_or_default());
            Ok(ChatOutcome {
                content,
                usage: None,
            })
        }
    }

    fn valid_memo(chapter: u32) -> String {
        // 必备小节内容均 ≥20 UTF-16 字（parse_memo 空小节校验门槛）。
        format!(
            "# 第 {chapter} 章 memo\n\n## 本章目标\n推进主线，兑现玉符第一步\n\n## 关联线索\n- H01\n\n## 场景与篇幅预算\n- 场景 1：夜探藏书阁避封锁｜约 700 字\n- 场景 2：夺符交锋与异象｜约 1200 字\n- 场景 3：章尾封印一角真相｜约 900 字\n\n## 当前任务\n林动夜探藏书阁夺回祖符，避开巡夜执事的封锁线。\n\n## 读者此刻在等什么\n期待玉符来历揭开一部分；本章部分兑现并制造更强缺口。\n\n## 该兑现的 / 暂不掀的\n- 该兑现：玉符效力 → 兑现到第一层；暂不掀：幕后主使身份继续压住。\n\n## 日常/过渡承担什么任务\n不适用 - 本章无日常过渡，全程高压推进不留闲笔。\n\n## 关键抉择过三连问\n- 主角：为救族人冒险夺符；符合当前利益；符合坚忍人设。\n\n## 章尾必须发生的改变\n信息改变：林动得知符中封印之物的一角真相。\n\n## 本章 hook 账\nopen:\n- [new] 巡夜执事的怀疑 || 理由：现在开不点破\n\nadvance:\n- H01 \"祖符来历\" → 推进（planted → pressured）\n\nresolve:\n- 无\n\ndefer:\n- H09 \"幕后主使\" → 时机未到\n\n## 不要做\n- 不要让反派降智，不要新增第三条支线。\n\n"
        )
    }

    #[test]
    fn derive_goal_priority_chain() {
        assert_eq!(
            derive_goal(Some("先夺回玉符\n其他"), "## Focus\n- 聚焦甲", "作者意图", None, 3),
            "先夺回玉符"
        );
        // current_focus 局部覆盖段。
        let focus = "## Local Override\n- 本章只写对质";
        assert_eq!(
            derive_goal(None, focus, "作者", Some("第 3 章 大纲"), 3),
            "本章只写对质"
        );
        // 大纲节点首行。
        // extract_first_directive 不剥「第 N 章」前缀（真实链路中该串已由
        // find_outline_node 剥过）；这里验证的是透传语义。
        assert_eq!(
            derive_goal(None, "", "作者", Some("第 3 章 宗门大比开赛"), 3),
            "第 3 章 宗门大比开赛"
        );
        // 默认句。
        assert_eq!(
            derive_goal(None, "", "", None, 7),
            "Advance chapter 7 with clear narrative focus."
        );
    }

    #[test]
    fn extract_section_finds_target_and_stops_at_level() {
        let content = "## avoid\n- 降智\n- 圣母\n\n## 其他\n- 无关";
        assert_eq!(extract_section(content, &["avoid", "禁止", "避免", "避雷"]).as_deref(), Some("- 降智\n- 圣母"));
        assert_eq!(extract_section("无标题内容", &["avoid"]), None);
    }

    #[test]
    fn find_outline_node_exact_range_and_beats() {
        let exact = "## 卷一\n- 第 3 章：林动夺符\n- 第 4 章：杂役反扑";
        assert_eq!(find_outline_node(exact, 3).as_deref(), Some("林动夺符"));

        // 范围行有内联内容时内联优先（TS inline-first）。
        let range_inline = "## 第 1-5 章 试炼\n1. 觉醒";
        assert_eq!(find_outline_node(range_inline, 3).as_deref(), Some("试炼"));
        // 无内联 → 所属小节 + 章节节拍编号（beatIndex = chapter - rangeStart）。
        let range = "## 卷一 试炼\n第 1-5 章\n1. 觉醒\n2. 夺符\n3. 结怨";
        assert_eq!(find_outline_node(range, 3).as_deref(), Some("结怨"));
        // 超出节拍数回退整个小节。
        assert_eq!(find_outline_node(range, 5).as_deref(), Some("## 卷一 试炼\n第 1-5 章\n1. 觉醒\n2. 夺符\n3. 结怨"));

        // 范围行不被误配为精确行（123 vs 12、12-15）。
        let tricky = "- Chapter 12: mid clash\n- Chapter 123: later\n- Chapter 12-15: span";
        assert_eq!(find_outline_node(tricky, 12).as_deref(), Some("mid clash"));
        assert_eq!(find_outline_node(tricky, 123).as_deref(), Some("later"));

        // 首个锚点行兜底（章节号不匹配任何行）。
        let anchor = "开场\n- 第 9 章：远期";
        assert_eq!(find_outline_node(anchor, 2).as_deref(), Some("远期"));
    }

    #[test]
    fn arc_context_placeholder_guard() {
        assert_eq!(build_arc_context(Some("zh"), "(文件尚未创建)", Some("节点")), None);
        assert_eq!(
            build_arc_context(Some("zh"), "有大纲", Some("节点")).as_deref(),
            Some("卷纲节点：节点")
        );
        assert_eq!(
            build_arc_context(Some("en"), "outline", Some("node")).as_deref(),
            Some("Outline node: node")
        );
    }

    #[test]
    fn golden_opening_window_differs_by_language() {
        assert!(is_golden_opening_chapter(Some("zh"), 3));
        assert!(!is_golden_opening_chapter(Some("zh"), 4));
        assert!(is_golden_opening_chapter(Some("en"), 5));
        assert!(!is_golden_opening_chapter(Some("en"), 6));
        assert!(is_golden_opening_chapter(None, 1));
    }

    #[test]
    fn hook_budget_thresholds() {
        assert!(render_hook_budget(9, WritingLanguage::Zh).contains("当前 9 条活跃伏笔（容量：12）"));
        let warn = render_hook_budget(11, WritingLanguage::Zh);
        assert!(warn.contains("接近容量上限") && warn.contains("仅剩 1 个新坑位"));
        assert!(render_hook_budget(11, WritingLanguage::En).contains("Only 1 new hook(s) allowed"));
    }

    #[test]
    fn render_intent_markdown_shape() {
        let intent = ChapterIntent {
            chapter: 2,
            goal: "目标句".into(),
            outline_node: Some("节点".into()),
            arc_context: None,
            must_keep: vec!["保A".into()],
            must_avoid: vec![],
            style_emphasis: vec!["紧凑".into()],
        };
        let memo = ChapterMemo {
            chapter: 2,
            goal: "目标句".into(),
            is_golden_opening: true,
            body: "正文 memo".into(),
            thread_refs: vec!["H01".into()],
        };
        let got = render_intent_markdown(&intent, &memo, WritingLanguage::Zh, "- none", "- none", 2);
        assert!(got.starts_with("# Chapter Intent\n"));
        assert!(got.contains("## Goal\n目标句"));
        assert!(got.contains("## Arc Context\n(none)"));
        assert!(got.contains("## Must Avoid\n- none"));
        assert!(got.contains("- isGoldenOpening: true"));
        assert!(got.contains("### Thread Refs\n- H01"));
        assert!(got.contains("### 伏笔预算"));
        assert!(got.ends_with("## Chapter Summaries Snapshot\n- none\n"));
    }

    #[tokio::test]
    async fn plan_chapter_full_flow_with_mock_chat() {
        let dir = tempfile::tempdir().unwrap();
        let story = dir.path().join("story");
        tokio::fs::create_dir_all(story.join("runtime")).await.unwrap();

        // 卷纲：第 2 章精确行。
        let outline_dir = story.join("outline");
        tokio::fs::create_dir_all(&outline_dir).await.unwrap();
        tokio::fs::write(outline_dir.join("volume_map.md"), "- 第 2 章：夜探藏书阁\n").await.unwrap();
        // 前章正文（末屏节选来源）。
        let chapters = dir.path().join("chapters");
        tokio::fs::create_dir_all(&chapters).await.unwrap();
        tokio::fs::write(chapters.join("0001-x.md"), "# 一\n林动初醒。").await.unwrap();
        tokio::fs::write(story.join("author_intent.md"), "- 节奏快\n").await.unwrap();
        tokio::fs::write(story.join("current_focus.md"), "## 当前聚焦\n- 聚焦夺符\n").await.unwrap();

        let chat = MockChat::new(vec![valid_memo(2)]);
        let out = plan_chapter(
            &chat,
            &PlanChapterInput {
                book_language: "zh",
                book_dir: dir.path(),
                chapter_number: 2,
                external_context: Some("本章加入新导师"),
                    chapter_word_count: 3000,
            },
        )
        .await
        .unwrap();

        // goal 链：externalContext 首行优先（intent.goal 初值），
        // 成功解析后被 memo.goal 回写。
        assert_eq!(out.intent.goal, "推进主线，兑现玉符第一步");
        assert_eq!(out.memo.thread_refs, vec!["H01".to_string()]);
        assert!(out.intent_markdown.contains("## Goal\n推进主线"));
        // 意图文件落盘。
        let written = tokio::fs::read_to_string(&out.runtime_path).await.unwrap();
        assert_eq!(written, out.intent_markdown);
        assert!(out.runtime_path.ends_with("chapter-0002.intent.md"));
        // planner_inputs 含 story 真相文件清单。
        assert!(out.planner_inputs.iter().any(|p| p.ends_with("pending_hooks.md")));
        // user prompt 含外部指令块与卷纲节点。
        let (messages, temperature) = &chat.calls.lock().unwrap()[0];
        assert_eq!(*temperature, 0.7);
        assert_eq!(messages[0].role, LLMRole::System);
        assert!(messages[1].content.contains("## 本章用户指令（本章最高优先级）\n本章加入新导师"));
        // 前章末屏节选进入 user prompt。
        assert!(messages[1].content.contains("林动初醒。"));
        // outline 节点（夜探藏书阁）不直接进 user prompt——它只经 goal 推导链
        // 间接生效（本例 externalContext 优先级更高，节点未胜出）。
    }

    #[tokio::test]
    async fn memo_retry_then_success() {
        let dir = tempfile::tempdir().unwrap();
        let story = dir.path().join("story");
        tokio::fs::create_dir_all(&story).await.unwrap();

        let chat = MockChat::new(vec![
            "垃圾输出".to_string(),
            valid_memo(1),
        ]);
        let memo = plan_chapter_memo(
            &chat,
            &PlanChapterMemoInput {
                story_dir: &story,
                book_dir: dir.path(),
                chapter_number: 1,
                is_golden_opening: true,
                fallback_goal: "兜底目标",
                chapter_summaries_raw: "",
                previous_ending_excerpt: None,
                brief: None,
                chapter_context: None,
                recyclable_hooks: &[],
                language: WritingLanguage::Zh,
                chapter_word_count: 3000,
            },
        )
        .await
        .unwrap();
        assert_eq!(memo.goal, "推进主线，兑现玉符第一步");
        // 第二次调用的 user message 含错误反馈块。
        let calls = chat.calls.lock().unwrap();
        assert_eq!(calls.len(), 2);
        assert!(calls[1].0[1].content.contains("## 上次输出的错误"));
    }

    #[tokio::test]
    async fn memo_falls_back_after_exhausted_retries() {
        let dir = tempfile::tempdir().unwrap();
        let story = dir.path().join("story");
        tokio::fs::create_dir_all(&story).await.unwrap();

        let chat = MockChat::new(vec!["始终垃圾".to_string()]);
        let memo = plan_chapter_memo(
            &chat,
            &PlanChapterMemoInput {
                story_dir: &story,
                book_dir: dir.path(),
                chapter_number: 1,
                is_golden_opening: true,
                fallback_goal: "兜底目标",
                chapter_summaries_raw: "",
                previous_ending_excerpt: None,
                brief: None,
                chapter_context: None,
                recyclable_hooks: &[],
                language: WritingLanguage::Zh,
                chapter_word_count: 3000,
            },
        )
        .await
        .unwrap();
        assert_eq!(memo.goal, "兜底目标");
        assert!(memo.body.contains("## Planner warning"));
        assert_eq!(chat.calls.lock().unwrap().len(), MEMO_RETRY_LIMIT);

        // en 版 fallback。
        let chat_en = MockChat::new(vec!["junk".to_string()]);
        let memo_en = plan_chapter_memo(
            &chat_en,
            &PlanChapterMemoInput {
                story_dir: &story,
                book_dir: dir.path(),
                chapter_number: 1,
                is_golden_opening: false,
                fallback_goal: "",
                chapter_summaries_raw: "",
                previous_ending_excerpt: None,
                brief: None,
                chapter_context: None,
                recyclable_hooks: &[],
                language: WritingLanguage::En,
                chapter_word_count: 3000,
            },
        )
        .await
        .unwrap();
        // 兜底 goal 51 字，超 50 上限 → make_display_goal 截 47 码元 + "..."。
        assert_eq!(memo_en.goal, "Continue chapter 1 according to the current out...");
        assert!(memo_en.body.contains("The model failed to produce a valid chapter memo"));
    }

    #[test]
    fn fallback_memo_markdown_matches_parser() {
        let markdown = build_fallback_memo_markdown(&FallbackMemoInput {
            chapter_number: 4,
            is_golden_opening: false,
            fallback_goal: "目标",
            error_message: "缺小节",
            language: WritingLanguage::Zh,
            length_spec: &LengthSpec {
                target: 3000,
                soft_min: 2250,
                soft_max: 3750,
                hard_min: 1500,
                hard_max: 4500,
                counting_mode: crate::models::length_governance::LengthCountingMode::ZhChars,
            },
        });
        let memo = parse_memo(&markdown, 4, false).expect("fallback memo 应可解析");
        assert_eq!(memo.goal, "目标");
        assert!(memo.body.contains("advance: 推进当前活跃承诺"));
    }

    use crate::models::input_governance::ChapterIntent;
}
