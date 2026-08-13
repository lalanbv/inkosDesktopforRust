//! 连续性审查（continuity）——纯类型 + 维度标签 + 工具函数层。
//!
//! 移植自 `packages/core/src/agents/continuity.ts`（842 行）的纯叶子层。
//! `ContinuityAuditor`（BaseAgent 子类，依赖 rules-reader/governed-context 等未移植模块）留待后续。
//!
//! 本模块的 [`AuditIssue`] / [`AuditResult`] 是多个下游 agent（writer-parser / sensitive-words /
//! hook-health）的共同依赖类型，先移植以解锁它们。

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::utils::language::WritingLanguage;

/// 审查结果。对齐 TS `AuditResult`。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AuditResult {
    pub passed: bool,
    pub issues: Vec<AuditIssue>,
    pub summary: String,
    /// 审查响应本身不可解析时为 true；调用方不得据此结果自动修订内容。
    pub parse_failed: Option<bool>,
    /// 0-100 整体质量分（审查器支持打分时存在）。
    pub overall_score: Option<u32>,
    pub token_usage: Option<AuditTokenUsage>,
}

/// token 用量。对齐 TS `AuditResult.tokenUsage`。
#[derive(Debug, Clone, PartialEq)]
pub struct AuditTokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

/// 单条审查问题。对齐 TS `AuditIssue`。
#[derive(Debug, Clone, PartialEq)]
pub struct AuditIssue {
    pub severity: AuditSeverity,
    pub category: String,
    pub description: String,
    pub suggestion: String,
    pub repair_scope: Option<RepairScope>,
}

/// 问题严重度。对齐 TS `"critical" | "warning" | "info"`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "export-bindings",
    ts(export, type = "\"critical\" | \"warning\" | \"info\"")
)]
pub enum AuditSeverity {
    #[serde(rename = "critical")]
    Critical,
    #[serde(rename = "warning")]
    Warning,
    #[serde(rename = "info")]
    Info,
}

/// 修复范围。对齐 TS `"local" | "structural" | "unknown"`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepairScope {
    Local,
    Structural,
    Unknown,
}

/// 规范化修复范围值（未知值 → None）。对齐 TS `normalizeRepairScope`。
pub fn normalize_repair_scope(value: &serde_json::Value) -> Option<RepairScope> {
    match value.as_str()? {
        "local" => Some(RepairScope::Local),
        "structural" => Some(RepairScope::Structural),
        "unknown" => Some(RepairScope::Unknown),
        _ => None,
    }
}

/// 文本是否含中文字符。对齐 TS `containsChinese`。
pub fn contains_chinese(text: &str) -> bool {
    text.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c))
}

/// 解析题材标签：中文语言或 profileName 不含中文时直接用 profileName；
/// 否则 "other" → "general"，余者把 `_-` 转空格。对齐 TS `resolveGenreLabel`。
pub fn resolve_genre_label(genre_id: &str, profile_name: &str, language: WritingLanguage) -> String {
    if language == WritingLanguage::Zh || !contains_chinese(profile_name) {
        return profile_name.to_string();
    }
    if genre_id == "other" {
        return "general".to_string();
    }
    genre_id.replace(['_', '-'], " ")
}

/// 维度名（37 个，中英）。对齐 TS `dimensionName`。
pub fn dimension_name(id: u32, language: WritingLanguage) -> Option<&'static str> {
    let labels = dimension_labels();
    let entry = labels.get(&id)?;
    Some(if language == WritingLanguage::En { entry.en } else { entry.zh })
}

/// 本地化 join（中文「、」，英文「, 」）。对齐 TS `joinLocalized`。
pub fn join_localized(items: &[String], language: WritingLanguage) -> String {
    let sep = if language == WritingLanguage::En { ", " } else { "、" };
    items.join(sep)
}

/// 同人维度严重度提示。对齐 TS `formatFanficSeverityNote`。
pub fn format_fanfic_severity_note(severity: AuditSeverity, language: WritingLanguage) -> &'static str {
    if language == WritingLanguage::En {
        return match severity {
            AuditSeverity::Critical => "Strict check.",
            AuditSeverity::Info => "Log only; do not fail the chapter.",
            AuditSeverity::Warning => "Warning level.",
        };
    }
    match severity {
        AuditSeverity::Critical => "（严格检查）",
        AuditSeverity::Info => "（仅记录，不判定失败）",
        AuditSeverity::Warning => "（警告级别）",
    }
}

// --- 37 维度标签（OnceLock HashMap）-------------------------------------------

struct DimensionLabel {
    zh: &'static str,
    en: &'static str,
}

fn dimension_labels() -> &'static HashMap<u32, DimensionLabel> {
    static M: OnceLock<HashMap<u32, DimensionLabel>> = OnceLock::new();
    M.get_or_init(|| {
        let mut m = HashMap::new();
        let entries: [(u32, &str, &str); 37] = [
            (1, "OOC检查", "OOC Check"),
            (2, "时间线检查", "Timeline Check"),
            (3, "设定冲突", "Lore Conflict Check"),
            (4, "战力崩坏", "Power Scaling Check"),
            (5, "数值检查", "Numerical Consistency Check"),
            (6, "伏笔检查", "Hook Check"),
            (7, "节奏检查", "Pacing Check"),
            (8, "文风检查", "Style Check"),
            (9, "信息越界", "Information Boundary Check"),
            (10, "词汇疲劳", "Lexical Fatigue Check"),
            (11, "利益链断裂", "Incentive Chain Check"),
            (12, "年代考据", "Era Accuracy Check"),
            (13, "配角降智", "Side Character Competence Check"),
            (14, "配角工具人化", "Side Character Instrumentalization Check"),
            (15, "爽点虚化", "Payoff Dilution Check"),
            (16, "台词失真", "Dialogue Authenticity Check"),
            (17, "流水账", "Chronicle Drift Check"),
            (18, "知识库污染", "Knowledge Base Pollution Check"),
            (19, "视角一致性", "POV Consistency Check"),
            (20, "段落等长", "Paragraph Uniformity Check"),
            (21, "套话密度", "Cliche Density Check"),
            (22, "公式化转折", "Formulaic Twist Check"),
            (23, "列表式结构", "List-like Structure Check"),
            (24, "支线停滞", "Subplot Stagnation Check"),
            (25, "弧线平坦", "Arc Flatline Check"),
            (26, "节奏单调", "Pacing Monotony Check"),
            (27, "敏感词检查", "Sensitive Content Check"),
            (28, "正传事件冲突", "Mainline Canon Event Conflict"),
            (29, "未来信息泄露", "Future Knowledge Leak Check"),
            (30, "世界规则跨书一致性", "Cross-Book World Rule Check"),
            (31, "番外伏笔隔离", "Spinoff Hook Isolation Check"),
            (32, "读者期待管理", "Reader Expectation Check"),
            (33, "章节备忘偏离", "Chapter Memo Drift Check"),
            (34, "角色还原度", "Character Fidelity Check"),
            (35, "世界规则遵守", "World Rule Compliance Check"),
            (36, "关系动态", "Relationship Dynamics Check"),
            (37, "正典事件一致性", "Canon Event Consistency Check"),
        ];
        for (id, zh, en) in entries {
            m.insert(id, DimensionLabel { zh, en });
        }
        m
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_repair_scope_accepts_known_values() {
        assert_eq!(
            normalize_repair_scope(&serde_json::json!("local")),
            Some(RepairScope::Local)
        );
        assert_eq!(
            normalize_repair_scope(&serde_json::json!("structural")),
            Some(RepairScope::Structural)
        );
        assert_eq!(
            normalize_repair_scope(&serde_json::json!("unknown")),
            Some(RepairScope::Unknown)
        );
        assert_eq!(normalize_repair_scope(&serde_json::json!("bogus")), None);
        assert_eq!(normalize_repair_scope(&serde_json::json!(42)), None);
    }

    #[test]
    fn contains_chinese_detects_cjk() {
        assert!(contains_chinese("abc中文"));
        assert!(contains_chinese("中"));
        assert!(!contains_chinese("pure ascii"));
        assert!(!contains_chinese(""));
    }

    #[test]
    fn resolve_genre_label_zh_or_non_chinese_profile_uses_profile_name() {
        assert_eq!(
            resolve_genre_label("xianxia", "仙侠", WritingLanguage::Zh),
            "仙侠"
        );
        // 英文语言但 profileName 无中文 → 仍用 profileName。
        assert_eq!(
            resolve_genre_label("other", "My Profile", WritingLanguage::En),
            "My Profile"
        );
    }

    #[test]
    fn resolve_genre_label_en_with_chinese_profile_falls_back() {
        // 英文语言 + profileName 含中文 + genre=other → "general"。
        assert_eq!(
            resolve_genre_label("other", "仙侠", WritingLanguage::En),
            "general"
        );
        // 否则 genre 的 _- 转空格。
        assert_eq!(
            resolve_genre_label("urban-fantasy", "仙侠", WritingLanguage::En),
            "urban fantasy"
        );
    }

    #[test]
    fn dimension_name_covers_all_37_in_both_languages() {
        for id in 1..=37u32 {
            assert!(dimension_name(id, WritingLanguage::Zh).is_some(), "维度 {id} 缺中文");
            assert!(dimension_name(id, WritingLanguage::En).is_some(), "维度 {id} 缺英文");
        }
        assert_eq!(dimension_name(0, WritingLanguage::Zh), None);
        assert_eq!(dimension_name(38, WritingLanguage::En), None);
    }

    #[test]
    fn dimension_name_specific_labels() {
        assert_eq!(dimension_name(1, WritingLanguage::Zh), Some("OOC检查"));
        assert_eq!(dimension_name(1, WritingLanguage::En), Some("OOC Check"));
        assert_eq!(dimension_name(37, WritingLanguage::Zh), Some("正典事件一致性"));
    }

    #[test]
    fn join_localized_uses_correct_separator() {
        let items = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        assert_eq!(join_localized(&items, WritingLanguage::Zh), "a、b、c");
        assert_eq!(join_localized(&items, WritingLanguage::En), "a, b, c");
        assert_eq!(join_localized(&[], WritingLanguage::Zh), "");
    }

    #[test]
    fn format_fanfic_severity_note_per_language() {
        assert_eq!(
            format_fanfic_severity_note(AuditSeverity::Critical, WritingLanguage::En),
            "Strict check."
        );
        assert_eq!(
            format_fanfic_severity_note(AuditSeverity::Critical, WritingLanguage::Zh),
            "（严格检查）"
        );
        assert_eq!(
            format_fanfic_severity_note(AuditSeverity::Info, WritingLanguage::Zh),
            "（仅记录，不判定失败）"
        );
        assert_eq!(
            format_fanfic_severity_note(AuditSeverity::Warning, WritingLanguage::En),
            "Warning level."
        );
    }
}
