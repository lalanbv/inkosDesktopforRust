//! 观察发现契约。移植自 `packages/core/src/models/observation.ts` 的
//! `ObservationSchema`（zod strict ↔ serde deny_unknown_fields 精确互逆——
//! 876/881 号跨引擎硬门禁纪律）。
//!
//! 消费方：短篇生产状态机（production/short_fiction.rs 的 ShortStage/
//! delivery.observations）与 M3b sourced-review 面887 号 M3a 只落契约与
//! 校验函数，LLM 提交面（resolveObservationSources 等）随 M3b。

use serde::{Deserialize, Serialize};

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
pub enum ObservationCategory {
    Execution,
    Quality,
    Scope,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
