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

/// TS `commitProductionArtifacts`：validate 先行（拒即零写入）→ 产物与
/// **权威完成快照同一原子事务**（快照末位——completed 的运行绝不指向
/// 半写产物集）。deletes 随事务提交。
pub async fn commit_production_artifacts(
    root_dir: &std::path::Path,
    artifacts: Vec<AtomicFileWrite>,
    run_path: &str,
    run: &ProductionRunSnapshot,
    deletes: Vec<String>,
    validate: Option<&(dyn Fn() -> Result<(), String> + Send + Sync)>,
) -> Result<(), crate::utils::atomic_file_set::AtomicFileSetError> {
    if let Some(validate) = validate {
        validate().map_err(|message| {
            crate::utils::atomic_file_set::AtomicFileSetError::UnsafePath(format!(
                "production artifact validation failed: {message}"
            ))
        })?;
    }
    let snapshot_json = serde_json::to_string_pretty(run).map_err(|e| {
        crate::utils::atomic_file_set::AtomicFileSetError::UnsafePath(e.to_string())
    })?;
    let mut writes = artifacts;
    writes.push(AtomicFileWrite {
        relative_path: run_path.to_string(),
        content: FileContent::Text(format!("{snapshot_json}\n")),
    });
    commit_atomic_file_set(&AtomicFileSet { root_dir, writes, deletes }).await
}

/// TS `writeProductionRunSnapshot`：commitProductionArtifacts 的空集薄包装。
pub async fn write_production_run_snapshot(
    root_dir: &std::path::Path,
    run_path: &str,
    run: &ProductionRunSnapshot,
) -> Result<(), crate::utils::atomic_file_set::AtomicFileSetError> {
    commit_production_artifacts(root_dir, Vec::new(), run_path, run, Vec::new(), None).await
}

/// 运行时观测工件的保留章数（每书）。`INKOS_RUNTIME_RETENTION_CHAPTERS`
/// 可覆盖；0 = 关闭清理（完全兼容旧行为）。
pub fn runtime_retention_chapters() -> u32 {
    std::env::var("INKOS_RUNTIME_RETENTION_CHAPTERS")
        .ok()
        .and_then(|raw| raw.parse::<u32>().ok())
        .unwrap_or(20)
}

/// 清理旧章的运行时观测工件（200 号稳定性审计）。
///
/// `story/runtime/` 下每章累积 run.json / trace.json / context.json /
/// rule-stack.yaml 四类观测文件（trace 含完整 LLM 交互轨迹，长书无限增长）。
/// 本函数只保留最近 `keep` 章的观测工件；**plan.md / intent.md 治理产物
/// 保留**（体积小、回溯有用）。清理失败仅逐文件忽略——属事后打扫，
/// 任何失败都不值得让写作管线报错。
pub async fn prune_runtime_artifacts(book_dir: &std::path::Path, latest_chapter: u32, keep: u32) {
    if keep == 0 || latest_chapter <= keep {
        return;
    }
    let cutoff = latest_chapter - keep; // 章号 <= cutoff 的观测工件过期
    let runtime_dir = book_dir.join("story").join("runtime");
    let Ok(mut entries) = tokio::fs::read_dir(&runtime_dir).await else {
        return;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name().to_string_lossy().into_owned();
        // 观测工件命名：chapter-{padded}.{run.json|trace.json|context.json|rule-stack.yaml}
        let is_observability = ["run.json", "trace.json", "context.json", "rule-stack.yaml"]
            .iter()
            .any(|suffix| name.ends_with(suffix));
        if !is_observability {
            continue;
        }
        let Some(digits) = name
            .strip_prefix("chapter-")
            .and_then(|rest| rest.split('.').next())
        else {
            continue;
        };
        let Ok(number) = digits.parse::<u32>() else {
            continue;
        };
        if number <= cutoff {
            let _ = tokio::fs::remove_file(entry.path()).await;
        }
    }
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
    async fn prune_keeps_recent_observability_files_and_governance_files() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = dir.path().join("story").join("runtime");
        tokio::fs::create_dir_all(&runtime).await.unwrap();
        // 1..=5 章的观测工件 + 治理产物。
        for n in 1..=5u32 {
            let padded = format!("{n:04}");
            for suffix in ["run.json", "trace.json", "context.json", "rule-stack.yaml"] {
                tokio::fs::write(runtime.join(format!("chapter-{padded}.{suffix}")), "x").await.unwrap();
            }
            tokio::fs::write(runtime.join(format!("chapter-{padded}.plan.md")), "plan").await.unwrap();
            tokio::fs::write(runtime.join(format!("chapter-{padded}.intent.md")), "intent").await.unwrap();
        }
        prune_runtime_artifacts(dir.path(), 5, 2).await;
        // 章号 <= 3 的观测工件被清；最近 2 章 + 治理产物全保留。
        for n in 1..=3u32 {
            let padded = format!("{n:04}");
            assert!(!runtime.join(format!("chapter-{padded}.run.json")).exists(), "chapter {n} run.json 应被清理");
            assert!(!runtime.join(format!("chapter-{padded}.trace.json")).exists(), "chapter {n} trace.json 应被清理");
        }
        for n in 4..=5u32 {
            let padded = format!("{n:04}");
            assert!(runtime.join(format!("chapter-{padded}.run.json")).exists());
            assert!(runtime.join(format!("chapter-{padded}.trace.json")).exists());
        }
        for n in 1..=5u32 {
            let padded = format!("{n:04}");
            assert!(runtime.join(format!("chapter-{padded}.plan.md")).exists(), "plan.md 应保留");
            assert!(runtime.join(format!("chapter-{padded}.intent.md")).exists(), "intent.md 应保留");
        }
    }

    #[tokio::test]
    async fn prune_is_noop_when_keep_is_zero_or_range_not_exceeded() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = dir.path().join("story").join("runtime");
        tokio::fs::create_dir_all(&runtime).await.unwrap();
        tokio::fs::write(runtime.join("chapter-0001.run.json"), "x").await.unwrap();
        // keep=0 = 关闭清理。
        prune_runtime_artifacts(dir.path(), 5, 0).await;
        assert!(runtime.join("chapter-0001.run.json").exists());
        // 未超出保留窗口：无操作。
        prune_runtime_artifacts(dir.path(), 1, 20).await;
        assert!(runtime.join("chapter-0001.run.json").exists());
    }

    #[tokio::test]
    async fn commits_artifacts_before_authoritative_completion_snapshot() {
        // TS production-harness.test.ts 同款：产物与完成快照同事务落盘。
        let dir = tempfile::tempdir().unwrap();
        let run = ProductionRunSnapshot::create(CreateRunInput {
            kind: ProductionKind::Script,
            id: "night-shift".into(),
            status: ProductionRunStatus::Complete,
            stage: "commit".into(),
            artifacts: vec!["script.md".into()],
            observations: vec![],
            model: None,
            skill_ids: None,
            resume_cursor: None,
            error: None,
        });
        let validated = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let seen = validated.clone();
        commit_production_artifacts(
            dir.path(),
            vec![AtomicFileWrite {
                relative_path: "script.md".into(),
                content: FileContent::Text("# Night Shift".into()),
            }],
            "status.json",
            &run,
            Vec::new(),
            Some(&move || {
                seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            }),
        )
        .await
        .unwrap();
        assert_eq!(validated.load(std::sync::atomic::Ordering::SeqCst), 1, "validate 先行");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("script.md")).unwrap(),
            "# Night Shift"
        );
        let status = std::fs::read_to_string(dir.path().join("status.json")).unwrap();
        assert!(status.contains("\"status\": \"complete\""), "{status}");
    }

    #[tokio::test]
    async fn validation_rejection_leaves_no_writes() {
        let dir = tempfile::tempdir().unwrap();
        let run = ProductionRunSnapshot::create(CreateRunInput {
            kind: ProductionKind::Script,
            id: "x".into(),
            status: ProductionRunStatus::Complete,
            stage: "commit".into(),
            artifacts: vec![],
            observations: vec![],
            model: None,
            skill_ids: None,
            resume_cursor: None,
            error: None,
        });
        let err = commit_production_artifacts(
            dir.path(),
            vec![AtomicFileWrite {
                relative_path: "draft.md".into(),
                content: FileContent::Text("半成品".into()),
            }],
            "status.json",
            &run,
            Vec::new(),
            Some(&|| Err("empty deliverable".to_string())),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("validation failed"), "{err}");
        assert!(!dir.path().join("draft.md").exists(), "拒即零写入");
        assert!(!dir.path().join("status.json").exists());
    }

    #[tokio::test]
    async fn deletes_commit_with_the_transaction() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("stale")).unwrap();
        std::fs::write(dir.path().join("stale").join("old.md"), "旧稿").unwrap();
        let run = ProductionRunSnapshot::create(CreateRunInput {
            kind: ProductionKind::Translation,
            id: "t1".into(),
            status: ProductionRunStatus::Complete,
            stage: "commit".into(),
            artifacts: vec![],
            observations: vec![],
            model: None,
            skill_ids: None,
            resume_cursor: None,
            error: None,
        });
        commit_production_artifacts(
            dir.path(),
            vec![],
            "status.json",
            &run,
            vec!["stale/old.md".into()],
            None,
        )
        .await
        .unwrap();
        assert!(!dir.path().join("stale").join("old.md").exists(), "deletes 生效");
        assert!(dir.path().join("status.json").exists());
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
