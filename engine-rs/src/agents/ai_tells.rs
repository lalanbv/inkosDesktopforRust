//! 结构化 AI 痕迹检测（ai-tells）。
//!
//! 移植自 `packages/core/src/agents/ai-tells.ts`（161 行）。纯规则分析（无 LLM），
//! 检测 AI 生成中文/英文文本的常见模式：
//! - dim 20：段落长度均匀（低变异系数 CV<0.15）
//! - dim 21：套话/模糊词密度（>3 次/千字）
//! - dim 22：公式化转折重复（同一词 ≥3 次）
//! - dim 23：列表式结构（连续 ≥3 句同前缀）

use regex::Regex;
use std::sync::OnceLock;

use crate::utils::language::WritingLanguage;

/// 单条 AI 痕迹问题。对齐 TS `AITellIssue`。
#[derive(Debug, Clone, PartialEq)]
pub struct AITellIssue {
    pub severity: AITellSeverity,
    pub category: String,
    pub description: String,
    pub suggestion: String,
}

/// 问题严重度。对齐 TS `"warning" | "info"`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AITellSeverity {
    Warning,
    Info,
}

/// 检测结果。对齐 TS `AITellResult`。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AITellResult {
    pub issues: Vec<AITellIssue>,
}

const HEDGE_WORDS_ZH: &[&str] = &["似乎", "可能", "或许", "大概", "某种程度上", "一定程度上", "在某种意义上"];
const HEDGE_WORDS_EN: &[&str] = &["seems", "seemed", "perhaps", "maybe", "apparently", "in some ways", "to some extent"];
const TRANSITION_WORDS_ZH: &[&str] = &["然而", "不过", "与此同时", "另一方面", "尽管如此", "话虽如此", "但值得注意的是"];
const TRANSITION_WORDS_EN: &[&str] = &["however", "meanwhile", "on the other hand", "nevertheless", "even so", "still"];

fn paragraph_split_re() -> &'static Regex {
    // TS: /\n\s*\n/
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\n\s*\n").expect("paragraph split regex"))
}

/// 分析文本的 AI 痕迹。对齐 TS `analyzeAITells`。
///
/// `content: AsRef<str>` 兼容 `&str` / `String` 调用（TS 签名 `content: string`）。
pub fn analyze_ai_tells<C: AsRef<str>>(content: C, language: WritingLanguage) -> AITellResult {
    let content = content.as_ref();
    let mut issues: Vec<AITellIssue> = Vec::new();
    let is_english = language == WritingLanguage::En;
    let joiner = if is_english { ", " } else { "、" };

    let paragraphs: Vec<String> = paragraph_split_re()
        .split(content)
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect();

    // dim 20: 段落长度均匀（≥3 段，UTF-16 长度 CV<0.15）。
    if paragraphs.len() >= 3 {
        let lengths: Vec<f64> = paragraphs.iter().map(|p| p.encode_utf16().count() as f64).collect();
        let mean = lengths.iter().sum::<f64>() / lengths.len() as f64;
        if mean > 0.0 {
            let variance = lengths.iter().map(|l| (l - mean).powi(2)).sum::<f64>() / lengths.len() as f64;
            let std_dev = variance.sqrt();
            let cv = std_dev / mean;
            if cv < 0.15 {
                issues.push(AITellIssue {
                    severity: AITellSeverity::Warning,
                    category: if is_english { "Paragraph uniformity" } else { "段落等长" }.to_string(),
                    description: if is_english {
                        format!("Paragraph-length coefficient of variation is only {cv:.3} (threshold <0.15), which suggests unnaturally uniform paragraph sizing")
                    } else {
                        format!("段落长度变异系数仅{cv:.3}（阈值<0.15），段落长度过于均匀，呈现AI生成特征")
                    },
                    suggestion: if is_english {
                        "Increase paragraph-length contrast: use shorter beats for impact and longer blocks for immersive detail".to_string()
                    } else {
                        "增加段落长度差异：短段落用于节奏加速或冲击，长段落用于沉浸描写".to_string()
                    },
                });
            }
        }
    }

    // dim 21: 套话词密度（>3 次/千字）。
    let total_chars = content.encode_utf16().count();
    if total_chars > 0 {
        let hedge_words = if is_english { HEDGE_WORDS_EN } else { HEDGE_WORDS_ZH };
        let hedge_count: usize = hedge_words
            .iter()
            .map(|w| count_word(content, w, is_english))
            .sum();
        let hedge_density = hedge_count as f64 / (total_chars as f64 / 1000.0);
        if hedge_density > 3.0 {
            issues.push(AITellIssue {
                severity: AITellSeverity::Warning,
                category: if is_english { "Hedge density" } else { "套话密度" }.to_string(),
                description: if is_english {
                    format!("Hedge-word density is {hedge_density:.1} per 1k characters (threshold >3), making the prose sound overly tentative")
                } else {
                    format!("套话词（似乎/可能/或许等）密度为{hedge_density:.1}次/千字（阈值>3），语气过于模糊犹豫")
                },
                suggestion: if is_english {
                    "Replace hedges with firmer narration: remove vague qualifiers and use concrete detail instead".to_string()
                } else {
                    "用确定性叙述替代模糊表达：去掉「似乎」直接描述状态，用具体细节替代「可能」".to_string()
                },
            });
        }
    }

    // dim 22: 公式化转折重复（同一词 ≥3 次）。
    let transition_words = if is_english { TRANSITION_WORDS_EN } else { TRANSITION_WORDS_ZH };
    let mut transition_counts: Vec<(String, usize)> = Vec::new();
    for word in transition_words {
        let count = count_word(content, word, is_english);
        if count > 0 {
            let key = if is_english { word.to_lowercase() } else { word.to_string() };
            transition_counts.push((key, count));
        }
    }
    let repeated: Vec<&(String, usize)> = transition_counts.iter().filter(|(_, c)| *c >= 3).collect();
    if !repeated.is_empty() {
        let detail = repeated
            .iter()
            .map(|(w, c)| format!("\"{w}\"×{c}"))
            .collect::<Vec<_>>()
            .join(joiner);
        issues.push(AITellIssue {
            severity: AITellSeverity::Warning,
            category: if is_english { "Formulaic transitions" } else { "公式化转折" }.to_string(),
            description: if is_english {
                format!("Transition words repeat too often: {detail}. Reusing the same transition pattern 3+ times creates a formulaic AI texture")
            } else {
                format!("转折词重复使用：{detail}。同一转折模式≥3次暴露AI生成痕迹")
            },
            suggestion: if is_english {
                "Let scenes pivot through action, timing, or viewpoint shifts instead of repeating the same transitions".to_string()
            } else {
                "用情节自然转折替代转折词，或换用不同的过渡手法（动作切入、时间跳跃、视角切换）".to_string()
            },
        });
    }

    // dim 23: 列表式结构（连续 ≥3 句同前缀）。
    let sentence_split_re = if is_english { sentence_split_en_re() } else { sentence_split_zh_re() };
    let sentences: Vec<String> = sentence_split_re
        .split(content)
        .map(|s| s.trim().to_string())
        .filter(|s| s.encode_utf16().count() > 2)
        .collect();
    if sentences.len() >= 3 {
        let prefixes: Vec<String> = sentences
            .iter()
            .map(|s| {
                if is_english {
                    s.split_whitespace().next().map(|w| w.to_lowercase()).unwrap_or_default()
                } else {
                    // 中文取前 2 字符（char）。
                    s.chars().take(2).collect::<String>()
                }
            })
            .collect();
        let mut consecutive = 1usize;
        let mut max_consecutive = 1usize;
        for i in 1..prefixes.len() {
            if prefixes[i] == prefixes[i - 1] && !prefixes[i].is_empty() {
                consecutive += 1;
                if consecutive > max_consecutive {
                    max_consecutive = consecutive;
                }
            } else {
                consecutive = 1;
            }
        }
        if max_consecutive >= 3 {
            issues.push(AITellIssue {
                severity: AITellSeverity::Info,
                category: if is_english { "List-like structure" } else { "列表式结构" }.to_string(),
                description: if is_english {
                    format!("Detected {max_consecutive} consecutive sentences with the same opening pattern, creating a list-like generated cadence")
                } else {
                    format!("检测到{max_consecutive}句连续以相同开头的句子，呈现列表式AI生成结构")
                },
                suggestion: if is_english {
                    "Vary how sentences open: change subject, timing, or action entry to break the list effect".to_string()
                } else {
                    "变换句式开头：用不同主语、时间词、动作词开头，打破列表感".to_string()
                },
            });
        }
    }

    AITellResult { issues }
}

/// 统计 word 在 content 中的出现次数。英文大小写不敏感（对齐 TS `gi` flag）。
fn count_word(content: &str, word: &str, case_insensitive: bool) -> usize {
    let pattern = if case_insensitive {
        format!("(?i){}", regex::escape(word))
    } else {
        regex::escape(word)
    };
    Regex::new(&pattern)
        .expect("count_word regex")
        .find_iter(content)
        .count()
}

fn sentence_split_en_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"[.!?\n]").expect("en sentence split regex"))
}

fn sentence_split_zh_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"[。！？\n]").expect("zh sentence split regex"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_content_no_issues() {
        let r = analyze_ai_tells("", WritingLanguage::Zh);
        assert!(r.issues.is_empty());
    }

    #[test]
    fn dim20_detects_uniform_paragraph_length() {
        // 3 段，每段 10 字（完全等长）→ CV=0 < 0.15。
        let content = "一二三四五六七八九十\n\n一二三四五六七八九十\n\n一二三四五六七八九十";
        let r = analyze_ai_tells(content, WritingLanguage::Zh);
        let uniform = r.issues.iter().find(|i| i.category == "段落等长");
        assert!(uniform.is_some(), "应检测出段落等长");
        assert_eq!(uniform.unwrap().severity, AITellSeverity::Warning);
    }

    #[test]
    fn dim20_skipped_when_fewer_than_three_paragraphs() {
        let content = "短\n\n长长长长长长长长长长长长长长长长长长长长";
        let r = analyze_ai_tells(content, WritingLanguage::Zh);
        assert!(r.issues.iter().all(|i| i.category != "段落等长"));
    }

    #[test]
    fn dim21_detects_high_hedge_density() {
        // 千字内出现多次「可能」「或许」，密度 > 3。
        let mut content = String::new();
        for _ in 0..8 {
            content.push_str("可能或许似乎大概");
        }
        let r = analyze_ai_tells(content, WritingLanguage::Zh);
        assert!(r.issues.iter().any(|i| i.category == "套话密度"));
    }

    #[test]
    fn dim22_detects_repeated_transitions() {
        let content = "然而A。然而B。然而C。";
        let r = analyze_ai_tells(content, WritingLanguage::Zh);
        let trans = r.issues.iter().find(|i| i.category == "公式化转折");
        assert!(trans.is_some(), "应检测出转折重复");
        assert!(trans.unwrap().description.contains("\"然而\"×3"));
    }

    #[test]
    fn dim23_detects_list_like_same_prefix() {
        // 中文 3 句同前缀（「我们」），连续 ≥3。
        let content = "我们出发。我们战斗。我们胜利。";
        let r = analyze_ai_tells(content, WritingLanguage::Zh);
        assert!(r.issues.iter().any(|i| i.category == "列表式结构"));
    }

    #[test]
    fn english_hedge_detection_case_insensitive() {
        let content = "It seems perhaps Maybe seems apparently. More text here to pad the length out beyond thresholds.";
        let r = analyze_ai_tells(content, WritingLanguage::En);
        // seems(2) + perhaps(1) + maybe(1) + apparently(1) = 5，密度高。
        assert!(r.issues.iter().any(|i| i.category == "Hedge density"));
    }

    #[test]
    fn varied_text_produces_few_issues() {
        // 段落长度差异大、无套话、无重复转折、无连续同前缀。
        let content = "短。\n\n中中中中中中中中中中中中。\n\n长长长长长长长长长长长长长长长长长长长长长长长长长长长长长长长长长长长长长长长长长。";
        let r = analyze_ai_tells(content, WritingLanguage::Zh);
        assert!(r.issues.iter().all(|i| i.category != "段落等长"), "长度差异大不应触发等长");
    }
}
