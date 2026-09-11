//! 检测管线 runner——单章检测 + detect-and-rewrite 自动改写环 + 历史落盘。
//!
//! 移植自 `packages/core/src/pipeline/detection-runner.ts`（163 行，111 号——
//! 72 号 Scheduler 精简面备案收口）。历史文件 `story/detection_history.json`
//! 与 52 号 `utils/detection_insights` 共用（此处补齐写入面）。

use std::path::Path;

use serde::Serialize;

use crate::agents::continuity::{AuditIssue, AuditSeverity};
use crate::agents::detector::{detect_ai_content, DetectionResult};
use crate::agents::reviser::{revise_chapter as reviser_revise_chapter, ReviseMode, ReviseOptions, ReviserChat, ReviserCtx};
use crate::models::project::DetectionConfig;
use crate::utils::utc_time::utc_now_iso;

/// 单章检测结果。对齐 TS `DetectChapterResult`。
#[derive(Debug)]
pub struct DetectChapterOutcome {
    pub chapter_number: u32,
    pub detection: DetectionResult,
    pub passed: bool,
    /// G4/340 号：专名泄露命中（protectedNames 配置时；命中即不通过）。
    pub leaks: Vec<crate::utils::style_feature_engine::ProperNounLeakHit>,
}

/// 自动改写环结果。对齐 TS `DetectAndRewriteResult`。
#[derive(Debug)]
pub struct DetectAndRewriteOutcome {
    pub chapter_number: u32,
    pub original_score: f64,
    pub final_score: f64,
    pub attempts: u32,
    pub passed: bool,
    pub final_content: String,
}

/// API key 解析（TS `process.env[config.apiKeyEnv]`；缺失 → 空串）。
fn detection_api_key(config: &DetectionConfig) -> String {
    std::env::var(&config.api_key_env).unwrap_or_default()
}

/// `detectChapter`：单章检测，`score <= threshold` 判过。
pub async fn detect_chapter(
    client: &reqwest::Client,
    config: &DetectionConfig,
    content: &str,
    chapter_number: u32,
) -> Result<DetectChapterOutcome, String> {
    let detection = detect_ai_content(client, config, content, &detection_api_key(config), &utc_now_iso())
        .await
        .map_err(|e| e.to_string())?;
    let mut passed = detection.score <= config.threshold;
    // G4/340 号：专名泄露参与 detect——命中即 fail（泄露无法靠降 AI 味修复）。
    let leaks = crate::utils::style_feature_engine::detect_proper_noun_leak(
        content,
        &config.protected_names,
        1,
    );
    if !leaks.is_empty() {
        passed = false;
    }
    Ok(DetectChapterOutcome {
        chapter_number,
        detection,
        passed,
        leaks,
    })
}

/// `detectAndRewrite`：detect → reviser（anti-detect 模式）→ re-detect 循环，
/// 过阈值或耗尽 max_retries 停；每轮动作落 detection_history.json。
#[allow(clippy::too_many_arguments)]
pub async fn detect_and_rewrite(
    client: &reqwest::Client,
    config: &DetectionConfig,
    chat: &dyn ReviserChat,
    reviser_ctx: &ReviserCtx<'_>,
    book_dir: &Path,
    content: &str,
    chapter_number: u32,
    genre: Option<&str>,
) -> Result<DetectAndRewriteOutcome, String> {
    let api_key = detection_api_key(config);
    let mut current_content = content.to_string();
    let first_detection = detect_ai_content(client, config, &current_content, &api_key, &utc_now_iso())
        .await
        .map_err(|e| e.to_string())?;
    let original_score = first_detection.score;

    if first_detection.score <= config.threshold {
        record_history(
            book_dir,
            &HistoryEntry {
                chapter_number,
                timestamp: first_detection.detected_at.clone(),
                provider: first_detection.provider.clone(),
                score: first_detection.score,
                action: "detect".to_string(),
                attempt: 0,
            },
        )
        .await;
        return Ok(DetectAndRewriteOutcome {
            chapter_number,
            original_score,
            final_score: first_detection.score,
            attempts: 0,
            passed: true,
            final_content: current_content,
        });
    }

    let mut final_score = first_detection.score;
    let mut attempts = 0u32;
    for iteration in 0..config.max_retries {
        attempts = iteration + 1;
        // anti-detect 模式重写（issue 文案 TS 逐字）。
        let issues = vec![AuditIssue {
            severity: AuditSeverity::Warning,
            category: "AIGC检测".to_string(),
            description: format!(
                "AI检测分数 {:.2} 超过阈值 {}",
                final_score, config.threshold
            ),
            suggestion: "降低AI生成痕迹：增加段落长度差异、减少套话、用口语化表达替代书面语".to_string(),
            repair_scope: None,
        }];
        let revise_output = reviser_revise_chapter(
            chat,
            reviser_ctx,
            book_dir,
            &current_content,
            chapter_number,
            &issues,
            ReviseMode::AntiDetect,
            genre,
            &ReviseOptions {
                chapter_intent: None,
                chapter_memo: None,
                chapter_intent_data: None,
                context_package: None,
                rule_stack: None,
                length_spec: None,
            },
        )
        .await
        .map_err(|e| e.to_string())?;
        if revise_output.revised_content.is_empty() {
            break;
        }
        current_content = revise_output.revised_content;

        let re_detection = detect_ai_content(client, config, &current_content, &api_key, &utc_now_iso())
            .await
            .map_err(|e| e.to_string())?;
        final_score = re_detection.score;
        record_history(
            book_dir,
            &HistoryEntry {
                chapter_number,
                timestamp: re_detection.detected_at.clone(),
                provider: re_detection.provider.clone(),
                score: re_detection.score,
                action: "rewrite".to_string(),
                attempt: attempts,
            },
        )
        .await;
        if final_score <= config.threshold {
            break;
        }
    }

    Ok(DetectAndRewriteOutcome {
        chapter_number,
        original_score,
        final_score,
        attempts,
        passed: final_score <= config.threshold,
        final_content: current_content,
    })
}

/// 历史条目（写入形态——与 `DetectionHistoryEntry` 同键，补序列化）。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct HistoryEntry {
    chapter_number: u32,
    timestamp: String,
    provider: String,
    score: f64,
    action: String,
    attempt: u32,
}

/// `recordHistory`：追加一条到 story/detection_history.json（pretty + 无尾换行，
/// TS `JSON.stringify(history, null, 2)` 同形态）。
async fn record_history(book_dir: &Path, entry: &HistoryEntry) {
    let history_path = book_dir.join("story").join("detection_history.json");
    let mut history: Vec<serde_json::Value> = tokio::fs::read_to_string(&history_path)
        .await
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default();
    history.push(serde_json::to_value(entry).unwrap_or(serde_json::Value::Null));
    let _ = tokio::fs::create_dir_all(book_dir.join("story")).await;
    let _ = tokio::fs::write(
        &history_path,
        serde_json::to_string_pretty(&history).unwrap_or_else(|_| "[]".to_string()),
    )
    .await;
}

/// 兼容引用（load 侧由 detection_insights 提供）。
pub use crate::utils::detection_insights::load_detection_history as load_history;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn record_history_appends_and_persists() {
        let dir = tempfile::tempdir().unwrap();
        let book_dir = dir.path();
        record_history(book_dir, &HistoryEntry {
            chapter_number: 3,
            timestamp: "t1".into(),
            provider: "custom".into(),
            score: 0.9,
            action: "rewrite".into(),
            attempt: 1,
        }).await;
        record_history(book_dir, &HistoryEntry {
            chapter_number: 3,
            timestamp: "t2".into(),
            provider: "custom".into(),
            score: 0.4,
            action: "rewrite".into(),
            attempt: 2,
        }).await;
        let loaded = load_history(book_dir).await;
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].action, "rewrite");
        assert_eq!(loaded[1].attempt, 2);
    }
}
