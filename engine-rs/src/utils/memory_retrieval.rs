//! 记忆检索（planner 的数据供给层核心）。
//!
//! 移植自 `packages/core/src/utils/memory-retrieval.ts`（530 行）。
//! [`retrieve_memory_selection`] 是 planner/composer 共用的记忆装配入口：
//! 结构化 state/*.json 优先、markdown 真相文件兜底、SQLite memory.db 加速
//! 摘要与事实（hook 走权威路径不走 DB——DB 表只存小子集，保不住
//! promoted/core/dependency 元数据）。
//!
//! 纯函数（golden 差分守门）：
//! - [`compute_recyclable_hooks`]：陈旧 hook 回收判定（阈值随状态/core 变化）
//! - [`extract_query_terms`]：goal/outline/mustKeep → 检索词
//! - [`render_summary_snapshot`] 复用自 [`crate::utils::story_markdown`]
//!
//! ## 移植纪律
//! - TS `StoredHook.status` 是**原始字符串**（"pressured"/"near_payoff" 等非枚举值
//!   直接参与回收阈值/终态判定）→ Rust 经 [`HookRecord::status_raw`] +
//!   [`crate::utils::hook_lifecycle::hook_status_text`] 还原同语义
//! - 中文焦点词提取的 `slice(-size)` 后缀窗口按 UTF-16 语义（CJK BMP 与
//!   chars 等价）；`extractChineseFocusTerms` 的 `(A|B|...)+` 重复剥离组逐字移植
//! - JS `\b` 是 ASCII 词边界 → Rust 侧 `(?-u:\b)`（Rust 默认 \b 是 Unicode
//!   词边界，CJK 邻接 ASCII 处行为不同，必须显式关 unicode）
//! - Set 去重保插入序 → Vec + HashSet；`Array.prototype.sort` 稳定 → `sort_by`

use std::collections::HashSet;
use std::path::Path;

use regex::Regex;
use serde::de::DeserializeOwned;
use std::sync::OnceLock;

use crate::models::runtime_state::{
    ChapterSummariesState, ChapterSummaryRow, CurrentStateState, HookRecord, HooksState,
};
use crate::state::memory_db::{MemoryDb, NewFact, StoredSummary};
use crate::state::state_bootstrap::bootstrap_structured_state_from_markdown;
use crate::state::store::FsStateStore;
use crate::utils::hook_lifecycle::{
    filter_active_hooks, hook_status_text, is_future_planned_hook, is_hook_within_chapter_window,
    DEFAULT_HOOK_LOOKAHEAD_CHAPTERS,
};
use crate::utils::outline_paths::read_current_state_with_fallback;
use crate::utils::story_markdown::{
    parse_chapter_summaries_markdown, parse_current_state_facts, parse_pending_hooks_markdown,
};

/// 检索结果集。对齐 TS `MemorySelection`。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MemorySelection {
    pub summaries: Vec<StoredSummary>,
    pub hooks: Vec<HookRecord>,
    pub active_hooks: Vec<HookRecord>,
    /// 有回收压力的陈旧 hook（planner 必须在本章 advance/resolve/defer）。
    /// 按沉默章数 DESC 排序（最逾期的排最前）。
    pub recyclable_hooks: Vec<HookRecord>,
    pub facts: Vec<NewFact>,
    pub volume_summaries: Vec<VolumeSummarySelection>,
    pub db_path: Option<String>,
}

/// 卷摘要选段。对齐 TS `VolumeSummarySelection`。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VolumeSummarySelection {
    pub heading: String,
    pub content: String,
    pub anchor: String,
}

/// 检索入参。对齐 TS `retrieveMemorySelection` 的参数对象。
pub struct RetrieveMemoryParams<'a> {
    pub book_dir: &'a Path,
    pub chapter_number: u32,
    pub goal: &'a str,
    pub outline_node: Option<&'a str>,
    pub must_keep: &'a [String],
}

/// 装配 planner 记忆选集：结构化状态优先 → markdown 兜底 → SQLite 加速。
///
/// 副作用（对齐 TS）：bootstrap 结构化状态；memory.db 为空时回填摘要/事实。
pub async fn retrieve_memory_selection(params: &RetrieveMemoryParams<'_>) -> MemorySelection {
    let story_dir = params.book_dir.join("story");
    let state_dir = story_dir.join("state");
    let fallback_chapter = params.chapter_number.saturating_sub(1);

    // TS：bootstrapStructuredStateFromMarkdown(...).catch(() => undefined)
    let book_dir_str = params.book_dir.to_string_lossy().into_owned();
    let _ = bootstrap_structured_state_from_markdown(
        &FsStateStore,
        &book_dir_str,
        Some(fallback_chapter),
    )
    .await;

    let current_state_path = state_dir.join("current_state.json");
    let hooks_state_path = state_dir.join("hooks.json");
    let summaries_state_path = state_dir.join("chapter_summaries.json");
    let pending_hooks_path = story_dir.join("pending_hooks.md");
    let volume_summaries_path = story_dir.join("volume_summaries.md");
    let (
        current_state_markdown,
        hooks_markdown,
        volume_summaries_markdown,
        structured_current_state,
        structured_hooks,
        structured_summaries,
    ) = tokio::join!(
        read_current_state_with_fallback(params.book_dir, ""),
        read_file_or_empty(&pending_hooks_path),
        read_file_or_empty(&volume_summaries_path),
        read_structured_state::<CurrentStateState>(&current_state_path),
        read_structured_state::<HooksState>(&hooks_state_path),
        read_structured_state::<ChapterSummariesState>(&summaries_state_path),
    );

    let facts: Vec<NewFact> = structured_current_state
        .map(|state| {
            state
                .facts
                .into_iter()
                .map(|fact| NewFact {
                    subject: fact.subject,
                    predicate: fact.predicate,
                    object: fact.object,
                    valid_from_chapter: i64::from(fact.valid_from_chapter),
                    valid_until_chapter: fact.valid_until_chapter.map(i64::from),
                    source_chapter: i64::from(fact.source_chapter),
                })
                .collect()
        })
        .unwrap_or_else(|| {
            parse_current_state_facts(&current_state_markdown, i64::from(fallback_chapter))
        });

    let narrative_query_terms =
        extract_query_terms(params.goal, params.outline_node, &[]);
    let fact_query_terms =
        extract_query_terms(params.goal, params.outline_node, params.must_keep);
    let volume_summaries = select_relevant_volume_summaries(
        &parse_volume_summaries_markdown(&volume_summaries_markdown),
        &narrative_query_terms,
    );

    // hook 走权威路径（结构化 hooks.json / pending_hooks.md），不走 SQLite——
    // DB 表只存小子集，保不住 promoted/core/dependency 元数据。
    let hooks = structured_hooks
        .map(|state| state.hooks)
        .unwrap_or_else(|| parse_pending_hooks_markdown(&hooks_markdown));
    let active_hooks = filter_active_hooks(&hooks);

    if let Ok(memory_db) = MemoryDb::open(params.book_dir) {
        let selection = assemble_db_selection(
            &memory_db,
            params,
            &active_hooks,
            &facts,
            &narrative_query_terms,
            &fact_query_terms,
            &structured_summaries,
            &story_dir,
            volume_summaries,
        )
        .await;
        let _ = memory_db.close().ok();
        return selection;
    }

    let summaries_markdown =
        read_file_or_empty(&story_dir.join("chapter_summaries.md")).await;
    let summaries = structured_summaries
        .map(|state| state.rows.iter().map(summary_from_row).collect())
        .unwrap_or_else(|| parse_chapter_summaries_markdown(&summaries_markdown));

    MemorySelection {
        summaries: select_relevant_summaries(&summaries, params.chapter_number, &narrative_query_terms),
        hooks: select_relevant_hooks(&active_hooks, &narrative_query_terms, params.chapter_number),
        active_hooks: active_hooks.clone(),
        recyclable_hooks: compute_recyclable_hooks(&active_hooks, params.chapter_number),
        facts: select_relevant_facts(&facts, &fact_query_terms),
        volume_summaries,
        db_path: None,
    }
}

/// DB 路径的选集装配：空库回填 + 摘要窗口检索 + dbPath 标注。
#[allow(clippy::too_many_arguments)]
async fn assemble_db_selection(
    memory_db: &MemoryDb,
    params: &RetrieveMemoryParams<'_>,
    active_hooks: &[HookRecord],
    facts: &[NewFact],
    narrative_query_terms: &[String],
    fact_query_terms: &[String],
    structured_summaries: &Option<ChapterSummariesState>,
    story_dir: &Path,
    volume_summaries: Vec<VolumeSummarySelection>,
) -> MemorySelection {
    // 空库回填：摘要来自结构化 rows，缺失时读 markdown 真相文件。
    if memory_db.get_chapter_count().unwrap_or(0) == 0 {
        let summaries: Vec<StoredSummary> = if let Some(state) = structured_summaries {
            state.rows.iter().map(summary_from_row).collect()
        } else {
            let markdown = read_file_or_empty(&story_dir.join("chapter_summaries.md")).await;
            parse_chapter_summaries_markdown(&markdown)
        };
        if !summaries.is_empty() {
            let _ = memory_db.replace_summaries(&summaries);
        }
    }
    if memory_db.get_current_facts().map(|f| f.is_empty()).unwrap_or(true) && !facts.is_empty() {
        let _ = memory_db.replace_current_facts(facts);
    }

    // 结构化/markdown hook 状态是权威（元数据完整）；迁移/极简项目里 SQLite
    // 可能已有可用行而权威路径为空，此时才回退 DB。
    let effective_active_hooks: Vec<HookRecord> = if active_hooks.is_empty() {
        let db_hooks = memory_db.get_active_hooks().unwrap_or_default();
        filter_active_hooks(
            &db_hooks.iter().map(hook_record_from_db_row).collect::<Vec<_>>(),
        )
    } else {
        active_hooks.to_vec()
    };

    let db_summaries = memory_db
        .get_summaries(1, i64::from(params.chapter_number.saturating_sub(1)).max(1))
        .unwrap_or_default();
    let db_facts: Vec<NewFact> = memory_db
        .get_current_facts()
        .unwrap_or_default()
        .into_iter()
        .map(|fact| NewFact {
            subject: fact.subject,
            predicate: fact.predicate,
            object: fact.object,
            valid_from_chapter: fact.valid_from_chapter,
            valid_until_chapter: fact.valid_until_chapter,
            source_chapter: fact.source_chapter,
        })
        .collect();

    MemorySelection {
        summaries: select_relevant_summaries(&db_summaries, params.chapter_number, narrative_query_terms),
        hooks: select_relevant_hooks(&effective_active_hooks, narrative_query_terms, params.chapter_number),
        active_hooks: effective_active_hooks.clone(),
        recyclable_hooks: compute_recyclable_hooks(&effective_active_hooks, params.chapter_number),
        facts: select_relevant_facts(&db_facts, fact_query_terms),
        volume_summaries,
        db_path: Some(story_dir.join("memory.db").to_string_lossy().into_owned()),
    }
}

// ---- 陈旧 hook 回收判定（纯函数，golden 守门） ----

/// 有回收压力的 hook（planner 必须本章处置）。
///
/// 阈值：pressured/near_payoff/progressing 系沉默 ≥ 5 章；coreHook 沉默 ≥ 8 章；
/// 其余 ≥ 10 章。未来计划 hook 排除。按沉默 DESC、startChapter ASC 排序。
pub fn compute_recyclable_hooks(hooks: &[HookRecord], chapter_number: u32) -> Vec<HookRecord> {
    let mut scored: Vec<(HookRecord, i64)> = hooks
        .iter()
        .filter(|hook| !is_recycle_terminal_status(hook_status_text(hook)))
        .filter(|hook| !is_future_planned_hook(hook, chapter_number, DEFAULT_HOOK_LOOKAHEAD_CHAPTERS))
        .map(|hook| {
            let silence = hook_silence(hook, chapter_number);
            (hook.clone(), silence)
        })
        .filter(|(hook, silence)| *silence >= i64::from(recycle_threshold(hook)))
        .collect();

    scored.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then(a.0.start_chapter.cmp(&b.0.start_chapter))
    });
    scored.into_iter().map(|(hook, _)| hook).collect()
}

fn is_recycle_terminal_status(status: &str) -> bool {
    recycle_terminal_re().is_match(status.trim())
}

fn recycle_terminal_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"(?i)^(resolved|closed|done|已回收|已解决|deferred|paused|hold|延后|延期|搁置|暂缓)$")
            .unwrap()
    })
}

fn hook_silence(hook: &HookRecord, chapter_number: u32) -> i64 {
    let last_touch = hook.start_chapter.max(hook.last_advanced_chapter);
    if last_touch == 0 {
        return i64::from(chapter_number);
    }
    i64::from(chapter_number).saturating_sub(i64::from(last_touch)).max(0)
}

fn recycle_threshold(hook: &HookRecord) -> u32 {
    let status = hook_status_text(hook).trim().to_lowercase();
    if pressured_re().is_match(&status) {
        return 5;
    }
    if hook.core_hook == Some(true) {
        return 8;
    }
    10
}

fn pressured_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"pressured|near[_\s-]?payoff|progressing|重大推进|持续推进").unwrap())
}

// ---- 检索词提取（纯函数，golden 守门） ----

/// goal + mustKeep 为主，不足 2 词时并入 outlineNode，上限 12 词。
pub fn extract_query_terms(
    goal: &str,
    outline_node: Option<&str>,
    must_keep: &[String],
) -> Vec<String> {
    let mut primary = unique_terms(&extract_terms_from_text(&strip_negative_guidance(goal)));
    for item in must_keep {
        primary.extend(extract_terms_from_text(item));
    }
    let primary = unique_terms(&primary);

    if primary.len() >= 2 {
        return primary.into_iter().take(12).collect();
    }

    let mut merged = primary;
    merged.extend(extract_terms_from_text(&strip_negative_guidance(
        outline_node.unwrap_or(""),
    )));
    unique_terms(&merged).into_iter().take(12).collect()
}

fn extract_terms_from_text(text: &str) -> Vec<String> {
    if text.trim().is_empty() {
        return Vec::new();
    }

    let normalized = chapter_ref_re().replace_all(text, " ");

    let english: Vec<String> = english_word_re()
        .find_iter(&normalized)
        .map(|m| m.as_str().trim().to_string())
        .filter(|term| term.chars().count() >= 2)
        .filter(|term| !STOP_WORDS.contains(&term.to_lowercase().as_str()))
        .collect();

    let chinese: Vec<String> = cjk_segment_re()
        .find_iter(&normalized)
        .flat_map(|m| extract_chinese_focus_terms(m.as_str()))
        .collect();

    let mut terms = english;
    terms.extend(chinese);
    terms
}

/// 中文焦点词：剥离引导前缀（重复组），目标 ≤4 字整词 + 2..4 字尾部后缀窗口。
fn extract_chinese_focus_terms(segment: &str) -> Vec<String> {
    let after_first = focus_prefix_re().replace_all(segment, "");
    let stripped = focus_prefix2_re().replace_all(&after_first, "");
    let stripped = stripped.trim().to_string();

    let target: &str = if stripped.chars().count() >= 2 {
        &stripped
    } else {
        segment
    };

    let target_len = target.chars().count();
    let mut terms: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let push = |value: String, terms: &mut Vec<String>, seen: &mut HashSet<String>| {
        if value.chars().count() >= 2 && seen.insert(value.clone()) {
            terms.push(value);
        }
    };

    if target_len <= 4 {
        push(target.to_string(), &mut terms, &mut seen);
    }
    for size in 2..=4usize {
        if target_len >= size {
            let suffix: String = target.chars().skip(target_len - size).collect();
            push(suffix, &mut terms, &mut seen);
        }
    }
    terms
}

fn strip_negative_guidance(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    let after_en = negative_guidance_en_re().replace(text, " ");
    let after_zh = negative_guidance_zh_re().replace(&after_en, " ");
    after_zh.trim().to_string()
}

fn unique_terms(terms: &[String]) -> Vec<String> {
    let mut result: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for term in terms {
        let normalized = term.trim().to_lowercase();
        if normalized.is_empty() || seen.contains(&normalized) {
            continue;
        }
        seen.insert(normalized);
        result.push(term.trim().to_string());
    }
    result
}

// ---- 卷摘要解析 ----

fn parse_volume_summaries_markdown(markdown: &str) -> Vec<VolumeSummarySelection> {
    if markdown.trim().is_empty() {
        return Vec::new();
    }

    volume_section_re()
        .split(markdown)
        .map(|section| section.trim())
        .filter(|section| !section.is_empty())
        .map(|section| {
            let mut lines = section.split('\n');
            let heading = lines.next().unwrap_or("").trim().to_string();
            let content = lines.collect::<Vec<_>>().join("\n").trim().to_string();
            let anchor = slugify_anchor(&heading);
            VolumeSummarySelection {
                heading,
                content,
                anchor,
            }
        })
        .filter(|section| !section.heading.is_empty() && !section.content.is_empty())
        .collect()
}

fn slugify_anchor(value: &str) -> String {
    let lower = value.trim().to_lowercase();
    let dashed = anchor_non_word_re().replace_all(&lower, "-");
    let trimmed = anchor_edge_dash_re().replace_all(&dashed, "");
    let result = trimmed.to_string();
    if result.is_empty() {
        "volume-summary".to_string()
    } else {
        result
    }
}

// ---- 相关性选择（私有，Rust 单测镜像 TS 行为） ----

fn is_unresolved_hook(status: &str) -> bool {
    status.trim().is_empty() || unresolved_status_re().is_match(status)
}

fn select_relevant_summaries(
    summaries: &[StoredSummary],
    chapter_number: u32,
    query_terms: &[String],
) -> Vec<StoredSummary> {
    let chapter_number = i64::from(chapter_number);
    let mut ranked: Vec<(StoredSummary, i64, bool)> = summaries
        .iter()
        .filter(|summary| summary.chapter < chapter_number)
        .map(|summary| {
            let text = summary_text(summary);
            (
                summary.clone(),
                score_summary(summary, chapter_number, query_terms, &text),
                matches_any(&text, query_terms),
            )
        })
        .filter(|(summary, _, matched)| *matched || summary.chapter >= chapter_number - 3)
        .collect();

    ranked.sort_by(|left, right| {
        right
            .1
            .cmp(&left.1)
            .then(right.0.chapter.cmp(&left.0.chapter))
    });
    ranked.truncate(4);
    ranked.sort_by(|left, right| left.0.chapter.cmp(&right.0.chapter));
    ranked.into_iter().map(|(summary, _, _)| summary).collect()
}

fn select_relevant_hooks(
    hooks: &[HookRecord],
    query_terms: &[String],
    chapter_number: u32,
) -> Vec<HookRecord> {
    #[derive(Clone)]
    struct Ranked {
        hook: HookRecord,
        score: i64,
        matched: bool,
    }

    let ranked: Vec<Ranked> = hooks
        .iter()
        .map(|hook| Ranked {
            score: score_hook(hook, query_terms),
            matched: matches_any(&hook_text(hook), query_terms),
            hook: hook.clone(),
        })
        .filter(|entry| entry.matched || is_unresolved_hook(hook_status_text(&entry.hook)))
        .collect();

    let mut primary: Vec<Ranked> = ranked
        .iter()
        .filter(|entry| {
            entry.matched
                || is_hook_within_chapter_window(&entry.hook, chapter_number, 5, DEFAULT_HOOK_LOOKAHEAD_CHAPTERS)
        })
        .cloned()
        .collect();
    primary.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then(
                right
                    .hook
                    .last_advanced_chapter
                    .cmp(&left.hook.last_advanced_chapter),
            )
    });
    primary.truncate(6);

    let selected_ids: HashSet<String> = primary.iter().map(|e| e.hook.hook_id.clone()).collect();
    let mut stale: Vec<Ranked> = ranked
        .into_iter()
        .filter(|entry| {
            !selected_ids.contains(&entry.hook.hook_id)
                && !is_future_planned_hook(&entry.hook, chapter_number, DEFAULT_HOOK_LOOKAHEAD_CHAPTERS)
                && is_unresolved_hook(hook_status_text(&entry.hook))
        })
        .collect();
    stale.sort_by(|left, right| {
        left.hook
            .last_advanced_chapter
            .cmp(&right.hook.last_advanced_chapter)
            .then(right.score.cmp(&left.score))
    });
    stale.truncate(2);

    primary
        .into_iter()
        .chain(stale)
        .map(|entry| entry.hook)
        .collect()
}

fn select_relevant_facts(facts: &[NewFact], query_terms: &[String]) -> Vec<NewFact> {
    let mut ranked: Vec<(NewFact, i64, bool)> = facts
        .iter()
        .map(|fact| {
            let text = format!("{} {} {}", fact.subject, fact.predicate, fact.object);
            let priority = prioritized_predicate_index(&fact.predicate);
            let base_score = match priority {
                Some(index) => 20 - 2 * index as i64,
                None => 5,
            };
            let term_score: i64 = query_terms
                .iter()
                .map(|term| {
                    if includes_term(&text, term) {
                        std::cmp::max(8, (term.chars().count() as i64) * 2)
                    } else {
                        0
                    }
                })
                .sum();
            (
                fact.clone(),
                base_score + term_score,
                matches_any(&text, query_terms),
            )
        })
        .filter(|(_, score, matched)| *matched || *score >= 14)
        .collect();

    ranked.sort_by(|left, right| right.1.cmp(&left.1));
    ranked.truncate(4);
    ranked.into_iter().map(|(fact, _, _)| fact).collect()
}

fn select_relevant_volume_summaries(
    summaries: &[VolumeSummarySelection],
    query_terms: &[String],
) -> Vec<VolumeSummarySelection> {
    if summaries.is_empty() {
        return Vec::new();
    }

    #[derive(Clone)]
    struct Ranked {
        index: usize,
        summary: VolumeSummarySelection,
        score: i64,
        matched: bool,
    }

    let ranked: Vec<Ranked> = summaries
        .iter()
        .enumerate()
        .map(|(index, summary)| {
            let text = format!("{} {}", summary.heading, summary.content);
            let term_score: i64 = query_terms
                .iter()
                .map(|term| {
                    if includes_term(&text, term) {
                        std::cmp::max(8, (term.chars().count() as i64) * 2)
                    } else {
                        0
                    }
                })
                .sum();
            Ranked {
                index,
                summary: summary.clone(),
                score: term_score + index as i64,
                matched: matches_any(&text, query_terms),
            }
        })
        .collect();

    let last_index = ranked.len().saturating_sub(1);
    let mut selected: Vec<Ranked> = ranked
        .into_iter()
        .enumerate()
        .filter(|(position, entry)| entry.matched || *position == last_index)
        .map(|(_, entry)| entry)
        .collect();
    selected.sort_by(|left, right| right.score.cmp(&left.score));
    selected.truncate(2);
    selected.sort_by(|left, right| left.index.cmp(&right.index));
    selected.into_iter().map(|entry| entry.summary).collect()
}

fn prioritized_predicate_index(predicate: &str) -> Option<usize> {
    const PATTERNS: [&str; 6] = [
        "当前冲突", "当前目标", "主角状态", "当前限制", "当前位置", "当前敌我",
    ];
    let trimmed = predicate.trim();
    let lower = trimmed.to_lowercase();
    for (index, canonical) in PATTERNS.iter().enumerate() {
        if lower == *canonical {
            return Some(index);
        }
    }
    // 英文别名（TS 正则 ^(current conflict)$ 等，大小写不敏感）
    const EN_ALIASES: [&str; 6] = [
        "current conflict",
        "current goal",
        "protagonist state",
        "current constraint",
        "current location",
        "",
    ];
    for (index, alias) in EN_ALIASES.iter().enumerate() {
        if !alias.is_empty() && lower == *alias {
            return Some(index);
        }
    }
    // 当前敌我/盟友（TS：^(当前敌我|current alliances|current relationships)$）
    if lower == "current alliances" || lower == "current relationships" {
        return Some(5);
    }
    None
}

fn score_summary(
    summary: &StoredSummary,
    chapter_number: i64,
    query_terms: &[String],
    text: &str,
) -> i64 {
    let age = (chapter_number - summary.chapter).max(0);
    let recency_score = (12 - age).max(0);
    let term_score: i64 = query_terms
        .iter()
        .map(|term| {
            if includes_term(text, term) {
                std::cmp::max(8, (term.chars().count() as i64) * 2)
            } else {
                0
            }
        })
        .sum();
    recency_score + term_score
}

fn score_hook(hook: &HookRecord, query_terms: &[String]) -> i64 {
    let text = hook_text(hook);
    let freshness = i64::from(hook.last_advanced_chapter);
    let term_score: i64 = query_terms
        .iter()
        .map(|term| {
            if includes_term(&text, term) {
                std::cmp::max(8, (term.chars().count() as i64) * 2)
            } else {
                0
            }
        })
        .sum();
    term_score + freshness
}

fn matches_any(text: &str, query_terms: &[String]) -> bool {
    query_terms
        .iter()
        .any(|term| includes_term(text, term))
}

fn includes_term(text: &str, term: &str) -> bool {
    text.to_lowercase().contains(&term.to_lowercase())
}

fn summary_text(summary: &StoredSummary) -> String {
    [
        summary.title.as_str(),
        summary.characters.as_str(),
        summary.events.as_str(),
        summary.state_changes.as_str(),
        summary.hook_activity.as_str(),
        summary.chapter_type.as_str(),
    ]
    .join(" ")
}

fn hook_text(hook: &HookRecord) -> String {
    let timing = hook
        .payoff_timing
        .map(crate::utils::hook_lifecycle::hook_payoff_timing_canonical)
        .unwrap_or_default();
    [
        hook.hook_id.as_str(),
        hook.hook_type.as_str(),
        hook.expected_payoff.as_str(),
        timing,
        hook.notes.as_str(),
    ]
    .join(" ")
}

fn summary_from_row(row: &ChapterSummaryRow) -> StoredSummary {
    StoredSummary {
        chapter: i64::from(row.chapter),
        title: row.title.clone(),
        characters: row.characters.clone(),
        events: row.events.clone(),
        state_changes: row.state_changes.clone(),
        hook_activity: row.hook_activity.clone(),
        mood: row.mood.clone(),
        chapter_type: row.chapter_type.clone(),
    }
}

/// DB 8 列行 → 全量记录（元数据缺省，status 原文入 status_raw）。
fn hook_record_from_db_row(hook: &crate::state::memory_db::StoredHook) -> HookRecord {
    HookRecord {
        hook_id: hook.hook_id.clone(),
        start_chapter: hook.start_chapter.max(0) as u32,
        hook_type: hook.r#type.clone(),
        status: crate::utils::hook_lifecycle::normalize_stored_hook_status(&hook.status),
        status_raw: hook.status.clone(),
        last_advanced_chapter: hook.last_advanced_chapter.max(0) as u32,
        expected_payoff: hook.expected_payoff.clone(),
        payoff_timing: crate::utils::hook_lifecycle::normalize_hook_payoff_timing(
            if hook.payoff_timing.is_empty() {
                None
            } else {
                Some(hook.payoff_timing.as_str())
            },
        ),
        notes: hook.notes.clone(),
        depends_on: None,
        pays_off_in_arc: None,
        core_hook: None,
        half_life_chapters: None,
        advanced_count: None,
        promoted: None,
    }
}

async fn read_file_or_empty(path: &Path) -> String {
    tokio::fs::read_to_string(path).await.unwrap_or_default()
}

async fn read_structured_state<T: DeserializeOwned>(path: &Path) -> Option<T> {
    let raw = tokio::fs::read_to_string(path).await.ok()?;
    serde_json::from_str(&raw).ok()
}

// ---- 静态正则 / 停用词 ----

const STOP_WORDS: &[&str] = &[
    "bring", "focus", "back", "chapter", "clear", "narrative", "before", "opening",
    "track", "the", "with", "from", "that", "this", "into", "still", "cannot",
    "current", "state", "advance", "conflict", "story", "keep", "must", "local",
    "does", "not", "only", "just", "then", "than",
];

fn chapter_ref_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"第\d+章").unwrap())
}

fn english_word_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)[a-z]{4,}").unwrap())
}

fn cjk_segment_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"[\x{4e00}-\x{9fff}]{2,}").unwrap())
}

fn focus_prefix_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"^(?:本章|继续|重新|拉回|回到|推进|优先|围绕|聚焦|坚持|保持|把注意力|注意力|将注意力|请把注意力|先把注意力)+")
            .unwrap()
    })
}

fn focus_prefix2_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^(?:处理|推进|回拉|拉回到)+").unwrap())
}

fn negative_guidance_en_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    // JS \b 为 ASCII 词边界（\w 不含 CJK）；Rust 默认 \b 是 Unicode 边界，
    // 必须显式 (?-u:\b) 才能得到「CJK 紧邻 ASCII 词首也判边界」的 JS 行为。
    R.get_or_init(|| {
        Regex::new(r"(?i)(?-u:\b(?:do not|don't|avoid|without|instead of)(?-u:\b))[\s\S]*$")
            .unwrap()
    })
}

fn negative_guidance_zh_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?:不要|不让|别|禁止|避免|但不允许)[\s\S]*$").unwrap())
}

fn unresolved_status_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)open|待定|推进|active|progressing").unwrap())
}

fn volume_section_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?m)^##\s+").unwrap())
}

fn anchor_non_word_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"[^a-z0-9\x{4e00}-\x{9fff}]+").unwrap())
}

fn anchor_edge_dash_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^-+|-+$").unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::runtime_state::HookStatus;

    fn hook(id: &str, start: u32, last: u32, status: HookStatus, status_raw: &str) -> HookRecord {
        HookRecord {
            hook_id: id.into(),
            start_chapter: start,
            hook_type: "plot".into(),
            status,
            status_raw: status_raw.into(),
            last_advanced_chapter: last,
            expected_payoff: String::new(),
            payoff_timing: None,
            notes: String::new(),
            depends_on: None,
            pays_off_in_arc: None,
            core_hook: None,
            half_life_chapters: None,
            advanced_count: None,
            promoted: None,
        }
    }

    #[test]
    fn recycle_threshold_pressured_hits_at_five() {
        // pressured：沉默 5 章即回收（TS recycleThreshold → 5）。
        let pressured = hook("H01", 1, 4, HookStatus::Open, "pressured");
        assert_eq!(compute_recyclable_hooks(std::slice::from_ref(&pressured), 9).len(), 1);
        assert!(compute_recyclable_hooks(&[pressured], 8).is_empty());
    }

    #[test]
    fn recycle_threshold_raw_near_payoff_variant() {
        // near_payoff / "near payoff" / near-payoff 三种写法都命中 5 章阈值。
        for raw in ["near_payoff", "near payoff", "near-payoff"] {
            let h = hook("H01", 1, 2, HookStatus::Open, raw);
            assert_eq!(compute_recyclable_hooks(std::slice::from_ref(&h), 7).len(), 1, "{raw}");
        }
    }

    #[test]
    fn recycle_threshold_default_ten_and_core_eight() {
        let open = hook("H01", 1, 0, HookStatus::Open, "open");
        assert!(compute_recyclable_hooks(std::slice::from_ref(&open), 10).is_empty());
        assert_eq!(compute_recyclable_hooks(std::slice::from_ref(&open), 11).len(), 1);

        let mut core = hook("H02", 1, 0, HookStatus::Open, "open");
        core.core_hook = Some(true);
        assert!(compute_recyclable_hooks(std::slice::from_ref(&core), 8).is_empty());
        assert_eq!(compute_recyclable_hooks(std::slice::from_ref(&core), 9).len(), 1);
    }

    #[test]
    fn recycle_excludes_terminal_and_future_hooks_and_orders_by_silence() {
        let terminal = hook("H01", 1, 1, HookStatus::Open, "已解决"); // 终态（原文判定）
        let future = hook("H02", 99, 0, HookStatus::Open, "open"); // 未来计划
        let stale = hook("H03", 1, 1, HookStatus::Open, "open"); // 沉默 13
        let staler = hook("H04", 2, 2, HookStatus::Open, "open"); // 沉默 12

        let out = compute_recyclable_hooks(&[terminal, future, stale, staler], 14);
        let ids: Vec<&str> = out.iter().map(|h| h.hook_id.as_str()).collect();
        assert_eq!(ids, vec!["H03", "H04"]); // 沉默 DESC

        // 沉默相同（15-5=10）时按 startChapter ASC。
        let a = hook("HA", 5, 5, HookStatus::Open, "open");
        let b = hook("HB", 3, 5, HookStatus::Open, "open");
        let out = compute_recyclable_hooks(&[a, b], 15);
        let ids: Vec<&str> = out.iter().map(|h| h.hook_id.as_str()).collect();
        assert_eq!(ids, vec!["HB", "HA"]);
    }

    #[test]
    fn extract_query_terms_chinese_focus_suffixes() {
        let terms = extract_query_terms("本章围绕林动崛起推进", None, &[]);
        // 引导前缀（^锚定的重复组）剥掉「本章围绕」→ 目标「林动崛起推进」(7 字 > 4)：
        // 不加整词，只留 2..4 字尾部窗口。
        assert_eq!(
            terms,
            vec!["推进".to_string(), "起推进".to_string(), "崛起推进".to_string()]
        );
    }

    #[test]
    fn extract_query_terms_english_case_and_stopwords() {
        // 大写保留原样；stopword（focus/keep/must）被滤掉。
        let terms = extract_query_terms("Focus on the Alliance lineage", None, &[]);
        assert_eq!(terms, vec!["Alliance".to_string(), "lineage".to_string()]);
    }

    #[test]
    fn extract_query_terms_falls_back_to_outline_when_primary_sparse() {
        let terms = extract_query_terms("继续", Some("第3章 宗门大比"), &[]);
        assert!(!terms.is_empty());
        // 主词不足 2 时并入 outlineNode 的词（「第N章」被剔除，剩「宗门大比」后缀窗）。
        assert!(terms.contains(&"宗门大比".to_string()));
    }

    #[test]
    fn extract_query_terms_must_keep_contributes_before_outline() {
        let terms = extract_query_terms("", None, &["保持 海上孤舟".into()]);
        // primary 已 ≥2 词，outline 不再并入。「保持」是前缀组词但剥空后
        // target 回退原段（TS `stripped.length >= 2 ? stripped : segment`）→ 保留。
        assert_eq!(
            terms,
            vec![
                "保持".to_string(),
                "海上孤舟".to_string(),
                "孤舟".to_string(),
                "上孤舟".to_string()
            ]
        );
    }

    #[test]
    fn strip_negative_guidance_truncates_at_boundary() {
        // 英文否定引导词截断（\b 语义：CJK 紧邻 ASCII 也算边界）。
        assert_eq!(strip_negative_guidance("守住城池，不要弃城"), "守住城池，");
        assert_eq!(strip_negative_guidance("能不能do not retreat further"), "能不能");
    }

    #[test]
    fn select_relevant_summaries_prefers_recent_and_matched() {
        let summary = |chapter: i64, events: &str| StoredSummary {
            chapter,
            title: format!("第{chapter}章"),
            characters: "林动".into(),
            events: events.into(),
            state_changes: String::new(),
            hook_activity: String::new(),
            mood: "高压".into(),
            chapter_type: "高潮".into(),
        };
        let summaries = vec![
            summary(1, "旧事件"),
            summary(7, "祖符争夺"),
            summary(8, "祖符争夺"),
            summary(9, "日常过渡"),
        ];
        let terms = vec!["祖符".to_string()];
        let out = select_relevant_summaries(&summaries, 10, &terms);
        // 第8章：recency 12-2=10 + 命中 2*2=4 → 最高；第7章次之；
        // 第9章 recency 高未命中但近 3 章内保留；第1章超出近窗且未命中 → 排除。
        let chapters: Vec<i64> = out.iter().map(|s| s.chapter).collect();
        assert_eq!(chapters, vec![7, 8, 9]);
    }

    #[test]
    fn select_relevant_hooks_caps_and_appends_stale() {
        let mut hooks = Vec::new();
        for i in 0..8 {
            let mut h = hook(&format!("H{i:02}"), 1, i, HookStatus::Open, "open");
            h.notes = format!("线索{i}");
            hooks.push(h);
        }
        let out = select_relevant_hooks(&hooks, &["线索".to_string()], 10);
        // 主选 ≤6 + 陈旧 ≤2 = 8 条全部保留；排序按分数（freshness）降序。
        assert_eq!(out.len(), 8);
        assert_eq!(out[0].hook_id, "H07");
    }

    #[test]
    fn parse_volume_summaries_and_slugify() {
        let md = "# 卷摘要\n\n## 第一卷 觉醒\n林动初入宗门。\n\n## 第二卷: 大比\nhostile 氏族登场。\n";
        let sections = parse_volume_summaries_markdown(md);
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].heading, "第一卷 觉醒");
        assert_eq!(sections[0].content, "林动初入宗门。");
        assert_eq!(sections[0].anchor, "第一卷-觉醒");
        assert_eq!(sections[1].anchor, "第二卷-大比");
    }

    #[test]
    fn slugify_anchor_fallback_and_dash_trimming() {
        assert_eq!(slugify_anchor("!!!"), "volume-summary");
        assert_eq!(slugify_anchor("  A  B  "), "a-b");
    }

    #[tokio::test]
    async fn retrieve_memory_selection_markdown_path() {
        let dir = tempfile::tempdir().unwrap();
        let story = dir.path().join("story");
        tokio::fs::create_dir_all(&story).await.unwrap();

        tokio::fs::write(
            story.join("pending_hooks.md"),
            "| hook_id | 起始章节 | 类型 | 状态 | 最近推进 | 预期回收 | 回收节奏 | 上游依赖 | 回收卷 | 核心 | 半衰期 | 升级 | 备注 |\n\
             | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |\n\
             | H01 | 1 | main | pressured | 1 | 10 | near-term | 无 | 一卷 | 是 | 5 | 是 | 祖符 |\n\
             | H02 | 2 | support | resolved | 3 | 20 | slow-burn | 无 | 二卷 | 否 | 8 | 否 | 已结 |\n",
        )
        .await
        .unwrap();
        tokio::fs::write(
            story.join("chapter_summaries.md"),
            "| 章节 | 标题 | 出场人物 | 关键事件 | 状态变化 | 伏笔动态 | 情绪基调 | 章节类型 |\n\
             | --- | --- | --- | --- | --- | --- | --- | --- |\n\
             | 7 | 初入 | 林动 | 祖符觉醒 | 无 | H01 open | 平静 | 开局 |\n",
        )
        .await
        .unwrap();

        let selection = retrieve_memory_selection(&RetrieveMemoryParams {
            book_dir: dir.path(),
            chapter_number: 8,
            goal: "推进祖符线",
            outline_node: None,
            must_keep: &[],
        })
        .await;

        // markdown 路径：hooks 来自 pending_hooks.md，resolved 被滤出活跃集；
        // H01 pressured 沉默 7 章 ≥ 5 → 进入回收集。
        assert_eq!(selection.active_hooks.len(), 1);
        assert_eq!(selection.active_hooks[0].hook_id, "H01");
        assert_eq!(selection.active_hooks[0].status_raw, "pressured");
        assert_eq!(selection.recyclable_hooks.len(), 1);
        assert_eq!(selection.summaries.len(), 1);
        // tempdir 内 SQLite 可开（memory.db 创建于 story/ 下）→ db 分支生效。
        assert!(selection.db_path.is_some());
    }
}
