//! 敏感词检测（sensitive-words）。
//!
//! 移植自 `packages/core/src/agents/sensitive-words.ts`（142 行）。纯规则分析（无 LLM），
//! 检测中文网文的政治敏感（block）/ 色情（warn）/ 极端暴力（warn）词，产出 [`AuditIssue`]。


use crate::agents::continuity::{AuditIssue, AuditSeverity};
use crate::utils::language::WritingLanguage;

/// 单个敏感词命中。对齐 TS `SensitiveWordMatch`。
#[derive(Debug, Clone, PartialEq)]
pub struct SensitiveWordMatch {
    pub word: String,
    pub count: usize,
    pub severity: SensitiveWordSeverity,
}

/// 敏感词严重度。对齐 TS `"block" | "warn"`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SensitiveWordSeverity {
    Block,
    Warn,
}

/// 检测结果。对齐 TS `SensitiveWordResult`。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SensitiveWordResult {
    pub issues: Vec<AuditIssue>,
    pub found: Vec<SensitiveWordMatch>,
}

// 政治敏感词 — severity block
const POLITICAL_WORDS: &[&str] = &[
    "习近平", "习主席", "习总书记", "共产党", "中国共产党", "共青团",
    "六四", "天安门事件", "天安门广场事件", "法轮功", "法轮大法",
    "台独", "藏独", "疆独", "港独",
    "新疆集中营", "再教育营",
    "维吾尔", "达赖喇嘛", "达赖",
    "刘晓波", "艾未未", "赵紫阳",
    "文化大革命", "文革", "大跃进",
    "反右运动", "镇压", "六四屠杀",
    "中南海", "政治局常委",
    "翻墙", "防火长城",
];

// 色情敏感词 — severity warn
const SEXUAL_WORDS: &[&str] = &[
    "性交", "做爱", "口交", "肛交", "自慰", "手淫",
    "阴茎", "阴道", "阴蒂", "乳房", "乳头",
    "射精", "高潮", "潮吹",
    "淫荡", "淫乱", "荡妇", "婊子",
    "强奸", "轮奸",
];

// 极端暴力词 — severity warn
const VIOLENCE_EXTREME: &[&str] = &[
    "肢解", "碎尸", "挖眼", "剥皮", "开膛破肚",
    "虐杀", "凌迟", "活剥", "活埋", "烹煮活人",
];

struct WordListEntry {
    words: &'static [&'static str],
    severity: SensitiveWordSeverity,
    label: &'static str,
    english_label: &'static str,
}

const WORD_LISTS: &[WordListEntry] = &[
    WordListEntry { words: POLITICAL_WORDS, severity: SensitiveWordSeverity::Block, label: "政治敏感词", english_label: "political sensitive terms" },
    WordListEntry { words: SEXUAL_WORDS, severity: SensitiveWordSeverity::Warn, label: "色情敏感词", english_label: "sexual sensitive terms" },
    WordListEntry { words: VIOLENCE_EXTREME, severity: SensitiveWordSeverity::Warn, label: "极端暴力词", english_label: "extreme violence terms" },
];

/// 分析文本中的敏感词。对齐 TS `analyzeSensitiveWords`。
///
/// 3 个内置词表（政治 block / 色情 warn / 暴力 warn）+ 可选自定义词表（warn）。
/// 每个命中的词表产一条 [`AuditIssue`]（block → critical，warn → warning）。
pub fn analyze_sensitive_words(
    content: &str,
    custom_words: Option<&[String]>,
    language: WritingLanguage,
) -> SensitiveWordResult {
    let mut found: Vec<SensitiveWordMatch> = Vec::new();
    let mut issues: Vec<AuditIssue> = Vec::new();
    let is_english = language == WritingLanguage::En;
    let joiner = if is_english { ", " } else { "、" };

    for list in WORD_LISTS {
        let matches = scan_static_list(content, list);
        if matches.is_empty() {
            continue;
        }
        let word_summary = matches
            .iter()
            .map(|m| format!("\"{}\"×{}", m.word, m.count))
            .collect::<Vec<_>>()
            .join(joiner);
        found.extend(matches.iter().cloned());
        let severity = if list.severity == SensitiveWordSeverity::Block {
            AuditSeverity::Critical
        } else {
            AuditSeverity::Warning
        };
        let (description, suggestion) = if is_english {
            (
                format!("Detected {}: {}", list.english_label, word_summary),
                if list.severity == SensitiveWordSeverity::Block {
                    "You must remove or replace these blocked terms before publication".to_string()
                } else {
                    format!("Replace or soften these {} to reduce moderation risk", list.english_label)
                },
            )
        } else {
            (
                format!("检测到{}：{}", list.label, word_summary),
                if list.severity == SensitiveWordSeverity::Block {
                    "必须删除或替换政治敏感词，否则无法发布".to_string()
                } else {
                    format!("建议替换或弱化{}，避免平台审核问题", list.label)
                },
            )
        };
        issues.push(AuditIssue {
            severity,
            category: if is_english { "Sensitive terms" } else { "敏感词" }.to_string(),
            description,
            suggestion,
            repair_scope: None,
        });
    }

    if let Some(custom) = custom_words {
        if !custom.is_empty() {
            let custom_matches = scan_words_strings(content, custom, SensitiveWordSeverity::Warn);
            if !custom_matches.is_empty() {
                let word_summary = custom_matches
                    .iter()
                    .map(|m| format!("\"{}\"×{}", m.word, m.count))
                    .collect::<Vec<_>>()
                    .join(joiner);
                found.extend(custom_matches.iter().cloned());
                issues.push(AuditIssue {
                    severity: AuditSeverity::Warning,
                    category: if is_english { "Sensitive terms" } else { "敏感词" }.to_string(),
                    description: if is_english {
                        format!("Detected custom sensitive term(s): {word_summary}")
                    } else {
                        format!("检测到自定义敏感词：{word_summary}")
                    },
                    suggestion: if is_english {
                        "Replace or remove these terms according to project rules".to_string()
                    } else {
                        "根据项目规则替换或删除这些词语".to_string()
                    },
                    repair_scope: None,
                });
            }
        }
    }

    SensitiveWordResult { issues, found }
}

/// 静态词表扫描。
fn scan_static_list(content: &str, list: &WordListEntry) -> Vec<SensitiveWordMatch> {
    scan_words_impl(content, list.words.iter().copied(), list.severity)
}

/// 自定义词表扫描。
fn scan_words_strings(content: &str, words: &[String], severity: SensitiveWordSeverity) -> Vec<SensitiveWordMatch> {
    scan_words_impl(content, words.iter().map(|s| s.as_str()), severity)
}

/// 逐词子串计数（177 号 W-D4：TS 原实现是 `escapeRegExp` 后的字面正则
/// 全局匹配——escape 产物只匹配字面子串，与 `str::matches` 完全等价
/// （memmem，零编译）。基线逐词 `Regex::new` 重编译 225µs/章 → 子串扫描
/// 后该项归零，语义由 8 个既有单测 + golden 向量锁定）。空词两侧同为
/// 「每个位置一次空匹配」计数，行为一致。
fn scan_words_impl<'a, I: Iterator<Item = &'a str>>(
    content: &str,
    words: I,
    severity: SensitiveWordSeverity,
) -> Vec<SensitiveWordMatch> {
    let mut matches = Vec::new();
    for word in words {
        let count = content.matches(word).count();
        if count > 0 {
            matches.push(SensitiveWordMatch {
                word: word.to_string(),
                count,
                severity,
            });
        }
    }
    matches
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_text_no_issues() {
        let r = analyze_sensitive_words("普通正文，无敏感内容。", None, WritingLanguage::Zh);
        assert!(r.issues.is_empty());
        assert!(r.found.is_empty());
    }

    #[test]
    fn political_word_blocks_as_critical() {
        let r = analyze_sensitive_words("他提到了文革和台独。", None, WritingLanguage::Zh);
        let block = r.issues.iter().find(|i| i.severity == AuditSeverity::Critical).expect("应有 critical");
        assert!(block.description.contains("政治敏感词"));
        assert!(block.description.contains("文革"));
        assert!(block.description.contains("台独"));
        assert_eq!(r.found.len(), 2);
    }

    #[test]
    fn sexual_and_violence_words_warn() {
        let r = analyze_sensitive_words("含强奸与碎尸的描写。", None, WritingLanguage::Zh);
        // 两个 warn 词表各一条 issue。
        assert!(r.issues.iter().all(|i| i.severity == AuditSeverity::Warning));
        assert!(r.issues.iter().any(|i| i.description.contains("色情敏感词")));
        assert!(r.issues.iter().any(|i| i.description.contains("极端暴力词")));
    }

    #[test]
    fn word_count_tallies_repeats() {
        let r = analyze_sensitive_words("文革。文革。文革。", None, WritingLanguage::Zh);
        let m = r.found.iter().find(|m| m.word == "文革").unwrap();
        assert_eq!(m.count, 3);
    }

    #[test]
    fn custom_words_match_as_warn() {
        let custom = vec!["禁忌词".to_string()];
        let r = analyze_sensitive_words("这里有禁忌词。", Some(&custom), WritingLanguage::Zh);
        let issue = r.issues.iter().find(|i| i.description.contains("自定义敏感词")).expect("应有自定义 issue");
        assert_eq!(issue.severity, AuditSeverity::Warning);
        assert!(issue.description.contains("禁忌词"));
    }

    #[test]
    fn english_language_emits_english_message() {
        let r = analyze_sensitive_words("mentions 文革 here", None, WritingLanguage::En);
        let block = r.issues.iter().find(|i| i.severity == AuditSeverity::Critical).unwrap();
        assert!(block.description.contains("political sensitive terms"));
        assert!(block.suggestion.contains("must remove"));
    }

    #[test]
    fn empty_custom_words_no_issue() {
        let r = analyze_sensitive_words("普通文本", Some(&[]), WritingLanguage::Zh);
        assert!(r.issues.is_empty());
    }

    #[test]
    fn special_regex_chars_in_word_are_escaped() {
        // 自定义词含正则元字符，应被 escape 而非误解析。
        let custom = vec!["a+b".to_string()];
        let r = analyze_sensitive_words("match a+b literally", Some(&custom), WritingLanguage::Zh);
        let m = r.found.iter().find(|m| m.word == "a+b");
        assert!(m.is_some(), "字面匹配 a+b");
        assert_eq!(m.unwrap().count, 1);
    }
}
