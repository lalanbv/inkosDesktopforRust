//! 风格指纹分析（style-analyzer）。
//!
//! 移植自 `packages/core/src/agents/style-analyzer.ts`（116 行）。纯文本统计（无 LLM），
//! 从参考文本提取统计特征构建 [`StyleProfile`]：句长/段落长/词汇多样性(TTR)/开头模式/修辞特征。
//!
//! ## 反向引用正则的处理
//! TS 的排比模式 `/[，。；]([^，。；]{2,6})[，。；]\1/g` 用了反向引用 `\1`——Rust `regex` crate
//! 不支持反向引用。不为单个模式引入 `fancy-regex` 重依赖，改用 [`count_parallelism`] 手动扫描
//! 等价实现（标点 X + 2-6 非标点 + 标点 + 同一 2-6 串）。其余 11 个正则用 regex crate。

use regex::Regex;
use std::sync::OnceLock;

use crate::models::style_profile::{ParagraphLengthRange, StyleProfile};
use crate::utils::language::WritingLanguage;

/// 分析参考文本，提取风格指纹。对齐 TS `analyzeStyle`。
///
/// `analyzed_at` 由调用方注入（ISO8601，对齐纯内核范式——内核不依赖系统时钟）。
pub fn analyze_style(
    text: &str,
    source_name: Option<&str>,
    language: WritingLanguage,
    analyzed_at: Option<&str>,
) -> StyleProfile {
    let is_en = language == WritingLanguage::En;

    let sentences: Vec<&str> = {
        let re = if is_en { sentence_split_en_re() } else { sentence_split_zh_re() };
        re.split(text)
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect()
    };

    let paragraphs: Vec<&str> = paragraph_split_re()
        .split(text)
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .collect();

    // 句长统计（英文按词，中文按字符）。
    let sentence_lengths: Vec<f64> = sentences.iter().map(|s| measure(s, is_en) as f64).collect();
    let avg_sentence_length = if !sentence_lengths.is_empty() {
        sentence_lengths.iter().sum::<f64>() / sentence_lengths.len() as f64
    } else {
        0.0
    };
    let sentence_length_std_dev = if sentence_lengths.len() > 1 {
        let var = sentence_lengths.iter().map(|l| (l - avg_sentence_length).powi(2)).sum::<f64>()
            / sentence_lengths.len() as f64;
        var.sqrt()
    } else {
        0.0
    };

    // 段落长统计。
    let paragraph_lengths: Vec<f64> = paragraphs.iter().map(|p| measure(p, is_en) as f64).collect();
    let avg_paragraph_length = if !paragraph_lengths.is_empty() {
        paragraph_lengths.iter().sum::<f64>() / paragraph_lengths.len() as f64
    } else {
        0.0
    };
    let (min_p, max_p) = if paragraph_lengths.is_empty() {
        (0.0_f64, 0.0_f64)
    } else {
        (
            paragraph_lengths.iter().cloned().fold(f64::INFINITY, f64::min),
            paragraph_lengths.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
        )
    };

    // 词汇多样性（TTR）：英文词级，中文字符级。
    let vocabulary_diversity = if is_en {
        let words: Vec<&str> = en_word_re().find_iter(text).map(|m| m.as_str()).collect();
        // lower-case 比较需 owned——直接收集 lower 后的字符串。
        let lower: Vec<String> = words.iter().map(|w| w.to_lowercase()).collect();
        if lower.is_empty() {
            0.0
        } else {
            let mut set: std::collections::HashSet<&str> = std::collections::HashSet::new();
            for w in &lower {
                set.insert(w.as_str());
            }
            set.len() as f64 / lower.len() as f64
        }
    } else {
        let kept: String = zh_char_keep_re().replace_all(text, "").to_string();
        if kept.is_empty() {
            0.0
        } else {
            let set: std::collections::HashSet<char> = kept.chars().collect();
            set.len() as f64 / kept.chars().count() as f64
        }
    };

    // 句子开头模式 top5（≥3 次）。
    let mut opening_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for s in &sentences {
        let key = if is_en {
            en_first_word_re()
                .find(s)
                .map(|m| m.as_str().to_lowercase())
                .unwrap_or_default()
        } else {
            let chars: String = s.chars().take(2).collect();
            if s.chars().count() >= 2 { chars } else { String::new() }
        };
        if !key.is_empty() {
            *opening_counts.entry(key).or_insert(0) += 1;
        }
    }
    let mut top_patterns: Vec<(String, usize)> = opening_counts.into_iter().collect();
    top_patterns.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let top_patterns: Vec<String> = top_patterns
        .into_iter()
        .take(5)
        .filter(|(_, c)| *c >= 3)
        .map(|(p, c)| {
            if is_en {
                format!("{p}… ({c})")
            } else {
                format!("{p}...({c}次)")
            }
        })
        .collect();

    // 修辞特征（≥2 次命中）。
    let rhetorical_features = detect_rhetorical_features(text, is_en);

    StyleProfile {
        avg_sentence_length: round1(avg_sentence_length),
        sentence_length_std_dev: round1(sentence_length_std_dev),
        avg_paragraph_length: avg_paragraph_length.round(),
        paragraph_length_range: ParagraphLengthRange { min: min_p, max: max_p },
        vocabulary_diversity: round3(vocabulary_diversity),
        top_patterns,
        rhetorical_features,
        source_name: source_name.map(|s| s.to_string()),
        analyzed_at: analyzed_at.map(|s| s.to_string()),
    }
}

/// 计量单元：英文按词数，中文按非空白字符数。对齐 TS `measure`。
fn measure(s: &str, is_en: bool) -> usize {
    if is_en {
        en_word_re().find_iter(s).count()
    } else {
        let stripped = whitespace_re().replace_all(s, "");
        stripped.chars().count()
    }
}

/// 修辞特征检测。排比用手动扫描，其余用 regex crate。
fn detect_rhetorical_features(text: &str, is_en: bool) -> Vec<String> {
    let mut features: Vec<String> = Vec::new();
    if is_en {
        // EN: simile / rhetorical question / tricolon / short punchy rhythm。
        for (name, count) in [
            ("simile (like/as if)", count_re(text, en_simile_re())),
            ("rhetorical question", count_re(text, en_rhetorical_q_re())),
            ("tricolon", count_re(text, en_tricolon_re())),
            ("short punchy rhythm", count_re(text, en_short_punchy_re())),
        ] {
            if count >= 2 {
                features.push(format!("{name} ({count})"));
            }
        }
    } else {
        // ZH: 比喻/排比/反问/夸张/拟人/短句节奏。
        for (name, count) in [
            ("比喻(像/如/仿佛)", count_re(text, zh_simile_re())),
            ("排比", count_parallelism(text)),
            ("反问", count_re(text, zh_rhetorical_q_re())),
            ("夸张", count_re(text, zh_hyperbole_re())),
            ("拟人", count_re(text, zh_personification_re())),
            ("短句节奏", count_re(text, zh_short_rhythm_re())),
        ] {
            if count >= 2 {
                features.push(format!("{name}({count}处)"));
            }
        }
    }
    features
}

fn count_re(text: &str, re: &Regex) -> usize {
    re.find_iter(text).count()
}

/// 排比检测（手动扫描，等价 TS `[，。；]([^，。；]{2,6})[，。；]\1`）：
/// 找「标点 X + 2-6 非标点串 S + 标点 + 同 S」的模式。
fn count_parallelism(text: &str) -> usize {
    const PUNCT: &[char] = &['，', '。', '；'];
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let mut count = 0usize;
    let mut i = 0;
    while i < n {
        if PUNCT.contains(&chars[i]) {
            // 尝试在 i+1.. 起取 2-6 长度的非标点串 S，然后期望标点 + 同 S。
            'len: for slen in 2..=6usize {
                let s_start = i + 1;
                let s_end = s_start + slen;
                if s_end >= n {
                    break 'len;
                }
                let s_slice = &chars[s_start..s_end];
                // S 内不得含 PUNCT。
                if s_slice.iter().any(|c| PUNCT.contains(c)) {
                    continue;
                }
                // s_end 应为 PUNCT。
                if !PUNCT.contains(&chars[s_end]) {
                    continue;
                }
                // 之后紧跟同 S。
                let after = s_end + 1;
                let after_end = after + slen;
                if after_end > n {
                    continue;
                }
                if &chars[after..after_end] == s_slice {
                    count += 1;
                    // 跳到匹配末尾避免重叠计数（与 JS `match` 全局非重叠语义接近）。
                    i = after_end;
                    break 'len;
                }
            }
        }
        i += 1;
    }
    count
}

fn round1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}
fn round3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

// --- 正则（OnceLock 编译一次）-------------------------------------------------

fn sentence_split_zh_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"[。！？\n]").expect("zh sentence split"))
}
fn sentence_split_en_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"[.!?\n]+").expect("en sentence split"))
}
fn paragraph_split_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\n\s*\n").expect("paragraph split"))
}
fn whitespace_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\s+").expect("whitespace"))
}
fn en_word_re() -> &'static Regex {
    // TS: /[A-Za-z0-9]+(?:'[A-Za-z0-9]+)?/g
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"[A-Za-z0-9]+(?:'[A-Za-z0-9]+)?").expect("en word"))
}
fn en_first_word_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"[A-Za-z']+").expect("en first word"))
}
fn zh_char_keep_re() -> &'static Regex {
    // TS: 剥离空白 + 标点 + 数字，保留中文字符。
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"[\s\n\r，。！？、：；“”‘’（）【】《》\d]").expect("zh char keep"))
}
fn zh_simile_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"[像如仿佛似](?:是|同|一般|一样)").expect("zh simile"))
}
fn zh_rhetorical_q_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"难道|怎么可能|岂不是|何尝不").expect("zh rhetorical q"))
}
fn zh_hyperbole_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"天崩地裂|惊天动地|翻天覆地|震耳欲聋").expect("zh hyperbole"))
}
fn zh_personification_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"[风雨雪月花树草石](?:在|像|仿佛).*?(?:笑|哭|叹|呻|吟|怒|舞)").expect("zh personification"))
}
fn zh_short_rhythm_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"[。！？][^。！？]{1,8}[。！？]").expect("zh short rhythm"))
}
fn en_simile_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)\b(?:like a|like an|as if|as though)\b").expect("en simile"))
}
fn en_rhetorical_q_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)\b(?:how could|why would|what if|wasn't it|isn't it|could it be)\b[^.!?]*\?").expect("en rhetorical q"))
}
fn en_tricolon_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)\b\w+,\s+\w+,\s+and\s+\w+\b").expect("en tricolon"))
}
fn en_short_punchy_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"[.!?]\s+[A-Z][^.!?]{1,24}[.!?]").expect("en short punchy"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_text_returns_zeros() {
        let p = analyze_style("", None, WritingLanguage::Zh, None);
        assert_eq!(p.avg_sentence_length, 0.0);
        assert!(p.top_patterns.is_empty());
        assert!(p.rhetorical_features.is_empty());
    }

    #[test]
    fn chinese_sentence_length_measured_by_chars() {
        let text = "一二三四五。一二三四五六七八。一二。";
        let p = analyze_style(text, None, WritingLanguage::Zh, None);
        // 句长 = 5, 8, 2，均值 5.0。
        assert_eq!(p.avg_sentence_length, 5.0);
    }

    #[test]
    fn english_sentence_length_measured_by_words() {
        let text = "The cat sat. The dog ran fast. Birds.";
        let p = analyze_style(text, None, WritingLanguage::En, None);
        // 句长（词数）= 3, 4, 1，均值 8/3 ≈ 2.7。
        assert_eq!(p.avg_sentence_length, 2.7);
    }

    #[test]
    fn paragraph_length_range_min_max() {
        // 段落含句号（measure 中文不去标点）：短段字符数 < 长段。
        let text = "短。\n\n中中中中中。\n\n长长长长长长长长长长长长。";
        let p = analyze_style(text, None, WritingLanguage::Zh, None);
        assert_eq!(p.paragraph_length_range.min, 2.0, "「短。」=2 字符");
        assert!(p.paragraph_length_range.max > p.paragraph_length_range.min);
        assert!(p.paragraph_length_range.max >= 12.0, "长段至少 12 字符");
    }

    #[test]
    fn vocabulary_diversity_chinese_char_level() {
        // 完全相同的字符 → TTR 低；全不同 → TTR=1。
        let text = "啊啊啊啊"; // 全同 → 1/4 = 0.25
        let p = analyze_style(text, None, WritingLanguage::Zh, None);
        assert_eq!(p.vocabulary_diversity, 0.25);
    }

    #[test]
    fn top_patterns_filters_below_three_and_takes_five() {
        // 构造 3 句同前缀（「我们」）+ 其它各异。
        let text = "我们出发。我们战斗。我们胜利。他来了。她走了。";
        let p = analyze_style(text, None, WritingLanguage::Zh, None);
        assert!(p.top_patterns.iter().any(|s| s.contains("我们") && s.contains("3次")));
    }

    #[test]
    fn rhetorical_simile_detected_when_two_or_more() {
        // 2+ 处比喻 → 命中。
        let text = "她像是一阵风。他仿佛同雾气。";
        let p = analyze_style(text, None, WritingLanguage::Zh, None);
        assert!(p.rhetorical_features.iter().any(|f| f.contains("比喻")), "features={:?}", p.rhetorical_features);
    }

    #[test]
    fn parallelism_manual_scan_counts_repeated_segment() {
        // 「风，风，风」式排比：标点 + 单字 + 标点 + 单字——但 slen≥2。
        // 构造 slen=2：「啊吧，啊吧，」。
        let text = "，啊吧，啊吧，";
        assert_eq!(count_parallelism(text), 1);
    }

    #[test]
    fn source_name_and_analyzed_at_propagated() {
        let p = analyze_style("一二三。", Some("ref.txt"), WritingLanguage::Zh, Some("2024-01-01T00:00:00.000Z"));
        assert_eq!(p.source_name.as_deref(), Some("ref.txt"));
        assert_eq!(p.analyzed_at.as_deref(), Some("2024-01-01T00:00:00.000Z"));
    }

    #[test]
    fn en_vocabulary_diversity_word_level() {
        // 重复词多 → TTR 低。
        let text = "the the the cat";
        let p = analyze_style(text, None, WritingLanguage::En, None);
        // 词 = [the, the, the, cat] → 2 unique / 4 = 0.5
        assert_eq!(p.vocabulary_diversity, 0.5);
    }
}
