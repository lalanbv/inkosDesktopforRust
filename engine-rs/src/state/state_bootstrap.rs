//! 状态引导（state-bootstrap）。
//!
//! 移植自 `packages/core/src/state/state-bootstrap.ts`（645 行）。分两阶段移植：
//! - **纯逻辑**（已移植）：[`resolve_contiguous_chapter_prefix`] / [`deduplicate_summary_rows`] /
//!   [`normalize_hook_status`] 等 integer/hook 字段归一
//! - **async fs 编排**（逐步移植）：本模块已含 [`resolve_durable_story_progress`]（durable 进度，
//!   首个 [`StateStore`](crate::state::store::StateStore) 消费者）；完整 `bootstrapStructuredStateFromMarkdown`
//!   / `loadOrBootstrap*` 系列待后续阶段
//!
//! ## 移植纪律
//! `normalize_hook_status` 的正则模式（resolved/deferred/progressing/open 四类 + 中英文同义词）须与
//! TS **逐字一致**——它决定 hooks.json 反序列化后的状态收敛结果，影响 stale-detection/governance 的判断。

use regex::Regex;
use std::sync::OnceLock;

use crate::models::runtime_state::{
    ChapterSummariesState, CurrentStateFact, CurrentStateState, HookRecord, HookStatus, HooksState,
    StateManifest,
};
use crate::state::store::{join_path, StateStore};
use crate::utils::story_markdown::normalize_hook_id;

fn resolved_status_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"(?i)(resolved|closed|done|paid[_ -]?off|已回收|回收|完成|已解决|已兑现|兑现)")
            .expect("resolved status regex")
    })
}

fn deferred_status_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"(?i)(deferred|paused|hold|dormant|inactive|unplanted|unseeded|not[_ -]?started|not[_ -]?active|搁置|延后|延期|暂缓|休眠|未激活|未启动|待启动|未推进|尚未推进)")
            .expect("deferred status regex")
    })
}

fn progressing_status_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"(?i)(confirmed[_ -]?hit|confirmed|advanced|progressing|progress|active|pressured|命中|已确认命中|已推进|推进|进行中|持续推进|重大推进)")
            .expect("progressing status regex")
    })
}

fn open_status_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"(?i)(open|pending|seeded|planted|待定|未回收|已埋|已种下|已铺垫)").expect("open status regex")
    })
}

fn digits_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\d+").expect("digits regex"))
}

fn strict_integer_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^\d+$").expect("strict integer regex"))
}

/// 计算连续章节前缀长度：从 1 开始连续的最大章节数。
///
/// 对齐 TS `resolveContiguousChapterPrefix`（export）。过滤非正整数后，从 1 起递增探测。
pub fn resolve_contiguous_chapter_prefix(chapter_numbers: &[i64]) -> u32 {
    let chapters: std::collections::HashSet<i64> = chapter_numbers
        .iter()
        .copied()
        .filter(|c| c.is_positive())
        .collect();
    let mut contiguous: i64 = 0;
    while chapters.contains(&(contiguous + 1)) {
        contiguous += 1;
    }
    contiguous as u32
}

/// 按 chapter 去重（后出现者覆盖）并升序排序。
///
/// 对齐 TS `deduplicateSummaryRows<T extends {chapter: number}>`。Rust 用 `chapter_of` 闭包
/// 提取每行的 chapter 键（TS 的结构约束在这里转为显式提取器）。
pub fn deduplicate_summary_rows<T: Clone>(rows: &[T], chapter_of: impl Fn(&T) -> i64) -> Vec<T> {
    use std::collections::BTreeMap;
    let mut by_chapter: BTreeMap<i64, T> = BTreeMap::new();
    for row in rows {
        by_chapter.insert(chapter_of(row), row.clone());
    }
    by_chapter.into_values().collect()
}

/// 模糊归一 hook 状态字符串 → [`HookStatus`]，未识别时追加 warning 并回落 open。
///
/// 对齐 TS `normalizeHookStatus`。匹配顺序：resolved → deferred → progressing → open（首个命中返回）。
pub fn normalize_hook_status(value: Option<&str>, warnings: &mut Vec<String>, hook_id: &str) -> HookStatus {
    let normalized = value.unwrap_or("").trim().to_lowercase();
    if normalized.is_empty() {
        return HookStatus::Open;
    }
    if resolved_status_re().is_match(&normalized) {
        return HookStatus::Resolved;
    }
    if deferred_status_re().is_match(&normalized) {
        return HookStatus::Deferred;
    }
    if progressing_status_re().is_match(&normalized) {
        return HookStatus::Progressing;
    }
    if open_status_re().is_match(&normalized) {
        return HookStatus::Open;
    }
    append_warning(
        warnings,
        &format!("{hook_id}:status normalized from \"{}\" to \"open\"", value.unwrap_or("")),
    );
    HookStatus::Open
}

/// 归一 hook 类型：非空 trim 后保留，空 → "unspecified"（追加 warning）。
/// 对齐 TS `normalizeHookType`。
pub fn normalize_hook_type(value: Option<&str>, warnings: &mut Vec<String>, hook_id: &str) -> String {
    let normalized = value.unwrap_or("").trim();
    if !normalized.is_empty() {
        return normalized.to_string();
    }
    append_warning(
        warnings,
        &format!("{hook_id}: empty hook type normalized to \"unspecified\""),
    );
    "unspecified".to_string()
}

/// 严格整数解析 + warning：空 → 0；非纯数字（经 normalizeHookId）→ 0 并 warning。
/// 对齐 TS `parseStrictIntegerWithWarning`。
pub fn parse_strict_integer_with_warning(
    value: Option<&str>,
    warnings: &mut Vec<String>,
    field_label: &str,
) -> i64 {
    let Some(v) = value else { return 0 };
    if v.is_empty() {
        return 0;
    }
    if let Some(parsed) = parse_strict_integer_cell(Some(v)) {
        return parsed;
    }
    append_warning(warnings, &format!("{field_label} normalized from \"{v}\" to 0"));
    0
}

/// 宽松整数解析 + fallback：空/无数字 → max(0, fallback)，无数字时 warning。
/// 对齐 TS `parseIntegerWithFallback`。
pub fn parse_integer_with_fallback(
    value: Option<&str>,
    fallback: i64,
    warnings: &mut Vec<String>,
    field_label: &str,
) -> i64 {
    let fallback = fallback.max(0);
    let Some(v) = value else { return fallback };
    if v.is_empty() {
        return fallback;
    }
    match digits_re().find(v) {
        Some(m) => m.as_str().parse::<i64>().unwrap_or(fallback),
        None => {
            append_warning(warnings, &format!("{field_label} normalized from \"{v}\" to {fallback}"));
            fallback
        }
    }
}

/// 严格整数单元格解析：经 normalizeHookId 后须纯数字，否则 None。
/// 对齐 TS `parseStrictIntegerCell`（与 story_markdown 的 parse_strict_chapter_integer 同语义，返回 Option）。
pub fn parse_strict_integer_cell(value: Option<&str>) -> Option<i64> {
    let v = value?;
    let normalized = normalize_hook_id(Some(v));
    if normalized.is_empty() || !strict_integer_re().is_match(&normalized) {
        return None;
    }
    normalized.parse::<i64>().ok()
}

/// 归一显式章节号：非正整数 → 0。对齐 TS `normalizeExplicitChapter`。
pub fn normalize_explicit_chapter(value: Option<i64>) -> i64 {
    match value {
        Some(v) if v.is_positive() => v,
        _ => 0,
    }
}

/// 去重追加 warning（已存在则跳过）。对齐 TS `appendWarning`。
pub fn append_warning(warnings: &mut Vec<String>, warning: &str) {
    if !warnings.iter().any(|w| w == warning) {
        warnings.push(warning.to_string());
    }
}

/// 去空 + 去重字符串列表（保留首次出现顺序）。对齐 TS `uniqueStrings`。
pub fn unique_strings(values: &[String]) -> Vec<String> {
    use std::collections::HashSet;
    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<String> = Vec::new();
    for v in values {
        if v.trim().is_empty() {
            continue;
        }
        if seen.insert(v.clone()) {
            out.push(v.clone());
        }
    }
    out
}

fn chapter_filename_re() -> &'static Regex {
    // TS loadDurableArtifactChapterNumbers: /^(\d+)_/（章节文件名前缀 N_）
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^(\d+)_").expect("chapter filename regex"))
}

/// 计算 durable story progress：max(连续章节产物前缀, 显式 fallback)。
///
/// 对齐 TS `resolveDurableStoryProgress`（export）。`fallback` 非正整数视为 0。
/// 「只信任 durable 产物进度」是关键设计——current_state.chapter 来自 markdown，
/// 可能含幻觉数字（如 1988 年被误读为第 1988 章）。
pub async fn resolve_durable_story_progress(
    store: &dyn StateStore,
    book_dir: &str,
    fallback: Option<i64>,
) -> crate::Result<i64> {
    let explicit_fallback = normalize_explicit_chapter(fallback);
    let artifact_progress = resolve_contiguous_artifact_chapter_progress(store, book_dir).await?;
    Ok(artifact_progress.max(explicit_fallback))
}

/// 连续章节产物前缀：从章节 index.json + 章节文件名提取章节数，取连续前缀。
/// 对齐 TS `resolveContiguousArtifactChapterProgress`。
async fn resolve_contiguous_artifact_chapter_progress(
    store: &dyn StateStore,
    book_dir: &str,
) -> crate::Result<i64> {
    let chapter_numbers = load_durable_artifact_chapter_numbers(store, book_dir).await?;
    Ok(i64::from(resolve_contiguous_chapter_prefix(&chapter_numbers)))
}

/// 从 `{book_dir}/chapters/index.json`（数组 number 字段）+ `chapters/` 目录文件名（`N_*`）提取章节数。
/// 对齐 TS `loadDurableArtifactChapterNumbers`：两个源任一失败 → 空，最终合并。
async fn load_durable_artifact_chapter_numbers(
    store: &dyn StateStore,
    book_dir: &str,
) -> crate::Result<Vec<i64>> {
    let chapters_dir = join_path(book_dir, "chapters");
    let index_path = join_path(&chapters_dir, "index.json");

    // index.json：解析为 [{number}, ...]，取正整数。
    let index_chapters: Vec<i64> = match store.read_to_string(&index_path).await? {
        Some(raw) => serde_json::from_str::<Vec<serde_json::Value>>(&raw)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|entry| entry.get("number")?.as_i64())
            .filter(|n| n.is_positive())
            .collect(),
        None => Vec::new(),
    };

    // chapters/ 目录：文件名 N_xxx 提取 N。
    let entries = store.list_dir(&chapters_dir).await?;
    let file_chapters: Vec<i64> = entries
        .iter()
        .filter_map(|name| {
            chapter_filename_re()
                .captures(name)
                .and_then(|c| c.get(1)?.as_str().parse::<i64>().ok())
        })
        .collect();

    let mut all = index_chapters;
    all.extend(file_chapters);
    Ok(all)
}

// =============================================================================
// 编排中间件层（主入口 bootstrap_structured_state_from_markdown 的依赖）
// =============================================================================

/// 由 `{book_dir}/book.json` 的 language 字段推断运行时语言：仅 "zh" → Zh，其余（缺失/无效）→ En。
///
/// 对齐 TS `resolveRuntimeLanguage`。读失败或解析失败 → En（不报错，与 TS try/catch 一致）。
pub async fn resolve_runtime_language(
    store: &dyn StateStore,
    book_dir: &str,
) -> crate::Result<crate::utils::language::WritingLanguage> {
    use crate::utils::language::WritingLanguage;
    let path = join_path(book_dir, "book.json");
    let language = match store.read_to_string(&path).await? {
        Some(raw) => serde_json::from_str::<serde_json::Value>(&raw)
            .ok()
            .and_then(|v| v.get("language")?.as_str().map(|s| s.to_string()))
            .unwrap_or_default(),
        None => String::new(),
    };
    Ok(if language == "zh" { WritingLanguage::Zh } else { WritingLanguage::En })
}

/// 修复 hooks 状态的 `serde_json::Value`：对每个 hook，空/缺失 type → "unspecified"（追加 warning），
/// 非空 type 去除首尾空白。返回是否变更。
///
/// 对齐 TS `repairHooksStateInput`（操作反序列化前的 unknown；Rust 用 Value 同构）。
/// 主入口在 deserialize 前调用，确保 hooks.json 的历史数据能通过强类型校验。
pub fn repair_hooks_state_value(value: &mut serde_json::Value, warnings: &mut Vec<String>) -> bool {
    let Some(hooks) = value.get_mut("hooks").and_then(|h| h.as_array_mut()) else {
        return false;
    };
    let mut changed = false;
    for (index, hook) in hooks.iter_mut().enumerate() {
        let Some(obj) = hook.as_object_mut() else { continue };
        let hook_id = obj
            .get("hookId")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| format!("hooks[{index}]"));

        let current_type = obj.get("type").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let trimmed = current_type.trim();
        if !trimmed.is_empty() {
            if trimmed != current_type {
                changed = true;
                obj.insert("type".to_string(), serde_json::Value::String(trimmed.to_string()));
            }
            continue;
        }
        changed = true;
        append_warning(
            warnings,
            &format!("{hook_id}: empty hook type normalized to \"unspecified\""),
        );
        obj.insert("type".to_string(), serde_json::Value::String("unspecified".to_string()));
    }
    changed
}

/// 从 store 读取 + 解析 JSON 为 `T`；文件缺失 → None（无 warning）；解析失败 → None + warning。
///
/// 对齐 TS `loadJsonIfValid<T>`。`file_label` 用于 warning 文案。
pub async fn load_json_if_valid<T: serde::de::DeserializeOwned>(
    store: &dyn StateStore,
    path: &str,
    warnings: &mut Vec<String>,
    file_label: &str,
) -> crate::Result<Option<T>> {
    let Some(raw) = store.read_to_string(path).await? else {
        return Ok(None);
    };
    match serde_json::from_str::<T>(&raw) {
        Ok(v) => Ok(Some(v)),
        Err(_) => {
            append_warning(warnings, &format!("{file_label} invalid, rebuilt from markdown"));
            Ok(None)
        }
    }
}

/// 从 store 读取 + repair + 解析 hooks 状态。
///
/// 对齐 TS `loadHooksStateIfValid`。返回 `(state, repaired)`；文件缺失/解析失败 → None。
/// 失败（非缺失）时追加 warning。
pub async fn load_hooks_state_if_valid(
    store: &dyn StateStore,
    path: &str,
    warnings: &mut Vec<String>,
    file_label: &str,
) -> crate::Result<Option<(crate::models::runtime_state::HooksState, bool)>> {
    use crate::models::runtime_state::HooksState;
    let Some(raw) = store.read_to_string(path).await? else {
        return Ok(None);
    };
    let mut value: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(_) => {
            append_warning(warnings, &format!("{file_label} invalid, rebuilt from markdown"));
            return Ok(None);
        }
    };
    let repaired = repair_hooks_state_value(&mut value, warnings);
    match serde_json::from_value::<HooksState>(value) {
        Ok(state) => Ok(Some((state, repaired))),
        Err(_) => {
            append_warning(warnings, &format!("{file_label} invalid, rebuilt from markdown"));
            Ok(None)
        }
    }
}

// =============================================================================
// markdown 引导层（从 *.md 重建结构化状态）
// =============================================================================

/// 从 `pending_hooks.md` 重建 [`HooksState`]。
///
/// 对齐 TS `parsePendingHooksStateMarkdown`：调用 [`parse_pending_hooks_markdown`]（已含初步 status 归一）
/// → 对每个 hook 的 type 走 [`normalize_hook_type`]。
///
/// **与 TS 的等价性说明**：TS 在此对 status 也走 `normalizeHookStatus`（模糊正则）；Rust 的
/// [`parse_pending_hooks_markdown`] 已用精确匹配把 status 收敛为 [`HookStatus`] 枚举，
/// 故此处不再二次 normalize（语义等价——精确匹配是模糊正则的子集命中）。
/// bullet fallback（无表格时 `- xxx` → notes-only hook）已在 [`parse_pending_hooks_markdown`] 内处理。
pub fn parse_pending_hooks_state_markdown(
    markdown: &str,
    warnings: &mut Vec<String>,
) -> HooksState {
    use crate::utils::story_markdown::parse_pending_hooks_markdown;
    let hooks: Vec<HookRecord> = parse_pending_hooks_markdown(markdown)
        .into_iter()
        .map(|mut h| {
            h.hook_type = normalize_hook_type(Some(&h.hook_type), warnings, &h.hook_id);
            h
        })
        .collect();
    HooksState { hooks }
}

/// 从 `current_state.md` 重建 [`CurrentStateState`]。
///
/// 对齐 TS `parseCurrentStateStateMarkdown`：字段/值表格（含「当前章节」行定 chapter）→ facts；
/// bullet fallback → note_N facts。chapter 经 [`parse_integer_with_fallback`]（含 warning）。
pub fn parse_current_state_state_markdown(
    markdown: &str,
    fallback_chapter: u32,
    warnings: &mut Vec<String>,
) -> CurrentStateState {
    use crate::utils::story_markdown::{infer_fact_subject, is_current_chapter_label, parse_markdown_table_rows};

    let table_rows = parse_markdown_table_rows(markdown);
    let field_value_rows: Vec<&Vec<String>> = table_rows
        .iter()
        .filter(|row| row.len() >= 2)
        .filter(|row| !crate::utils::story_markdown::is_state_table_header_row(row))
        .collect();

    if !field_value_rows.is_empty() {
        let state_chapter = field_value_rows
            .iter()
            .find(|row| row.first().is_some_and(|c| is_current_chapter_label(c)))
            .and_then(|row| row.get(1).map(|s| s.as_str()))
            .map(|v| parse_integer_with_fallback(Some(v), fallback_chapter as i64, warnings, "current_state:chapter"))
            .unwrap_or(fallback_chapter as i64)
            .max(0) as u32;

        let facts: Vec<CurrentStateFact> = field_value_rows
            .iter()
            .filter(|row| row.first().is_some_and(|c| !is_current_chapter_label(c)))
            .filter_map(|row| {
                let label = row.first().map(|s| s.trim()).unwrap_or("");
                let value = row.get(1).map(|s| s.trim()).unwrap_or("");
                if label.is_empty() || value.is_empty() {
                    return None;
                }
                Some(CurrentStateFact {
                    subject: infer_fact_subject(label),
                    predicate: label.to_string(),
                    object: value.to_string(),
                    valid_from_chapter: state_chapter,
                    valid_until_chapter: None,
                    source_chapter: state_chapter,
                })
            })
            .collect();

        return CurrentStateState { chapter: state_chapter, facts };
    }

    // bullet fallback：predicate=note_N，subject=current_state。
    let bullet_facts: Vec<String> = markdown
        .lines()
        .map(|line| line.trim())
        .filter(|line| line.starts_with('-'))
        .map(|line| line.replacen("- ", "", 1).replacen('-', "", 1).trim().to_string())
        .filter(|line| !line.is_empty())
        .collect();

    let chapter = fallback_chapter;
    let facts: Vec<CurrentStateFact> = bullet_facts
        .into_iter()
        .enumerate()
        .map(|(index, object)| CurrentStateFact {
            subject: "current_state".to_string(),
            predicate: format!("note_{}", index + 1),
            object,
            valid_from_chapter: chapter,
            valid_until_chapter: None,
            source_chapter: chapter,
        })
        .collect();
    CurrentStateState { chapter, facts }
}

/// 读 `chapter_summaries.md` → [`ChapterSummariesState`]（去重 + 升序）。
/// 对齐 TS `loadMarkdownSummariesState`。
pub async fn load_markdown_summaries_state(
    store: &dyn StateStore,
    story_dir: &str,
) -> crate::Result<ChapterSummariesState> {
    use crate::models::runtime_state::ChapterSummaryRow;
    use crate::utils::story_markdown::parse_chapter_summaries_markdown;
    let markdown = store
        .read_to_string(&join_path(story_dir, "chapter_summaries.md"))
        .await?
        .unwrap_or_default();
    let raw_rows = parse_chapter_summaries_markdown(&markdown);
    // ChapterSummaryRow 与 StoredSummary 字段一致；去重按 chapter。
    let rows: Vec<ChapterSummaryRow> = raw_rows
        .into_iter()
        .map(|s| ChapterSummaryRow {
            chapter: s.chapter.max(0) as u32,
            title: s.title,
            characters: s.characters,
            events: s.events,
            state_changes: s.state_changes,
            hook_activity: s.hook_activity,
            mood: s.mood,
            chapter_type: s.chapter_type,
        })
        .collect();
    let rows = deduplicate_summary_rows(&rows, |r| r.chapter as i64);
    Ok(ChapterSummariesState { rows })
}

/// 读 `pending_hooks.md` → [`HooksState`]。
/// 对齐 TS `loadMarkdownHooksState`。
pub async fn load_markdown_hooks_state(
    store: &dyn StateStore,
    story_dir: &str,
    warnings: &mut Vec<String>,
) -> crate::Result<HooksState> {
    let markdown = store
        .read_to_string(&join_path(story_dir, "pending_hooks.md"))
        .await?
        .unwrap_or_default();
    Ok(parse_pending_hooks_state_markdown(&markdown, warnings))
}

/// 读 `current_state.md` → [`CurrentStateState`]。
/// 对齐 TS `loadMarkdownCurrentState`。
pub async fn load_markdown_current_state(
    store: &dyn StateStore,
    story_dir: &str,
    fallback_chapter: u32,
    warnings: &mut Vec<String>,
) -> crate::Result<CurrentStateState> {
    let markdown = store
        .read_to_string(&join_path(story_dir, "current_state.md"))
        .await?
        .unwrap_or_default();
    Ok(parse_current_state_state_markdown(&markdown, fallback_chapter, warnings))
}

/// markdown 引导状态的聚合（summaries + hooks + currentState + durableStoryProgress）。
///
/// 对齐 TS `MarkdownBootstrapState` / `loadMarkdownBootstrapState`。
/// authoritativeProgress = max(显式 fallback, durable 产物前缀)，作为 current_state 的 fallback。
#[derive(Debug, Clone, Default)]
pub struct MarkdownBootstrapState {
    pub summaries_state: ChapterSummariesState,
    pub hooks_state: HooksState,
    pub current_state: CurrentStateState,
    /// durable story progress（max(显式 fallback, 章节产物前缀)）。
    pub durable_story_progress: u32,
}

/// 聚合加载 markdown 引导状态。
/// 对齐 TS `loadMarkdownBootstrapState`。
pub async fn load_markdown_bootstrap_state(
    store: &dyn StateStore,
    book_dir: &str,
    story_dir: &str,
    fallback_chapter: u32,
    warnings: &mut Vec<String>,
) -> crate::Result<MarkdownBootstrapState> {
    let summaries_state = load_markdown_summaries_state(store, story_dir).await?;
    let hooks_state = load_markdown_hooks_state(store, story_dir, warnings).await?;
    let durable_artifact_progress =
        resolve_contiguous_artifact_chapter_progress(store, book_dir).await? as u32;
    let authoritative_progress = fallback_chapter.max(durable_artifact_progress);
    let current_state =
        load_markdown_current_state(store, story_dir, authoritative_progress, warnings).await?;
    Ok(MarkdownBootstrapState {
        summaries_state,
        hooks_state,
        current_state,
        durable_story_progress: authoritative_progress,
    })
}

// =============================================================================
// load_or_bootstrap_* + 主入口 bootstrap_structured_state_from_markdown
// =============================================================================

/// `bootstrap_structured_state_from_markdown` 的返回值。对齐 TS `BootstrapStructuredStateResult`。
#[derive(Debug, Clone)]
pub struct BootstrapStructuredStateResult {
    pub created_files: Vec<String>,
    pub warnings: Vec<String>,
    pub manifest: StateManifest,
}

/// 读 summaries JSON；失败/缺失/强制 → 用预加载 markdown 状态或现读；写回；记录新文件。
/// 对齐 TS `loadOrBootstrapSummaries`。
///
/// 即使从 JSON 加载也去重（历史数据可能含重复）。
pub async fn load_or_bootstrap_summaries(
    store: &dyn StateStore,
    story_dir: &str,
    state_path: &str,
    created_files: &mut Vec<String>,
    warnings: &mut Vec<String>,
    bootstrap_state: Option<ChapterSummariesState>,
) -> crate::Result<ChapterSummariesState> {
    if let Some(existing) =
        load_json_if_valid::<ChapterSummariesState>(store, state_path, warnings, "chapter_summaries.json").await?
    {
        let deduped = deduplicate_summary_rows(&existing.rows, |r| r.chapter as i64);
        if deduped.len() < existing.rows.len() {
            let repaired = ChapterSummariesState { rows: deduped };
            let json = serde_json::to_string_pretty(&repaired)?;
            store.write_string(state_path, &json).await?;
            return Ok(repaired);
        }
        return Ok(existing);
    }

    let summaries = match bootstrap_state {
        Some(s) => s,
        None => load_markdown_summaries_state(store, story_dir).await?,
    };
    let existed = store.exists(state_path).await?;
    let json = serde_json::to_string_pretty(&summaries)?;
    store.write_string(state_path, &json).await?;
    if !existed {
        created_files.push("chapter_summaries.json".to_string());
    }
    Ok(summaries)
}

/// 读 hooks JSON（含 repair）；失败/缺失 → 用预加载 markdown 状态或现读；写回；记录新文件。
/// 对齐 TS `loadOrBootstrapHooks`。
pub async fn load_or_bootstrap_hooks(
    store: &dyn StateStore,
    story_dir: &str,
    state_path: &str,
    created_files: &mut Vec<String>,
    warnings: &mut Vec<String>,
    bootstrap_state: Option<HooksState>,
) -> crate::Result<HooksState> {
    if let Some((state, repaired)) =
        load_hooks_state_if_valid(store, state_path, warnings, "hooks.json").await?
    {
        if repaired {
            let json = serde_json::to_string_pretty(&state)?;
            store.write_string(state_path, &json).await?;
        }
        return Ok(state);
    }

    let hooks = match bootstrap_state {
        Some(h) => h,
        None => load_markdown_hooks_state(store, story_dir, warnings).await?,
    };
    let existed = store.exists(state_path).await?;
    let json = serde_json::to_string_pretty(&hooks)?;
    store.write_string(state_path, &json).await?;
    if !existed {
        created_files.push("hooks.json".to_string());
    }
    Ok(hooks)
}

/// 读 current_state JSON；失败/缺失 → 用预加载 markdown 状态或现读；写回；记录新文件。
/// 对齐 TS `loadOrBootstrapCurrentState`。
pub async fn load_or_bootstrap_current_state(
    store: &dyn StateStore,
    story_dir: &str,
    state_path: &str,
    fallback_chapter: u32,
    created_files: &mut Vec<String>,
    warnings: &mut Vec<String>,
    bootstrap_state: Option<CurrentStateState>,
) -> crate::Result<CurrentStateState> {
    if let Some(existing) =
        load_json_if_valid::<CurrentStateState>(store, state_path, warnings, "current_state.json").await?
    {
        return Ok(existing);
    }

    let current = match bootstrap_state {
        Some(c) => c,
        None => load_markdown_current_state(store, story_dir, fallback_chapter, warnings).await?,
    };
    let existed = store.exists(state_path).await?;
    let json = serde_json::to_string_pretty(&current)?;
    store.write_string(state_path, &json).await?;
    if !existed {
        created_files.push("current_state.json".to_string());
    }
    Ok(current)
}

/// 主入口：从 markdown 引导结构化状态（manifest/current_state/hooks/chapter_summaries 四文件编排）。
///
/// 对齐 TS `bootstrapStructuredStateFromMarkdown`。幂等：已存在的 JSON 保留并校验/修复；
/// 缺失则从 markdown 引导并写入。manifest 始终按 durable progress 重算并写回。
///
/// `book_dir` 下的 `story/state/` 为状态目录；`story/*.md` 为 markdown 源。
pub async fn bootstrap_structured_state_from_markdown(
    store: &dyn StateStore,
    book_dir: &str,
    fallback_chapter: Option<u32>,
) -> crate::Result<BootstrapStructuredStateResult> {
    let story_dir = join_path(book_dir, "story");
    let state_dir = join_path(&story_dir, "state");
    let manifest_path = join_path(&state_dir, "manifest.json");
    let current_state_path = join_path(&state_dir, "current_state.json");
    let hooks_path = join_path(&state_dir, "hooks.json");
    let summaries_path = join_path(&state_dir, "chapter_summaries.json");

    store.mkdir_p(&state_dir).await?;

    let mut created_files: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    let existing_manifest: Option<StateManifest> =
        load_json_if_valid(store, &manifest_path, &mut warnings, "manifest.json").await?;
    // language：existing manifest 合法值优先，否则读 book.json（仅 "zh" 认定，其余 "en"）。
    let language_str: String = match &existing_manifest {
        Some(m) if m.language == "zh" || m.language == "en" => m.language.clone(),
        _ => match resolve_runtime_language(store, book_dir).await? {
            crate::utils::language::WritingLanguage::Zh => "zh".to_string(),
            crate::utils::language::WritingLanguage::En => "en".to_string(),
        },
    };

    let markdown_fallback = normalize_explicit_chapter(fallback_chapter.map(|v| v as i64)) as u32;
    let markdown_state =
        load_markdown_bootstrap_state(store, book_dir, &story_dir, markdown_fallback, &mut warnings).await?;

    let _summaries = load_or_bootstrap_summaries(
        store,
        &story_dir,
        &summaries_path,
        &mut created_files,
        &mut warnings,
        Some(markdown_state.summaries_state.clone()),
    )
    .await?;
    let _hooks = load_or_bootstrap_hooks(
        store,
        &story_dir,
        &hooks_path,
        &mut created_files,
        &mut warnings,
        Some(markdown_state.hooks_state.clone()),
    )
    .await?;
    let _current = load_or_bootstrap_current_state(
        store,
        &story_dir,
        &current_state_path,
        markdown_state.durable_story_progress,
        &mut created_files,
        &mut warnings,
        Some(markdown_state.current_state.clone()),
    )
    .await?;

    let derived_progress = markdown_state.durable_story_progress;
    if let Some(m) = &existing_manifest {
        if m.last_applied_chapter > derived_progress {
            append_warning(
                &mut warnings,
                &format!(
                    "manifest lastAppliedChapter normalized from {} to {derived_progress}",
                    m.last_applied_chapter
                ),
            );
        }
    }

    let migration_warnings = {
        let existing = existing_manifest.as_ref().map(|m| m.migration_warnings.clone()).unwrap_or_default();
        unique_strings(&[existing, warnings.clone()].concat())
    };
    let manifest = StateManifest {
        schema_version: 2,
        language: language_str,
        last_applied_chapter: derived_progress,
        projection_version: existing_manifest
            .as_ref()
            .map(|m| m.projection_version)
            .unwrap_or(1),
        migration_warnings,
    };

    let manifest_existed = existing_manifest.is_some();
    let json = serde_json::to_string_pretty(&manifest)?;
    store.write_string(&manifest_path, &json).await?;
    if !manifest_existed {
        created_files.push("manifest.json".to_string());
    }

    Ok(BootstrapStructuredStateResult {
        created_files,
        warnings: manifest.migration_warnings.clone(),
        manifest,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::runtime_state::{ChapterSummaryRow, StateManifest};
    use crate::state::store::InMemoryStateStore;
    use crate::utils::language::WritingLanguage;

    #[test]
    fn resolve_contiguous_chapter_prefix_counts_from_one() {
        assert_eq!(resolve_contiguous_chapter_prefix(&[1, 2, 3]), 3);
        assert_eq!(resolve_contiguous_chapter_prefix(&[1, 2, 4, 5]), 2);
        assert_eq!(resolve_contiguous_chapter_prefix(&[2, 3, 4]), 0, "缺 1 → 0");
        assert_eq!(resolve_contiguous_chapter_prefix(&[]), 0);
    }

    #[test]
    fn resolve_contiguous_chapter_prefix_ignores_non_positive_and_duplicates() {
        assert_eq!(resolve_contiguous_chapter_prefix(&[1, 1, 2, 0, -3, 3]), 3);
    }

    #[test]
    fn deduplicate_summary_rows_last_wins_and_sorted() {
        let rows: Vec<(i64, &str)> = vec![(2, "b"), (1, "a-old"), (1, "a-new"), (3, "c")];
        let deduped = deduplicate_summary_rows(&rows, |(c, _)| *c);
        assert_eq!(deduped.len(), 3);
        assert_eq!(deduped[0], (1, "a-new"), "后出现者覆盖");
        assert_eq!(deduped[1], (2, "b"));
        assert_eq!(deduped[2], (3, "c"));
    }

    #[test]
    fn normalize_hook_status_matches_resolved_synonyms() {
        let mut w = Vec::new();
        assert_eq!(normalize_hook_status(Some("resolved"), &mut w, "h1"), HookStatus::Resolved);
        assert_eq!(normalize_hook_status(Some("已回收"), &mut w, "h1"), HookStatus::Resolved);
        assert_eq!(normalize_hook_status(Some("paid off"), &mut w, "h1"), HookStatus::Resolved);
        assert_eq!(normalize_hook_status(Some("PAID_OFF"), &mut w, "h1"), HookStatus::Resolved);
        assert_eq!(normalize_hook_status(Some("兑现"), &mut w, "h1"), HookStatus::Resolved);
        assert!(w.is_empty(), "命中的归一不应产 warning");
    }

    #[test]
    fn normalize_hook_status_matches_deferred_synonyms() {
        let mut w = Vec::new();
        assert_eq!(normalize_hook_status(Some("deferred"), &mut w, "h1"), HookStatus::Deferred);
        assert_eq!(normalize_hook_status(Some("dormant"), &mut w, "h1"), HookStatus::Deferred);
        assert_eq!(normalize_hook_status(Some("搁置"), &mut w, "h1"), HookStatus::Deferred);
        assert_eq!(normalize_hook_status(Some("not-started"), &mut w, "h1"), HookStatus::Deferred);
    }

    #[test]
    fn normalize_hook_status_matches_progressing_synonyms() {
        let mut w = Vec::new();
        assert_eq!(normalize_hook_status(Some("progressing"), &mut w, "h1"), HookStatus::Progressing);
        assert_eq!(normalize_hook_status(Some("confirmed-hit"), &mut w, "h1"), HookStatus::Progressing);
        assert_eq!(normalize_hook_status(Some("已推进"), &mut w, "h1"), HookStatus::Progressing);
    }

    #[test]
    fn normalize_hook_status_matches_open_synonyms() {
        let mut w = Vec::new();
        assert_eq!(normalize_hook_status(Some("open"), &mut w, "h1"), HookStatus::Open);
        assert_eq!(normalize_hook_status(Some("planted"), &mut w, "h1"), HookStatus::Open);
        assert_eq!(normalize_hook_status(Some("已种下"), &mut w, "h1"), HookStatus::Open);
        assert_eq!(normalize_hook_status(Some(""), &mut w, "h1"), HookStatus::Open);
        assert_eq!(normalize_hook_status(None, &mut w, "h1"), HookStatus::Open);
    }

    #[test]
    fn normalize_hook_status_unrecognized_warns_and_falls_back_open() {
        let mut w = Vec::new();
        let s = normalize_hook_status(Some("????"), &mut w, "h9");
        assert_eq!(s, HookStatus::Open);
        assert_eq!(w.len(), 1);
        assert!(w[0].contains("h9:status"));
        assert!(w[0].contains("????"));
    }

    #[test]
    fn normalize_hook_type_preserves_non_empty() {
        let mut w = Vec::new();
        assert_eq!(normalize_hook_type(Some("  mystery  "), &mut w, "h1"), "mystery");
        assert!(w.is_empty());
    }

    #[test]
    fn normalize_hook_type_empty_falls_back_unspecified_with_warning() {
        let mut w = Vec::new();
        assert_eq!(normalize_hook_type(Some("   "), &mut w, "h1"), "unspecified");
        assert_eq!(normalize_hook_type(None, &mut w, "h1"), "unspecified");
        // 两次产生相同 warning，append_warning 去重 → 仅 1 条。
        assert_eq!(w.len(), 1);
    }

    #[test]
    fn parse_strict_integer_cell_rejects_narrative() {
        assert_eq!(parse_strict_integer_cell(Some("12")), Some(12));
        assert_eq!(parse_strict_integer_cell(Some("第141号文明")), None);
        assert_eq!(parse_strict_integer_cell(Some("12章")), None);
        assert_eq!(parse_strict_integer_cell(Some("")), None);
        assert_eq!(parse_strict_integer_cell(None), None);
    }

    #[test]
    fn parse_strict_integer_with_warning_emits_warning_on_invalid() {
        let mut w = Vec::new();
        assert_eq!(parse_strict_integer_with_warning(Some("12"), &mut w, "f"), 12);
        assert!(w.is_empty());
        assert_eq!(parse_strict_integer_with_warning(Some("x"), &mut w, "f"), 0);
        assert_eq!(w.len(), 1);
    }

    #[test]
    fn parse_integer_with_fallback_uses_fallback_when_no_digits() {
        let mut w = Vec::new();
        assert_eq!(parse_integer_with_fallback(Some("42"), 9, &mut w, "f"), 42);
        assert_eq!(parse_integer_with_fallback(Some("abc"), 9, &mut w, "f"), 9);
        assert_eq!(w.len(), 1);
        assert_eq!(parse_integer_with_fallback(None, -5, &mut w, "f"), 0, "负 fallback → max(0,..)=0");
    }

    #[test]
    fn normalize_explicit_chapter_rejects_non_positive() {
        assert_eq!(normalize_explicit_chapter(Some(7)), 7);
        assert_eq!(normalize_explicit_chapter(Some(0)), 0);
        assert_eq!(normalize_explicit_chapter(Some(-3)), 0);
        assert_eq!(normalize_explicit_chapter(None), 0);
    }

    #[test]
    fn append_warning_deduplicates() {
        let mut w = Vec::new();
        append_warning(&mut w, "a");
        append_warning(&mut w, "a");
        append_warning(&mut w, "b");
        assert_eq!(w, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn unique_strings_drops_empty_and_dedups_preserving_order() {
        let v: Vec<String> = vec!["a".into(), "".into(), "b".into(), "a".into(), "  ".into(), "c".into()];
        assert_eq!(unique_strings(&v), vec!["a".to_string(), "b".to_string(), "c".to_string()]);
    }

    // --- async fs 编排（durable progress）----------------------------------------

    fn setup_book_with_chapters(store: &InMemoryStateStore, book_dir: &str, files: &[(&str, &str)]) {
        for (name, content) in files {
            store.set(&format!("{book_dir}/chapters/{name}"), content);
        }
    }

    #[tokio::test]
    async fn resolve_durable_progress_from_chapter_filename_prefix() {
        let store = InMemoryStateStore::new();
        setup_book_with_chapters(&store, "book", &[
            ("1_intro.md", "x"),
            ("2_rise.md", "x"),
            ("3_climax.md", "x"),
            ("notes.md", "x"), // 无 N_ 前缀，忽略
        ]);
        // 1/2/3 连续 → 前缀 3。
        assert_eq!(
            resolve_durable_story_progress(&store, "book", None).await.unwrap(),
            3
        );
    }

    #[tokio::test]
    async fn resolve_durable_progress_takes_max_with_fallback() {
        let store = InMemoryStateStore::new();
        setup_book_with_chapters(&store, "book", &[("1_a.md", "x"), ("2_b.md", "x")]);
        // 连续前缀 2，fallback 5 → max = 5。
        assert_eq!(
            resolve_durable_story_progress(&store, "book", Some(5)).await.unwrap(),
            5
        );
        // 连续前缀 2，fallback 1 → max = 2。
        assert_eq!(
            resolve_durable_story_progress(&store, "book", Some(1)).await.unwrap(),
            2
        );
    }

    #[tokio::test]
    async fn resolve_durable_progress_ignores_non_positive_fallback() {
        let store = InMemoryStateStore::new();
        setup_book_with_chapters(&store, "book", &[("1_a.md", "x")]);
        // fallback=0 / 负数 → 视为 0，取连续前缀 1。
        assert_eq!(resolve_durable_story_progress(&store, "book", Some(0)).await.unwrap(), 1);
        assert_eq!(resolve_durable_story_progress(&store, "book", Some(-3)).await.unwrap(), 1);
    }

    #[tokio::test]
    async fn resolve_durable_progress_merges_index_json_and_filenames() {
        let store = InMemoryStateStore::new();
        // index.json 给 1,2；文件名给 3,4 → 合并后连续前缀 4。
        store.set(
            "book/chapters/index.json",
            r#"[{"number":1,"title":"a"},{"number":2,"title":"b"}]"#,
        );
        setup_book_with_chapters(&store, "book", &[("3_c.md", "x"), ("4_d.md", "x")]);
        assert_eq!(
            resolve_durable_story_progress(&store, "book", None).await.unwrap(),
            4
        );
    }

    #[tokio::test]
    async fn resolve_durable_progress_index_json_with_invalid_entries_filtered() {
        let store = InMemoryStateStore::new();
        // 非 number / 非正整数 / 非整数 全部过滤。
        store.set(
            "book/chapters/index.json",
            r#"[{"number":1},{"number":"x"},{"number":0},{"number":-5},{"title":"no-num"},{"number":2}]"#,
        );
        assert_eq!(
            resolve_durable_story_progress(&store, "book", None).await.unwrap(),
            2
        );
    }

    #[tokio::test]
    async fn resolve_durable_progress_empty_book_returns_fallback() {
        let store = InMemoryStateStore::new();
        // 无任何章节产物 → 前缀 0，取 fallback。
        assert_eq!(resolve_durable_story_progress(&store, "book", Some(7)).await.unwrap(), 7);
        assert_eq!(resolve_durable_story_progress(&store, "book", None).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn resolve_durable_progress_gap_caps_at_contiguous_prefix() {
        let store = InMemoryStateStore::new();
        setup_book_with_chapters(&store, "book", &[
            ("1_a.md", "x"),
            ("2_b.md", "x"),
            ("4_d.md", "x"), // 缺 3，连续前缀只到 2
        ]);
        assert_eq!(
            resolve_durable_story_progress(&store, "book", None).await.unwrap(),
            2
        );
    }

    // --- 编排中间件（language / repair / load）-------------------------------

    #[tokio::test]
    async fn resolve_runtime_language_zh_when_book_json_says_zh() {
        let store = InMemoryStateStore::new();
        store.set("book/book.json", r#"{"language":"zh"}"#);
        assert_eq!(
            resolve_runtime_language(&store, "book").await.unwrap(),
            WritingLanguage::Zh
        );
    }

    #[tokio::test]
    async fn resolve_runtime_language_en_when_missing_or_invalid() {
        let store = InMemoryStateStore::new();
        assert_eq!(resolve_runtime_language(&store, "book").await.unwrap(), WritingLanguage::En);
        store.set("book/book.json", r#"{"language":"en"}"#);
        assert_eq!(resolve_runtime_language(&store, "book").await.unwrap(), WritingLanguage::En);
        store.set("book/book.json", r#"{"language":"fr"}"#);
        assert_eq!(resolve_runtime_language(&store, "book").await.unwrap(), WritingLanguage::En);
        store.set("book/book.json", "not json");
        assert_eq!(resolve_runtime_language(&store, "book").await.unwrap(), WritingLanguage::En);
    }

    #[test]
    fn repair_hooks_state_value_fills_empty_type_with_warning() {
        let mut value = serde_json::json!({
            "hooks": [
                { "hookId": "h1", "type": "mystery" },
                { "hookId": "h2", "type": "   " },
                { "hookId": "h3" }
            ]
        });
        let mut warnings = Vec::new();
        let changed = repair_hooks_state_value(&mut value, &mut warnings);
        assert!(changed);
        assert_eq!(value["hooks"][1]["type"], "unspecified");
        assert_eq!(value["hooks"][2]["type"], "unspecified");
        assert_eq!(value["hooks"][0]["type"], "mystery", "非空 type 不变");
        assert_eq!(warnings.len(), 2, "两个空 type 各一条 warning");
        assert!(warnings.iter().all(|w| w.contains("empty hook type")));
    }

    #[test]
    fn repair_hooks_state_value_trims_surrounding_whitespace() {
        let mut value = serde_json::json!({ "hooks": [{ "hookId": "h1", "type": "  mystery  " }] });
        let mut warnings = Vec::new();
        assert!(repair_hooks_state_value(&mut value, &mut warnings));
        assert_eq!(value["hooks"][0]["type"], "mystery");
        assert!(warnings.is_empty(), "trim 不产 warning");
    }

    #[test]
    fn repair_hooks_state_value_no_change_when_all_types_present() {
        let mut value = serde_json::json!({ "hooks": [{ "hookId": "h1", "type": "mystery" }] });
        let mut warnings = Vec::new();
        assert!(!repair_hooks_state_value(&mut value, &mut warnings));
        assert!(warnings.is_empty());
    }

    #[test]
    fn repair_hooks_state_value_noop_when_hooks_missing_or_not_array() {
        let mut v1 = serde_json::json!({ "foo": 1 });
        let mut w = Vec::new();
        assert!(!repair_hooks_state_value(&mut v1, &mut w));
        let mut v2 = serde_json::json!({ "hooks": "notarray" });
        assert!(!repair_hooks_state_value(&mut v2, &mut w));
    }

    #[test]
    fn repair_hooks_state_value_uses_index_when_hookid_missing() {
        let mut value = serde_json::json!({ "hooks": [{ "type": "" }] });
        let mut warnings = Vec::new();
        repair_hooks_state_value(&mut value, &mut warnings);
        assert!(warnings[0].contains("hooks[0]"), "缺失 hookId 时用索引占位");
    }

    #[tokio::test]
    async fn load_json_if_valid_returns_none_when_absent_no_warning() {
        let store = InMemoryStateStore::new();
        let mut warnings = Vec::new();
        let result: Option<StateManifest> =
            load_json_if_valid(&store, "book/manifest.json", &mut warnings, "manifest.json")
                .await
                .unwrap();
        assert!(result.is_none());
        assert!(warnings.is_empty(), "文件缺失不产 warning");
    }

    #[tokio::test]
    async fn load_json_if_valid_returns_parsed_and_warns_on_invalid() {
        let store = InMemoryStateStore::new();
        store.set(
            "m.json",
            r#"{"schemaVersion":2,"language":"zh","lastAppliedChapter":3,"projectionVersion":1}"#,
        );
        let mut warnings = Vec::new();
        let manifest: Option<StateManifest> =
            load_json_if_valid(&store, "m.json", &mut warnings, "manifest.json").await.unwrap();
        let manifest = manifest.expect("合法 JSON 应解析");
        assert_eq!(manifest.schema_version, 2);
        assert_eq!(manifest.last_applied_chapter, 3);
        assert!(warnings.is_empty());

        // 非法 JSON → None + warning。
        store.set("bad.json", "{not json");
        let bad: Option<StateManifest> =
            load_json_if_valid(&store, "bad.json", &mut warnings, "bad.json").await.unwrap();
        assert!(bad.is_none());
        assert_eq!(warnings.len(), 1);
    }

    #[tokio::test]
    async fn load_hooks_state_if_valid_repairs_and_reports_changed() {
        let store = InMemoryStateStore::new();
        // 完整 hook（仅 type 为空待 repair）；其余字段齐全以满足反序列化。
        store.set(
            "hooks.json",
            r#"{"hooks":[{"hookId":"h1","startChapter":1,"type":"  ","status":"open","lastAdvancedChapter":1,"expectedPayoff":"x","notes":"n"}]}"#,
        );
        let mut warnings = Vec::new();
        let (state, repaired) = load_hooks_state_if_valid(&store, "hooks.json", &mut warnings, "hooks.json")
            .await
            .unwrap()
            .expect("应解析");
        assert!(repaired, "空 type 被 repair → changed=true");
        assert_eq!(state.hooks[0].hook_type, "unspecified");
        assert!(!warnings.is_empty());
    }

    #[tokio::test]
    async fn load_hooks_state_if_valid_none_when_absent() {
        let store = InMemoryStateStore::new();
        let mut warnings = Vec::new();
        let result = load_hooks_state_if_valid(&store, "hooks.json", &mut warnings, "hooks.json")
            .await
            .unwrap();
        assert!(result.is_none());
        assert!(warnings.is_empty(), "缺失不产 warning");
        let _ = (HooksState::default(),); // 确认类型可构造（compile check）
    }

    // --- markdown 引导层 --------------------------------------------------------

    #[test]
    fn parse_pending_hooks_state_markdown_normalizes_empty_type() {
        // 8 列 hook，type 为空 → normalize "unspecified" + warning。
        let md = "| h01 | 1 | mystery | open | 1 | x | mid-arc | n |\n| h02 | 2 |    | open | 2 | y | mid-arc | n2 |";
        let mut warnings = Vec::new();
        let state = parse_pending_hooks_state_markdown(md, &mut warnings);
        assert_eq!(state.hooks.len(), 2);
        assert_eq!(state.hooks[0].hook_type, "mystery");
        assert_eq!(state.hooks[1].hook_type, "unspecified");
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn parse_pending_hooks_state_markdown_bullet_fallback() {
        let md = "- 线索一\n- 线索二";
        let mut warnings = Vec::new();
        let state = parse_pending_hooks_state_markdown(md, &mut warnings);
        assert_eq!(state.hooks.len(), 2);
        assert_eq!(state.hooks[0].hook_id, "hook-1");
        assert_eq!(state.hooks[0].hook_type, "unspecified", "bullet fallback type 已是 unspecified，不再 warning");
        assert!(warnings.is_empty());
    }

    #[test]
    fn parse_current_state_state_markdown_table_extracts_facts_and_chapter() {
        let md = "| 字段 | 值 |\n|---|---|\n| 当前章节 | 7 |\n| 当前位置 | 森林 |\n| 当前目标 | 圣杯 |";
        let mut warnings = Vec::new();
        let state = parse_current_state_state_markdown(md, 99, &mut warnings);
        assert_eq!(state.chapter, 7);
        assert_eq!(state.facts.len(), 2, "「当前章节」行不产 fact");
        assert_eq!(state.facts[0].predicate, "当前位置");
        assert_eq!(state.facts[0].subject, "protagonist");
        assert_eq!(state.facts[0].valid_from_chapter, 7);
        assert_eq!(state.facts[1].predicate, "当前目标");
    }

    #[test]
    fn parse_current_state_state_markdown_invalid_chapter_warns_and_uses_fallback() {
        // 「当前章节」值非数字 → parse_integer_with_fallback warning + fallback。
        let md = "| 字段 | 值 |\n|---|---|\n| 当前章节 | abc |\n| 当前目标 | x |";
        let mut warnings = Vec::new();
        let state = parse_current_state_state_markdown(md, 5, &mut warnings);
        assert_eq!(state.chapter, 5, "无效 chapter → fallback 5");
        assert!(!warnings.is_empty());
    }

    #[test]
    fn parse_current_state_state_markdown_bullet_fallback_uses_fallback_chapter() {
        let md = "- 笔记一";
        let mut warnings = Vec::new();
        let state = parse_current_state_state_markdown(md, 3, &mut warnings);
        assert_eq!(state.chapter, 3);
        assert_eq!(state.facts.len(), 1);
        assert_eq!(state.facts[0].predicate, "note_1");
        assert_eq!(state.facts[0].subject, "current_state");
        assert_eq!(state.facts[0].valid_from_chapter, 3);
    }

    #[tokio::test]
    async fn load_markdown_summaries_state_deduplicates_and_sorts() {
        let store = InMemoryStateStore::new();
        store.set(
            "story/chapter_summaries.md",
            "| 2 | t2 |\n| 1 | t1-old |\n| 1 | t1-new |\n| note | x |",
        );
        let state = load_markdown_summaries_state(&store, "story").await.unwrap();
        let chapters: Vec<u32> = state.rows.iter().map(|r| r.chapter).collect();
        assert_eq!(chapters, vec![1, 2], "去重（后覆盖）+ 升序；非数字首列过滤");
        assert_eq!(state.rows[0].title, "t1-new");
        let _ = ChapterSummaryRow::default();
    }

    #[tokio::test]
    async fn load_markdown_summaries_state_absent_returns_empty() {
        let store = InMemoryStateStore::new();
        let state = load_markdown_summaries_state(&store, "story").await.unwrap();
        assert!(state.rows.is_empty());
    }

    #[tokio::test]
    async fn load_markdown_bootstrap_state_aggregates_all_three_with_durable_progress() {
        let store = InMemoryStateStore::new();
        store.set("book/chapters/1_a.md", "x");
        store.set("book/chapters/2_b.md", "x");
        store.set("story/chapter_summaries.md", "| 1 | t1 |");
        store.set("story/pending_hooks.md", "| h01 | 1 | mystery | open | 1 | x | mid-arc | n |");
        store.set("story/current_state.md", "| 字段 | 值 |\n|---|---|\n| 当前章节 | 5 |\n| 当前位置 | 森林 |");

        let mut warnings = Vec::new();
        let bs = load_markdown_bootstrap_state(&store, "book", "story", 3, &mut warnings)
            .await
            .unwrap();
        assert_eq!(bs.summaries_state.rows.len(), 1);
        assert_eq!(bs.hooks_state.hooks.len(), 1);
        assert_eq!(bs.current_state.facts.len(), 1);
        assert_eq!(bs.current_state.chapter, 5);
        // durable = max(fallback=3, 产物前缀=2) = 3。
        assert_eq!(bs.durable_story_progress, 3);
    }

    #[tokio::test]
    async fn load_markdown_bootstrap_state_durable_caps_when_no_explicit_fallback() {
        let store = InMemoryStateStore::new();
        store.set("book/chapters/1_a.md", "x");
        store.set("book/chapters/2_b.md", "x");
        store.set("book/chapters/3_c.md", "x");
        // fallback=0 → durable = max(0, 3) = 3。
        let mut warnings = Vec::new();
        let bs = load_markdown_bootstrap_state(&store, "book", "story", 0, &mut warnings)
            .await
            .unwrap();
        assert_eq!(bs.durable_story_progress, 3);
    }

    // --- load_or_bootstrap + 主入口 ---------------------------------------------

    #[tokio::test]
    async fn bootstrap_creates_all_files_when_state_absent() {
        let store = InMemoryStateStore::new();
        store.set("book/book.json", r#"{"language":"zh"}"#);
        store.set("book/chapters/1_a.md", "x");
        store.set("book/chapters/2_b.md", "x");
        store.set("book/story/chapter_summaries.md", "| 1 | t1 |");
        store.set(
            "book/story/pending_hooks.md",
            "| h01 | 1 | mystery | open | 1 | x | mid-arc | n |",
        );
        store.set("book/story/current_state.md", "| 字段 | 值 |\n|---|---|\n| 当前章节 | 2 |\n| 当前位置 | 森林 |");

        let result = bootstrap_structured_state_from_markdown(&store, "book", None)
            .await
            .unwrap();
        // 4 个文件全部新建。
        assert_eq!(result.created_files.len(), 4);
        assert!(result.created_files.contains(&"manifest.json".to_string()));
        assert!(result.created_files.contains(&"current_state.json".to_string()));
        assert!(result.created_files.contains(&"hooks.json".to_string()));
        assert!(result.created_files.contains(&"chapter_summaries.json".to_string()));
        // manifest 用 durable progress（2）+ zh。
        assert_eq!(result.manifest.language, "zh");
        assert_eq!(result.manifest.last_applied_chapter, 2);
        assert_eq!(result.manifest.schema_version, 2);
        // 文件确实写入 store。
        assert!(store.read_to_string("book/story/state/manifest.json").await.unwrap().is_some());
        assert!(store.read_to_string("book/story/state/hooks.json").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn bootstrap_preserves_existing_valid_json_and_recomputes_manifest() {
        let store = InMemoryStateStore::new();
        // 预置合法 JSON 状态。
        store.set(
            "book/story/state/manifest.json",
            r#"{"schemaVersion":2,"language":"en","lastAppliedChapter":99,"projectionVersion":3,"migrationWarnings":[]}"#,
        );
        store.set(
            "book/story/state/chapter_summaries.json",
            r#"{"rows":[{"chapter":1,"title":"t1","characters":"","events":"","stateChanges":"","hookActivity":"","mood":"","chapterType":""}]}"#,
        );
        store.set("book/book.json", r#"{"language":"en"}"#);
        store.set("book/chapters/1_a.md", "x");

        let result = bootstrap_structured_state_from_markdown(&store, "book", None)
            .await
            .unwrap();
        // manifest 存在 → 不在 createdFiles。
        assert!(!result.created_files.contains(&"manifest.json".to_string()));
        // lastAppliedChapter=99 > derived(1) → 规范化到 1 + warning。
        assert_eq!(result.manifest.last_applied_chapter, 1);
        assert!(result.warnings.iter().any(|w| w.contains("normalized from 99 to 1")));
        // projection_version 保留。
        assert_eq!(result.manifest.projection_version, 3);
        // 已存在的 summaries.json 不新建（load_or_bootstrap 命中 existing 分支）。
        assert!(!result.created_files.contains(&"chapter_summaries.json".to_string()));
    }

    #[tokio::test]
    async fn bootstrap_repairs_hooks_with_empty_type_and_persists() {
        let store = InMemoryStateStore::new();
        store.set("book/book.json", r#"{"language":"en"}"#);
        // hooks.json 含空 type（完整字段以便反序列化）。
        store.set(
            "book/story/state/hooks.json",
            r#"{"hooks":[{"hookId":"h1","startChapter":1,"type":"  ","status":"open","lastAdvancedChapter":1,"expectedPayoff":"x","notes":"n"}]}"#,
        );

        let result = bootstrap_structured_state_from_markdown(&store, "book", None)
            .await
            .unwrap();
        // 修复后的 hooks.json 写回（repaired=true）。
        let written = store
            .read_to_string("book/story/state/hooks.json")
            .await
            .unwrap()
            .unwrap();
        assert!(written.contains("\"unspecified\""), "修复后 type 应为 unspecified");
        assert!(result.warnings.iter().any(|w| w.contains("empty hook type")));
    }

    #[tokio::test]
    async fn bootstrap_invalid_json_warns_and_rebuilds_from_markdown() {
        let store = InMemoryStateStore::new();
        store.set("book/book.json", r#"{"language":"en"}"#);
        store.set("book/chapters/1_a.md", "x");
        // 损坏的 summaries.json（不是 JSON）。
        store.set("book/story/state/chapter_summaries.json", "not json");
        store.set("book/story/chapter_summaries.md", "| 1 | from-md |");

        let result = bootstrap_structured_state_from_markdown(&store, "book", None)
            .await
            .unwrap();
        // 损坏 → warning + 从 markdown 重建（因 state 文件已存在，不计入 createdFiles）。
        assert!(result.warnings.iter().any(|w| w.contains("chapter_summaries.json invalid")));
        let written = store
            .read_to_string("book/story/state/chapter_summaries.json")
            .await
            .unwrap()
            .unwrap();
        assert!(written.contains("from-md"), "从 markdown 重建");
    }

    #[tokio::test]
    async fn bootstrap_deduplicates_summaries_loaded_from_json() {
        let store = InMemoryStateStore::new();
        store.set("book/book.json", r#"{"language":"en"}"#);
        // summaries.json 含重复 chapter（1 出现两次）。
        store.set(
            "book/story/state/chapter_summaries.json",
            r#"{"rows":[{"chapter":1,"title":"old","characters":"","events":"","stateChanges":"","hookActivity":"","mood":"","chapterType":""},{"chapter":1,"title":"new","characters":"","events":"","stateChanges":"","hookActivity":"","mood":"","chapterType":""}]}"#,
        );

        bootstrap_structured_state_from_markdown(&store, "book", None)
            .await
            .unwrap();
        let written = store
            .read_to_string("book/story/state/chapter_summaries.json")
            .await
            .unwrap()
            .unwrap();
        // 去重后只剩 1 行（后覆盖），写回。
        let parsed: ChapterSummariesState = serde_json::from_str(&written).unwrap();
        assert_eq!(parsed.rows.len(), 1);
        assert_eq!(parsed.rows[0].title, "new");
    }

    #[tokio::test]
    async fn bootstrap_uses_fallback_when_no_artifacts() {
        let store = InMemoryStateStore::new();
        store.set("book/book.json", r#"{"language":"en"}"#);
        // 无章节产物，无 md → durable = max(fallback=5, 0) = 5。
        let result = bootstrap_structured_state_from_markdown(&store, "book", Some(5))
            .await
            .unwrap();
        assert_eq!(result.manifest.last_applied_chapter, 5);
    }
}
