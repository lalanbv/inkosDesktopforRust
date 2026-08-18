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

/// `NarrativeForecastSchema` 的 zod 约束手工等价（store 保存/装载双端校验）。
/// 90 号：错误面聚焦"哪条约束不过"，供 store 包装为逐字错误文案。
pub fn validate_narrative_forecast(forecast: &NarrativeForecast) -> Result<(), String> {
    if forecast.version != 1 {
        return Err(format!("version must be 1, got {}", forecast.version));
    }
    if forecast.forecast_id.is_empty() {
        return Err("forecastId must be non-empty".to_string());
    }
    if forecast.book_id.is_empty() {
        return Err("bookId must be non-empty".to_string());
    }
    if forecast.created_at.is_empty() {
        return Err("createdAt must be non-empty".to_string());
    }
    if forecast.language != "zh" && forecast.language != "en" {
        return Err(format!("language must be zh or en, got {}", forecast.language));
    }
    if forecast.divergence.is_empty() {
        return Err("divergence must be non-empty".to_string());
    }
    if !(FORECAST_MIN_HORIZON..=FORECAST_MAX_HORIZON).contains(&forecast.horizon) {
        return Err(format!(
            "horizon must be between {} and {}",
            FORECAST_MIN_HORIZON, FORECAST_MAX_HORIZON
        ));
    }
    if forecast.context_fingerprint.is_empty() {
        return Err("contextFingerprint must be non-empty".to_string());
    }
    let branch_count = forecast.branches.len();
    if !(FORECAST_MIN_BRANCHES..=FORECAST_MAX_BRANCHES).contains(&branch_count) {
        return Err(format!(
            "branches count {branch_count} not in [{},{}]",
            FORECAST_MIN_BRANCHES, FORECAST_MAX_BRANCHES
        ));
    }
    static BRANCH_ID_RE: OnceLock<Regex> = OnceLock::new();
    let branch_id_re = BRANCH_ID_RE.get_or_init(|| Regex::new(r"^branch-\d+$").unwrap());
    let mut seen: Vec<&str> = Vec::new();
    for (index, branch) in forecast.branches.iter().enumerate() {
        if !branch_id_re.is_match(&branch.branch_id) {
            return Err(format!("branch[{index}].branchId must match ^branch-\\d+$"));
        }
        if seen.contains(&branch.branch_id.as_str()) {
            return Err(format!("duplicate branchId: {}", branch.branch_id));
        }
        seen.push(&branch.branch_id);
        if branch.title.is_empty() {
            return Err(format!("branch[{index}].title must be non-empty"));
        }
        if branch.premise.is_empty() {
            return Err(format!("branch[{index}].premise must be non-empty"));
        }
        if branch.beats.is_empty() {
            return Err(format!("branch[{index}].beats must have at least 1 item"));
        }
        for (beat_index, beat) in branch.beats.iter().enumerate() {
            if beat.chapter < 1 {
                return Err(format!("branch[{index}].beats[{beat_index}].chapter must be >= 1"));
            }
            if beat.summary.is_empty() {
                return Err(format!(
                    "branch[{index}].beats[{beat_index}].summary must be non-empty"
                ));
            }
        }
        for (decision_index, decision) in branch.character_decisions.iter().enumerate() {
            if decision.character.is_empty() {
                return Err(format!(
                    "branch[{index}].characterDecisions[{decision_index}].character must be non-empty"
                ));
            }
            if decision.decision.is_empty() {
                return Err(format!(
                    "branch[{index}].characterDecisions[{decision_index}].decision must be non-empty"
                ));
            }
        }
        for (risk_index, risk) in branch.risks.iter().enumerate() {
            if risk.description.is_empty() {
                return Err(format!(
                    "branch[{index}].risks[{risk_index}].description must be non-empty"
                ));
            }
        }
        if branch.intent_alignment.score > 100 {
            return Err(format!("branch[{index}].intentAlignment.score must be <= 100"));
        }
        if branch.intent_alignment.rationale.is_empty() {
            return Err(format!("branch[{index}].intentAlignment.rationale must be non-empty"));
        }
    }
    Ok(())
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
