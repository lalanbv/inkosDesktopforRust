//! 后写规则校验器（post-write-validator）。
//!
//! 移植自 `packages/core/src/agents/post-write-validator.ts`（927 行）。纯函数、零 LLM 成本：
//! 每章生成后跑确定性规则校验，捕获 prompt-only 规则无法保证的违规。
//!
//! ## parity 关键
//! - 所有 TS `string.length` → [`utf16_len`]（UTF-16 码元计数，对齐 JS）
//! - `content.split("我").length - 1` → `matches("我").count()`（无首尾空串差异）
//! - lookbehind `(?<=[。！？!?])` → 手动扫描（Rust regex 不支持 lookbehind）
//! - `\w` ASCII-only → 显式 `[A-Za-z0-9_]`（Rust regex 默认 Unicode）
//! - regex 编译一次，[`OnceLock`] 缓存

use regex::Regex;
use serde::Serialize;
use std::sync::OnceLock;

use crate::models::book_rules::{BookRules, NarrativePerson};
use crate::models::genre_profile::GenreProfile;
use crate::utils::cadence_policy::CadencePressure;
use crate::utils::chapter_cadence::{analyze_chapter_cadence, CadenceSummaryRow};
use crate::utils::language::{utf16_len, WritingLanguage};

/// 后写校验违规（对齐 TS `PostWriteViolation`）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PostWriteViolation {
    pub rule: String,
    pub severity: ViolationSeverity,
    pub description: String,
    pub suggestion: String,
}

/// 违规严重度（对齐 TS `"error" | "warning"`）。序列化为小写串以与 TS JSON 对齐。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ViolationSeverity {
    Error,
    Warning,
}

impl ViolationSeverity {
    pub fn as_str(self) -> &'static str {
        match self {
            ViolationSeverity::Error => "error",
            ViolationSeverity::Warning => "warning",
        }
    }
}

// ── 标记词表（对齐 TS 常量）──

const SURPRISE_MARKERS: &[&str] = &["仿佛", "忽然", "竟然", "猛地", "猛然", "不禁", "宛如"];

const REPORT_TERMS: &[&str] = &[
    "核心动机",
    "信息边界",
    "信息落差",
    "核心风险",
    "利益最大化",
    "当前处境",
    "行为约束",
    "性格过滤",
    "情绪外化",
    "锚定效应",
    "沉没成本",
    "认知共鸣",
];

const SERMON_WORDS: &[&str] = &["显然", "毋庸置疑", "不言而喻", "众所周知", "不难看出"];

const AI_TELL_WORDS: &[&str] = &[
    "delve",
    "tapestry",
    "testament",
    "intricate",
    "pivotal",
    "vibrant",
    "embark",
    "comprehensive",
    "nuanced",
];

const ENGLISH_NAME_STOP_WORDS: &[&str] = &[
    "The", "And", "But", "When", "While", "After", "Before", "Even", "Then", "They",
];

const CHINESE_TITLE_STOP_WORDS: &[&str] = &[
    "这次", "正文", "标题", "重复", "不同", "完全", "只是", "碰巧", "没有", "回头",
];

const CHINESE_TITLE_STOP_CHARS: &[char] = &[
    '的', '了', '着', '一', '只', '从', '在', '和', '与', '把', '被', '有', '没', '里', '又', '才',
];

fn meta_narration_patterns() -> &'static [Regex] {
    static R: OnceLock<Vec<Regex>> = OnceLock::new();
    R.get_or_init(|| {
        [
            r"到这里[，,]?算是",
            r"接下来[，,]?(?:就是|将会|即将)",
            r"(?:后面|之后)[，,]?(?:会|将|还会)",
            r"(?:故事|剧情)(?:发展)?到了",
            r"读者[，,]?(?:可能|应该|也许)",
            r"我们[，,]?(?:可以|不妨|来看)",
        ]
        .iter()
        .map(|p| Regex::new(p).expect("meta narration regex"))
        .collect()
    })
}

fn collective_shock_patterns() -> &'static [Regex] {
    static R: OnceLock<Vec<Regex>> = OnceLock::new();
    R.get_or_init(|| {
        [
            r"(?:全场|众人|所有人|在场的人)[，,]?(?:都|全|齐齐|纷纷)?(?:震惊|惊呆|倒吸凉气|目瞪口呆|哗然|惊呼)",
            r"(?:全场|一片)[，,]?(?:寂静|哗然|沸腾|震动)",
        ]
        .iter()
        .map(|p| Regex::new(p).expect("collective shock regex"))
        .collect()
    })
}

fn meta_note_en_re() -> &'static Regex {
    // TS: /^\s*\[(?:polisher|writer|reviser|reviewer)-note\]\s*/i
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"(?i)^\s*\[(?:polisher|writer|reviser|reviewer)-note\]\s*").expect("meta en regex")
    })
}

fn meta_note_zh_re() -> &'static Regex {
    // TS: /^\s*\[(?:润色|写作|修订|审稿)备注\]\s*/
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^\s*\[(?:润色|写作|修订|审稿)备注\]\s*").expect("meta zh regex"))
}

fn dash_re() -> &'static Regex {
    // TS: /——+/g
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new("——+").expect("dash regex"))
}

fn not_but_re() -> &'static Regex {
    // TS: /不是[^，。！？\n]{0,30}[，,]?\s*而是/
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"不是[^，。！？\n]{0,30}[，,]?\s*而是").expect("not-but regex"))
}

fn chapter_ref_re() -> &'static Regex {
    // TS: /(?:第\s*\d+\s*章|[Cc]hapter\s+\d+)/g
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?:第\s*\d+\s*章|[Cc]hapter\s+\d+)").expect("chapter ref regex"))
}

fn sentence_split_re() -> &'static Regex {
    // TS: /[。！？]/
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"[。！？]").expect("sentence split regex"))
}

fn paragraph_split_re() -> &'static Regex {
    // TS: /\n\s*\n/
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\n\s*\n").expect("paragraph split regex"))
}

fn inner_state_slip_re() -> &'static Regex {
    // TS: /^[他她][^。！？!?]{0,18}(?:觉得|感到|意识到|明白|想起|脑子里|心里|太阳穴)/
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"^[他她][^。！？!?]{0,18}(?:觉得|感到|意识到|明白|想起|脑子里|心里|太阳穴)")
            .expect("inner state slip regex")
    })
}

fn en_word_re() -> &'static Regex {
    // TS: /[A-Za-z]{4,}/g
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"[A-Za-z]{4,}").expect("en word regex"))
}

fn en_quoted_re() -> &'static Regex {
    // TS: /"[^"]+"/g
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r#""[^"]+""#).expect("en quoted regex"))
}

fn en_name_re() -> &'static Regex {
    // TS: /\b[A-Z][a-z]{2,}\b/g
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\b[A-Z][a-z]{2,}\b").expect("en name regex"))
}

fn en_non_word_re() -> &'static Regex {
    // TS: /[^\w\s']/g （JS \w = ASCII [A-Za-z0-9_]）
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"[^A-Za-z0-9_\s']").expect("en non-word regex"))
}

fn cjk_segment_re() -> &'static Regex {
    // TS: /[\u4e00-\u9fff]+/g
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"[\u{4e00}-\u{9fff}]+").expect("cjk segment regex"))
}

fn cjk_six_re() -> &'static Regex {
    // TS: /^[\u4e00-\u9fff]{6}$/
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^[\u{4e00}-\u{9fff}]{6}$").expect("cjk six regex"))
}

fn whitespace_re() -> &'static Regex {
    // TS: /[\s\n\r]/g
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"[\s]").expect("whitespace regex"))
}

fn non_alnum_re() -> &'static Regex {
    // TS: /[^\p{L}\p{N}]/gu
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"[^\p{L}\p{N}]").expect("non-alnum regex"))
}

// ── normalizePostWriteSurface ──

/// 规范化正文表面：剥 meta 备注行 + 破折号转逗号（非 en）+ trimEnd。
/// 逐字移植 TS `normalizePostWriteSurface`。
pub fn normalize_post_write_surface(content: &str, language_override: Option<WritingLanguage>) -> String {
    let normalized = strip_post_write_meta_lines(content);
    let normalized = if language_override != Some(WritingLanguage::En) {
        dash_re().replace_all(&normalized, "，").to_string()
    } else {
        normalized
    };
    // TS: trimEnd() —— 去尾部空白（含 \n \r \t space）。
    normalized.trim_end().to_string()
}

fn strip_post_write_meta_lines(content: &str) -> String {
    // TS: split(/\r?\n/) —— \r\n 视为单分隔符。Rust split('\n') 后剥尾部 \r 等价。
    content
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .filter(|line| !meta_note_en_re().is_match(line) && !meta_note_zh_re().is_match(line))
        .collect::<Vec<_>>()
        .join("\n")
}

// ── validatePostWrite ──

/// 中文/英文后写校验主入口。逐字移植 TS `validatePostWrite`。
pub fn validate_post_write(
    content: &str,
    genre_profile: &GenreProfile,
    book_rules: Option<&BookRules>,
    language_override: Option<WritingLanguage>,
) -> Vec<PostWriteViolation> {
    let is_english = match language_override {
        Some(lang) => lang == WritingLanguage::En,
        None => genre_profile.language == "en",
    };
    if is_english {
        return validate_post_write_english(content, genre_profile, book_rules);
    }
    validate_post_write_zh(content, genre_profile, book_rules)
}

fn validate_post_write_zh(
    content: &str,
    genre_profile: &GenreProfile,
    book_rules: Option<&BookRules>,
) -> Vec<PostWriteViolation> {
    let mut violations = Vec::new();
    let len = utf16_len(content);

    // 1. 不是…而是… 句式
    if not_but_re().is_match(content) {
        violations.push(PostWriteViolation {
            rule: "禁止句式".to_string(),
            severity: ViolationSeverity::Error,
            description: "出现了「不是……而是……」句式".to_string(),
            suggestion: "改用直述句".to_string(),
        });
    }

    // 2. 破折号
    if content.contains("——") {
        violations.push(PostWriteViolation {
            rule: "禁止破折号".to_string(),
            severity: ViolationSeverity::Error,
            description: "出现了破折号「——」".to_string(),
            suggestion: "用逗号或句号断句".to_string(),
        });
    }

    // 3. 转折/惊讶标记词密度
    let mut marker_counts: Vec<(&str, usize)> = Vec::new();
    let mut total_marker_count = 0usize;
    for &word in SURPRISE_MARKERS {
        let count = count_substring(content, word);
        if count > 0 {
            marker_counts.push((word, count));
            total_marker_count += count;
        }
    }
    let marker_limit = (len / 3000).max(1);
    if total_marker_count > marker_limit {
        let detail = marker_counts
            .iter()
            .map(|(w, c)| format!("\"{w}\"×{c}"))
            .collect::<Vec<_>>()
            .join("、");
        violations.push(PostWriteViolation {
            rule: "转折词密度".to_string(),
            severity: ViolationSeverity::Warning,
            description: format!(
                "转折/惊讶标记词共{total_marker_count}次（上限{marker_limit}次/{len}字），明细：{detail}"
            ),
            suggestion: "改用具体动作或感官描写传递突然性".to_string(),
        });
    }

    // 4. 高疲劳词（bookRules override 优先）
    let fatigue_override = book_rules.and_then(|r| {
        if !r.fatigue_words_override.is_empty() {
            Some(&r.fatigue_words_override)
        } else {
            None
        }
    });
    let fatigue_words: &[String] = fatigue_override.unwrap_or(&genre_profile.fatigue_words);
    for word in fatigue_words {
        let count = count_substring(content, word);
        if count > 1 {
            violations.push(PostWriteViolation {
                rule: "高疲劳词".to_string(),
                severity: ViolationSeverity::Warning,
                description: format!("高疲劳词\"{word}\"出现{count}次（上限1次/章）"),
                suggestion: format!("替换多余的\"{word}\"为同义但不同形式的表达"),
            });
        }
    }

    // 5. 元叙事（报一次即止）
    for pattern in meta_narration_patterns() {
        if let Some(m) = pattern.find(content) {
            violations.push(PostWriteViolation {
                rule: "元叙事".to_string(),
                severity: ViolationSeverity::Warning,
                description: format!("出现编剧旁白式表述：\"{}\"", &content[m.start()..m.end()]),
                suggestion: "删除元叙事，让剧情自然展开".to_string(),
            });
            break;
        }
    }

    // 6. 分析报告式术语
    let found_terms: Vec<&str> = REPORT_TERMS.iter().copied().filter(|t| content.contains(*t)).collect();
    if !found_terms.is_empty() {
        let detail = found_terms
            .iter()
            .map(|t| format!("\"{t}\""))
            .collect::<Vec<_>>()
            .join("、");
        violations.push(PostWriteViolation {
            rule: "报告术语".to_string(),
            severity: ViolationSeverity::Error,
            description: format!("正文中出现分析报告术语：{detail}"),
            suggestion: "这些术语只能用于 PRE_WRITE_CHECK 内部推理，正文中用口语化表达替代".to_string(),
        });
    }

    // 7. 章节号指称
    let chapter_refs: Vec<String> = chapter_ref_re()
        .find_iter(content)
        .map(|m| m.as_str().to_string())
        .collect();
    if !chapter_refs.is_empty() {
        let mut unique: Vec<String> = Vec::new();
        for r in &chapter_refs {
            if !unique.contains(r) {
                unique.push(r.clone());
            }
        }
        let detail = unique.iter().map(|r| format!("\"{r}\"")).collect::<Vec<_>>().join("、");
        violations.push(PostWriteViolation {
            rule: "章节号指称".to_string(),
            severity: ViolationSeverity::Error,
            description: format!("正文中出现了章节号指称：{detail}。角色不知道自己在第几章。"),
            suggestion: "改成自然表达：\"那天晚上\"、\"仓库出事那次\"、\"码头上的事\"".to_string(),
        });
    }

    // 8. 作者说教词
    let found_sermons: Vec<&str> = SERMON_WORDS.iter().copied().filter(|w| content.contains(*w)).collect();
    if !found_sermons.is_empty() {
        let detail = found_sermons
            .iter()
            .map(|w| format!("\"{w}\""))
            .collect::<Vec<_>>()
            .join("、");
        violations.push(PostWriteViolation {
            rule: "作者说教".to_string(),
            severity: ViolationSeverity::Warning,
            description: format!("出现说教词：{detail}"),
            suggestion: "删除说教词，让读者自己从情节中判断".to_string(),
        });
    }

    // 8b. 全场震惊类集体反应（报一次即止）
    for pattern in collective_shock_patterns() {
        if let Some(m) = pattern.find(content) {
            violations.push(PostWriteViolation {
                rule: "集体反应".to_string(),
                severity: ViolationSeverity::Warning,
                description: format!("出现集体反应套话：\"{}\"", &content[m.start()..m.end()]),
                suggestion: "改写成1-2个具体角色的身体反应".to_string(),
            });
            break;
        }
    }

    // 9. 连续"了"字
    let sentences: Vec<&str> = sentence_split_re()
        .split(content)
        .map(str::trim)
        .filter(|s| utf16_len(s) > 2)
        .collect();
    let mut consecutive_le = 0usize;
    let mut max_consecutive_le = 0usize;
    for sentence in &sentences {
        if sentence.contains('了') {
            consecutive_le += 1;
            max_consecutive_le = max_consecutive_le.max(consecutive_le);
        } else {
            consecutive_le = 0;
        }
    }
    if max_consecutive_le >= 6 {
        violations.push(PostWriteViolation {
            rule: "连续了字".to_string(),
            severity: ViolationSeverity::Warning,
            description: format!("检测到{max_consecutive_le}句连续包含\"了\"字，节奏拖沓"),
            suggestion: "保留最有力的一个「了」，其余改为无「了」句式".to_string(),
        });
    }

    // 10. 段落过长
    let paragraphs: Vec<&str> = paragraph_split_re()
        .split(content)
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    let long_paragraphs = paragraphs.iter().filter(|p| utf16_len(p) > 300).count();
    if long_paragraphs >= 2 {
        violations.push(PostWriteViolation {
            rule: "段落过长".to_string(),
            severity: ViolationSeverity::Warning,
            description: format!("{long_paragraphs}个段落超过300字，不适合手机阅读"),
            suggestion: "长段落拆分为3-5行的短段落，在动作切换或情绪节点处断开".to_string(),
        });
    }

    violations.extend(detect_paragraph_shape_warnings(content, WritingLanguage::Zh));

    // 11. 本书禁忌（短词 2-30 字符 substring 匹配）
    if let Some(rules) = book_rules {
        for prohibition in &rules.prohibitions {
            let plen = utf16_len(prohibition);
            if (2..=30).contains(&plen) && content.contains(prohibition.as_str()) {
                violations.push(PostWriteViolation {
                    rule: "本书禁忌".to_string(),
                    severity: ViolationSeverity::Error,
                    description: format!("出现了本书禁忌内容：\"{prohibition}\""),
                    suggestion: "删除或改写该内容".to_string(),
                });
            }
        }
    }

    // 12. 叙事人称漂移
    if let Some(v) = detect_narrative_person_drift(content, book_rules) {
        violations.push(v);
    }

    violations
}

fn detect_narrative_person_drift(
    content: &str,
    book_rules: Option<&BookRules>,
) -> Option<PostWriteViolation> {
    let rules = book_rules?;
    if rules.narrative_person != Some(NarrativePerson::First) {
        return None;
    }
    if let Some(slip) = detect_first_person_inner_state_slip(content) {
        return Some(PostWriteViolation {
            rule: "叙事人称".to_string(),
            severity: ViolationSeverity::Error,
            description: format!("本书设定为第一人称，但出现了第三人称内感叙述：\"{slip}\""),
            suggestion: "把这类主观感受、意识、脑内活动改回「我」的内心视角；不要切到第三人称或全知视角。".to_string(),
        });
    }

    let name = rules.protagonist.as_ref().map(|p| p.name.trim()).filter(|n| !n.is_empty())?;
    let wo_count = count_substring(content, "我");
    let name_count = count_substring(content, name);
    if utf16_len(content) >= 800 && wo_count < 12 && name_count >= 6 && name_count > wo_count {
        return Some(PostWriteViolation {
            rule: "叙事人称".to_string(),
            severity: ViolationSeverity::Error,
            description: format!(
                "本书设定为第一人称，但本章几乎不用「我」（{wo_count} 次）却反复以「{name}」第三人称叙述（{name_count} 次）"
            ),
            suggestion: "改用第一人称（主角内心视角）重写本章叙事".to_string(),
        });
    }
    None
}

/// lookbehind `(?<=[。！？!?])|\n+` 的手动等价：句末标点后切分（标点归前段），换行处切分。
fn split_sentences_for_inner_state(content: &str) -> Vec<String> {
    let mut sentences = Vec::new();
    let mut current = String::new();
    for c in content.chars() {
        if c == '\n' {
            if !current.is_empty() {
                sentences.push(std::mem::take(&mut current));
            }
        } else {
            current.push(c);
            if matches!(c, '。' | '！' | '？' | '!' | '?') {
                sentences.push(std::mem::take(&mut current));
            }
        }
    }
    if !current.is_empty() {
        sentences.push(current);
    }
    sentences
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn detect_first_person_inner_state_slip(content: &str) -> Option<String> {
    let sentences = split_sentences_for_inner_state(content);
    for sentence in &sentences {
        let first = sentence.chars().next();
        if !matches!(first, Some('他') | Some('她')) {
            continue;
        }
        if inner_state_slip_re().is_match(sentence) {
            return Some(if utf16_len(sentence) > 40 {
                let truncated: String = sentence.chars().take(39).collect();
                format!("{truncated}…")
            } else {
                sentence.clone()
            });
        }
    }
    None
}

// ── validatePostWriteEnglish ──

fn validate_post_write_english(
    content: &str,
    genre_profile: &GenreProfile,
    book_rules: Option<&BookRules>,
) -> Vec<PostWriteViolation> {
    let mut violations = Vec::new();
    let len = utf16_len(content);

    // 1. AI-tell word density
    for &word in AI_TELL_WORDS {
        let re = word_boundary_re(word);
        let count = re.find_iter(content).count();
        if count > (len as f64 / 3000.0).ceil() as usize {
            violations.push(PostWriteViolation {
                rule: "AI-tell word density".to_string(),
                severity: ViolationSeverity::Warning,
                description: format!("\"{word}\" appears {count} times (limit: 1 per 3000 chars)"),
                suggestion: "Replace with a more specific word".to_string(),
            });
        }
    }

    // 2. Paragraph overflow（>500 字符）
    let paragraphs: Vec<&str> = paragraph_split_re()
        .split(content)
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    let long_paragraphs = paragraphs.iter().filter(|p| utf16_len(p) > 500).count();
    if long_paragraphs >= 2 {
        violations.push(PostWriteViolation {
            rule: "Paragraph length".to_string(),
            severity: ViolationSeverity::Warning,
            description: format!("{long_paragraphs} paragraphs exceed 500 characters"),
            suggestion: "Break into shorter paragraphs for readability".to_string(),
        });
    }

    violations.extend(detect_paragraph_shape_warnings(content, WritingLanguage::En));

    // 2.5. Multi-character scene with almost no direct exchange
    let quoted_lines: Vec<String> = en_quoted_re().find_iter(content).map(|m| m.as_str().to_string()).collect();
    let mut english_names: Vec<String> = Vec::new();
    for m in en_name_re().find_iter(content) {
        let name = m.as_str();
        if !ENGLISH_NAME_STOP_WORDS.contains(&name) && !english_names.iter().any(|n| n == name) {
            english_names.push(name.to_string());
        }
    }
    if english_names.len() >= 2 && quoted_lines.len() < 2 && len >= 120 {
        let names_preview = english_names.iter().take(3).cloned().collect::<Vec<_>>().join(", ");
        violations.push(PostWriteViolation {
            rule: "Dialogue pressure".to_string(),
            severity: ViolationSeverity::Warning,
            description: format!(
                "Multi-character scene appears to rely on narration with almost no direct exchange ({names_preview})."
            ),
            suggestion: "Add at least one resistance-bearing exchange so characters push back, withhold, or pressure each other directly.".to_string(),
        });
    }

    // 3. Book prohibitions（toLowerCase includes，2-50 字符）
    if let Some(rules) = book_rules {
        let content_lower = content.to_lowercase();
        for prohibition in &rules.prohibitions {
            let plen = utf16_len(prohibition);
            if (2..=50).contains(&plen) {
                let prohibition_lower = prohibition.to_lowercase();
                if content_lower.contains(&prohibition_lower) {
                    violations.push(PostWriteViolation {
                        rule: "Book prohibition".to_string(),
                        severity: ViolationSeverity::Error,
                        description: format!("Found banned content: \"{prohibition}\""),
                        suggestion: "Remove or rewrite this content".to_string(),
                    });
                }
            }
        }
    }

    // 4. Genre fatigue words
    let fatigue_override = book_rules.and_then(|r| {
        if !r.fatigue_words_override.is_empty() {
            Some(&r.fatigue_words_override)
        } else {
            None
        }
    });
    let fatigue_words: &[String] = fatigue_override.unwrap_or(&genre_profile.fatigue_words);
    for word in fatigue_words {
        let re = word_boundary_re(word);
        let count = re.find_iter(content).count();
        if count > 1 {
            violations.push(PostWriteViolation {
                rule: "Fatigue word".to_string(),
                severity: ViolationSeverity::Warning,
                description: format!("\"{word}\" appears {count} times (max 1 per chapter)"),
                suggestion: "Vary the vocabulary".to_string(),
            });
        }
    }

    violations
}

/// 构造 `\b{word}\b` 大小写不敏感正则。TS: `new RegExp("\\b${word}\\b", "gi")`。
fn word_boundary_re(word: &str) -> Regex {
    // word 来自配置/词表，假定无 regex 元字符；保险起见仍 escape 非字母数字。
    let escaped = regex_escape(word);
    Regex::new(&format!(r"(?i)\b{escaped}\b")).unwrap_or_else(|_| Regex::new(r"(?i)\b\b").expect("fallback regex"))
}

/// 对齐 TS `word.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")`：转义 regex 元字符。
fn regex_escape(word: &str) -> String {
    word.chars()
        .map(|c| {
            if ".*+?^${}()|[]\\".contains(c) {
                format!("\\{c}")
            } else {
                c.to_string()
            }
        })
        .collect()
}

// ── 段落形状 ──

#[derive(Debug, Clone)]
struct ParagraphShape {
    paragraphs: Vec<String>,
    short_threshold: usize,
    short_paragraphs: Vec<String>,
    short_ratio: f64,
    average_length: f64,
    max_consecutive_short: usize,
}

fn detect_paragraph_shape_warnings(content: &str, language: WritingLanguage) -> Vec<PostWriteViolation> {
    let mut violations = Vec::new();
    append_paragraph_shape_warnings(&mut violations, content, language);
    violations
}

fn append_paragraph_shape_warnings(
    violations: &mut Vec<PostWriteViolation>,
    content: &str,
    language: WritingLanguage,
) {
    let shape = analyze_paragraph_shape(content, language);
    if shape.paragraphs.len() < 4 {
        return;
    }

    if shape.short_paragraphs.len() >= 4 && shape.short_ratio >= 0.6 {
        let total = shape.paragraphs.len();
        let short = shape.short_paragraphs.len();
        let threshold = shape.short_threshold;
        violations.push(if language == WritingLanguage::En {
            PostWriteViolation {
                rule: "Paragraph fragmentation".to_string(),
                severity: ViolationSeverity::Warning,
                description: format!("{short} of {total} paragraphs are shorter than {threshold} characters."),
                suggestion: "Merge adjacent action, observation, and reaction beats so the chapter does not collapse into one-line paragraphs.".to_string(),
            }
        } else {
            PostWriteViolation {
                rule: "段落过碎".to_string(),
                severity: ViolationSeverity::Warning,
                description: format!("{total}个段落里有{short}个不足{threshold}字，段落被切得过碎。"),
                suggestion: "把相邻的动作、观察、反应适当并段，不要每句话都单独起段。".to_string(),
            }
        });
    }

    if shape.max_consecutive_short >= 3 {
        let mcs = shape.max_consecutive_short;
        let threshold = shape.short_threshold;
        violations.push(if language == WritingLanguage::En {
            PostWriteViolation {
                rule: "Consecutive short paragraphs".to_string(),
                severity: ViolationSeverity::Warning,
                description: format!("{mcs} short paragraphs appear back to back."),
                suggestion: "Break the one-beat-per-paragraph rhythm by folding connected beats into fuller paragraphs.".to_string(),
            }
        } else {
            PostWriteViolation {
                rule: "连续短段".to_string(),
                severity: ViolationSeverity::Warning,
                description: format!("连续出现{mcs}个不足{threshold}字的短段，容易形成短句堆砌。"),
                suggestion: "把连续的碎动作重新编组，至少让一个段落承载完整的动作链或情绪推进。".to_string(),
            }
        });
    }
}

fn is_dialogue_paragraph(paragraph: &str) -> bool {
    let trimmed = paragraph.trim();
    let first = trimmed.chars().next();
    match first {
        Some('"') | Some('\u{201C}') | Some('\u{201D}') | Some('「') | Some('『') | Some('\'') | Some('《') => true,
        _ => trimmed.starts_with("——"),
    }
}

fn analyze_paragraph_shape(content: &str, language: WritingLanguage) -> ParagraphShape {
    let paragraphs = extract_paragraphs(content);
    let narrative_paragraphs: Vec<&String> = paragraphs.iter().filter(|p| !is_dialogue_paragraph(p)).collect();
    let short_threshold = if language == WritingLanguage::En { 120 } else { 35 };
    let short_paragraphs: Vec<String> = narrative_paragraphs
        .iter()
        .filter(|p| utf16_len(p.as_str()) < short_threshold)
        .map(|p| (*p).clone())
        .collect();
    let average_length = if !paragraphs.is_empty() {
        let total: usize = paragraphs.iter().map(|p| utf16_len(p.as_str())).sum();
        total as f64 / paragraphs.len() as f64
    } else {
        0.0
    };

    let mut max_consecutive_short = 0usize;
    let mut current_consecutive = 0usize;
    for paragraph in &narrative_paragraphs {
        if utf16_len(paragraph.as_str()) < short_threshold {
            current_consecutive += 1;
            max_consecutive_short = max_consecutive_short.max(current_consecutive);
        } else {
            current_consecutive = 0;
        }
    }

    let short_ratio = if !narrative_paragraphs.is_empty() {
        short_paragraphs.len() as f64 / narrative_paragraphs.len() as f64
    } else {
        0.0
    };

    ParagraphShape {
        paragraphs,
        short_threshold,
        short_paragraphs,
        short_ratio,
        average_length,
        max_consecutive_short,
    }
}

fn extract_paragraphs(content: &str) -> Vec<String> {
    paragraph_split_re()
        .split(content)
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .filter(|p| *p != "---")
        .filter(|p| !p.starts_with('#'))
        .map(str::to_string)
        .collect()
}

// ── 跨章重复 ──

/// 跨章重复检测。逐字移植 TS `detectCrossChapterRepetition`。
pub fn detect_cross_chapter_repetition(
    current_content: &str,
    recent_chapters_content: &str,
    language: WritingLanguage,
) -> Vec<PostWriteViolation> {
    if recent_chapters_content.is_empty() || utf16_len(recent_chapters_content) < 100 {
        return Vec::new();
    }

    let mut violations = Vec::new();

    if language == WritingLanguage::En {
        // 3-word phrases
        let lower = current_content.to_lowercase();
        let cleaned = en_non_word_re().replace_all(&lower, "");
        let words: Vec<&str> = cleaned.split_whitespace().filter(|w| w.chars().count() > 2).collect();
        let mut phrase_counts: Vec<(String, usize)> = Vec::new();
        if words.len() >= 3 {
            for window in words.windows(3) {
                let phrase = format!("{} {} {}", window[0], window[1], window[2]);
                if let Some(entry) = phrase_counts.iter_mut().find(|(p, _)| *p == phrase) {
                    entry.1 += 1;
                } else {
                    phrase_counts.push((phrase, 1));
                }
            }
        }
        let recent_lower = recent_chapters_content.to_lowercase();
        let mut cross_repeats: Vec<String> = Vec::new();
        for (phrase, count) in &phrase_counts {
            if *count >= 2 && recent_lower.contains(phrase.as_str()) {
                cross_repeats.push(format!("\"{phrase}\" (×{count})"));
            }
        }
        if cross_repeats.len() >= 3 {
            let preview = cross_repeats.iter().take(5).cloned().collect::<Vec<_>>().join(", ");
            violations.push(PostWriteViolation {
                rule: "Cross-chapter repetition".to_string(),
                severity: ViolationSeverity::Warning,
                description: format!(
                    "{} repeated phrases also found in recent chapters: {}",
                    cross_repeats.len(),
                    preview
                ),
                suggestion: "Vary action verbs and descriptive phrases to avoid cross-chapter repetition".to_string(),
            });
        }
    } else {
        // Chinese 6-char ngrams
        let chars: String = whitespace_re().replace_all(current_content, "").to_string();
        let chars_vec: Vec<char> = chars.chars().collect();
        let mut phrase_counts: Vec<(String, usize)> = Vec::new();
        if chars_vec.len() >= 6 {
            for window in chars_vec.windows(6) {
                let phrase: String = window.iter().collect();
                if cjk_six_re().is_match(&phrase) {
                    if let Some(entry) = phrase_counts.iter_mut().find(|(p, _)| *p == phrase) {
                        entry.1 += 1;
                    } else {
                        phrase_counts.push((phrase, 1));
                    }
                }
            }
        }
        let recent_clean = whitespace_re().replace_all(recent_chapters_content, "").to_string();
        let mut cross_repeats: Vec<String> = Vec::new();
        for (phrase, count) in &phrase_counts {
            if *count >= 2 && recent_clean.contains(phrase.as_str()) {
                cross_repeats.push(format!("\"{phrase}\"(×{count})"));
            }
        }
        if cross_repeats.len() >= 3 {
            let preview = cross_repeats.iter().take(5).cloned().collect::<Vec<_>>().join("、");
            violations.push(PostWriteViolation {
                rule: "跨章重复".to_string(),
                severity: ViolationSeverity::Warning,
                description: format!("{}个重复短语在近期章节中也出现过：{}", cross_repeats.len(), preview),
                suggestion: "变换动作描写和场景用语，避免跨章节机械重复".to_string(),
            });
        }
    }

    violations
}

// ── 段落长度漂移 ──

/// 段落密度漂移检测。逐字移植 TS `detectParagraphLengthDrift`。
pub fn detect_paragraph_length_drift(
    current_content: &str,
    recent_chapters_content: &str,
    language: WritingLanguage,
) -> Vec<PostWriteViolation> {
    if recent_chapters_content.trim().is_empty() {
        return Vec::new();
    }

    let current = analyze_paragraph_shape(current_content, language);
    let recent = analyze_paragraph_shape(recent_chapters_content, language);

    if current.paragraphs.len() < 4 || recent.paragraphs.len() < 4 {
        return Vec::new();
    }
    if recent.average_length <= 0.0 || current.average_length <= 0.0 {
        return Vec::new();
    }

    let shrink_ratio = current.average_length / recent.average_length;
    let short_ratio_delta = current.short_ratio - recent.short_ratio;

    if shrink_ratio >= 0.6 || current.short_ratio < 0.5 || short_ratio_delta < 0.25 {
        return Vec::new();
    }

    let drop_percent = ((1.0 - shrink_ratio) * 100.0).round() as i64;
    let recent_avg = recent.average_length.round() as i64;
    let current_avg = current.average_length.round() as i64;

    let violation = if language == WritingLanguage::En {
        PostWriteViolation {
            rule: "Paragraph density drift".to_string(),
            severity: ViolationSeverity::Warning,
            description: format!(
                "Average paragraph length dropped from {recent_avg} to {current_avg} characters ({drop_percent}% shorter) compared with recent chapters."
            ),
            suggestion: "Let action, observation, and reaction share paragraphs more often instead of cutting every beat into a single short line.".to_string(),
        }
    } else {
        PostWriteViolation {
            rule: "段落密度漂移".to_string(),
            severity: ViolationSeverity::Warning,
            description: format!(
                "当前章平均段长从近期章节的{recent_avg}字降到{current_avg}字，缩短了{drop_percent}%。"
            ),
            suggestion: "不要把每个动作都切成单独短句；适当把动作、观察和反应并入同一段，恢复段落层次。".to_string(),
        }
    };

    vec![violation]
}

// ── 标题查重 / 去重 / 重生成 ──

/// 标题查重。逐字移植 TS `detectDuplicateTitle`。
pub fn detect_duplicate_title(new_title: &str, existing_titles: &[String]) -> Vec<PostWriteViolation> {
    if new_title.trim().is_empty() {
        return Vec::new();
    }
    let normalized = new_title.trim().to_lowercase();
    let mut violations = Vec::new();

    for existing in existing_titles {
        let existing_norm = existing.trim().to_lowercase();
        if existing_norm.is_empty() {
            continue;
        }
        if normalized == existing_norm {
            violations.push(PostWriteViolation {
                rule: "duplicate-title".to_string(),
                severity: ViolationSeverity::Warning,
                description: format!("章节标题\"{new_title}\"与已有章节标题完全相同"),
                suggestion: "更换一个不同的章节标题".to_string(),
            });
            break;
        }
        let strip_new = strip_punct(&normalized);
        let strip_existing = strip_punct(&existing_norm);
        if strip_new == strip_existing {
            violations.push(PostWriteViolation {
                rule: "near-duplicate-title".to_string(),
                severity: ViolationSeverity::Warning,
                description: format!("章节标题\"{new_title}\"与已有标题\"{existing}\"高度相似"),
                suggestion: "避免使用相似的章节标题".to_string(),
            });
            break;
        }
    }
    violations
}

/// 标题去重解析（含重生成）。逐字移植 TS `resolveDuplicateTitle`。
pub fn resolve_duplicate_title(
    new_title: &str,
    existing_titles: &[String],
    language: WritingLanguage,
    content: Option<&str>,
) -> ResolveDuplicateTitleResult {
    let trimmed = new_title.trim();
    if trimmed.is_empty() {
        return ResolveDuplicateTitleResult {
            title: new_title.to_string(),
            issues: Vec::new(),
        };
    }

    let duplicate_issues = detect_duplicate_title(trimmed, existing_titles);
    if !duplicate_issues.is_empty() {
        if let Some(regenerated) = regenerate_duplicate_title(trimmed, existing_titles, language, content) {
            if detect_duplicate_title(&regenerated, existing_titles).is_empty() {
                return ResolveDuplicateTitleResult {
                    title: regenerated,
                    issues: duplicate_issues,
                };
            }
        }
        let mut counter = 2u32;
        while counter < 100 {
            let candidate = if language == WritingLanguage::En {
                format!("{trimmed} ({counter})")
            } else {
                format!("{trimmed}（{counter}）")
            };
            if detect_duplicate_title(&candidate, existing_titles).is_empty() {
                return ResolveDuplicateTitleResult {
                    title: candidate,
                    issues: duplicate_issues,
                };
            }
            counter += 1;
        }
        return ResolveDuplicateTitleResult {
            title: trimmed.to_string(),
            issues: duplicate_issues,
        };
    }

    let collapse_issues = detect_title_collapse(trimmed, existing_titles, language);
    if collapse_issues.is_empty() {
        return ResolveDuplicateTitleResult {
            title: trimmed.to_string(),
            issues: Vec::new(),
        };
    }

    if let Some(regenerated) = regenerate_collapsed_title(trimmed, existing_titles, language, content) {
        if detect_duplicate_title(&regenerated, existing_titles).is_empty()
            && detect_title_collapse(&regenerated, existing_titles, language).is_empty()
        {
            return ResolveDuplicateTitleResult {
                title: regenerated,
                issues: collapse_issues,
            };
        }
    }

    ResolveDuplicateTitleResult {
        title: trimmed.to_string(),
        issues: collapse_issues,
    }
}

/// 标题去重结果（对齐 TS `{ title, issues }`）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ResolveDuplicateTitleResult {
    pub title: String,
    pub issues: Vec<PostWriteViolation>,
}

fn detect_title_collapse(
    new_title: &str,
    existing_titles: &[String],
    language: WritingLanguage,
) -> Vec<PostWriteViolation> {
    let all_titles: Vec<String> = existing_titles
        .iter()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect();
    // TS: .slice(-3)
    let start = all_titles.len().saturating_sub(3);
    let recent_titles = &all_titles[start..];
    if recent_titles.len() < 3 {
        return Vec::new();
    }

    let new_title_owned = new_title.to_string();
    let mut rows: Vec<CadenceSummaryRow> = Vec::new();
    for (i, title) in recent_titles.iter().chain(std::iter::once(&new_title_owned)).enumerate() {
        rows.push(CadenceSummaryRow {
            chapter: (i + 1) as u32,
            title: title.clone(),
            mood: String::new(),
            chapter_type: String::new(),
        });
    }
    let cadence = analyze_chapter_cadence(&rows, language);
    let title_pressure = match cadence.title_pressure {
        Some(p) => p,
        None => return Vec::new(),
    };
    if title_pressure.pressure != CadencePressure::High {
        return Vec::new();
    }
    if !new_title.contains(title_pressure.repeated_token.as_str()) {
        return Vec::new();
    }

    let token = &title_pressure.repeated_token;
    vec![if language == WritingLanguage::En {
        PostWriteViolation {
            rule: "title-collapse".to_string(),
            severity: ViolationSeverity::Warning,
            description: format!("Chapter title \"{new_title}\" keeps leaning on the recent \"{token}\" title shell."),
            suggestion: "Rename the chapter around a new image, action, consequence, or character focus.".to_string(),
        }
    } else {
        PostWriteViolation {
            rule: "title-collapse".to_string(),
            severity: ViolationSeverity::Warning,
            description: format!("章节标题\"{new_title}\"仍在沿用近期围绕\u{201c}{token}\u{201d}的命名壳。"),
            suggestion: "换一个新的意象、动作、后果或人物焦点来命名。".to_string(),
        }
    }]
}

fn regenerate_duplicate_title(
    base_title: &str,
    existing_titles: &[String],
    language: WritingLanguage,
    content: Option<&str>,
) -> Option<String> {
    let content = content.filter(|c| !c.trim().is_empty())?;
    let qualifier = if language == WritingLanguage::En {
        extract_english_title_qualifier(base_title, existing_titles, content)
    } else {
        extract_chinese_title_qualifier(base_title, existing_titles, content)
    }?;
    Some(if language == WritingLanguage::En {
        format!("{base_title}: {qualifier}")
    } else {
        format!("{base_title}：{qualifier}")
    })
}

fn regenerate_collapsed_title(
    base_title: &str,
    existing_titles: &[String],
    language: WritingLanguage,
    content: Option<&str>,
) -> Option<String> {
    let content = content.filter(|c| !c.trim().is_empty())?;
    let fresh = if language == WritingLanguage::En {
        extract_english_title_qualifier(base_title, existing_titles, content)
    } else {
        extract_chinese_title_qualifier(base_title, existing_titles, content)
    }?;
    if fresh == base_title {
        None
    } else {
        Some(fresh)
    }
}

fn extract_english_title_qualifier(
    base_title: &str,
    existing_titles: &[String],
    content: &str,
) -> Option<String> {
    let joined = format!("{base_title} {}", existing_titles.join(" "));
    let blocked = extract_english_title_terms(&joined);

    let mut words: Vec<String> = Vec::new();
    for m in en_word_re().find_iter(content) {
        let word = m.as_str().to_lowercase();
        let capitalized = capitalize(&word);
        if ENGLISH_NAME_STOP_WORDS.contains(&capitalized.as_str()) {
            continue;
        }
            if blocked.contains(&word) {
                continue;
            }
        if !words.contains(&word) {
            words.push(word);
        }
    }
    let first = words.first()?.clone();
    let second = words.iter().find(|&w| w != &first && !blocked.contains(w)).cloned();
    Some(match second {
        Some(s) => format!("{} {}", capitalize(&first), capitalize(&s)),
        None => capitalize(&first),
    })
}

fn extract_chinese_title_qualifier(
    base_title: &str,
    existing_titles: &[String],
    content: &str,
) -> Option<String> {
    let joined = format!("{base_title}{}", existing_titles.join(""));
    let blocked = extract_chinese_title_terms(&joined);
    let segments: Vec<String> = cjk_segment_re().find_iter(content).map(|m| m.as_str().to_string()).collect();

    for segment in &segments {
        let chars: Vec<char> = segment.chars().collect();
        for start in 0..chars.len() {
            for size in 2..=4usize {
                if start + size > chars.len() {
                    break;
                }
                let candidate: String = chars[start..start + size].iter().collect::<String>().trim().to_string();
                if utf16_len(&candidate) < 2 {
                    continue;
                }
                if CHINESE_TITLE_STOP_WORDS.contains(&candidate.as_str()) {
                    continue;
                }
                if candidate.chars().any(|c| CHINESE_TITLE_STOP_CHARS.contains(&c)) {
                    continue;
                }
                if blocked.contains(&candidate) {
                    continue;
                }
                return Some(candidate);
            }
        }
    }
    None
}

fn extract_english_title_terms(text: &str) -> Vec<String> {
    let mut terms: Vec<String> = Vec::new();
    for m in en_word_re().find_iter(text) {
        let word = m.as_str().to_lowercase();
        if !terms.contains(&word) {
            terms.push(word);
        }
    }
    terms
}

fn extract_chinese_title_terms(text: &str) -> Vec<String> {
    let mut terms: Vec<String> = Vec::new();
    let segments: Vec<String> = cjk_segment_re().find_iter(text).map(|m| m.as_str().to_string()).collect();
    for segment in &segments {
        let chars: Vec<char> = segment.chars().collect();
        for start in 0..chars.len() {
            for size in 2..=4usize {
                if start + size > chars.len() {
                    break;
                }
                let candidate: String = chars[start..start + size].iter().collect::<String>().trim().to_string();
                if utf16_len(&candidate) < 2 {
                    continue;
                }
                if candidate.chars().any(|c| CHINESE_TITLE_STOP_CHARS.contains(&c)) {
                    continue;
                }
                if !terms.contains(&candidate) {
                    terms.push(candidate);
                }
            }
        }
    }
    terms
}

fn capitalize(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => format!("{}{}", first.to_ascii_uppercase(), chars.as_str()),
        None => String::new(),
    }
}

/// 剥离标点（保留 letter/number）。对齐 TS `s.replace(/[^\p{L}\p{N}]/gu, "")`。
fn strip_punct(s: &str) -> String {
    non_alnum_re().replace_all(s, "").to_string()
}

/// 子串计数（非重叠）。对齐 TS `content.split(needle).length - 1`。
fn count_substring(haystack: &str, needle: &str) -> usize {
    if needle.is_empty() {
        return 0;
    }
    haystack.matches(needle).count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::book_rules::{BookRules, NarrativePerson, Protagonist};
    use crate::models::genre_profile::GenreProfile;

    fn gp_zh() -> GenreProfile {
        GenreProfile {
            name: "都市".into(),
            id: "urban".into(),
            language: "zh".into(),
            fatigue_words: vec!["震惊".into(), "仿佛".into()],
            ..GenreProfile::default()
        }
    }

    fn rules_with(prohibitions: &[&str], fatigue_override: &[&str], person: Option<NarrativePerson>, name: &str) -> BookRules {
        BookRules {
            prohibitions: prohibitions.iter().map(|s| s.to_string()).collect(),
            fatigue_words_override: fatigue_override.iter().map(|s| s.to_string()).collect(),
            narrative_person: person,
            protagonist: if name.is_empty() {
                None
            } else {
                Some(Protagonist { name: name.into(), ..Protagonist::default() })
            },
            ..BookRules::default()
        }
    }

    fn has_rule(violations: &[PostWriteViolation], rule: &str) -> bool {
        violations.iter().any(|v| v.rule == rule)
    }

    #[test]
    fn normalize_replaces_dash_and_trims_end() {
        let out = normalize_post_write_surface("前——后  \n", None);
        assert_eq!(out, "前，后");
    }

    #[test]
    fn normalize_en_keeps_dash() {
        let out = normalize_post_write_surface("a——b", Some(WritingLanguage::En));
        assert_eq!(out, "a——b");
    }

    #[test]
    fn normalize_strips_meta_note_lines() {
        let content = "[writer-note] 临时备注\n正文第一行\n[润色备注] 润色说明\n正文第二行";
        let out = normalize_post_write_surface(content, None);
        assert_eq!(out, "正文第一行\n正文第二行");
    }

    #[test]
    fn validate_zh_not_but_pattern_is_error() {
        let v = validate_post_write("这不是勇气，而是鲁莽。", &gp_zh(), None, None);
        assert!(has_rule(&v, "禁止句式"));
    }

    #[test]
    fn validate_zh_dash_is_error() {
        let v = validate_post_write("他走了过来——然后停下。", &gp_zh(), None, None);
        assert!(has_rule(&v, "禁止破折号"));
    }

    #[test]
    fn validate_zh_surprise_marker_density_warns() {
        // 7 个标记词全用，content < 3000 字 → markerLimit=1，totalMarker=7 > 1。
        let content = "仿佛忽然竟然猛地猛然不禁宛如还有正文内容填充。";
        let v = validate_post_write(content, &gp_zh(), None, None);
        assert!(has_rule(&v, "转折词密度"));
    }

    #[test]
    fn validate_zh_fatigue_word_warns_when_over_one() {
        // genreProfile.fatigueWords = ["震惊","仿佛"]，震惊出现 2 次。
        let content = "震惊了他。又震惊了她。这是正文。";
        let v = validate_post_write(content, &gp_zh(), None, None);
        assert!(has_rule(&v, "高疲劳词"));
    }

    #[test]
    fn validate_zh_report_term_is_error() {
        let v = validate_post_write("他的核心动机很明确。正文。", &gp_zh(), None, None);
        assert!(has_rule(&v, "报告术语"));
    }

    #[test]
    fn validate_zh_chapter_ref_is_error() {
        let v = validate_post_write("这是第33章的内容。正文。", &gp_zh(), None, None);
        assert!(has_rule(&v, "章节号指称"));
    }

    #[test]
    fn validate_zh_sermon_word_warns() {
        let v = validate_post_write("显然他是对的。正文内容。", &gp_zh(), None, None);
        assert!(has_rule(&v, "作者说教"));
    }

    #[test]
    fn validate_zh_collective_shock_warns() {
        let v = validate_post_write("全场震惊地看着他。正文。", &gp_zh(), None, None);
        assert!(has_rule(&v, "集体反应"));
    }

    #[test]
    fn validate_zh_consecutive_le_warns_at_six() {
        // 6 句含「了」，每句 >2 码元。
        let content = "他吃了。他喝了。他走了。他跑了。他笑了。他哭了。结束。";
        let v = validate_post_write(content, &gp_zh(), None, None);
        assert!(has_rule(&v, "连续了字"));
    }

    #[test]
    fn validate_zh_book_prohibition_matches() {
        let rules = rules_with(&["禁词甲"], &[], None, "");
        let v = validate_post_write("这里出现了禁词甲。正文。", &gp_zh(), Some(&rules), None);
        assert!(has_rule(&v, "本书禁忌"));
    }

    #[test]
    fn validate_zh_first_person_inner_state_slip_is_error() {
        let rules = rules_with(&[], &[], Some(NarrativePerson::First), "陆承烬");
        let v = validate_post_write("他觉得一阵寒意。正文内容足够长。", &gp_zh(), Some(&rules), None);
        assert!(has_rule(&v, "叙事人称"));
    }

    #[test]
    fn validate_zh_first_person_name_drift_is_error() {
        // 第一人称，几乎不用「我」，反复以名字第三人称叙述。
        let rules = rules_with(&[], &[], Some(NarrativePerson::First), "陆承烬");
        let mut content = String::new();
        for _ in 0..8 {
            content.push_str("陆承烬走向前方。");
        }
        while utf16_len(&content) < 820 {
            content.push_str("他继续走。");
        }
        let v = validate_post_write(&content, &gp_zh(), Some(&rules), None);
        assert!(has_rule(&v, "叙事人称"));
    }

    #[test]
    fn validate_english_ai_tell_density_warns() {
        // "delve" 出现多次，短 content → ceil(len/3000)=1，count>1 触发。
        let content = "delve delve delve into the matter.";
        let v = validate_post_write(content, &gp_zh(), None, Some(WritingLanguage::En));
        assert!(has_rule(&v, "AI-tell word density"));
    }

    #[test]
    fn validate_english_book_prohibition_case_insensitive() {
        let rules = rules_with(&["ForbiddenWord"], &[], None, "");
        let content = "This has forbiddenword in it. ".repeat(20);
        let v = validate_post_write(&content, &gp_zh(), Some(&rules), Some(WritingLanguage::En));
        assert!(has_rule(&v, "Book prohibition"));
    }

    #[test]
    fn cross_chapter_zh_repetition_three_phrases_warns() {
        let p1 = "风吹过山岗上";
        let p2 = "雨落在屋檐下";
        let p3 = "雪覆盖了田野";
        let current = format!("{p1}{p1}{p2}{p2}{p3}{p3}其他内容填充。");
        let recent = format!("历史章节提到{p1}和{p2}与{p3}。{}", "长".repeat(100));
        let v = detect_cross_chapter_repetition(&current, &recent, WritingLanguage::Zh);
        assert!(has_rule(&v, "跨章重复"));
    }

    #[test]
    fn paragraph_length_drift_detects_shrink() {
        // recent 段落长，current 段落短 → 漂移。
        let long_para: String = "长段落内容".repeat(20);
        let recent = format!("{long_para}\n\n{long_para}\n\n{long_para}\n\n{long_para}");
        let short_para = "短。";
        let current = format!("{short_para}\n\n{short_para}\n\n{short_para}\n\n{short_para}");
        let v = detect_paragraph_length_drift(&current, &recent, WritingLanguage::Zh);
        assert!(has_rule(&v, "段落密度漂移"));
    }

    #[test]
    fn duplicate_title_exact_match() {
        let existing = vec!["开局".to_string(), "转折".to_string()];
        let v = detect_duplicate_title("开局", &existing);
        assert!(has_rule(&v, "duplicate-title"));
    }

    #[test]
    fn duplicate_title_near_match_stripped_punct() {
        let existing = vec!["开局！".to_string()];
        let v = detect_duplicate_title("开局", &existing);
        assert!(has_rule(&v, "near-duplicate-title"));
    }

    #[test]
    fn resolve_duplicate_counter_fallback_when_no_content() {
        let existing = vec!["同一标题".to_string()];
        let r = resolve_duplicate_title("同一标题", &existing, WritingLanguage::Zh, None);
        assert_eq!(r.title, "同一标题（2）");
        assert!(has_rule(&r.issues, "duplicate-title"));
    }

    #[test]
    fn resolve_duplicate_clean_title_unchanged() {
        let existing = vec!["其他标题".to_string()];
        let r = resolve_duplicate_title("全新标题", &existing, WritingLanguage::Zh, None);
        assert_eq!(r.title, "全新标题");
        assert!(r.issues.is_empty());
    }
}
