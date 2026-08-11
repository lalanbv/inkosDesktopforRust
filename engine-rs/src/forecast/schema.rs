//! 预测 schema + 模型输出解析。
//!
//! 移植自 `packages/core/src/forecast/schema.ts`（137 行）。

use regex::Regex;
use serde::{Deserialize, Serialize};
#[cfg(feature = "export-bindings")]
use ts_rs::TS;
use std::sync::OnceLock;

pub const FORECAST_MIN_BRANCHES: usize = 2;
pub const FORECAST_MAX_BRANCHES: usize = 5;
pub const FORECAST_DEFAULT_BRANCHES: usize = 3;
pub const FORECAST_MIN_HORIZON: u32 = 1;
pub const FORECAST_MAX_HORIZON: u32 = 10;
pub const FORECAST_DEFAULT_HORIZON: u32 = 5;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
pub struct ForecastRisk {
    pub kind: ForecastRiskKind,
    pub description: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export, type = "\"continuity\" | \"causality\" | \"character\""))]
pub enum ForecastRiskKind {
    #[serde(rename = "continuity")] Continuity,
    #[serde(rename = "causality")] Causality,
    #[serde(rename = "character")] Character,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ForecastBeat {
    pub chapter: u32,
    pub summary: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ForecastCharacterDecision {
    pub character: String,
    pub decision: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
pub struct ForecastProjectedChanges {
    #[serde(default)]
    pub characters: Vec<String>,
    #[serde(default)]
    pub relationships: Vec<String>,
    #[serde(default)]
    pub world: Vec<String>,
    #[serde(default)]
    pub hooks: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ForecastIntentAlignment {
    pub score: u32, // 0-100
    pub rationale: String,
}

/// 预测分支（模型输出不含 branchId，由 runner 派生）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ForecastModelBranch {
    pub title: String,
    pub premise: String,
    pub beats: Vec<ForecastBeat>,
    #[serde(default)]
    pub character_decisions: Vec<ForecastCharacterDecision>,
    pub projected_changes: ForecastProjectedChanges,
    #[serde(default)]
    pub risks: Vec<ForecastRisk>,
    #[serde(default)]
    pub uncertainties: Vec<String>,
    pub intent_alignment: ForecastIntentAlignment,
}

/// 带分支 id 的预测分支（runner 产出）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ForecastBranch {
    #[serde(rename = "branchId")]
    pub branch_id: String, // ^branch-\d+$
    pub title: String,
    pub premise: String,
    pub beats: Vec<ForecastBeat>,
    #[serde(default)]
    pub character_decisions: Vec<ForecastCharacterDecision>,
    pub projected_changes: ForecastProjectedChanges,
    #[serde(default)]
    pub risks: Vec<ForecastRisk>,
    #[serde(default)]
    pub uncertainties: Vec<String>,
    pub intent_alignment: ForecastIntentAlignment,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export, type = "\"active\" | \"stale\""))]
pub enum ForecastStatus {
    #[serde(rename = "active")] Active,
    #[serde(rename = "stale")] Stale,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct NarrativeForecast {
    pub version: u32, // literal 1
    #[serde(rename = "forecastId")]
    pub forecast_id: String,
    #[serde(rename = "bookId")]
    pub book_id: String,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    pub language: String,
    pub divergence: String,
    pub horizon: u32,
    #[serde(rename = "baseChapter")]
    pub base_chapter: u32,
    #[serde(rename = "contextFingerprint")]
    pub context_fingerprint: String,
    pub status: ForecastStatus,
    pub branches: Vec<ForecastBranch>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
pub struct ForecastModelOutput {
    pub branches: Vec<ForecastModelBranch>,
}

fn fence_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?is)^```(?:json)?\s*(.*?)\s*```$").unwrap())
}
fn control_chars_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"[\x00-\x08\x0B\x0C\x0E-\x1F\x7F]").unwrap())
}
fn trailing_comma_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r",\s*([}\]])").unwrap())
}

fn strip_code_fence(value: &str) -> String {
    if let Some(caps) = fence_re().captures(value) {
        caps[1].trim().to_string()
    } else {
        value.to_string()
    }
}

fn extract_json_object(value: &str) -> String {
    let start = match value.find('{') {
        Some(s) => s,
        None => return value.to_string(),
    };
    let end = match value.rfind('}') {
        Some(e) => e,
        None => return value.to_string(),
    };
    if end <= start {
        return value.to_string();
    }
    value[start..=end].to_string()
}

fn sanitize_json(value: &str) -> String {
    let s = control_chars_re().replace_all(value, "").into_owned();
    trailing_comma_re().replace_all(&s, "$1").into_owned()
}

/// 解析并校验预测模型输出。容错代码栅栏/包裹散文/尾逗号；其余为硬错误。
pub fn parse_forecast_model_output(raw: &str) -> Result<ForecastModelOutput, String> {
    let json_slice = extract_json_object(&strip_code_fence(raw.trim()));
    let sanitized = sanitize_json(&json_slice);
    let parsed: ForecastModelOutput = serde_json::from_str(&sanitized)
        .map_err(|e| format!("narrative forecast model output is not valid JSON: {e}"))?;
    // 校验分支数区间（对齐 zod min/max）
    let n = parsed.branches.len();
    if !(FORECAST_MIN_BRANCHES..=FORECAST_MAX_BRANCHES).contains(&n) {
        return Err(format!(
            "narrative forecast model output failed schema validation: branches count {n} not in [{},{}]",
            FORECAST_MIN_BRANCHES, FORECAST_MAX_BRANCHES
        ));
    }
    // 每分支至少 1 个 beat
    for (i, b) in parsed.branches.iter().enumerate() {
        if b.beats.is_empty() {
            return Err(format!("branch[{i}] beats 不能为空"));
        }
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_clean_json() {
        let raw = r#"{"branches":[{"title":"A","premise":"p","beats":[{"chapter":1,"summary":"s"}],"characterDecisions":[],"projectedChanges":{"characters":[],"relationships":[],"world":[],"hooks":[]},"risks":[],"uncertainties":[],"intentAlignment":{"score":80,"rationale":"r"}},{"title":"B","premise":"p","beats":[{"chapter":2,"summary":"s"}],"characterDecisions":[],"projectedChanges":{"characters":[],"relationships":[],"world":[],"hooks":[]},"risks":[],"uncertainties":[],"intentAlignment":{"score":70,"rationale":"r"}}]}"#;
        let out = parse_forecast_model_output(raw).unwrap();
        assert_eq!(out.branches.len(), 2);
        assert_eq!(out.branches[0].intent_alignment.score, 80);
    }

    #[test]
    fn parse_tolerates_fence_and_trailing_comma() {
        let raw = "```json\n{\"branches\":[{\"title\":\"A\",\"premise\":\"p\",\"beats\":[{\"chapter\":1,\"summary\":\"s\"}],\"characterDecisions\":[],\"projectedChanges\":{\"characters\":[],\"relationships\":[],\"world\":[],\"hooks\":[]},\"risks\":[],\"uncertainties\":[],\"intentAlignment\":{\"score\":50,\"rationale\":\"r\"},},{\"title\":\"B\",\"premise\":\"p\",\"beats\":[{\"chapter\":2,\"summary\":\"s\"}],\"characterDecisions\":[],\"projectedChanges\":{\"characters\":[],\"relationships\":[],\"world\":[],\"hooks\":[]},\"risks\":[],\"uncertainties\":[],\"intentAlignment\":{\"score\":60,\"rationale\":\"r\"},},]}\n```";
        let out = parse_forecast_model_output(raw).unwrap();
        assert_eq!(out.branches.len(), 2);
    }

    #[test]
    fn rejects_too_few_branches() {
        let raw = r#"{"branches":[]}"#;
        assert!(parse_forecast_model_output(raw).is_err());
    }

    #[test]
    fn helpers() {
        assert_eq!(strip_code_fence("```json\n{}\n```"), "{}");
        assert_eq!(strip_code_fence("plain"), "plain");
        assert_eq!(extract_json_object("prose {\"a\":1} tail"), "{\"a\":1}");
        assert_eq!(sanitize_json(r#"{"a":1,}"#), r#"{"a":1}"#);
    }
}
