//! 书籍规则（book_rules.md 契约）。
//!
//! 移植自 `packages/core/src/models/book-rules.ts`（313 行）。
//! 注：parseBookRules / tryParseBookRulesFrontmatter 依赖 js-yaml，待加 serde_yaml 后移植。

use serde::{Deserialize, Serialize};
#[cfg(feature = "export-bindings")]
use ts_rs::TS;
use regex::Regex;
use std::sync::OnceLock;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase", default)]
pub struct Protagonist {
    pub name: String,
    #[serde(default)]
    pub personality_lock: Vec<String>,
    #[serde(default)]
    pub behavioral_constraints: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase", default)]
pub struct GenreLock {
    pub primary: String,
    #[serde(default)]
    pub forbidden: Vec<String>,
}

/// 数值上限：number | string（对齐 TS `z.union([z.number(), z.string()])`）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export, untagged))]
pub enum HardCap {
    Number(f64),
    Text(String),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase", default)]
pub struct NumericalOverrides {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hard_cap: Option<HardCap>,
    #[serde(default)]
    pub resource_types: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase", default)]
pub struct EraConstraints {
    #[serde(default)]
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub period: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
}

/// 叙事人称。对齐 TS `z.enum(["first","third"])`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export, type = "\"first\" | \"third\""))]
pub enum NarrativePerson {
    #[serde(rename = "first")]
    First,
    #[serde(rename = "third")]
    Third,
}

/// 审计维度值：number | string（对齐 TS `z.union([z.number(), z.string()])`）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export, untagged))]
pub enum AuditDimension {
    Number(f64),
    Text(String),
}

/// 书籍规则（book_rules.md 的结构化规则面）。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase", default)]
pub struct BookRules {
    #[serde(default = "default_version")]
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protagonist: Option<Protagonist>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub genre_lock: Option<GenreLock>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub narrative_person: Option<NarrativePerson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub numerical_system_overrides: Option<NumericalOverrides>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub era_constraints: Option<EraConstraints>,
    #[serde(default)]
    pub prohibitions: Vec<String>,
    #[serde(default)]
    pub chapter_types_override: Vec<String>,
    #[serde(default)]
    pub fatigue_words_override: Vec<String>,
    #[serde(default)]
    pub additional_audit_dimensions: Vec<AuditDimension>,
    #[serde(default)]
    pub enable_full_cast_tracking: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fanfic_mode: Option<super::book::FanficMode>,
    #[serde(default)]
    pub allowed_deviations: Vec<String>,
}

fn default_version() -> String { "1.0".into() }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
pub struct ParsedBookRules {
    pub rules: BookRules,
    pub body: String,
}

// 4 个 shim 标记正则（逐字移植 TS isBookRulesShim）
fn shim_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(
            r"本书规则（兼容指针——已废弃）|Book Rules \(compat pointer — deprecated\)|本文件仅为外部读取保留|This file is kept for external readers only",
        ).unwrap()
    })
}

/// 检测 Phase 5 兼容指针 shim（无真实规则，调用方应回退 story_frame）。
pub fn is_book_rules_shim(raw: &str) -> bool {
    shim_re().is_match(raw)
}

// TODO(serde_yaml): parseBookRules / tryParseBookRulesFrontmatter 待加 serde_yaml。
// 也含 parseMarkdownBookRules（从普通 markdown 提取规则面），逻辑较重，后续移植。

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shim_detection() {
        assert!(is_book_rules_shim("本书规则（兼容指针——已废弃）\n其余内容"));
        assert!(is_book_rules_shim("Book Rules (compat pointer — deprecated)"));
        assert!(is_book_rules_shim("本文件仅为外部读取保留"));
        assert!(is_book_rules_shim("This file is kept for external readers only"));
        assert!(!is_book_rules_shim("# 真实规则\n主角: 林动"));
    }

    #[test]
    fn book_rules_default_deserialize() {
        let r: BookRules = serde_json::from_str("{}").unwrap();
        assert_eq!(r.version, "1.0");
        assert!(r.prohibitions.is_empty());
        assert!(!r.enable_full_cast_tracking);
    }

    #[test]
    fn hard_cap_and_audit_dim_untagged() {
        let hc: HardCap = serde_json::from_str("100").unwrap();
        assert_eq!(hc, HardCap::Number(100.0));
        let hc2: HardCap = serde_json::from_str(r#""unlimited""#).unwrap();
        assert_eq!(hc2, HardCap::Text("unlimited".into()));
    }
}
