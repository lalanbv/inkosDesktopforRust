//! chapter-persistence —— 章节工件落盘编排（索引/审计漂移/快照）。
//!
//! 移植自 `packages/core/src/pipeline/chapter-persistence.ts`（79 行）。
//! 线性序列：saveChapter →（非降级）saveTruthFiles → 索引 upsert（保留
//! createdAt）→ markBookActive → 审计漂移指引（降级时空）→（非降级）状态
//! 快照 + 事实历史同步。所有副作用经 [`PersistenceHooks`] trait 注入。
//!
//! ## 移植纪律
//! - TS `persistAuditDriftGuidance(...).catch(() => undefined)`——漂移指引
//!   失败**静默吞掉**（不影响主链），Rust 侧 log 后继续
//! - 索引 upsert：同号条目保留原 `createdAt`；追加在尾部
//! - 降级状态跳过真相文件/快照/事实历史（保留旧真相，等修复）

use async_trait::async_trait;

use crate::agents::continuity::{AuditIssue, AuditSeverity, AuditTokenUsage};
use crate::models::chapter::{ChapterMeta, ChapterStatus};
use crate::models::length_governance::LengthTelemetry;
use crate::pipeline::chapter_state_recovery::build_state_degraded_review_note;

/// 落盘副作用端口（runner 的 state/writer 方法注入）。
#[async_trait]
pub trait PersistenceHooks: Send + Sync {
    async fn load_chapter_index(&self) -> Result<Vec<ChapterMeta>, String>;
    async fn save_chapter(&self) -> Result<(), String>;
    async fn save_truth_files(&self) -> Result<(), String>;
    async fn save_chapter_index(&self, index: &[ChapterMeta]) -> Result<(), String>;
    async fn mark_book_active_if_needed(&self) -> Result<(), String>;
    async fn persist_audit_drift_guidance(&self, issues: &[AuditIssue]) -> Result<(), String>;
    async fn snapshot_state(&self) -> Result<(), String>;
    async fn sync_current_state_fact_history(&self) -> Result<(), String>;
}

/// 落盘入参。
pub struct PersistChapterArtifactsParams<'a> {
    pub chapter_number: u32,
    pub chapter_title: &'a str,
    pub status: ChapterStatus,
    pub audit_passed: bool,
    pub audit_issues: &'a [AuditIssue],
    pub final_word_count: u32,
    pub length_warnings: &'a [String],
    pub length_telemetry: Option<LengthTelemetry>,
    pub degraded_issues: &'a [AuditIssue],
    pub token_usage: Option<AuditTokenUsage>,
    /// 快照阶段日志（落快照前的中文阶段提示）。
    pub log_snapshot_stage: &'a (dyn Fn() + Send + Sync),
    /// 时间源注入（测试用；缺省取当前时间）。
    pub now: Option<&'a (dyn Fn() -> String + Send + Sync)>,
}

#[derive(Debug, thiserror::Error)]
pub enum PersistChapterArtifactsError {
    #[error("persistence hook failed: {0}")]
    Hook(String),
}

/// 章节工件落盘主入口。返回写入/更新的索引条目。
pub async fn persist_chapter_artifacts(
    hooks: &dyn PersistenceHooks,
    params: &PersistChapterArtifactsParams<'_>,
) -> Result<ChapterMeta, PersistChapterArtifactsError> {
    let hook_err = |message: String| PersistChapterArtifactsError::Hook(message);

    hooks.save_chapter().await.map_err(hook_err)?;
    if params.status != ChapterStatus::StateDegraded {
        hooks.save_truth_files().await.map_err(hook_err)?;
    }

    let existing_index = hooks.load_chapter_index().await.map_err(hook_err)?;
    let now = params
        .now
        .map(|f| f())
        .unwrap_or_else(chrono_like_now);
    let entry = ChapterMeta {
        number: params.chapter_number,
        title: params.chapter_title.to_string(),
        status: params.status,
        word_count: params.final_word_count,
        created_at: now.clone(),
        updated_at: now,
        audit_issues: params
            .audit_issues
            .iter()
            .map(|issue| format!("[{}] {}", severity_label(issue.severity), issue.description))
            .collect(),
        length_warnings: params.length_warnings.to_vec(),
        review_note: (params.status == ChapterStatus::StateDegraded).then(|| {
            build_state_degraded_review_note(
                if params.audit_passed { "ready-for-review" } else { "audit-failed" },
                params.degraded_issues,
            )
        }),
        detection_score: None,
        detection_provider: None,
        detected_at: None,
        length_telemetry: params.length_telemetry.clone(),
        token_usage: params.token_usage.as_ref().map(|usage| crate::models::chapter::TokenUsage {
            prompt_tokens: usage.prompt_tokens,
            completion_tokens: usage.completion_tokens,
            total_tokens: usage.total_tokens,
        }),
    };

    let existing_idx = existing_index
        .iter()
        .position(|meta| meta.number == params.chapter_number);
    let updated_index: Vec<ChapterMeta> = match existing_idx {
        Some(index) => existing_index
            .iter()
            .enumerate()
            .map(|(i, meta)| {
                if i == index {
                    ChapterMeta {
                        created_at: meta.created_at.clone(),
                        ..entry.clone()
                    }
                } else {
                    meta.clone()
                }
            })
            .collect(),
        None => {
            let mut index = existing_index;
            index.push(entry.clone());
            index
        }
    };
    hooks
        .save_chapter_index(&updated_index)
        .await
        .map_err(hook_err)?;
    hooks
        .mark_book_active_if_needed()
        .await
        .map_err(hook_err)?;

    let drift_issues: Vec<AuditIssue> = params
        .audit_issues
        .iter()
        .filter(|issue| {
            issue.severity == AuditSeverity::Critical || issue.severity == AuditSeverity::Warning
        })
        .cloned()
        .collect();
    // TS `.catch(() => undefined)`：漂移指引失败静默（不阻断落盘主链）。
    let drift_target: Vec<AuditIssue> = if params.status == ChapterStatus::StateDegraded {
        Vec::new()
    } else {
        drift_issues
    };
    if let Err(error) = hooks.persist_audit_drift_guidance(&drift_target).await {
        tracing::warn!("[audit-drift] guidance persist failed (ignored): {error}");
    }

    if params.status != ChapterStatus::StateDegraded {
        (params.log_snapshot_stage)();
        hooks.snapshot_state().await.map_err(hook_err)?;
        hooks
            .sync_current_state_fact_history()
            .await
            .map_err(hook_err)?;
    }

    Ok(entry)
}

fn severity_label(severity: AuditSeverity) -> &'static str {
    match severity {
        AuditSeverity::Critical => "critical",
        AuditSeverity::Warning => "warning",
        AuditSeverity::Info => "info",
    }
}

/// `new Date().toISOString()` 等价：UTC 毫秒精度的 RFC3339。
fn chrono_like_now() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs() as i64;
    let millis = now.subsec_millis();
    let (year, month, day, hour, minute, second) = civil_from_unix(secs);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z")
}

/// Unix 秒 → 公历 civil 时刻（Howard Hinnant 算法，无 chrono 依赖）。
fn civil_from_unix(secs: i64) -> (i64, u32, u32, u32, u32, u32) {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let hour = (rem / 3600) as u32;
    let minute = ((rem % 3600) / 60) as u32;
    let second = (rem % 60) as u32;

    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = if m <= 2 { y + 1 } else { y };
    (year, m, d, hour, minute, second)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct RecordingHooks {
        index: Mutex<Vec<ChapterMeta>>,
        calls: Mutex<Vec<&'static str>>,
        drift_fail: bool,
    }

    impl RecordingHooks {
        fn new(index: Vec<ChapterMeta>) -> Self {
            RecordingHooks {
                index: Mutex::new(index),
                calls: Mutex::new(Vec::new()),
                drift_fail: false,
            }
        }

        fn record(&self, name: &'static str) {
            self.calls.lock().unwrap().push(name);
        }
    }

    #[async_trait]
    impl PersistenceHooks for RecordingHooks {
        async fn load_chapter_index(&self) -> Result<Vec<ChapterMeta>, String> {
            self.record("load_index");
            Ok(self.index.lock().unwrap().clone())
        }
        async fn save_chapter(&self) -> Result<(), String> {
            self.record("save_chapter");
            Ok(())
        }
        async fn save_truth_files(&self) -> Result<(), String> {
            self.record("save_truth");
            Ok(())
        }
        async fn save_chapter_index(&self, index: &[ChapterMeta]) -> Result<(), String> {
            self.record("save_index");
            *self.index.lock().unwrap() = index.to_vec();
            Ok(())
        }
        async fn mark_book_active_if_needed(&self) -> Result<(), String> {
            self.record("mark_active");
            Ok(())
        }
        async fn persist_audit_drift_guidance(&self, issues: &[AuditIssue]) -> Result<(), String> {
            self.record("drift");
            if self.drift_fail {
                Err("drift boom".to_string())
            } else {
                let _ = issues;
                Ok(())
            }
        }
        async fn snapshot_state(&self) -> Result<(), String> {
            self.record("snapshot");
            Ok(())
        }
        async fn sync_current_state_fact_history(&self) -> Result<(), String> {
            self.record("fact_history");
            Ok(())
        }
    }

    fn issue(severity: AuditSeverity) -> AuditIssue {
        AuditIssue {
            severity,
            category: "cat".into(),
            description: "desc".into(),
            suggestion: String::new(),
            repair_scope: None,
        }
    }

    fn params<'a>(
        status: ChapterStatus,
        audit_issues: &'a [AuditIssue],
        degraded_issues: &'a [AuditIssue],
        warnings: &'a [String],
    ) -> PersistChapterArtifactsParams<'a> {
        PersistChapterArtifactsParams {
            chapter_number: 3,
            chapter_title: "夜探",
            status,
            audit_passed: true,
            audit_issues,
            final_word_count: 2400,
            length_warnings: warnings,
            length_telemetry: None,
            degraded_issues,
            token_usage: None,
            log_snapshot_stage: &|| {},
            now: Some(&|| "2026-08-15T00:00:00.000Z".to_string()),
        }
    }

    const WARNINGS: &[&str] = &["偏短"];

    #[tokio::test]
    async fn appends_entry_and_runs_full_chain() {
        let hooks = RecordingHooks::new(Vec::new());
        let issues = vec![issue(AuditSeverity::Warning), issue(AuditSeverity::Info)];
        let degraded: Vec<AuditIssue> = vec![];
        let warnings: Vec<String> = WARNINGS.iter().map(|w| w.to_string()).collect();
        let entry = persist_chapter_artifacts(
            &hooks,
            &params(ChapterStatus::ReadyForReview, &issues, &degraded, &warnings),
        )
        .await
        .unwrap();
        assert_eq!(entry.number, 3);
        assert_eq!(entry.audit_issues.len(), 2);
        assert_eq!(entry.audit_issues[0], "[warning] desc");
        assert_eq!(entry.review_note, None);
        // 完整链 + 漂移带 critical/warning 子集。
        let calls = hooks.calls.lock().unwrap().clone();
        assert_eq!(
            calls,
            vec![
                "save_chapter",
                "save_truth",
                "load_index",
                "save_index",
                "mark_active",
                "drift",
                "snapshot",
                "fact_history"
            ]
        );
        assert_eq!(hooks.index.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn degraded_skips_truth_snapshot_and_empties_drift() {
        let hooks = RecordingHooks::new(Vec::new());
        let issues = vec![issue(AuditSeverity::Critical)];
        let degraded = vec![issue(AuditSeverity::Warning)];
        let warnings: Vec<String> = WARNINGS.iter().map(|w| w.to_string()).collect();
        let entry = persist_chapter_artifacts(
            &hooks,
            &params(ChapterStatus::StateDegraded, &issues, &degraded, &warnings),
        )
        .await
        .unwrap();
        // 降级注记：baseStatus 按 audit_passed → ready-for-review。
        assert!(entry.review_note.as_deref().unwrap().contains("\"kind\":\"state-degraded\""));
        assert!(entry
            .review_note
            .as_deref()
            .unwrap()
            .contains("\"baseStatus\":\"ready-for-review\""));
        let calls = hooks.calls.lock().unwrap().clone();
        assert_eq!(
            calls,
            vec!["save_chapter", "load_index", "save_index", "mark_active", "drift"]
        );
    }

    #[tokio::test]
    async fn upsert_keeps_created_at() {
        let existing = ChapterMeta {
            number: 3,
            title: "旧标题".into(),
            status: ChapterStatus::Approved,
            word_count: 100,
            created_at: "2020-01-01T00:00:00.000Z".into(),
            updated_at: "2020-01-01T00:00:00.000Z".into(),
            audit_issues: vec![],
            length_warnings: vec![],
            review_note: None,
            detection_score: None,
            detection_provider: None,
            detected_at: None,
            length_telemetry: None,
            token_usage: None,
        };
        let hooks = RecordingHooks::new(vec![existing]);
        let issues: Vec<AuditIssue> = vec![];
        let degraded: Vec<AuditIssue> = vec![];
        let warnings: Vec<String> = WARNINGS.iter().map(|w| w.to_string()).collect();
        let entry = persist_chapter_artifacts(
            &hooks,
            &params(ChapterStatus::ReadyForReview, &issues, &degraded, &warnings),
        )
        .await
        .unwrap();
        // 返回的 entry 带 now；索引内同号条目保留旧 createdAt（TS 同语义：
        // map 进 index 的是 {...entry, createdAt: e.createdAt}）。
        assert_eq!(entry.created_at, "2026-08-15T00:00:00.000Z");
        assert_eq!(entry.updated_at, "2026-08-15T00:00:00.000Z");
        assert_eq!(hooks.index.lock().unwrap().len(), 1);
        assert_eq!(
            hooks.index.lock().unwrap()[0].created_at,
            "2020-01-01T00:00:00.000Z"
        );
    }

    #[tokio::test]
    async fn drift_failure_is_swallowed() {
        // 直接构造 drift_fail 版本。
        let hooks = RecordingHooks {
            index: Mutex::new(Vec::new()),
            calls: Mutex::new(Vec::new()),
            drift_fail: true,
        };
        let issues = vec![issue(AuditSeverity::Critical)];
        let degraded: Vec<AuditIssue> = vec![];
        let warnings: Vec<String> = WARNINGS.iter().map(|w| w.to_string()).collect();
        let result = persist_chapter_artifacts(
            &hooks,
            &params(ChapterStatus::ReadyForReview, &issues, &degraded, &warnings),
        )
        .await;
        assert!(result.is_ok());
        assert!(hooks.calls.lock().unwrap().contains(&"snapshot"));
    }

    #[test]
    fn civil_from_unix_known_values() {
        // 2026-08-15T00:00:00Z = 1786752000
        assert_eq!(civil_from_unix(1_786_752_000), (2026, 8, 15, 0, 0, 0));
        // 1970-01-01T00:00:00Z
        assert_eq!(civil_from_unix(0), (1970, 1, 1, 0, 0, 0));
        // 2000-02-29T12:34:56Z（闰年）
        assert_eq!(civil_from_unix(951_827_696), (2000, 2, 29, 12, 34, 56));
        let now = chrono_like_now();
        assert!(now.ends_with('Z') && now.len() == 24);
    }
}
