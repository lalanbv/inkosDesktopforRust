//! R5 反AI规则资产化 + G13 项目经验记忆（365 号契约层，二轮 P1）。
//!
//! TS 真源：`packages/core/src/utils/rule-experience-engine.ts`；golden 唯一
//! 事实源：`packages/core/src/__tests__/golden/rule-experience-vectors.json`
//! （差分测试 `tests/golden_rule_experience_diff.rs` 读同一文件）。
//!
//! - AntiAiRule：逐书反AI规则（type/severity/enabled）——写作预防
//!   `compose_anti_ai_guidance`（写法 guidance 同管道）/ detect
//!   `scan_anti_ai_rules` / fix `render_anti_ai_fix_guidance`；
//! - ExperienceEntry（G13 /learn 语义）：章审查后沉淀「本卷验证有效的手法」，
//!   `render_experience_guidance` 注入同管道 guidance。
//!
//! 移植纪律：正则语义以 Rust regex crate 对齐 JS（无 lookahead 场景）；
//! 排序纯码元比较；maxChars 按 UTF-16 码元截断（TS slice 语义，中文场景
//! 与 char 计数一致）。

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord)]
#[cfg_attr(feature = "export-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "lowercase")]
pub enum AntiAiRuleType {
    Phrase,
    Structure,
    Rhythm,
    Cliche,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord)]
#[cfg_attr(feature = "export-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "lowercase")]
pub enum AntiAiSeverity {
    Critical,
    Warning,
    Info,
}

impl AntiAiSeverity {
    fn rank(self) -> u8 {
        match self {
            AntiAiSeverity::Critical => 0,
            AntiAiSeverity::Warning => 1,
            AntiAiSeverity::Info => 2,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct AntiAiRule {
    pub id: String,
    pub r#type: AntiAiRuleType,
    pub pattern: String,
    #[serde(default)]
    pub is_regex: bool,
    #[serde(default = "default_severity")]
    pub severity: AntiAiSeverity,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replacement: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_severity() -> AntiAiSeverity {
    AntiAiSeverity::Warning
}

fn default_true() -> bool {
    true
}

fn is_snake_case_slug(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuleValidateResult {
    pub rule: Option<AntiAiRule>,
    pub errors: Vec<String>,
}

/// 单条规则校验：字段约束 + isRegex 时正则可编译性检查（双端一致）。
pub fn validate_anti_ai_rule(raw: &Value) -> RuleValidateResult {
    let rule: AntiAiRule = match serde_json::from_value(raw.clone()) {
        Ok(rule) => rule,
        Err(error) => {
            return RuleValidateResult {
                rule: None,
                errors: vec![format!("(root): {error}")],
            };
        }
    };
    if !is_snake_case_slug(&rule.id) {
        return RuleValidateResult {
            rule: None,
            errors: vec!["id: must be snake_case slug (a-z0-9_, 1-64 chars)".to_string()],
        };
    }
    if rule.pattern.is_empty() || rule.pattern.chars().count() > 200 {
        return RuleValidateResult {
            rule: None,
            errors: vec!["pattern: required, 1-200 chars".to_string()],
        };
    }
    if rule.is_regex && regex::Regex::new(&rule.pattern).is_err() {
        return RuleValidateResult {
            rule: None,
            errors: vec!["pattern: invalid regex".to_string()],
        };
    }
    if rule.message.is_empty() || rule.message.chars().count() > 200 {
        return RuleValidateResult {
            rule: None,
            errors: vec!["message: required, 1-200 chars".to_string()],
        };
    }
    RuleValidateResult { rule: Some(rule), errors: Vec::new() }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct AntiAiHit {
    pub rule_id: String,
    pub severity: AntiAiSeverity,
    pub message: String,
    /// 首个命中窗口（命中 ±20 字），供界面定位。
    pub excerpt: String,
    /// 命中次数。
    pub count: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replacement: Option<String>,
}

const EXCERPT_WINDOW: usize = 20;

/// detect 消费：扫描文本（仅 enabled 规则；字面计数 / regex 全匹配）。
pub fn scan_anti_ai_rules(text: &str, rules: &[AntiAiRule]) -> Vec<AntiAiHit> {
    let char_len = text.chars().count();
    let char_offset_of = |byte_index: usize| -> usize { text[..byte_index].chars().count() };
    let mut hits: Vec<AntiAiHit> = Vec::new();
    for rule in rules {
        if !rule.enabled {
            continue;
        }
        if rule.is_regex {
            let Ok(re) = regex::Regex::new(&rule.pattern) else { continue };
            let matches: Vec<regex::Captures> = re.captures_iter(text).collect();
            if matches.is_empty() {
                continue;
            }
            let first = &matches[0];
            let start_byte = first.get(0).map(|m| m.start()).unwrap_or(0);
            let end_byte = first.get(0).map(|m| m.end()).unwrap_or(0);
            let start_char = char_offset_of(start_byte);
            let end_char = char_offset_of(end_byte);
            let excerpt_start = start_char.saturating_sub(EXCERPT_WINDOW);
            let excerpt_end = (end_char + EXCERPT_WINDOW).min(char_len);
            let excerpt: String = text
                .chars()
                .skip(excerpt_start)
                .take(excerpt_end - excerpt_start)
                .collect();
            hits.push(AntiAiHit {
                rule_id: rule.id.clone(),
                severity: rule.severity,
                message: rule.message.clone(),
                excerpt,
                count: matches.len() as i64,
                replacement: rule.replacement.clone(),
            });
        } else {
            let pattern = &rule.pattern;
            let pattern_chars = pattern.chars().count();
            let mut count: i64 = 0;
            let mut first_index: Option<usize> = None;
            let mut from = 0usize;
            while from <= char_len {
                // 字面查找（按字符索引推进；重叠跳过按 pattern 长度步进）。
                let hay: String = text.chars().skip(from).collect();
                let found = hay.find(pattern);
                let Some(relative) = found else { break };
                let index = from + hay[..relative].chars().count();
                count += 1;
                if first_index.is_none() {
                    first_index = Some(index);
                }
                from = index + pattern_chars.max(1);
            }
            let Some(first_char) = first_index else { continue };
            let excerpt_start = first_char.saturating_sub(EXCERPT_WINDOW);
            let excerpt_end = (first_char + pattern_chars + EXCERPT_WINDOW).min(char_len);
            let excerpt: String = text
                .chars()
                .skip(excerpt_start)
                .take(excerpt_end - excerpt_start)
                .collect();
            hits.push(AntiAiHit {
                rule_id: rule.id.clone(),
                severity: rule.severity,
                message: rule.message.clone(),
                excerpt,
                count,
                replacement: rule.replacement.clone(),
            });
        }
    }
    hits.sort_by(|a, b| {
        a.severity
            .rank()
            .cmp(&b.severity.rank())
            .then_with(|| b.count.cmp(&a.count))
            .then_with(|| a.rule_id.cmp(&b.rule_id))
    });
    hits
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuidanceLanguage {
    Zh,
    En,
}

impl From<crate::utils::language::WritingLanguage> for GuidanceLanguage {
    fn from(value: crate::utils::language::WritingLanguage) -> Self {
        match value {
            crate::utils::language::WritingLanguage::Zh => GuidanceLanguage::Zh,
            crate::utils::language::WritingLanguage::En => GuidanceLanguage::En,
        }
    }
}

/// fix 消费：命中 → 修复提示块（输入序渲染；含 replacement 建议；无命中 None）。
pub fn render_anti_ai_fix_guidance(
    hits: &[AntiAiHit],
    language: GuidanceLanguage,
) -> Option<String> {
    if hits.is_empty() {
        return None;
    }
    let is_en = language == GuidanceLanguage::En;
    let mut parts: Vec<String> = Vec::new();
    parts.push(
        if is_en {
            "## Anti-AI violations (fix before delivery)".to_string()
        } else {
            "## 反AI规则命中（交付前修复）".to_string()
        },
    );
    for hit in hits {
        parts.push(format!(
            "- [{}] {}（×{}）",
            hit.severity.severity_str(),
            hit.message,
            hit.count
        ));
    }
    let replacements: Vec<String> = hits
        .iter()
        .filter_map(|hit| hit.replacement.as_ref().map(|value| format!("- {} → {}", hit.rule_id, value)))
        .collect();
    if !replacements.is_empty() {
        parts.push(if is_en { "Suggested rewrites:".to_string() } else { "建议改法：".to_string() });
        parts.extend(replacements);
    }
    Some(parts.join("\n"))
}

impl AntiAiSeverity {
    /// R22/392 号起 pub：canonical 形式指纹需要枚举的 serde 字面量。
    pub fn severity_str(self) -> &'static str {
        match self {
            AntiAiSeverity::Critical => "critical",
            AntiAiSeverity::Warning => "warning",
            AntiAiSeverity::Info => "info",
        }
    }
}

fn truncate_with_ellipsis(text: &str, max_chars: usize) -> String {
    if text.chars().count() > max_chars {
        let cut: String = text.chars().take(max_chars.saturating_sub(1)).collect();
        format!("{cut}…")
    } else {
        text.to_string()
    }
}

/// 写作预防：enabled 规则 → 写法 guidance 同管道禁则块（无 enabled 规则 None）。
pub fn compose_anti_ai_guidance(
    rules: &[AntiAiRule],
    language: GuidanceLanguage,
    max_chars: Option<usize>,
) -> Option<String> {
    let enabled: Vec<&AntiAiRule> = rules.iter().filter(|rule| rule.enabled).collect();
    if enabled.is_empty() {
        return None;
    }
    let is_en = language == GuidanceLanguage::En;
    let header = if is_en {
        "## Anti-AI rules (bound — never violate while writing)"
    } else {
        "## 反AI规则（绑定——写作时严禁违反）"
    };
    let lines: Vec<String> = enabled
        .iter()
        .map(|rule| {
            let replacement = rule
                .replacement
                .as_ref()
                .map(|value| {
                    if is_en {
                        format!(" (prefer: {value})")
                    } else {
                        format!("（改为：{value}）")
                    }
                })
                .unwrap_or_default();
            format!("- [{}] {}{}", rule.severity.severity_str(), rule.message, replacement)
        })
        .collect();
    let mut parts = vec![header.to_string()];
    parts.extend(lines);
    let text = parts.join("\n");
    Some(match max_chars {
        Some(max) => truncate_with_ellipsis(&text, max),
        None => text,
    })
}

// ── G13：项目经验记忆（/learn 语义）──

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord)]
#[cfg_attr(feature = "export-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "lowercase")]
pub enum ExperienceKind {
    Technique,
    Hook,
    Pacing,
    Dialogue,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ExperienceEntry {
    pub id: String,
    /// 沉淀来源章（0=书级通用经验）。
    pub chapter: i64,
    pub kind: ExperienceKind,
    /// 经验文本（「本卷验证有效的手法」，≤200）。
    pub text: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExperienceMergeResult {
    pub merged: Vec<ExperienceEntry>,
    pub added: usize,
}

/// 经验合并：同 text(trim) 去重保序——已有条目保留，新条目追加。
pub fn merge_experience_entries(
    existing: &[ExperienceEntry],
    incoming: &[ExperienceEntry],
) -> ExperienceMergeResult {
    let mut seen: std::collections::HashSet<String> = existing
        .iter()
        .map(|entry| entry.text.trim().to_string())
        .collect();
    let mut merged: Vec<ExperienceEntry> = existing.to_vec();
    let mut added = 0usize;
    for entry in incoming {
        let key = entry.text.trim().to_string();
        if seen.contains(&key) {
            continue;
        }
        seen.insert(key);
        merged.push(entry.clone());
        added += 1;
    }
    ExperienceMergeResult { merged, added }
}

/// G13 注入：enabled 经验 → guidance 块（≤10 条，章号升序稳定；无 enabled None）。
pub fn render_experience_guidance(
    entries: &[ExperienceEntry],
    language: GuidanceLanguage,
    max_chars: Option<usize>,
) -> Option<String> {
    let mut enabled: Vec<&ExperienceEntry> = entries.iter().filter(|entry| entry.enabled).collect();
    enabled.sort_by(|a, b| a.chapter.cmp(&b.chapter).then_with(|| a.id.cmp(&b.id)));
    let selected: Vec<&ExperienceEntry> = enabled.into_iter().take(10).collect();
    if selected.is_empty() {
        return None;
    }
    let is_en = language == GuidanceLanguage::En;
    let header = if is_en {
        "## Proven techniques (learned from this book — reuse)"
    } else {
        "## 本书验证有效的手法（经验记忆——复用）"
    };
    let lines: Vec<String> = selected.iter().map(|entry| format!("- {}", entry.text)).collect();
    let mut parts = vec![header.to_string()];
    parts.extend(lines);
    let text = parts.join("\n");
    Some(match max_chars {
        Some(max) => truncate_with_ellipsis(&text, max),
        None => text,
    })
}

// ── R22/392 号：反AI规则内容指纹 + 内置种子包（四轮 P1；363 号种子兜底先例）──

fn rule_type_str(rule_type: &AntiAiRuleType) -> &'static str {
    match rule_type {
        AntiAiRuleType::Phrase => "phrase",
        AntiAiRuleType::Structure => "structure",
        AntiAiRuleType::Rhythm => "rhythm",
        AntiAiRuleType::Cliche => "cliche",
    }
}

/// 规则内容指纹的 canonical 形式（双端逐字一致；与 362 assetCanonicalForm 同编码约定）。
pub fn anti_ai_rule_canonical_form(rule: &AntiAiRule) -> String {
    [
        rule.id.as_str(),
        rule_type_str(&rule.r#type),
        rule.pattern.as_str(),
        if rule.is_regex { "1" } else { "0" },
        rule.severity.severity_str(),
        rule.message.as_str(),
        rule.replacement.as_deref().unwrap_or(""),
        if rule.enabled { "1" } else { "0" },
    ]
    .join("|")
}

const FNV_OFFSET: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;

fn fnv1a_hex16(text: &str) -> String {
    let mut hash = FNV_OFFSET;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    format!("{hash:016x}")
}

/// 规则内容指纹（同内容必同指纹——种子幂等合并依据，对齐 362/360 hash 语义）。
pub fn anti_ai_rule_content_hash(rule: &AntiAiRule) -> String {
    fnv1a_hex16(&anti_ai_rule_canonical_form(rule))
}

/// 规则组指纹：逐条 canonical 以 \n 连接后的 FNV 指纹（种子包完整性锚点）。
pub fn anti_ai_rule_pack_hash(rules: &[AntiAiRule]) -> String {
    let joined: Vec<String> = rules.iter().map(anti_ai_rule_canonical_form).collect();
    fnv1a_hex16(&joined.join("\n"))
}

/// 内置反AI规则种子包（392 号，双端逐字镜像；随包分发的冷启动底线防线）。
/// 取精弃糟：只收 9 条最高频、可机检、有明确改法的 AI 腔（对标「模板数量
/// 战」的糟粕刻意做小）；覆盖全部四类型；全部字面匹配——正则种子留用户
/// 自建，误伤面不可控。
pub fn anti_ai_rule_seeds() -> Vec<AntiAiRule> {
    vec![
        AntiAiRule {
            id: "eye_glint_stock".into(),
            r#type: AntiAiRuleType::Phrase,
            pattern: "眼中闪过一丝".into(),
            is_regex: false,
            severity: AntiAiSeverity::Critical,
            message: "「眼中闪过一丝X」是最高频的 AI 神态套语——抽象神态报告没有画面。".into(),
            replacement: Some("改写成可看见的动作：捏紧的指节、挪开的视线、喉结滚动。".into()),
            enabled: true,
        },
        AntiAiRule {
            id: "essay_connective".into(),
            r#type: AntiAiRuleType::Structure,
            pattern: "值得注意的是".into(),
            is_regex: false,
            severity: AntiAiSeverity::Critical,
            message: "论文式连接词，AI 论述腔直接漏进叙事——删除，让事实自己说话。".into(),
            replacement: None,
            enabled: true,
        },
        AntiAiRule {
            id: "mouth_smirk_stock".into(),
            r#type: AntiAiRuleType::Phrase,
            pattern: "嘴角勾起一抹".into(),
            is_regex: false,
            severity: AntiAiSeverity::Warning,
            message: "「嘴角勾起一抹X」是 AI 高频表情模板——肌肉报告不等于情绪。".into(),
            replacement: Some("写对面的人看见了什么、感到了什么。".into()),
            enabled: true,
        },
        AntiAiRule {
            id: "air_freeze_cliche".into(),
            r#type: AntiAiRuleType::Cliche,
            pattern: "空气仿佛凝固".into(),
            is_regex: false,
            severity: AntiAiSeverity::Warning,
            message: "张力陈词——气氛凝固是报告不是体验。".into(),
            replacement: Some("让在场者的某个小动作失灵：端着的杯子停在半空。".into()),
            enabled: true,
        },
        AntiAiRule {
            id: "deep_breath_buffer".into(),
            r#type: AntiAiRuleType::Rhythm,
            pattern: "深吸一口气".into(),
            is_regex: false,
            severity: AntiAiSeverity::Warning,
            message: "高频缓冲动作、节奏拖延——每章至多一次，情绪转折处用更具体的身体反应。".into(),
            replacement: None,
            enabled: true,
        },
        AntiAiRule {
            id: "discourse_hedge".into(),
            r#type: AntiAiRuleType::Structure,
            pattern: "从某种意义上".into(),
            is_regex: false,
            severity: AntiAiSeverity::Warning,
            message: "对冲式论断削弱叙事权威——角色可以有立场，叙事者不骑墙。".into(),
            replacement: Some("改成明确判断，或删掉整个从句。".into()),
            enabled: true,
        },
        AntiAiRule {
            id: "pupil_contraction".into(),
            r#type: AntiAiRuleType::Phrase,
            pattern: "瞳孔骤缩".into(),
            is_regex: false,
            severity: AntiAiSeverity::Warning,
            message: "高频惊吓神态模板——惊惧写失控的后果：后退半步、手里的东西掉了。".into(),
            replacement: None,
            enabled: true,
        },
        AntiAiRule {
            id: "vague_time_transition".into(),
            r#type: AntiAiRuleType::Rhythm,
            pattern: "不知过了多久".into(),
            is_regex: false,
            severity: AntiAiSeverity::Info,
            message: "时间过渡含糊——用环境或动作标记流逝：灯换了颜色、茶凉透了。".into(),
            replacement: None,
            enabled: true,
        },
        AntiAiRule {
            id: "as_if_whisper_cliche".into(),
            r#type: AntiAiRuleType::Cliche,
            pattern: "仿佛在诉说".into(),
            is_regex: false,
            severity: AntiAiSeverity::Info,
            message: "「仿佛在诉说X」把情绪结论塞给读者——删掉，写让人产生此感的画面。".into(),
            replacement: None,
            enabled: true,
        },
    ]
}

/// 种子兜底解析结果（`seeded=true` = 来自内置种子的内存态兜底，不自动落盘）。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "export-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct AntiAiRulesWithSeeds {
    pub rules: Vec<AntiAiRule>,
    pub seeded: bool,
}

/// 种子兜底解析（363 号先例）：文件缺失/损坏（None）→ 内置种子；
/// **文件存在但规则为空 = 用户显式清空的选择，不回填种子**（否则用户
/// 永远无法关掉防线）。
pub fn resolve_anti_ai_rules_with_seeds(parsed: Option<Vec<AntiAiRule>>) -> AntiAiRulesWithSeeds {
    match parsed {
        None => AntiAiRulesWithSeeds {
            rules: anti_ai_rule_seeds(),
            seeded: true,
        },
        Some(rules) => AntiAiRulesWithSeeds {
            rules,
            seeded: false,
        },
    }
}
