//! 观察发现契约。移植自 `packages/core/src/models/observation.ts` 的
//! `ObservationSchema`（zod strict ↔ serde deny_unknown_fields 精确互逆——
//! 876/881 号跨引擎硬门禁纪律）。
//!
//! 消费方：短篇生产状态机（production/short_fiction.rs 的 ShortStage/
//! delivery.observations）与 M3b sourced-review 面887 号 M3a 只落契约与
//! 校验函数，LLM 提交面（resolveObservationSources 等）随 M3b。

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ObservationTarget {
    #[serde(rename = "workId")]
    pub work_id: String,
    #[serde(rename = "artifactId")]
    pub artifact_id: String,
    #[serde(rename = "revisionId")]
    pub revision_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservationSourceRef {
    #[serde(rename = "sourceId")]
    pub source_id: String,
    pub quote: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ObservationCategory {
    Execution,
    Quality,
    Scope,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ObservationAssessment {
    Issue,
    Resolved,
    Unavailable,
    Observation,
}

/// zod `.strict()` 全字段契约；`evidence` default([]) 在 TS parse 输出中恒在
/// 场（default 应用后序列化），serde 侧 `#[serde(default)]` + 恒序列化同款。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Observation {
    pub code: String,
    pub summary: String,
    #[serde(default)]
    pub evidence: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<ObservationCategory>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assessment: Option<ObservationAssessment>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<ObservationTarget>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_refs: Option<Vec<ObservationSourceRef>>,
}

impl Observation {
    /// zod parse 等价校验：code/summary min(1)、evidence 项、sourceRefs 引用
    /// 非空（z.string().min(1) 双字段）。
    pub fn validate(&self) -> Result<(), String> {
        if self.code.is_empty() {
            return Err("code must not be empty".into());
        }
        if self.summary.is_empty() {
            return Err("summary must not be empty".into());
        }
        if let Some(source_refs) = &self.source_refs {
            for reference in source_refs {
                if reference.source_id.is_empty() {
                    return Err("sourceRefs[].sourceId must not be empty".into());
                }
                if reference.quote.is_empty() {
                    return Err("sourceRefs[].quote must not be empty".into());
                }
            }
        }
        Ok(())
    }
}

// ── 审稿提交面（889 号 M3c：sourced review 的提交形态与 resolve）──────

/// 模型提交的源引用（quote 直提或 startLine/endLine 行区间——宿主截取）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ReviewSourceSelection {
    #[serde(rename = "sourceId")]
    pub source_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quote: Option<String>,
    #[serde(rename = "startLine", skip_serializing_if = "Option::is_none")]
    pub start_line: Option<u32>,
    #[serde(rename = "endLine", skip_serializing_if = "Option::is_none")]
    pub end_line: Option<u32>,
}

/// 模型提交的观察（ReviewObservationIndex——TS SourcedReviewIndexToolSchema
/// items 形态：evidence 不提交（宿主置 []）、sourceRefs 为行区间选择面）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ReviewObservationSubmission {
    pub code: String,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<ObservationCategory>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assessment: Option<ObservationAssessment>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<ObservationTarget>,
    #[serde(rename = "sourceRefs", default, skip_serializing_if = "Option::is_none")]
    pub source_refs: Option<Vec<ReviewSourceSelection>>,
}

/// `Error(JSON.stringify(details))` 形态的审稿源错误——message 即 details JSON
/// 串（TS 同形），结构化面另行携带。
#[derive(Debug, Clone, thiserror::Error)]
#[error("{message}")]
pub struct ReviewSourceError {
    pub code: String,
    pub message: String,
    pub details: Value,
}

impl ReviewSourceError {
    fn from_details(code: &str, mut details: Value) -> Self {
        if let Value::Object(map) = &mut details {
            map.insert("code".into(), Value::String(code.into()));
        }
        Self {
            code: code.into(),
            message: details.to_string(),
            details,
        }
    }
}

/// TS `resolveObservationSources`：模型行区间选择 → 宿主精确截取
///（REVIEW_SOURCE_RANGE_INVALID / REVIEW_SOURCE_REQUIRED 两码，details JSON
/// 消息面与 TS 同形）+ 末尾 `validateObservationSources`（REVIEW_SOURCE_REQUIRED
/// 无引用 / REVIEW_SOURCE_MISMATCH 引用不在源内）。
pub fn resolve_observation_sources(
    submissions: &[ReviewObservationSubmission],
    sources: &std::collections::BTreeMap<String, String>,
) -> Result<Vec<Observation>, ReviewSourceError> {
    let mut resolved: Vec<Observation> = Vec::with_capacity(submissions.len());
    for (index, submission) in submissions.iter().enumerate() {
        let mut source_refs: Vec<ObservationSourceRef> = Vec::new();
        for (reference_index, reference) in
            submission.source_refs.iter().flatten().enumerate()
        {
            let quote = match &reference.quote {
                Some(quote) => quote.clone(),
                None => {
                    let lines = sources
                        .get(&reference.source_id)
                        .map(|source| crate::utils::source_text::source_line_bodies(source));
                    let start = reference.start_line;
                    let end = reference.end_line;
                    let valid = lines.as_ref().map(|lines| {
                        match (start, end) {
                            (Some(start), Some(end)) => {
                                start >= 1 && end >= start && (end as usize) <= lines.len()
                            }
                            _ => false,
                        }
                    });
                    let Some(true) = valid else {
                        let details = serde_json::json!({
                            "path": format!("/observations/{index}/sourceRefs/{reference_index}"),
                            "sourceId": reference.source_id,
                            "startLine": start,
                            "endLine": end,
                            "lineCount": lines.as_ref().map(|lines| lines.len()).unwrap_or(0),
                        });
                        return Err(ReviewSourceError::from_details(
                            "REVIEW_SOURCE_RANGE_INVALID",
                            details,
                        ));
                    };
                    let lines = lines.unwrap();
                    let start = start.unwrap() as usize;
                    let end = end.unwrap() as usize;
                    let quote = lines[start - 1..end].join("\n");
                    if quote.trim().is_empty() {
                        // TS nearbyLines：候选邻行（去重、非空白、截 400 字）。
                        let mut nearby: std::collections::BTreeSet<usize> =
                            std::collections::BTreeSet::new();
                        for candidate in
                            [start as isize - 2, start as isize - 1, end as isize, end as isize + 1]
                        {
                            if candidate >= 0
                                && (candidate as usize) < lines.len()
                                && !lines[candidate as usize].trim().is_empty()
                            {
                                nearby.insert(candidate as usize);
                            }
                        }
                        let nearby: Vec<Value> = nearby
                            .iter()
                            .map(|line_index| {
                                serde_json::json!({
                                    "line": line_index + 1,
                                    "text": lines[*line_index].chars().take(400).collect::<String>(),
                                })
                            })
                            .collect();
                        let details = serde_json::json!({
                            "path": format!("/observations/{index}/sourceRefs/{reference_index}"),
                            "sourceId": reference.source_id,
                            "startLine": start,
                            "endLine": end,
                            "lineCount": lines.len(),
                            "nearbyLines": nearby,
                            "instruction": "This selection contains only blank lines. Select the numbered source lines that actually support this finding, or withdraw it. Preserve other valid references.",
                        });
                        return Err(ReviewSourceError::from_details(
                            "REVIEW_SOURCE_REQUIRED",
                            details,
                        ));
                    }
                    quote
                }
            };
            source_refs.push(ObservationSourceRef {
                source_id: reference.source_id.clone(),
                quote,
            });
        }
        resolved.push(Observation {
            code: submission.code.clone(),
            summary: submission.summary.clone(),
            evidence: Vec::new(),
            category: submission.category,
            assessment: submission.assessment,
            scope: submission.scope.clone(),
            target_hash: None,
            target: submission.target.clone(),
            source_refs: Some(source_refs),
        });
    }
    validate_observation_sources(&resolved, sources)?;
    Ok(resolved)
}

/// TS `validateObservationSources`：无引用（非 unavailable）→ REQUIRED；
/// quote 不在源内 → MISMATCH；issues 首码为错误码（details issues 截 16）。
pub fn validate_observation_sources(
    observations: &[Observation],
    sources: &std::collections::BTreeMap<String, String>,
) -> Result<(), ReviewSourceError> {
    let mut issues: Vec<Value> = Vec::new();
    for (index, observation) in observations.iter().enumerate() {
        let source_refs = observation.source_refs.as_deref().unwrap_or(&[]);
        if source_refs.is_empty() && observation.assessment != Some(ObservationAssessment::Unavailable)
        {
            issues.push(serde_json::json!({
                "code": "REVIEW_SOURCE_REQUIRED",
                "path": format!("/observations/{index}/sourceRefs"),
                "observationCode": observation.code,
            }));
        }
        for (reference_index, reference) in source_refs.iter().enumerate() {
            let source_contains = sources
                .get(&reference.source_id)
                .map(|source| source.contains(&reference.quote))
                .unwrap_or(false);
            if !source_contains {
                issues.push(serde_json::json!({
                    "code": "REVIEW_SOURCE_MISMATCH",
                    "path": format!("/observations/{index}/sourceRefs/{reference_index}"),
                    "observationCode": observation.code,
                    "sourceId": reference.source_id,
                    "quote": reference.quote.chars().take(1000).collect::<String>(),
                }));
            }
        }
    }
    if !issues.is_empty() {
        let code = issues[0]["code"].as_str().unwrap_or("REVIEW_SOURCE_REQUIRED").to_string();
        let issue_count = issues.len();
        let details = serde_json::json!({
            "code": code,
            "issues": issues.iter().take(16).cloned().collect::<Vec<_>>(),
            "issueCount": issue_count,
            "sourceIds": sources.keys().cloned().collect::<Vec<_>>(),
            "instruction": "Use exact excerpts from the named source for these references, or withdraw unsupported findings. Keep valid references unchanged.",
        });
        return Err(ReviewSourceError::from_details(&code, details));
    }
    Ok(())
}
