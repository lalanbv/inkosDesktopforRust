//! 流派画像。
//!
//! 移植自 `packages/core/src/models/genre-profile.ts`。
//! 注：TS 的 `parseGenreProfile` 依赖 js-yaml，Rust 端待加 `serde_yaml` 依赖后移植（TODO）。

use serde::{Deserialize, Serialize};
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

// TODO(serde_yaml): parseGenreProfile(raw) 需 YAML frontmatter 解析。
// 加 serde_yaml 依赖后移植：剥 --- 块 → YAML 反序列化为 GenreProfile → body = 其后正文。
