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
//! - `render_summary_snapshot` 复用自 `crate::utils::story_markdown`
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
#[derive(Debug, Clone, Default)]
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
    /// 244/247 号：BM25 + 语义精选溯源。
    pub retrieval_trace: Option<MemoryRetrievalTrace>,
}

/// 卷摘要选段。对齐 TS `VolumeSummarySelection`。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VolumeSummarySelection {
    pub heading: String,
    pub content: String,
    pub anchor: String,
}

/// 语义精简候选。对齐 TS `MemorySemanticSelectionRequest.candidates` 元素。
#[derive(Debug, Clone)]
pub struct MemoryCandidate {
    pub id: String,
    pub kind: String,
    pub source: String,
    pub title: String,
    pub excerpt: String,
}

/// 语义精简请求。对齐 TS `MemorySemanticSelectionRequest`。
pub struct MemorySemanticSelectionRequest<'a> {
    pub chapter_number: u32,
    pub query: &'a str,
    pub candidates: &'a [MemoryCandidate],
}

/// 语义精简器端口（TS `MemorySemanticSelector`——LLM 从 BM25 候选中精选）。
#[async_trait::async_trait]
pub trait MemorySemanticSelector: Send + Sync {
    async fn select(&self, request: &MemorySemanticSelectionRequest<'_>) -> Result<Vec<String>, String>;
}

/// 检索溯源（TS `MemoryRetrievalTrace` 对应面）。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryRetrievalCandidate {
    pub id: String,
    pub kind: String,
    pub source: String,
    pub score: f64,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryRetrievalTrace {
    pub engine: &'static str,
    pub query: String,
    pub candidates: Vec<MemoryRetrievalCandidate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic_selected_ids: Option<Vec<String>>,
}

/// 检索入参。对齐 TS `retrieveMemorySelection` 的参数对象。
pub struct RetrieveMemoryParams<'a> {
    pub book_dir: &'a Path,
    pub chapter_number: u32,
    pub goal: &'a str,
    pub outline_node: Option<&'a str>,
    pub must_keep: &'a [String],
    /// 241/244 号：语义精简层（TS `memorySemanticSelector`）。None/失败/候选
    /// ≤1 时跳过——BM25 候选直接按确定性优先级使用。
    pub semantic_selector: Option<&'a dyn MemorySemanticSelector>,
}

/// 装配 planner 记忆选集：结构化状态优先 → markdown 兜底 → SQLite 加速。
///
/// 副作用（对齐 TS）：bootstrap 结构化状态；memory.db 为空时回填摘要/事实。
async fn read_file_or_empty(path: &Path) -> String {
    tokio::fs::read_to_string(path).await.unwrap_or_default()
}

const STOP_WORDS: &[&str] = &[
    "bring", "focus", "back", "chapter", "clear", "narrative", "before", "opening",
    "track", "the", "with", "from", "that", "this", "into", "still", "cannot",
    "current", "state", "advance", "conflict", "story", "keep", "must", "local",
    "does", "not", "only", "just", "then", "than",
];

async fn read_structured_state<T: DeserializeOwned>(path: &Path) -> Option<T> {
    let raw = tokio::fs::read_to_string(path).await.ok()?;
    serde_json::from_str(&raw).ok()
}

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

    // 244 号：语义精简层接线——统一检索链（TS retrieveMemorySelection 对应）：
    // 候选文档 → 内存 BM25（top-32）→ 语义精选（可选，失败回退）→ rankScores
    // → 确定性优先级选择。此前 Rust 为词法评分独立实现（无 BM25/语义层）。
    let narrative_query = [params.goal, params.outline_node.unwrap_or("")]
        .iter()
        .filter(|part| !part.is_empty())
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");
    let retrieval_query = if params.must_keep.is_empty() {
        narrative_query
    } else {
        format!("{}\n{}", narrative_query, params.must_keep.join("\n"))
    };

    // hook 走权威路径（结构化 hooks.json / pending_hooks.md），不走 SQLite——
    // DB 表只存小子集，保不住 promoted/core/dependency 元数据。
    let hooks = structured_hooks
        .map(|state| state.hooks)
        .unwrap_or_else(|| parse_pending_hooks_markdown(&hooks_markdown));
    let active_hooks = filter_active_hooks(&hooks);
    // 休眠 architect 种子非活跃债务，但仍是可检索正典（TS searchableHooks）。
    let searchable_hooks: Vec<HookRecord> = hooks
        .iter()
        .filter(|hook| !is_recycle_terminal_status(hook_status_text(hook)))
        .cloned()
        .collect();

    // Send 纪律：rusqlite Connection 非 Sync——不得跨 await 持有。
    // markdown 预读提前到 DB 打开之前，DB 分支收敛为纯同步段。
    let summaries_markdown =
        read_file_or_empty(&story_dir.join("chapter_summaries.md")).await;

    // MemoryDb 副作用（对齐 TS）：空库回填摘要/事实（DB 为可重建投影）。
    if let Ok(memory_db) = MemoryDb::open(params.book_dir) {
        if memory_db.get_chapter_count().unwrap_or(0) == 0 {
            let backfill: Vec<StoredSummary> = if let Some(state) = &structured_summaries {
                state.rows.iter().map(summary_from_row).collect()
            } else {
                parse_chapter_summaries_markdown(&summaries_markdown)
            };
            if !backfill.is_empty() {
                let _ = memory_db.replace_summaries(&backfill);
            }
        }
        if memory_db.get_current_facts().map(|f| f.is_empty()).unwrap_or(true) && !facts.is_empty()
        {
            let _ = memory_db.replace_current_facts(&facts);
        }
        memory_db.close().ok();
    }

    let summaries = structured_summaries
        .map(|state| state.rows.iter().map(summary_from_row).collect())
        .unwrap_or_else(|| parse_chapter_summaries_markdown(&summaries_markdown));

    let volume_summaries = parse_volume_summaries_markdown(&volume_summaries_markdown);
    let documents = build_memory_search_documents(
        &summaries,
        &searchable_hooks,
        &facts,
        &volume_summaries,
    );
    let index = crate::utils::local_search::LocalSearchIndex::new(":memory:").ok();
    if let Some(index) = &index {
        let _ = index.replace_scope(STORY_MEMORY_SCOPE, &documents);
    }
    let hits = match &index {
        Some(index) => index.search(
            &retrieval_query,
            &crate::utils::local_search::SearchOptions {
                scope: STORY_MEMORY_SCOPE,
                kinds: &[],
                limit: 32,
            },
        ),
        None => Vec::new(),
    };

    // 语义精选：selector 缺失/候选 ≤1/失败 → None（回退 BM25 全量）。
    let semantic_selected = select_semantic_candidate_ids(
        params.semantic_selector,
        params.chapter_number,
        &retrieval_query,
        &hits,
    )
    .await;
    // 溯源在 hits 被 ranked_hits 消耗前快照（TS retrievalTrace.candidates 全量候选）。
    let trace_candidates: Vec<MemoryRetrievalCandidate> = hits
        .iter()
        .map(|hit| MemoryRetrievalCandidate {
            id: hit.id.clone(),
            kind: hit.kind.clone(),
            source: hit.source.clone(),
            score: hit.score,
        })
        .collect();
    let ranked_hits: Vec<crate::utils::local_search::SearchHit> = match &semantic_selected {
        Some(selected) => {
            let set: std::collections::HashSet<&String> = selected.iter().collect();
            hits.into_iter().filter(|hit| set.contains(&hit.id)).collect()
        }
        None => hits,
    };
    let rank_scores = build_rank_scores(&ranked_hits);

    MemorySelection {
        summaries: select_relevant_summaries(&summaries, params.chapter_number, &rank_scores),
        hooks: select_relevant_hooks(
            &searchable_hooks,
            &active_hooks,
            &rank_scores,
            params.chapter_number,
        ),
        active_hooks: active_hooks.clone(),
        recyclable_hooks: compute_recyclable_hooks(&active_hooks, params.chapter_number),
        facts: select_relevant_facts(&facts, &rank_scores),
        volume_summaries: select_relevant_volume_summaries(&volume_summaries, &rank_scores),
        db_path: index
            .as_ref()
            .map(|_| format!("{}/story/memory.db", params.book_dir.display())),
        retrieval_trace: index.as_ref().map(|_| MemoryRetrievalTrace {
            engine: "sqlite-fts5-bm25",
            query: retrieval_query.clone(),
            candidates: trace_candidates,
            semantic_selected_ids: semantic_selected.clone(),
        }),
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

// ---- 语义精简层与 rankScores（244 号，TS memory-retrieval 292-460 对应） ----

const STORY_MEMORY_SCOPE: &str = "story-memory";

fn summary_document_id(chapter: i64) -> String {
    format!("summary:{chapter}")
}

fn hook_document_id(hook_id: &str) -> String {
    format!("hook:{hook_id}")
}

fn fact_document_id(index: usize) -> String {
    format!("fact:{index}")
}

fn volume_summary_document_id(index: usize) -> String {
    format!("volume-summary:{index}")
}

fn to_fact_source_anchor(value: &str) -> String {
    let slug: String = value
        .trim()
        .chars()
        .map(|c| if c.is_whitespace() { '-' } else { c })
        .collect();
    if slug.is_empty() {
        "fact".to_string()
    } else {
        slug
    }
}

/// TS `buildMemorySearchDocuments`：四类记忆 → 检索文档。
fn build_memory_search_documents(
    summaries: &[StoredSummary],
    hooks: &[HookRecord],
    facts: &[NewFact],
    volume_summaries: &[VolumeSummarySelection],
) -> Vec<crate::utils::local_search::SearchDocument> {
    let mut documents: Vec<crate::utils::local_search::SearchDocument> = Vec::new();

    for summary in summaries {
        let body = [
            summary.characters.as_str(),
            summary.events.as_str(),
            summary.state_changes.as_str(),
            summary.hook_activity.as_str(),
            summary.mood.as_str(),
            summary.chapter_type.as_str(),
        ]
        .iter()
        .filter(|part| !part.is_empty())
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");
        documents.push(crate::utils::local_search::SearchDocument {
            id: summary_document_id(summary.chapter),
            scope: STORY_MEMORY_SCOPE.to_string(),
            kind: "chapter-summary".to_string(),
            source: format!("story/chapter_summaries.md#{}", summary.chapter),
            title: if summary.title.is_empty() {
                format!("Chapter {}", summary.chapter)
            } else {
                summary.title.clone()
            },
            body,
            metadata: Some(serde_json::json!({ "chapter": summary.chapter })),
        });
    }

    for hook in hooks {
        let status_text = hook_status_text(hook).to_string();
        let timing_text = hook
            .payoff_timing
            .as_ref()
            .map(|timing| serde_json::to_value(timing).ok())
            .and_then(|value| value)
            .and_then(|value| value.as_str().map(str::to_string))
            .unwrap_or_default();
        let body = [
            status_text.as_str(),
            hook.expected_payoff.as_str(),
            timing_text.as_str(),
            hook.notes.as_str(),
        ]
        .iter()
        .filter(|part| !part.is_empty())
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");
        documents.push(crate::utils::local_search::SearchDocument {
            id: hook_document_id(&hook.hook_id),
            scope: STORY_MEMORY_SCOPE.to_string(),
            kind: "hook".to_string(),
            source: format!("story/pending_hooks.md#{}", hook.hook_id),
            title: [hook.hook_id.as_str(), hook.hook_type.as_str()]
                .iter()
                .filter(|part| !part.is_empty())
                .cloned()
                .collect::<Vec<_>>()
                .join(" "),
            body,
            metadata: Some(serde_json::json!({ "hookId": hook.hook_id })),
        });
    }

    for (index, fact) in facts.iter().enumerate() {
        documents.push(crate::utils::local_search::SearchDocument {
            id: fact_document_id(index),
            scope: STORY_MEMORY_SCOPE.to_string(),
            kind: "fact".to_string(),
            source: format!(
                "story/current_state.md#{}",
                to_fact_source_anchor(&fact.predicate)
            ),
            title: [fact.subject.as_str(), fact.predicate.as_str()]
                .iter()
                .filter(|part| !part.is_empty())
                .cloned()
                .collect::<Vec<_>>()
                .join(" "),
            body: fact.object.clone(),
            metadata: Some(serde_json::json!({ "index": index })),
        });
    }

    for (index, summary) in volume_summaries.iter().enumerate() {
        documents.push(crate::utils::local_search::SearchDocument {
            id: volume_summary_document_id(index),
            scope: STORY_MEMORY_SCOPE.to_string(),
            kind: "volume-summary".to_string(),
            source: format!("story/volume_summaries.md#{}", summary.anchor),
            title: summary.heading.clone(),
            body: summary.content.clone(),
            metadata: Some(serde_json::json!({ "index": index })),
        });
    }

    documents
}

/// TS `buildRankScores`：命中序位 × 10（越前越高）。
fn build_rank_scores(hits: &[crate::utils::local_search::SearchHit]) -> std::collections::HashMap<String, i64> {
    hits.iter()
        .enumerate()
        .map(|(index, hit)| (hit.id.clone(), (hits.len() - index) as i64 * 10))
        .collect()
}

/// TS `selectSemanticCandidateIds`：selector 缺失/候选 ≤1 → None；selector
/// 失败 → None（回退 BM25 全量，检索保持可用）。
async fn select_semantic_candidate_ids(
    selector: Option<&dyn MemorySemanticSelector>,
    chapter_number: u32,
    query: &str,
    hits: &[crate::utils::local_search::SearchHit],
) -> Option<Vec<String>> {
    let selector = selector?;
    if hits.len() <= 1 {
        return None;
    }
    let candidates: Vec<MemoryCandidate> = hits
        .iter()
        .map(|hit| MemoryCandidate {
            id: hit.id.clone(),
            kind: hit.kind.clone(),
            source: hit.source.clone(),
            title: hit.title.clone(),
            excerpt: hit.body.clone(),
        })
        .collect();
    let request = MemorySemanticSelectionRequest {
        chapter_number,
        query,
        candidates: &candidates,
    };
    let selected = selector.select(&request).await.ok()?;
    let allowed: std::collections::HashSet<&String> = hits.iter().map(|hit| &hit.id).collect();
    let mut out: Vec<String> = Vec::new();
    for id in selected {
        if allowed.contains(&id) && !out.contains(&id) {
            out.push(id);
        }
    }
    Some(out)
}

// ---- 相关性选择（私有，Rust 单测镜像 TS 行为） ----

fn is_unresolved_hook(status: &str) -> bool {
    status.trim().is_empty() || unresolved_status_re().is_match(status)
}

fn select_relevant_summaries(
    summaries: &[StoredSummary],
    chapter_number: u32,
    rank_scores: &std::collections::HashMap<String, i64>,
) -> Vec<StoredSummary> {
    let chapter_number = i64::from(chapter_number);
    let chapter_number_u32 = chapter_number as u32;
    let ranked: Vec<(StoredSummary, i64, bool)> = summaries
        .iter()
        .filter(|summary| summary.chapter < chapter_number)
        .map(|summary| {
            let age = (chapter_number - summary.chapter).max(0);
            let retrieval_score =
                rank_scores.get(&summary_document_id(summary.chapter)).copied().unwrap_or(0);
            let score = retrieval_score + (12 - age).max(0);
            (
                summary.clone(),
                score,
                retrieval_score > 0,
            )
        })
        .collect();

    // recent：最近 3 章内 DESC chapter 取 3；recalled：retrieved 最高分取 1。
    let mut recent: Vec<&(StoredSummary, i64, bool)> = ranked
        .iter()
        .filter(|(summary, _, _)| summary.chapter >= chapter_number_u32 as i64 - 3)
        .collect();
    recent.sort_by(|left, right| right.0.chapter.cmp(&left.0.chapter));
    let recent: Vec<StoredSummary> =
        recent.iter().take(3).map(|(summary, _, _)| summary.clone()).collect();

    let mut recalled_ranked: Vec<&(StoredSummary, i64, bool)> = ranked
        .iter()
        .filter(|(_, _, retrieved)| *retrieved)
        .collect();
    recalled_ranked
        .sort_by(|left, right| right.1.cmp(&left.1).then(right.0.chapter.cmp(&left.0.chapter)));
    let recalled: Vec<StoredSummary> = recalled_ranked
        .iter()
        .take(1)
        .map(|(summary, _, _)| summary.clone())
        .collect();

    // Map 去重（按 chapter，后者覆盖前者）→ ASC chapter。
    let mut merged: std::collections::HashMap<i64, StoredSummary> =
        std::collections::HashMap::new();
    for summary in recalled.into_iter().chain(recent) {
        merged.insert(summary.chapter, summary);
    }
    let mut out: Vec<StoredSummary> = merged.into_values().collect();
    out.sort_by_key(|summary| summary.chapter);
    out
}

fn select_relevant_hooks(
    hooks: &[HookRecord],
    active_hooks: &[HookRecord],
    rank_scores: &std::collections::HashMap<String, i64>,
    chapter_number: u32,
) -> Vec<HookRecord> {
    let active_hook_ids: std::collections::HashSet<&str> =
        active_hooks.iter().map(|hook| hook.hook_id.as_str()).collect();
    let ranked: Vec<(HookRecord, i64, bool)> = hooks
        .iter()
        .map(|hook| {
            let retrieval_score =
                rank_scores.get(&hook_document_id(&hook.hook_id)).copied().unwrap_or(0);
            let score = retrieval_score + (hook.last_advanced_chapter as i64).max(0);
            (
                hook.clone(),
                score,
                retrieval_score > 0,
            )
        })
        .filter(|(hook, _, retrieved)| {
            *retrieved || active_hook_ids.contains(hook.hook_id.as_str())
        })
        .collect();

    let mut primary: Vec<&(HookRecord, i64, bool)> = ranked
        .iter()
        .filter(|(hook, _, retrieved)| {
            *retrieved
                || (active_hook_ids.contains(hook.hook_id.as_str())
                    && is_hook_within_chapter_window(
                        hook,
                        chapter_number,
                        5,
                        DEFAULT_HOOK_LOOKAHEAD_CHAPTERS,
                    ))
        })
        .collect();
    primary.sort_by(|left, right| {
        right
            .1
            .cmp(&left.1)
            .then(right.0.last_advanced_chapter.cmp(&left.0.last_advanced_chapter))
    });
    let primary: Vec<HookRecord> =
        primary.iter().take(6).map(|(hook, _, _)| hook.clone()).collect();

    let selected_ids: std::collections::HashSet<&str> =
        primary.iter().map(|hook| hook.hook_id.as_str()).collect();
    let mut stale: Vec<&(HookRecord, i64, bool)> = ranked
        .iter()
        .filter(|(hook, _, _retrieved)| {
            !selected_ids.contains(hook.hook_id.as_str())
                && active_hook_ids.contains(hook.hook_id.as_str())
                && !is_future_planned_hook(hook, chapter_number, DEFAULT_HOOK_LOOKAHEAD_CHAPTERS)
                && is_unresolved_hook(hook_status_text(hook))
        })
        .collect();
    stale.sort_by(|left, right| {
        left.0
            .last_advanced_chapter
            .cmp(&right.0.last_advanced_chapter)
            .then(right.1.cmp(&left.1))
    });
    let stale: Vec<HookRecord> =
        stale.iter().take(2).map(|(hook, _, _)| hook.clone()).collect();

    primary.into_iter().chain(stale).collect()
}

fn select_relevant_facts(
    facts: &[NewFact],
    rank_scores: &std::collections::HashMap<String, i64>,
) -> Vec<NewFact> {
    let prioritized_predicates: [&[&str]; 6] = [
        &["当前冲突", "current conflict"],
        &["当前目标", "current goal"],
        &["主角状态", "protagonist state"],
        &["当前限制", "current constraint"],
        &["当前位置", "current location"],
        &["当前敌我", "current alliances", "current relationships"],
    ];

    let mut ranked: Vec<(NewFact, i64, bool)> = facts
        .iter()
        .enumerate()
        .map(|(index, fact)| {
            let normalized_predicate = fact.predicate.trim().to_lowercase();
            let priority = prioritized_predicates
                .iter()
                .position(|values| values.iter().any(|value| *value == normalized_predicate));
            let base_score = match priority {
                Some(priority) => 20 - 2 * priority as i64,
                None => 5,
            };
            let retrieval_score =
                rank_scores.get(&fact_document_id(index)).copied().unwrap_or(0);
            (
                fact.clone(),
                base_score + retrieval_score,
                retrieval_score > 0,
            )
        })
        .filter(|(_, score, retrieved)| *retrieved || *score >= 14)
        .collect();

    ranked.sort_by(|left, right| right.1.cmp(&left.1));
    ranked.truncate(4);
    ranked.into_iter().map(|(fact, _, _)| fact).collect()
}

fn select_relevant_volume_summaries(
    summaries: &[VolumeSummarySelection],
    rank_scores: &std::collections::HashMap<String, i64>,
) -> Vec<VolumeSummarySelection> {
    if summaries.is_empty() {
        return Vec::new();
    }

    let ranked: Vec<(usize, VolumeSummarySelection, i64, bool)> = summaries
        .iter()
        .enumerate()
        .map(|(index, summary)| {
            let retrieval_score =
                rank_scores.get(&volume_summary_document_id(index)).copied().unwrap_or(0);
            (
                index,
                summary.clone(),
                retrieval_score + index as i64,
                retrieval_score > 0,
            )
        })
        .filter(|(index, _, _, retrieved)| *retrieved || *index + 1 == summaries.len())
        .collect();

    let mut selected = ranked;
    selected.sort_by(|left, right| right.2.cmp(&left.2));
    selected.truncate(2);
    selected.sort_by_key(|(index, _, _, _)| *index);
    selected.into_iter().map(|(_, summary, _, _)| summary).collect()
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
        // 244 号：rankScores 模式（TS selectRelevantSummaries 逐字）——
        // recent = 最近 3 章 DESC 取 3；recalled = retrieved 最高分取 1。
        let mut rank_scores = std::collections::HashMap::new();
        rank_scores.insert("summary:8".to_string(), 30);
        rank_scores.insert("summary:7".to_string(), 10);
        let out = select_relevant_summaries(&summaries, 10, &rank_scores);
        let chapters: Vec<i64> = out.iter().map(|s| s.chapter).collect();
        // recent（8,9,7 DESC 取 3）+ recalled（7）→ 全部去重 ASC。
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
        // 244 号：rankScores 模式（TS selectRelevantHooks 逐字）——
        // active（全 open）全量过滤后 primary ≤6 + stale ≤2 = 8 条保留。
        let mut rank_scores = std::collections::HashMap::new();
        for i in 0..8 {
            rank_scores.insert(format!("hook:H{i:02}"), (i + 1) * 10);
        }
        let active: Vec<HookRecord> = hooks.clone();
        let out = select_relevant_hooks(&hooks, &active, &rank_scores, 10);
        assert_eq!(out.len(), 8, "主选 6 + 陈旧 2");
        assert_eq!(out[0].hook_id, "H07", "primary 按分数 DESC，freshness 最高在前");
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
            semantic_selector: None,
        })
        .await;

        // markdown 路径：hooks 来自 pending_hooks.md，resolved 被滤出活跃集；
        // H01 pressured 沉默 7 章 ≥ 5 → 进入回收集。
        assert_eq!(selection.active_hooks.len(), 1);
        // 247 号：检索溯源可观测（engine/query/候选含 chapter-summary）。
        let trace = selection.retrieval_trace.as_ref().expect("retrieval_trace");
        assert_eq!(trace.engine, "sqlite-fts5-bm25");
        assert!(trace.query.contains("推进祖符线"), "{}", trace.query);
        assert!(
            trace
                .candidates
                .iter()
                .any(|candidate| candidate.id == "summary:7"),
            "{:?}",
            trace.candidates
        );
        assert_eq!(selection.active_hooks[0].hook_id, "H01");
        assert_eq!(selection.active_hooks[0].status_raw, "pressured");
        assert_eq!(selection.recyclable_hooks.len(), 1);
        assert_eq!(selection.summaries.len(), 1);
        // tempdir 内 SQLite 可开（memory.db 创建于 story/ 下）→ db 分支生效。
        assert!(selection.db_path.is_some());
    }
}
