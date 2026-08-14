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
