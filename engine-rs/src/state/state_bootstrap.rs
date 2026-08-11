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

use crate::models::runtime_state::HookStatus;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::store::InMemoryStateStore;

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
}
