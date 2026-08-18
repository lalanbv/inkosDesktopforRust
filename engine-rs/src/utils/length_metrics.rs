//! 章节长度度量。
//!
//! 移植自 `packages/core/src/utils/length-metrics.ts`（133 行，纯函数）。
//!
//! ## 关键移植细节
//! - **zh_chars 计数**：TS `normalized.replace(/\s+/g, "").length` —— `.length` 是
//!   UTF-16 码元数（含 emoji 时与 Rust `chars().count()` 不同）。本实现用
//!   `char::len_utf16()` 累加，逐码点对齐 JS。
//! - **`\s` 语义**：JS `\s` 含 `\u{feff}`（BOM），Rust `is_whitespace` 不含——显式补入。
//! - **en_words 计数**：TS 正则 `/[A-Za-z0-9]+(?:'[A-Za-z0-9]+)?/g`，本实现用 ASCII
//!   字节状态机（无 regex 依赖、0GC 友好）。
//! - **markdown 剥离**：frontmatter / 代码栅栏 / ATX 标题 / 水平线，逐行 char 判断。

use crate::models::length_governance::{LengthCountingMode, LengthSpec};
use crate::utils::language::WritingLanguage;

/// 跨语言基准目标字数（soft/hard 区间 delta 按此缩放）。移植 TS `REFERENCE_TARGET`。
const REFERENCE_TARGET: u64 = 2200;
const SOFT_RANGE_DELTA: u64 = 300;
const HARD_RANGE_DELTA: u64 = 600;

pub const DEFAULT_CHAPTER_LENGTH_ZH: u32 = 3000;
pub const DEFAULT_CHAPTER_LENGTH_EN: u32 = 2000;

/// 按语言返回默认章节目标长度。
pub fn default_chapter_length(language: WritingLanguage) -> u32 {
    match language {
        WritingLanguage::En => DEFAULT_CHAPTER_LENGTH_EN,
        WritingLanguage::Zh => DEFAULT_CHAPTER_LENGTH_ZH,
    }
}

/// 按计量模式统计章节长度（先剥 markdown 元数据）。
pub fn count_chapter_length(content: &str, counting_mode: LengthCountingMode) -> u32 {
    let normalized = strip_markdown_metadata(content);
    match counting_mode {
        LengthCountingMode::EnWords => count_en_words(&normalized),
        LengthCountingMode::ZhChars => count_zh_chars(&normalized),
    }
}

/// 按语言解析默认计量模式。
pub fn resolve_length_counting_mode(language: WritingLanguage) -> LengthCountingMode {
    match language {
        WritingLanguage::En => LengthCountingMode::EnWords,
        WritingLanguage::Zh => LengthCountingMode::ZhChars,
    }
}

/// 格式化长度计数为人类可读串（对齐 TS `${count} words` / `${count}字`）。
pub fn format_length_count(count: u32, counting_mode: LengthCountingMode) -> String {
    match counting_mode {
        LengthCountingMode::EnWords => format!("{count} words"),
        LengthCountingMode::ZhChars => format!("{count}字"),
    }
}

/// 由目标字数 + 语言构造完整长度规格。
pub fn build_length_spec(target: u32, language: WritingLanguage) -> LengthSpec {
    let soft_delta = scale_range_delta(target, SOFT_RANGE_DELTA);
    let hard_delta = std::cmp::max(soft_delta, scale_range_delta(target, HARD_RANGE_DELTA));
    let soft_min = std::cmp::max(1, target.saturating_sub(soft_delta));
    let soft_max = target + soft_delta;
    let hard_min = std::cmp::max(1, target.saturating_sub(hard_delta));
    let hard_max = target + hard_delta;
    LengthSpec {
        target,
        soft_min,
        soft_max,
        hard_min,
        hard_max,
        counting_mode: resolve_length_counting_mode(language),
    }
}

/// 计数是否落在软区间外。
pub fn is_outside_soft_range(count: u32, soft_min: u32, soft_max: u32) -> bool {
    count < soft_min || count > soft_max
}

/// 计数是否落在硬区间外。
pub fn is_outside_hard_range(count: u32, hard_min: u32, hard_max: u32) -> bool {
    count < hard_min || count > hard_max
}


/// 区间 delta 缩放：`max(1, floor(target * reference_delta / 2200))`。
fn scale_range_delta(target: u32, reference_delta: u64) -> u32 {
    let scaled = (target as u64 * reference_delta) / REFERENCE_TARGET;
    std::cmp::max(1, scaled) as u32
}

/// zh_chars 计数：剥空白后的 UTF-16 码元数（对齐 JS `.length`）。
fn count_zh_chars(s: &str) -> u32 {
    s.chars()
        .filter(|&c| !is_js_whitespace(c))
        .map(|c| c.len_utf16() as u32)
        .sum()
}

/// JS `\s` 等价：Unicode White_Space ∪ `\u{feff}`。
pub fn is_js_whitespace(c: char) -> bool {
    c.is_whitespace() || c == '\u{feff}'
}

/// en_words 计数：TS 正则 `[A-Za-z0-9]+(?:'[A-Za-z0-9]+)?` 的匹配数。
///
/// 字节状态机（ASCII 安全：多字节 UTF-8 序列的引导/续字节 ≥0x80 恒非 ASCII 字母数字，
/// 逐字节推进不会误匹配，仅计数不切片）。
fn count_en_words(s: &str) -> u32 {
    let b = s.as_bytes();
    let mut count: u32 = 0;
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_alphanumeric() {
            // 消费字母数字主干
            while i < b.len() && b[i].is_ascii_alphanumeric() {
                i += 1;
            }
            // 可选 `'` + 字母数字段（正则 `(?:'...)?`）
            if i < b.len() && b[i] == b'\'' {
                let mut j = i + 1;
                if j < b.len() && b[j].is_ascii_alphanumeric() {
                    while j < b.len() && b[j].is_ascii_alphanumeric() {
                        j += 1;
                    }
                    i = j; // 消费 `'xxx`
                }
                // 否则不消费 `'`（正则要求第二段至少一个字符）
            }
            count += 1;
        } else {
            i += 1;
        }
    }
    count
}

/// 剥离 markdown 元数据：frontmatter（`---` 包围块）、代码栅栏内容、ATX 标题、水平线。
fn strip_markdown_metadata(content: &str) -> String {
    // CRLF → LF；去 BOM
    let crlf = content.replace("\r\n", "\n");
    let normalized = crlf.strip_prefix('\u{feff}').unwrap_or(&crlf);
    let lines: Vec<&str> = normalized.split('\n').collect();
    let mut prose: Vec<&str> = Vec::with_capacity(lines.len());
    let mut idx = 0usize;

    // frontmatter：首行 trim == "---" → 跳到下一个 "---" 之后
    if lines.first().map(|l| l.trim() == "---").unwrap_or(false) {
        idx = 1;
        while idx < lines.len() && lines[idx].trim() != "---" {
            idx += 1;
        }
        if idx < lines.len() {
            idx += 1; // 跳过闭合的 ---
        }
    }

    let mut in_fence = false;
    while idx < lines.len() {
        let line = lines[idx];
        let trimmed = line.trim_end(); // TS 用 .trim() 做判定，trim_end 与之对齐（行首已无缩进判定）
        let trimmed = trimmed.trim_start();
        if is_fence_line(trimmed) {
            in_fence = !in_fence;
            idx += 1;
            continue;
        }
        if in_fence {
            idx += 1;
            continue;
        }
        if is_atx_header(trimmed) {
            idx += 1;
            continue;
        }
        if trimmed == "---" || trimmed == "..." {
            idx += 1;
            continue;
        }
        prose.push(line);
        idx += 1;
    }
    prose.join("\n")
}

fn is_fence_line(trimmed: &str) -> bool {
    trimmed.starts_with("```") || trimmed.starts_with("~~~")
}

/// ATX 标题判定：`^#{1,6}\s+`（1-6 个 # 后紧跟至少一个空白）。
fn is_atx_header(trimmed: &str) -> bool {
    let bytes = trimmed.as_bytes();
    let mut hashes = 0usize;
    while hashes < bytes.len() && bytes[hashes] == b'#' {
        hashes += 1;
    }
    (1..=6).contains(&hashes) && hashes < bytes.len() && bytes[hashes].is_ascii_whitespace()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_chapter_length_per_language() {
        assert_eq!(default_chapter_length(WritingLanguage::Zh), 3000);
        assert_eq!(default_chapter_length(WritingLanguage::En), 2000);
    }

    #[test]
    fn count_zh_strips_markdown_and_whitespace() {
        let md = "---\ntitle: x\n---\n# 标题\n\n正文内容。";
        assert_eq!(count_chapter_length(md, LengthCountingMode::ZhChars), 5); // "正文内容。"
    }

    #[test]
    fn count_en_words_apostrophe_joined() {
        let s = "don't count it's won't";
        assert_eq!(count_en_words(s), 4); // don't / count / it's / won't
    }

    #[test]
    fn count_en_words_trailing_apostrophe() {
        // 末尾 ' 不构成词（正则要求第二段至少一字符）
        assert_eq!(count_en_words("hello'"), 1);
        assert_eq!(count_en_words("a b c"), 3);
    }

    #[test]
    fn build_spec_shrinks_delta_for_small_target() {
        let spec = build_length_spec(1000, WritingLanguage::Zh);
        // soft_delta = max(1, floor(1000*300/2200)) = max(1, 136) = 136
        assert_eq!(spec.soft_min, 864);
        assert_eq!(spec.soft_max, 1136);
        assert_eq!(spec.counting_mode, LengthCountingMode::ZhChars);
    }

    #[test]
    fn build_spec_hard_delta_at_least_soft() {
        let spec = build_length_spec(2200, WritingLanguage::En);
        assert!(spec.hard_max - spec.target >= spec.soft_max - spec.target);
        assert_eq!(spec.counting_mode, LengthCountingMode::EnWords);
    }

    #[test]
    fn range_checks() {
        assert!(is_outside_soft_range(50, 100, 200));
        assert!(!is_outside_soft_range(150, 100, 200));
        assert!(is_outside_hard_range(10, 100, 300));
    }

    #[test]
    fn strip_handles_frontmatter_fence_header_hr() {
        let md = "---\nid: 1\n---\n```js\nconst x = 1;\n```\n# H1\n\n---\n保留这行";
        // 注：# H1 后的空行被保留（与 TS 行为一致——只剥元数据行，不删普通空行）
        assert_eq!(strip_markdown_metadata(md), "\n保留这行");
    }

    #[test]
    fn emoji_counts_as_utf16_units_in_zh() {
        // 😀 = U+1F600 = 2 UTF-16 码元；与 JS "😀".length === 2 对齐
        assert_eq!(count_zh_chars("😀"), 2);
    }

    #[test]
    fn length_spec_serializes_camel_case() {
        let spec = build_length_spec(2200, WritingLanguage::Zh);
        let v = serde_json::to_value(&spec).unwrap();
        // camelCase 字段名（与 TS 契约一致）
        assert!(v.get("softMin").is_some(), "应为 camelCase: {}", v);
        assert!(v.get("countingMode").is_some());
        assert!(v.get("soft_min").is_none(), "不应为 snake_case");
    }
}
