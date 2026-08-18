//! 生产运行快照（TS `production/harness.ts` 逐字）。
//!
//! 运行快照是运维真值：completed 的运行绝不指向半写的产物集——快照在
//! 产物之后原子提交（commitProductionArtifacts：validate → 事务集 + 快照
//! 末位）。write-next 主链在 running/终态/失败三点发布
//! `story/runtime/chapter-NNNN.run.json`。

use serde::{Deserialize, Serialize};

use crate::utils::atomic_file_set::{commit_atomic_file_set, AtomicFileSet, AtomicFileWrite, FileContent};

/// 生产类型。对齐 TS `ProductionKind`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProductionKind {
    #[serde(rename = "long-fiction")]
    LongFiction,
    #[serde(rename = "short-fiction")]
    ShortFiction,
    #[serde(rename = "script")]
    Script,
    #[serde(rename = "storyboard")]
    Storyboard,
    #[serde(rename = "interactive-film")]
    InteractiveFilm,
    #[serde(rename = "play")]
    Play,
    #[serde(rename = "translation")]
    Translation,
}

/// 运行状态。对齐 TS `ProductionRunStatus`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProductionRunStatus {
    #[serde(rename = "pending")]
    Pending,
    #[serde(rename = "running")]
    Running,
    #[serde(rename = "needs-review")]
    NeedsReview,
    #[serde(rename = "complete")]
    Complete,
    #[serde(rename = "failed")]
    Failed,
    #[serde(rename = "cancelled")]
    Cancelled,
}

/// 观测严重度。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProductionObservationSeverity {
    #[serde(rename = "info")]
    Info,
    #[serde(rename = "warning")]
    Warning,
    #[serde(rename = "blocking")]
    Blocking,
}

/// 单条观测。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductionObservation {
    pub metric: String,
    pub expected: serde_json::Value,
    pub actual: serde_json::Value,
    pub severity: ProductionObservationSeverity,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence: Option<String>,
    pub repairable: bool,
}

/// 运行快照。对齐 TS `ProductionRunSnapshot`（可选键省略序列化）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductionRunSnapshot {
    pub version: u32,
    pub kind: ProductionKind,
    pub id: String,
    pub status: ProductionRunStatus,
    pub stage: String,
    pub artifacts: Vec<String>,
    pub observations: Vec<ProductionObservation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill_ids: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resume_cursor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub updated_at: String,
}

/// `createProductionRunSnapshot` 入参（TS 对象入参形态——version/updatedAt
/// 由构造侧补齐）。
pub struct CreateRunInput {
    pub kind: ProductionKind,
    pub id: String,
    pub status: ProductionRunStatus,
    pub stage: String,
    pub artifacts: Vec<String>,
    pub observations: Vec<ProductionObservation>,
    pub model: Option<String>,
    pub skill_ids: Option<Vec<String>>,
    pub resume_cursor: Option<String>,
    pub error: Option<String>,
}

impl ProductionRunSnapshot {
    /// TS `createProductionRunSnapshot`：补 version=1 与 updatedAt（JS
    /// `new Date().toISOString()` 同款毫秒精度 UTC——复用 millis_to_iso）。
    pub fn create(input: CreateRunInput) -> Self {
        let now_millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or_default();
        let CreateRunInput {
            kind,
            id,
            status,
            stage,
            artifacts,
            observations,
            model,
            skill_ids,
            resume_cursor,
            error,
        } = input;
        ProductionRunSnapshot {
            version: 1,
            kind,
            id,
            status,
            stage,
            artifacts,
            observations,
            model,
            skill_ids,
            resume_cursor,
            error,
            updated_at: crate::state::chapter_workspace::millis_to_iso(now_millis)
                .unwrap_or_default(),
        }
    }
}

/// TS `createRangeObservation`：区间观测——在区间=info；出区间且 hard=false
/// 为 warning，否则 blocking；repairable=!inRange。
#[allow(clippy::too_many_arguments)]
pub fn create_range_observation(
    metric: &str,
    actual: u32,
    target: u32,
    min: u32,
    max: u32,
    unit: &str,
    evidence: Option<String>,
    hard: Option<bool>,
) -> ProductionObservation {
    let in_range = actual >= min && actual <= max;
    ProductionObservation {
        metric: metric.to_string(),
        expected: serde_json::json!({ "target": target, "min": min, "max": max, "unit": unit }),
        actual: serde_json::json!({ "value": actual, "unit": unit }),
        severity: if in_range {
            ProductionObservationSeverity::Info
        } else if hard == Some(false) {
            ProductionObservationSeverity::Warning
        } else {
            ProductionObservationSeverity::Blocking
        },
        evidence,
        repairable: !in_range,
    }
}

/// TS `writeProductionRunSnapshot`：单文件原子发布快照。
pub async fn write_production_run_snapshot(
    root_dir: &std::path::Path,
    run_path: &str,
    run: &ProductionRunSnapshot,
) -> Result<(), crate::utils::atomic_file_set::AtomicFileSetError> {
    commit_atomic_file_set(&AtomicFileSet {
        root_dir,
        writes: vec![AtomicFileWrite {
            relative_path: run_path.to_string(),
            content: FileContent::Text(format!(
                "{}\n",
                serde_json::to_string_pretty(run).map_err(|e| {
                    crate::utils::atomic_file_set::AtomicFileSetError::UnsafePath(e.to_string())
                })?,
            )),
        }],
        deletes: Vec::new(),
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_observation_severity_matrix() {
        let in_range = create_range_observation_helper(3000, 1500, 4500);
        assert_eq!(in_range.severity, ProductionObservationSeverity::Info);
        assert!(!in_range.repairable);
        let out = create_range_observation_helper(100, 1500, 4500);
        assert_eq!(out.severity, ProductionObservationSeverity::Blocking);
        assert!(out.repairable);
        let soft = create_range_observation(
            "m",
            100,
            3000,
            1500,
            4500,
            "zh_chars",
            None,
            Some(false),
        );
        assert_eq!(soft.severity, ProductionObservationSeverity::Warning);
        assert_eq!(
            soft.expected,
            serde_json::json!({ "target": 3000, "min": 1500, "max": 4500, "unit": "zh_chars" })
        );
        assert_eq!(soft.actual, serde_json::json!({ "value": 100, "unit": "zh_chars" }));
    }

    fn create_range_observation_helper(actual: u32, min: u32, max: u32) -> ProductionObservation {
        create_range_observation("chapter-length", actual, 3000, min, max, "zh_chars", None, None)
    }

    #[tokio::test]
    async fn snapshot_file_is_pretty_json_with_trailing_newline() {
        let dir = tempfile::tempdir().unwrap();
        let run = ProductionRunSnapshot::create(CreateRunInput {
            kind: ProductionKind::LongFiction,
            id: "b1:chapter-0001".into(),
            status: ProductionRunStatus::Running,
            stage: "chapter-1".into(),
            artifacts: vec![],
            observations: vec![],
            model: None,
            skill_ids: Some(vec!["inkos-long-writing".into()]),
            resume_cursor: Some("1".into()),
            error: None,
        });
        write_production_run_snapshot(dir.path(), "story/runtime/chapter-0001.run.json", &run)
            .await
            .unwrap();
        let raw = std::fs::read_to_string(
            dir.path()
                .join("story")
                .join("runtime")
                .join("chapter-0001.run.json"),
        )
        .unwrap();
        assert!(raw.ends_with("}\n"), "尾随换行：{raw:?}");
        let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(parsed["version"], 1);
        assert_eq!(parsed["kind"], "long-fiction");
        assert_eq!(parsed["status"], "running");
        assert_eq!(parsed["skillIds"][0], "inkos-long-writing");
        assert!(parsed["updatedAt"].as_str().unwrap().ends_with('Z'));
        assert!(parsed.get("error").is_none(), "可选键省略");
        assert!(parsed.get("model").is_none());
    }
}
