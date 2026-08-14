//! 长篇疲劳检测 —— 纯文本核心。
//!
//! 移植自 `packages/core/src/utils/long-span-fatigue.ts`（545 行）：
//! - [`dice_coefficient`] / [`build_bigrams`]：bigram Sørensen–Dice 相似度
//! - [`extract_boundary_sentence`]：提取章节首/尾句
//! - [`normalize_sentence`] / [`summarize_sentence`]：句式归一化与摘要
//! - [`build_english_variance_brief`]：writer en 长程变化性简报（读 chapters/ +
//!   chapter_summaries.md，调 [`analyze_chapter_cadence`]）
//!
//! ## 待移植（pipeline 域消费）
//! analyzeLongSpanFatigue（读盘 + cadence + 首尾句同构 issue 组装）——writer
//! 不依赖，随 pipeline 编排落地。

use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::OnceLock;

use crate::utils::cadence_policy::{window_defaults, CadencePressure};
use crate::utils::chapter_cadence::{analyze_chapter_cadence, CadenceSummaryRow, ChapterCadenceAnalysis};
use crate::utils::language::WritingLanguage;

const ENGLISH_STOP_WORDS_LSF: &[&str] = &[
    "the", "and", "but", "with", "from", "into", "that", "this", "there",
    "again", "while", "after", "before", "were", "was", "had", "has", "have", "kept",
];

/// 中文字符与标点（移植 TS CHINESE_PUNCTUATION）。
fn chinese_punct_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"[，。！？；：“”‘’（）《》、\s\-—…·]").unwrap())
}
/// 英文非字母数字（移植 TS ENGLISH_PUNCTUATION /[^a-z0-9]+/gi）。
fn english_punct_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)[^a-z0-9]+").unwrap())
}

/// Dice 相似度（bigram）。相同→1；长度<2→0；否则 2*overlap/(总 bigram 数)。
pub fn dice_coefficient(left: &str, right: &str) -> f64 {
    if left == right {
        return 1.0;
    }
    let left_chars: Vec<char> = left.chars().collect();
    let right_chars: Vec<char> = right.chars().collect();
    if left_chars.len() < 2 || right_chars.len() < 2 {
        return 0.0;
    }
    let left_bigrams = build_bigrams(left);
    let right_bigrams = build_bigrams(right);
    let mut overlap = 0u32;
    for (bg, count) in &left_bigrams {
        overlap += (*count).min(*right_bigrams.get(bg).unwrap_or(&0));
    }
    let left_count: u32 = left_bigrams.values().sum();
    let right_count: u32 = right_bigrams.values().sum();
    (2.0 * overlap as f64) / (left_count + right_count) as f64
}

/// 构建 bigram 计数（按 UTF-16 码元为单位的 2-gram；归一化后输出全 BMP，chars 与 UTF-16 一致）。
pub fn build_bigrams(value: &str) -> HashMap<String, u32> {
    let chars: Vec<char> = value.chars().collect();
    let mut result: HashMap<String, u32> = HashMap::new();
    for w in chars.windows(2) {
        let bg: String = w.iter().collect();
        *result.entry(bg).or_insert(0) += 1;
    }
    result
}

/// 提取章节首句或尾句：扁平非空非标题行，按句末标点分句（保留标点，对齐 TS lookbehind），取首/尾。
pub fn extract_boundary_sentence(content: &str, boundary: Boundary) -> Option<String> {
    let flattened: String = content
        .split('\n')
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect::<Vec<_>>()
        .join(" ");
    let sentences = split_sentences(&flattened);
    if sentences.is_empty() {
        return None;
    }
    match boundary {
        Boundary::Opening => Some(sentences[0].clone()),
        Boundary::Ending => Some(sentences.last().unwrap().clone()),
    }
}

/// 按句末标点（。！？!?\.）分句，保留标点在前句，句末空白剥离（对齐 TS `/(?<=[。！？!?\.])\s*/`）。
fn split_sentences(s: &str) -> Vec<String> {
    let enders = ['。', '！', '？', '!', '?', '.'];
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    for c in s.chars() {
        cur.push(c);
        if enders.contains(&c) {
            let trimmed = cur.trim().to_string();
            if !trimmed.is_empty() {
                out.push(trimmed);
            }
            cur.clear();
        }
    }
    let trimmed = cur.trim().to_string();
    if !trimmed.is_empty() {
        out.push(trimmed);
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Boundary {
    Opening,
    Ending,
}

/// 句式归一化：英文 lower + 去非字母数字；中文去标点 + lower。
pub fn normalize_sentence(sentence: &str, language: crate::utils::language::WritingLanguage) -> String {
    match language {
        crate::utils::language::WritingLanguage::En => english_punct_re()
            .replace_all(&sentence.to_lowercase(), "")
            .trim()
            .to_string(),
        crate::utils::language::WritingLanguage::Zh => chinese_punct_re()
            .replace_all(sentence, "")
            .to_lowercase(),
    }
}

/// 句式摘要：英文前 6 词；中文前 12 字符（去标点后）。
pub fn summarize_sentence(sentence: &str, language: crate::utils::language::WritingLanguage) -> String {
    match language {
        crate::utils::language::WritingLanguage::En => {
            let lower = sentence.to_lowercase();
            let cleaned = english_punct_re().replace_all(&lower, " ");
            let words: Vec<&str> = cleaned.split_whitespace().take(6).collect();
            let joined = words.join(" ");
            if !joined.is_empty() { joined } else { sentence.chars().take(32).collect() }
        }
        crate::utils::language::WritingLanguage::Zh => {
            let collapsed: String = chinese_punct_re().replace_all(sentence, "").into_owned();
            collapsed.chars().take(12).collect()
        }
    }
}

/// 英文停用词集合（LSF 局部版，移植自 long-span-fatigue.ts）。
pub fn is_english_stop_word_lsf(token: &str) -> bool {
    ENGLISH_STOP_WORDS_LSF.contains(&token)
}

// ---- English variance brief（IO 编排，writer en 路径消费） ----

/// 英文变化性简报。对齐 TS `EnglishVarianceBrief`。
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct EnglishVarianceBrief {
    pub high_frequency_phrases: Vec<String>,
    pub repeated_opening_patterns: Vec<String>,
    pub repeated_ending_shapes: Vec<String>,
    pub scene_obligation: String,
    pub text: String,
}

/// 构建 en 长程变化性简报。对齐 TS `buildEnglishVarianceBrief`：
/// 不足 2 章前文 → None；否则统计高频三词短语 / 首尾句式重复 + cadence 场景义务。
pub async fn build_english_variance_brief(
    book_dir: &Path,
    chapter_number: u32,
) -> Option<EnglishVarianceBrief> {
    let chapter_bodies = load_previous_chapter_bodies(
        book_dir,
        chapter_number,
        window_defaults::ENGLISH_VARIANCE_LOOKBACK,
    )
    .await;
    if chapter_bodies.len() < 2 {
        return None;
    }

    let summary_rows = load_summary_rows(
        &book_dir.join("story").join("chapter_summaries.md"),
    )
    .await;
    let mut recent_rows: Vec<CadenceSummaryRow> = summary_rows
        .into_iter()
        .filter(|row| row.chapter < chapter_number)
        .collect();
    recent_rows.sort_by_key(|row| row.chapter);
    let start = recent_rows.len().saturating_sub(window_defaults::SUMMARY_LOOKBACK as usize);
    let recent_rows = recent_rows[start..].to_vec();

    let high_frequency_phrases = collect_repeated_english_phrases(&chapter_bodies);
    let repeated_opening_patterns =
        collect_repeated_boundary_patterns(&chapter_bodies, Boundary::Opening);
    let repeated_ending_shapes =
        collect_repeated_boundary_patterns(&chapter_bodies, Boundary::Ending);
    let cadence = analyze_chapter_cadence(&recent_rows, WritingLanguage::En);
    let scene_obligation = choose_scene_obligation(
        &cadence,
        &repeated_opening_patterns,
        &repeated_ending_shapes,
    );

    let lines = [
        "## English Variance Brief".to_string(),
        String::new(),
        format!(
            "- High-frequency phrases to avoid: {}",
            format_english_list(&high_frequency_phrases)
        ),
        format!(
            "- Repeated opening patterns to avoid: {}",
            format_english_list(&repeated_opening_patterns)
        ),
        format!(
            "- Repeated ending patterns to avoid: {}",
            format_english_list(&repeated_ending_shapes)
        ),
        format!("- Scene obligation: {scene_obligation}"),
    ];

    Some(EnglishVarianceBrief {
        high_frequency_phrases,
        repeated_opening_patterns,
        repeated_ending_shapes,
        scene_obligation: scene_obligation.to_string(),
        text: lines.join("\n"),
    })
}

async fn load_summary_rows(path: &Path) -> Vec<CadenceSummaryRow> {
    let raw = match tokio::fs::read_to_string(path).await {
        Ok(raw) => raw,
        Err(_) => return Vec::new(),
    };
    raw.lines().filter_map(parse_summary_row).collect()
}

/// 加载 currentChapter 之前的章节正文（按章节号升序取末 limit 篇）。
/// 对齐 TS `loadPreviousChapterBodies`：文件名前 4 字符 parseInt 前缀解析。
async fn load_previous_chapter_bodies(
    book_dir: &Path,
    current_chapter: u32,
    limit: u32,
) -> Vec<String> {
    let chapters_dir = book_dir.join("chapters");
    let files = match tokio::fs::read_dir(&chapters_dir).await {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };

    let mut previous: Vec<(u32, std::path::PathBuf)> = Vec::new();
    let mut entries = files;
    while let Ok(Some(entry)) = entries.next_entry().await {
        let file_name = entry.file_name().to_string_lossy().into_owned();
        if !file_name.ends_with(".md") {
            continue;
        }
        let Some(chapter) = file_name
            .get(..4.min(file_name.len()))
            .and_then(parse_int_prefix)
        else {
            continue;
        };
        if chapter < current_chapter {
            previous.push((chapter, entry.path()));
        }
    }
    previous.sort_by_key(|(chapter, _)| *chapter);
    let start = previous.len().saturating_sub(limit as usize);

    let mut bodies = Vec::with_capacity(previous.len() - start);
    for (_, path) in &previous[start..] {
        if let Ok(body) = tokio::fs::read_to_string(path).await {
            bodies.push(body);
        }
    }
    bodies
}

/// 对齐 JS `Number.parseInt(value, 10)`：前导数字前缀解析，无数字 → None。
fn parse_int_prefix(value: &str) -> Option<u32> {
    let digits: String = value
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse::<u32>().ok()
}

/// 解析摘要表格行。对齐 TS `parseSummaryRow`：`|` 开头且非表头/分隔行，
/// 竖线切分 trim 后**过滤空单元格**再取列（索引对齐 TS 的 cells[i]）。
fn parse_summary_row(line: &str) -> Option<CadenceSummaryRow> {
    let trimmed = line.trim();
    if !trimmed.starts_with('|')
        || trimmed.contains("章节 |")
        || trimmed.contains("Chapter |")
        || trimmed.contains("---")
    {
        return None;
    }

    let cells: Vec<&str> = trimmed
        .split('|')
        .map(|cell| cell.trim())
        .filter(|cell| !cell.is_empty())
        .collect();
    if cells.len() < 8 {
        return None;
    }

    let chapter = parse_int_prefix(cells.first().unwrap_or(&""))?;
    if chapter == 0 {
        return None;
    }

    Some(CadenceSummaryRow {
        chapter,
        title: cells.get(1).copied().unwrap_or("").to_string(),
        mood: cells.get(6).copied().unwrap_or("").to_string(),
        chapter_type: cells.get(7).copied().unwrap_or("").to_string(),
    })
}

/// 分词：小写 → 非 [a-z0-9\s] → 空格 → 按空白切分。对齐 TS 的
/// `toLowerCase().replace(/[^a-z0-9\s]+/gi, " ").split(/\s+/)`。
fn tokenize_english(body: &str) -> Vec<String> {
    let lowered = body.to_lowercase();
    let cleaned = non_alnum_run_re().replace_all(&lowered, " ").into_owned();
    cleaned.split_whitespace().map(|t| t.to_string()).collect()
}

fn non_alnum_run_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)[^a-z0-9\s]+").unwrap())
}

/// 跨章高频三词短语（每章内去重，跨章计数 ≥2，频次降序 + 短语升序取前 3）。
/// 对齐 TS `collectRepeatedEnglishPhrases`。
fn collect_repeated_english_phrases(chapter_bodies: &[String]) -> Vec<String> {
    let mut counts: HashMap<String, u32> = HashMap::new();

    for body in chapter_bodies {
        let tokenized = tokenize_english(body);
        let tokens: Vec<&str> = tokenized
            .iter()
            .filter(|token| token.len() >= 3 && !is_english_stop_word_lsf(token))
            .map(|token| token.as_str())
            .collect();
        let mut seen: HashSet<String> = HashSet::new();

        if tokens.len() >= 3 {
            for index in 0..=tokens.len() - 3 {
                let phrase = format!("{} {} {}", tokens[index], tokens[index + 1], tokens[index + 2]);
                seen.insert(phrase);
            }
        }

        for phrase in seen {
            *counts.entry(phrase).or_insert(0) += 1;
        }
    }

    sorted_top_phrases(counts)
}

/// 首尾句式前 4 词重复模式（跨章计数 ≥2 取前 3）。对齐 TS
/// `collectRepeatedBoundaryPatterns`。
fn collect_repeated_boundary_patterns(chapter_bodies: &[String], boundary: Boundary) -> Vec<String> {
    let mut counts: HashMap<String, u32> = HashMap::new();

    for body in chapter_bodies {
        let Some(sentence) = extract_boundary_sentence(body, boundary) else {
            continue;
        };

        let tokenized = tokenize_english(&sentence);
        let tokens: Vec<&str> = tokenized
            .iter()
            .take(4)
            .map(|token| token.as_str())
            .collect();
        if tokens.len() < 2 {
            continue;
        }

        let pattern = tokens.join(" ");
        *counts.entry(pattern).or_insert(0) += 1;
    }

    sorted_top_phrases(counts)
}

/// 计数 ≥2 → 频次降序 → 短语升序 → 前 3。对齐 TS
/// `.filter(count >= 2).sort(b[1]-a[1] || a[0].localeCompare(b[0])).slice(0, 3)`。
fn sorted_top_phrases(counts: HashMap<String, u32>) -> Vec<String> {
    let mut entries: Vec<(String, u32)> = counts.into_iter().filter(|(_, count)| *count >= 2).collect();
    entries.sort_by(|(left_phrase, left_count), (right_phrase, right_count)| {
        right_count
            .cmp(left_count)
            .then_with(|| left_phrase.cmp(right_phrase))
    });
    entries
        .into_iter()
        .take(3)
        .map(|(phrase, _)| phrase)
        .collect()
}

/// 场景义务选择。对齐 TS `chooseSceneObligation`。
fn choose_scene_obligation(
    cadence: &ChapterCadenceAnalysis,
    repeated_openings: &[String],
    repeated_endings: &[String],
) -> &'static str {
    if cadence
        .scene_pressure
        .as_ref()
        .map(|p| p.pressure == CadencePressure::High)
        .unwrap_or(false)
    {
        return "confrontation under pressure";
    }
    if !repeated_endings.is_empty() {
        return "discovery under pressure";
    }
    if !repeated_openings.is_empty() {
        return "negotiation with withholding";
    }
    "concealment with active pushback"
}

fn format_english_list(values: &[String]) -> String {
    if values.is_empty() {
        "none".to_string()
    } else {
        values.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dice_identical() {
        assert_eq!(dice_coefficient("hello", "hello"), 1.0);
    }

    #[test]
    fn dice_short_returns_zero() {
        assert_eq!(dice_coefficient("a", "b"), 0.0);
        assert_eq!(dice_coefficient("ab", "a"), 0.0);
    }

    #[test]
    fn dice_partial_overlap() {
        // "abcd" bigrams: ab bc cd；"abce" bigrams: ab bc ce；overlap=2，total=6 → 4/6
        let d = dice_coefficient("abcd", "abce");
        assert!((d - 0.6667).abs() < 0.001, "实际: {d}");
    }

    #[test]
    fn dice_no_overlap() {
        assert_eq!(dice_coefficient("abcd", "wxyz"), 0.0);
    }

    #[test]
    fn extract_boundary_opening_ending() {
        let content = "# 标题\n第一句开头。第二句。最后结尾。";
        assert_eq!(extract_boundary_sentence(content, Boundary::Opening).unwrap(), "第一句开头。");
        assert_eq!(extract_boundary_sentence(content, Boundary::Ending).unwrap(), "最后结尾。");
    }

    #[test]
    fn extract_boundary_skips_headers() {
        let content = "# H1\n## H2\n真正的第一句。后续。";
        assert_eq!(extract_boundary_sentence(content, Boundary::Opening).unwrap(), "真正的第一句。");
    }

    #[test]
    fn normalize_zh_strips_punct() {
        let n = normalize_sentence("他，走了过来。", crate::utils::language::WritingLanguage::Zh);
        assert_eq!(n, "他走了过来");
    }

    #[test]
    fn normalize_en_lower_and_strip() {
        let n = normalize_sentence("He, walked! Over?", crate::utils::language::WritingLanguage::En);
        assert_eq!(n, "hewalkedover");
    }

    #[test]
    fn summarize_en_first_words() {
        let s = summarize_sentence("The quick brown fox jumps over", crate::utils::language::WritingLanguage::En);
        assert_eq!(s, "the quick brown fox jumps over");
        // 截断到 6 词
        let s2 = summarize_sentence("one two three four five six seven eight", crate::utils::language::WritingLanguage::En);
        assert_eq!(s2, "one two three four five six");
    }

    #[test]
    fn summarize_zh_first_chars() {
        let s = summarize_sentence("他静静地走向那扇古老的门", crate::utils::language::WritingLanguage::Zh);
        assert_eq!(s.chars().count(), 12);
    }

    // ---- buildEnglishVarianceBrief（IO 编排） ----

    async fn variance_fixture() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("临时目录");
        let chapters = dir.path().join("chapters");
        tokio::fs::create_dir_all(&chapters).await.expect("建 chapters");
        // 两章共享首句（"The night was cold."）与内容三词短语 "veiled lantern light"。
        tokio::fs::write(
            chapters.join("0001_ch1.md"),
            "# Chapter 1: One\n\nThe night was cold. He lifted the veiled lantern light and left.",
        )
        .await
        .expect("写第1章");
        tokio::fs::write(
            chapters.join("0002_ch2.md"),
            "# Chapter 2: Two\n\nThe night was cold. She raised the veiled lantern light again.",
        )
        .await
        .expect("写第2章");
        tokio::fs::create_dir_all(dir.path().join("story"))
            .await
            .expect("建 story");
        tokio::fs::write(
            dir.path().join("story/chapter_summaries.md"),
            "# 章节摘要\n\n| 章节 | 标题 | 人物 | 事件 | 状态 | 伏笔 | 情绪 | 类型 |\n|------|------|------|------|------|------|------|------|\n| 1 | One | A | lift | x | H1 | tense | setup |\n| 2 | Two | B | lift | y | H2 | tense | setup |\n",
        )
        .await
        .expect("写摘要");
        dir
    }

    #[tokio::test]
    async fn variance_brief_none_when_single_chapter() {
        let dir = tempfile::tempdir().expect("临时目录");
        assert!(build_english_variance_brief(dir.path(), 3).await.is_none());
    }

    #[tokio::test]
    async fn variance_brief_detects_repetition() {
        let dir = variance_fixture().await;
        let brief = build_english_variance_brief(dir.path(), 3)
            .await
            .expect("应产出简报");
        assert!(
            brief.high_frequency_phrases.iter().any(|p| p == "veiled lantern light"),
            "phrases: {:?}",
            brief.high_frequency_phrases
        );
        assert!(
            brief.repeated_opening_patterns.iter().any(|p| p == "the night was cold"),
            "openings: {:?}",
            brief.repeated_opening_patterns
        );
        // 摘要两章同类型 setup ×2 → scene pressure 未到 high（需 3 连击），
        // 且结尾句式不重复 → 回落 opening 分支。
        assert_eq!(brief.scene_obligation, "negotiation with withholding");
        assert!(brief.text.starts_with("## English Variance Brief"));
        assert!(brief.text.contains("- Scene obligation: "));
    }

    #[tokio::test]
    async fn variance_brief_scene_pressure_takes_priority() {
        let dir = tempfile::tempdir().expect("临时目录");
        let chapters = dir.path().join("chapters");
        tokio::fs::create_dir_all(&chapters).await.expect("建 chapters");
        for num in [1u32, 2, 3] {
            tokio::fs::write(
                chapters.join(format!("{num:04}_c{num}.md")),
                format!("# Chapter {num}\n\nBody {num} with unique words each time."),
            )
            .await
            .expect("写章");
        }
        tokio::fs::create_dir_all(dir.path().join("story"))
            .await
            .expect("建 story");
        tokio::fs::write(
            dir.path().join("story/chapter_summaries.md"),
            "| 章节 | 标题 | 人物 | 事件 | 状态 | 伏笔 | 情绪 | 类型 |\n|---|---|---|---|---|---|---|---|\n| 1 | a | x | e | s | h | m | setup |\n| 2 | b | x | e | s | h | m | setup |\n| 3 | c | x | e | s | h | m | setup |\n",
        )
        .await
        .expect("写摘要");
        let brief = build_english_variance_brief(dir.path(), 4)
            .await
            .expect("应产出简报");
        assert_eq!(brief.scene_obligation, "confrontation under pressure");
    }

    #[test]
    fn parse_int_prefix_matches_js_parseint() {
        assert_eq!(parse_int_prefix("0042"), Some(42));
        assert_eq!(parse_int_prefix("12ab"), Some(12));
        assert_eq!(parse_int_prefix("ab12"), None);
        assert_eq!(parse_int_prefix(""), None);
    }

    #[test]
    fn parse_summary_row_filters_headers_and_short_rows() {
        assert!(parse_summary_row("| 章节 | 标题 |").is_none());
        assert!(parse_summary_row("|---|---|").is_none());
        assert!(parse_summary_row("不是表格行").is_none());
        let row = parse_summary_row("| 3 | 标题 | a | b | c | d | 紧张 | 推进 |")
            .expect("应解析");
        assert_eq!(row.chapter, 3);
        assert_eq!(row.chapter_type, "推进");
        // < 8 列 → None。
        assert!(parse_summary_row("| 3 | t | a | b | c | d | m |").is_none());
    }
}

// ---- analyzeLongSpanFatigue 编排层（32 号预留，42 号 write-next 消费） ----

use crate::agents::continuity::AuditIssue;
use crate::utils::cadence_policy::long_span_fatigue_thresholds;

/// 长跨度疲劳问题。对齐 TS `LongSpanFatigueIssue`（severity 恒 warning）。
#[derive(Debug, Clone, PartialEq)]
pub struct LongSpanFatigueIssue {
    pub severity: &'static str,
    pub category: String,
    pub description: String,
    pub suggestion: String,
}

impl From<LongSpanFatigueIssue> for AuditIssue {
    fn from(issue: LongSpanFatigueIssue) -> AuditIssue {
        AuditIssue {
            severity: crate::agents::continuity::AuditSeverity::Warning,
            category: issue.category,
            description: issue.description,
            suggestion: issue.suggestion,
            repair_scope: None,
        }
    }
}

/// analyze 入参。
pub struct AnalyzeLongSpanFatigueInput<'a> {
    pub book_dir: &'a Path,
    pub chapter_number: u32,
    pub chapter_content: &'a str,
    /// 本章摘要表行（持久化产物的 chapterSummary）。
    pub chapter_summary: Option<&'a str>,
    pub language: WritingLanguage,
}

/// 长跨度疲劳分析：章节类型/情绪/标题三压力 + 首尾句式同构。
pub async fn analyze_long_span_fatigue(
    input: &AnalyzeLongSpanFatigueInput<'_>,
) -> Vec<LongSpanFatigueIssue> {
    let en = input.language == WritingLanguage::En;
    let mut issues: Vec<LongSpanFatigueIssue> = Vec::new();

    let summary_rows =
        load_summary_rows(&input.book_dir.join("story").join("chapter_summaries.md")).await;
    let merged_rows = merge_current_summary(summary_rows, input.chapter_summary);
    let recent: Vec<CadenceSummaryRow> = merged_rows
        .into_iter()
        .filter(|row| row.chapter <= input.chapter_number)
        .collect();
    let cadence = analyze_chapter_cadence(&recent, input.language);

    if let Some(scene) = &cadence.scene_pressure {
        if scene.pressure == CadencePressure::High {
            issues.push(LongSpanFatigueIssue {
                severity: "warning",
                category: (if en { "Pacing Monotony" } else { "节奏单调" }).to_string(),
                description: if en {
                    format!(
                        "The last {} chapter types have stayed on {}, which suggests macro pacing monotony.",
                        scene.streak, scene.repeated_type
                    )
                } else {
                    format!(
                        "最近{}章章节类型持续停留在“{}”，长篇节奏可能开始固化。",
                        scene.streak, scene.repeated_type
                    )
                },
                suggestion: if en {
                    "Switch the next chapter's function instead of extending the same beat again. Rotate setup, payoff, reversal, and fallout more deliberately.".to_string()
                } else {
                    "下一章应切换章节功能，不要连续重复同一种布局/推进节拍。".to_string()
                },
            });
        }
    }

    if let Some(mood) = &cadence.mood_pressure {
        if mood.pressure == CadencePressure::High {
            issues.push(LongSpanFatigueIssue {
                severity: "warning",
                category: (if en { "Mood Monotony" } else { "情绪单调" }).to_string(),
                description: if en {
                    format!(
                        "High-tension mood has locked in for {} chapters ({}), with no visible emotional release.",
                        mood.high_tension_streak,
                        mood.recent_moods.join(" -> ")
                    )
                } else {
                    format!(
                        "最近{}章持续高压（{}），缺少明显的情绪释放。",
                        mood.high_tension_streak,
                        mood.recent_moods.join(" -> ")
                    )
                },
                suggestion: if en {
                    "Insert a release beat, warmth, humor, intimacy, or reflective quiet before escalating again.".to_string()
                } else {
                    "下一章安排一次喘息、温情、幽默或静场释放，再继续加压。".to_string()
                },
            });
        }
    }

    if let Some(title) = &cadence.title_pressure {
        if title.pressure == CadencePressure::High {
            issues.push(LongSpanFatigueIssue {
                severity: "warning",
                category: (if en { "Title Collapse" } else { "标题重复" }).to_string(),
                description: if en {
                    format!(
                        "Recent titles keep collapsing around \"{}\" ({} hits in the current window), which makes chapter naming feel formulaic.",
                        title.repeated_token, title.count
                    )
                } else {
                    format!(
                        "最近标题持续围绕“{}”命名（当前窗口命中{}次），命名开始坍缩。",
                        title.repeated_token, title.count
                    )
                },
                suggestion: if en {
                    "Change the next title anchor. Use a new image, action, consequence, or character vector instead of the same keyword shell.".to_string()
                } else {
                    "下一章标题换一个新的意象、动作、后果或人物焦点，不要继续套同一个关键词壳。".to_string()
                },
            });
        }
    }

    let bodies = load_recent_chapter_bodies(
        input.book_dir,
        input.chapter_number,
        input.chapter_content,
    )
    .await;
    if let Some(issue) =
        build_sentence_pattern_issue(&bodies, Boundary::Opening, input.language)
    {
        issues.push(issue);
    }
    if let Some(issue) =
        build_sentence_pattern_issue(&bodies, Boundary::Ending, input.language)
    {
        issues.push(issue);
    }

    issues
}

/// 当前摘要行并入（同章节号替换）。
fn merge_current_summary(
    rows: Vec<CadenceSummaryRow>,
    current_summary: Option<&str>,
) -> Vec<CadenceSummaryRow> {
    let Some(current) = current_summary.and_then(parse_summary_row) else {
        return rows;
    };
    let mut next: Vec<CadenceSummaryRow> = rows
        .into_iter()
        .filter(|row| row.chapter != current.chapter)
        .collect();
    next.push(current);
    next
}

/// 前章正文（含当前正文）：近 2 章不足 2 篇时返回空（模式检测不成立）。
async fn load_recent_chapter_bodies(
    book_dir: &Path,
    current_chapter: u32,
    current_content: &str,
) -> Vec<String> {
    let previous = load_previous_chapter_bodies(
        book_dir,
        current_chapter,
        window_defaults::RECENT_BOUNDARY_PATTERN_BODIES,
    )
    .await;
    if previous.len() < window_defaults::RECENT_BOUNDARY_PATTERN_BODIES as usize {
        return Vec::new();
    }
    let mut bodies = previous;
    bodies.push(current_content.to_string());
    bodies
}

/// 首尾句式同构检测：三体两两相邻 Dice 相似度均 ≥ 0.72 才报。
fn build_sentence_pattern_issue(
    chapter_bodies: &[String],
    boundary: Boundary,
    language: WritingLanguage,
) -> Option<LongSpanFatigueIssue> {
    let en = language == WritingLanguage::En;
    if chapter_bodies.len() < long_span_fatigue_thresholds::BOUNDARY_PATTERN_MIN_BODIES as usize {
        return None;
    }

    let sentences: Vec<String> = chapter_bodies
        .iter()
        .map(|body| extract_boundary_sentence(body, boundary))
        .collect::<Option<Vec<_>>>()?;
    let normalized: Vec<String> = sentences
        .iter()
        .map(|sentence| normalize_sentence(sentence, language))
        .collect();
    if normalized
        .iter()
        .any(|sentence| sentence.chars().count()
            < long_span_fatigue_thresholds::BOUNDARY_SENTENCE_MIN_LENGTH as usize)
    {
        return None;
    }

    let first = dice_coefficient(&normalized[0], &normalized[1]);
    let second = dice_coefficient(&normalized[1], &normalized[2]);
    if first.min(second) < long_span_fatigue_thresholds::BOUNDARY_SIMILARITY_FLOOR {
        return None;
    }

    let sample = summarize_sentence(&sentences[2], language);
    let pair_text = format!("{first:.2}/{second:.2}");

    Some(LongSpanFatigueIssue {
        severity: "warning",
        category: match (boundary, en) {
            (Boundary::Opening, true) => "Opening Pattern Repetition".to_string(),
            (Boundary::Ending, true) => "Ending Pattern Repetition".to_string(),
            (Boundary::Opening, false) => "开头同构".to_string(),
            (Boundary::Ending, false) => "结尾同构".to_string(),
        },
        description: if en {
            let position = if boundary == Boundary::Opening { "openings" } else { "endings" };
            let boundary_label = if boundary == Boundary::Opening { "opening" } else { "ending" };
            format!(
                "The last 3 chapter {position} are highly similar (adjacent similarity {pair_text}), which risks a formulaic rhythm. Current {boundary_label} signature: \"{sample}\"."
            )
        } else {
            let position = if boundary == Boundary::Opening { "开头" } else { "结尾" };
            let tail = if boundary == Boundary::Opening { "开篇" } else { "章尾" };
            format!(
                "最近3章{position}句式高度相似（相邻相似度{pair_text}），容易形成模板化{tail}。当前句式近似“{sample}”。"
            )
        },
        suggestion: if boundary == Boundary::Opening {
            if en {
                "Change the next chapter opening vector. Start from action, consequence, or surprise instead of repeating the same camera move.".to_string()
            } else {
                "下一章换一个开篇入口，用动作、后果或异常信息切入，不要连续沿用同一种抬镜句。".to_string()
            }
        } else if en {
            "Change the next chapter landing pattern. End on consequence, decision, or a new variable instead of repeating the same explanatory cadence.".to_string()
        } else {
            "下一章换一个收束方式，用行动后果、角色决断或新变量落板，不要连续用解释性句子收尾。".to_string()
        },
    })
}

#[cfg(test)]
mod fatigue_tests {
    use super::*;

    #[tokio::test]
    async fn no_issues_on_empty_book() {
        let dir = tempfile::tempdir().unwrap();
        let issues = analyze_long_span_fatigue(&AnalyzeLongSpanFatigueInput {
            book_dir: dir.path(),
            chapter_number: 1,
            chapter_content: "正文。",
            chapter_summary: None,
            language: WritingLanguage::Zh,
        })
        .await;
        assert!(issues.is_empty());
    }

    #[tokio::test]
    async fn scene_monotony_and_boundary_patterns_detected() {
        let dir = tempfile::tempdir().unwrap();
        let story = dir.path().join("story");
        let chapters = dir.path().join("chapters");
        tokio::fs::create_dir_all(&story).await.unwrap();
        tokio::fs::create_dir_all(&chapters).await.unwrap();

        // 高压情绪 + 同类型章节连续（节奏/情绪双单调）。
        let header = "| 章节 | 标题 | 出场人物 | 关键事件 | 状态变化 | 伏笔动态 | 情绪基调 | 章节类型 |\n| --- | --- | --- | --- | --- | --- | --- | --- |\n";
        let rows = (1..=6)
            .map(|n| format!("| {n} | 风波{n} | 甲 | e | s | h | 紧张 | 推进章 |\n"))
            .collect::<String>();
        tokio::fs::write(
            story.join("chapter_summaries.md"),
            format!("{header}{rows}"),
        )
        .await
        .unwrap();

        // 前两章开头/结尾句式相同（Dice ≥ 0.72）。
        let opening = "夜色如墨，他攥紧了手中的令牌，指节因用力而发白。";
        let ending = "远处的钟声悠悠响起，他知道，一切才刚刚开始而已。";
        for n in [1u32, 2] {
            tokio::fs::write(
                chapters.join(format!("{n:04}_章.md")),
                format!("{opening}\n\n正文填充{noto}。\n\n{ending}", noto = "内容".repeat(8)),
            )
            .await
            .unwrap();
        }
        let current = format!("{opening}\n\n正文填充。\n\n{ending}");

        let issues = analyze_long_span_fatigue(&AnalyzeLongSpanFatigueInput {
            book_dir: dir.path(),
            chapter_number: 3,
            chapter_content: &current,
            chapter_summary: Some("| 3 | 风波3 | 甲 | e | s | h | 紧张 | 推进章 |"),
            language: WritingLanguage::Zh,
        })
        .await;

        let categories: Vec<&str> = issues.iter().map(|i| i.category.as_str()).collect();
        assert!(categories.contains(&"节奏单调"), "categories: {categories:?}");
        assert!(categories.contains(&"情绪单调"), "categories: {categories:?}");
        assert!(categories.contains(&"开头同构"), "categories: {categories:?}");
        assert!(categories.contains(&"结尾同构"), "categories: {categories:?}");
        assert!(issues.iter().all(|i| i.severity == "warning"));
        // chapterSummary 同章节号替换：3 号行不重复计数。
        let pacing = issues.iter().find(|i| i.category == "节奏单调").unwrap();
        assert!(pacing.description.contains("推进章"));
    }

    #[test]
    fn boundary_issue_requires_three_bodies() {
        // 两体不足三——直接 None。
        let bodies = vec!["a".to_string(), "b".to_string()];
        assert!(build_sentence_pattern_issue(&bodies, Boundary::Opening, WritingLanguage::Zh).is_none());
    }
}
