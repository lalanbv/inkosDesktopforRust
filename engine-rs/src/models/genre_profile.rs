//! 流派画像。
//!
//! 移植自 `packages/core/src/models/genre-profile.ts`。
//! [`parse_genre_profile`]：YAML frontmatter（serde_yaml_ng，对齐 TS js-yaml）+ zod 语义校验。

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
#[cfg(feature = "export-bindings")]
use ts_rs::TS;

/// 流派画像（从 genre markdown frontmatter 解析）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase", default)]
pub struct GenreProfile {
    pub name: String,
    pub id: String,
    #[serde(default = "default_language")]
    pub language: String,
    pub chapter_types: Vec<String>,
    pub fatigue_words: Vec<String>,
    #[serde(default)]
    pub numerical_system: bool,
    #[serde(default)]
    pub power_scaling: bool,
    #[serde(default)]
    pub era_research: bool,
    #[serde(default)]
    pub pacing_rule: String,
    #[serde(default)]
    pub satisfaction_types: Vec<String>,
    #[serde(default)]
    pub audit_dimensions: Vec<f64>,
}

fn default_language() -> String {
    "zh".to_string()
}

impl Default for GenreProfile {
    fn default() -> Self {
        GenreProfile {
            name: String::new(),
            id: String::new(),
            language: default_language(),
            chapter_types: vec![],
            fatigue_words: vec![],
            numerical_system: false,
            power_scaling: false,
            era_research: false,
            pacing_rule: String::new(),
            satisfaction_types: vec![],
            audit_dimensions: vec![],
        }
    }
}

/// parseGenreProfile 产物（profile + 正文）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
pub struct ParsedGenreProfile {
    pub profile: GenreProfile,
    pub body: String,
}

/// 解析错误。对齐 TS `parseGenreProfile` 抛出的 Error（frontmatter 缺失 / YAML 或字段校验失败）。
#[derive(Debug, thiserror::Error)]
pub enum GenreProfileParseError {
    #[error("Genre profile missing YAML frontmatter (--- ... ---)")]
    MissingFrontmatter,
    #[error("genre profile YAML parse failed: {0}")]
    Yaml(#[from] serde_yaml_ng::Error),
    #[error("genre profile field invalid: {0}")]
    InvalidField(String),
}

/// zod 语义的中间层：必填字段缺失去即报错（`name`/`id`/`chapterTypes`/`fatigueWords`
/// 无 default），可选字段显式 default。与公共 [`GenreProfile`]（全 default，供下游构造）解耦。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GenreProfileRaw {
    name: String,
    id: String,
    #[serde(default)]
    language: Option<String>,
    chapter_types: Vec<String>,
    fatigue_words: Vec<String>,
    #[serde(default)]
    numerical_system: bool,
    #[serde(default)]
    power_scaling: bool,
    #[serde(default)]
    era_research: bool,
    #[serde(default)]
    pacing_rule: String,
    #[serde(default)]
    satisfaction_types: Vec<String>,
    #[serde(default)]
    audit_dimensions: Vec<f64>,
}

/// frontmatter 提取正则。逐字移植 TS `^---\s*\n([\s\S]*?)\n---\s*\n([\s\S]*)$`。
fn genre_frontmatter_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^---\s*\n([\s\S]*?)\n---\s*\n([\s\S]*)$").unwrap())
}

/// 解析 genre profile markdown（frontmatter YAML + 正文）。对齐 TS `parseGenreProfile`。
///
/// - 无 frontmatter（或闭合 `---` 后无换行）→ [`GenreProfileParseError::MissingFrontmatter`]
/// - YAML 语法错 / 字段类型错 / 必填缺失 → `Yaml`（serde 报错，对齐 zod throw）
/// - `language` 非 zh/en → `InvalidField`（对齐 `z.enum(["zh","en"])`）
pub fn parse_genre_profile(raw: &str) -> Result<ParsedGenreProfile, GenreProfileParseError> {
    let caps = genre_frontmatter_re()
        .captures(raw)
        .ok_or(GenreProfileParseError::MissingFrontmatter)?;
    let fm = caps.get(1).expect("组 1 必在").as_str();
    let body = caps.get(2).expect("组 2 必在").as_str();

    let parsed: GenreProfileRaw = serde_yaml_ng::from_str(fm)?;
    let language = match parsed.language.as_deref() {
        None => "zh".to_string(),
        Some("zh") => "zh".to_string(),
        Some("en") => "en".to_string(),
        Some(other) => {
            return Err(GenreProfileParseError::InvalidField(format!(
                "language 须为 zh/en，得到 {other:?}"
            )))
        }
    };

    Ok(ParsedGenreProfile {
        profile: GenreProfile {
            name: parsed.name,
            id: parsed.id,
            language,
            chapter_types: parsed.chapter_types,
            fatigue_words: parsed.fatigue_words,
            numerical_system: parsed.numerical_system,
            power_scaling: parsed.power_scaling,
            era_research: parsed.era_research,
            pacing_rule: parsed.pacing_rule,
            satisfaction_types: parsed.satisfaction_types,
            audit_dimensions: parsed.audit_dimensions,
        },
        body: body.trim().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL_FM: &str = "---\nname: 通用\nid: other\nlanguage: en\nchapterTypes: [\"推进章\"]\nfatigueWords: [\"震惊\", \"仿佛\"]\nnumericalSystem: true\npowerScaling: false\neraResearch: true\npacingRule: \"每2章一个进展\"\nsatisfactionTypes: [\"目标达成\"]\nauditDimensions: [1, 2, 7]\n---\n\n## 题材禁忌\n\n- 无逻辑的巧合推进剧情\n";

    #[test]
    fn parse_full_frontmatter() {
        let parsed = parse_genre_profile(FULL_FM).expect("应解析成功");
        let p = &parsed.profile;
        assert_eq!(p.name, "通用");
        assert_eq!(p.id, "other");
        assert_eq!(p.language, "en");
        assert_eq!(p.chapter_types, vec!["推进章".to_string()]);
        assert_eq!(
            p.fatigue_words,
            vec!["震惊".to_string(), "仿佛".to_string()]
        );
        assert!(p.numerical_system);
        assert!(!p.power_scaling);
        assert!(p.era_research);
        assert_eq!(p.pacing_rule, "每2章一个进展");
        assert_eq!(p.satisfaction_types, vec!["目标达成".to_string()]);
        assert_eq!(p.audit_dimensions, vec![1.0, 2.0, 7.0]);
        assert_eq!(parsed.body, "## 题材禁忌\n\n- 无逻辑的巧合推进剧情");
    }

    #[test]
    fn parse_minimal_uses_defaults() {
        // 对齐 zod：language 默认 zh，可选字段默认空，body trim。
        let raw = "---\nname: X\nid: x\nchapterTypes: []\nfatigueWords: []\n---\n\n正文  \n";
        let parsed = parse_genre_profile(raw).expect("应解析成功");
        assert_eq!(parsed.profile.language, "zh");
        assert_eq!(parsed.profile.pacing_rule, "");
        assert!(parsed.profile.audit_dimensions.is_empty());
        assert!(!parsed.profile.numerical_system);
        assert_eq!(parsed.body, "正文");
    }

    #[test]
    fn missing_frontmatter_errors() {
        assert!(matches!(
            parse_genre_profile("# 无 frontmatter\n正文"),
            Err(GenreProfileParseError::MissingFrontmatter)
        ));
        // 闭合 --- 后无换行 → TS 正则同样不匹配。
        assert!(matches!(
            parse_genre_profile("---\nname: X\nid: x\nchapterTypes: []\nfatigueWords: []\n---"),
            Err(GenreProfileParseError::MissingFrontmatter)
        ));
    }

    #[test]
    fn missing_required_field_errors() {
        // 对齐 zod：name / chapterTypes 必填（无 default），缺失即 throw。
        let no_name = "---\nid: x\nchapterTypes: []\nfatigueWords: []\n---\nbody";
        assert!(parse_genre_profile(no_name).is_err());
        let no_chapter_types = "---\nname: X\nid: x\nfatigueWords: []\n---\nbody";
        assert!(parse_genre_profile(no_chapter_types).is_err());
    }

    #[test]
    fn wrong_field_type_errors() {
        // chapterTypes 非数组 → zod throw ↔ serde 报错。
        let bad = "---\nname: X\nid: x\nchapterTypes: 3\nfatigueWords: []\n---\nbody";
        assert!(parse_genre_profile(bad).is_err());
        // 非 YAML 对象（数组）→ zod throw。
        let arr = "---\n- a\n- b\n---\nbody";
        assert!(parse_genre_profile(arr).is_err());
    }

    #[test]
    fn invalid_language_errors() {
        let bad =
            "---\nname: X\nid: x\nlanguage: fr\nchapterTypes: []\nfatigueWords: []\n---\nbody";
        assert!(matches!(
            parse_genre_profile(bad),
            Err(GenreProfileParseError::InvalidField(_))
        ));
    }

    #[test]
    fn unknown_keys_are_stripped() {
        // 对齐 zod strip：未知键不报错、被丢弃。
        let raw = "---\nname: X\nid: x\nchapterTypes: []\nfatigueWords: []\nextra: 1\n---\nbody";
        let parsed = parse_genre_profile(raw).expect("未知键应被忽略");
        assert_eq!(parsed.profile.name, "X");
    }
}
