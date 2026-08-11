//! 运行时状态校验。
//!
//! 移植自 `packages/core/src/state/state-validator.ts`（117 行，纯函数）。
//! 依赖已移植的 [`crate::models::runtime_state`] 类型。

use crate::models::runtime_state::{
    ChapterSummariesState, CurrentStateState, HooksState, StateManifest,
};
use serde::{Deserialize, Serialize};
#[cfg(feature = "export-bindings")]
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct RuntimeStateValidationIssue {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// 校验输入（四部分各自的 JSON 值）。
pub struct ValidationInput {
    pub manifest: serde_json::Value,
    pub current_state: serde_json::Value,
    pub hooks: serde_json::Value,
    pub chapter_summaries: serde_json::Value,
}

/// 校验运行时状态快照：结构反序列化 + 重复 hookId + 重复 summary chapter + currentState 超前 manifest。
/// 任何一部分解析失败记一条 issue（不中断其余校验）。
pub fn validate_runtime_state(input: &ValidationInput) -> Vec<RuntimeStateValidationIssue> {
    let mut issues: Vec<RuntimeStateValidationIssue> = Vec::new();

    let manifest: Option<StateManifest> = parse_or_issue(
        &input.manifest, &mut issues, "invalid_manifest", "manifest");
    let current_state: Option<CurrentStateState> = parse_or_issue(
        &input.current_state, &mut issues, "invalid_current_state", "currentState");
    let hooks: Option<HooksState> = parse_or_issue(
        &input.hooks, &mut issues, "invalid_hooks_state", "hooks");
    let chapter_summaries: Option<ChapterSummariesState> = parse_or_issue(
        &input.chapter_summaries, &mut issues, "invalid_chapter_summaries_state", "chapterSummaries");

    if let Some(hooks) = &hooks {
        let mut seen = std::collections::HashSet::new();
        for hook in &hooks.hooks {
            if !seen.insert(&hook.hook_id) {
                issues.push(RuntimeStateValidationIssue {
                    code: "duplicate_hook_id".into(),
                    message: format!("duplicate hook id: {}", hook.hook_id),
                    path: Some(format!("hooks.{}", hook.hook_id)),
                });
            }
        }
    }

    if let Some(cs) = &chapter_summaries {
        let mut seen = std::collections::HashSet::new();
        for row in &cs.rows {
            if !seen.insert(row.chapter) {
                issues.push(RuntimeStateValidationIssue {
                    code: "duplicate_summary_chapter".into(),
                    message: format!("duplicate summary chapter: {}", row.chapter),
                    path: Some(format!("chapterSummaries.{}", row.chapter)),
                });
            }
        }
    }

    if let (Some(manifest), Some(current)) = (&manifest, &current_state) {
        if current.chapter > manifest.last_applied_chapter {
            issues.push(RuntimeStateValidationIssue {
                code: "current_state_ahead_of_manifest".into(),
                message: format!(
                    "current state chapter {} exceeds manifest {}",
                    current.chapter, manifest.last_applied_chapter
                ),
                path: Some("currentState.chapter".into()),
            });
        }
    }

    issues
}

fn parse_or_issue<T: serde::de::DeserializeOwned>(
    value: &serde_json::Value,
    issues: &mut Vec<RuntimeStateValidationIssue>,
    code: &str,
    path: &str,
) -> Option<T> {
    match serde_json::from_value::<T>(value.clone()) {
        Ok(v) => Some(v),
        Err(e) => {
            issues.push(RuntimeStateValidationIssue {
                code: code.into(),
                message: format!("{e}"),
                path: Some(path.into()),
            });
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn valid_manifest() -> serde_json::Value {
        json!({ "schemaVersion": 2, "language": "zh", "lastAppliedChapter": 5, "projectionVersion": 1, "migrationWarnings": [] })
    }
    fn valid_current() -> serde_json::Value {
        json!({ "chapter": 5, "facts": [] })
    }
    fn valid_hooks() -> serde_json::Value {
        json!({ "hooks": [{ "hookId": "h1", "startChapter": 1, "type": "plot", "status": "open", "lastAdvancedChapter": 3, "expectedPayoff": "soon", "notes": "" }] })
    }
    fn valid_summaries() -> serde_json::Value {
        json!({ "rows": [{ "chapter": 5, "title": "t", "characters": "", "events": "", "stateChanges": "", "hookActivity": "", "mood": "", "chapterType": "" }] })
    }

    #[test]
    fn valid_snapshot_no_issues() {
        let input = ValidationInput {
            manifest: valid_manifest(),
            current_state: valid_current(),
            hooks: valid_hooks(),
            chapter_summaries: valid_summaries(),
        };
        assert!(validate_runtime_state(&input).is_empty());
    }

    #[test]
    fn invalid_manifest_recorded() {
        let input = ValidationInput {
            manifest: json!({ "bad": true }),
            current_state: valid_current(),
            hooks: valid_hooks(),
            chapter_summaries: valid_summaries(),
        };
        let issues = validate_runtime_state(&input);
        assert!(issues.iter().any(|i| i.code == "invalid_manifest"));
    }

    #[test]
    fn duplicate_hook_id() {
        let hooks = json!({ "hooks": [
            { "hookId": "dup", "startChapter": 1, "type": "plot", "status": "open", "lastAdvancedChapter": 1, "expectedPayoff": "", "notes": "" },
            { "hookId": "dup", "startChapter": 2, "type": "plot", "status": "open", "lastAdvancedChapter": 2, "expectedPayoff": "", "notes": "" }
        ] });
        let input = ValidationInput { manifest: valid_manifest(), current_state: valid_current(), hooks, chapter_summaries: valid_summaries() };
        let issues = validate_runtime_state(&input);
        assert!(issues.iter().any(|i| i.code == "duplicate_hook_id"));
    }

    #[test]
    fn duplicate_summary_chapter() {
        let summaries = json!({ "rows": [
            { "chapter": 3, "title": "a", "characters": "", "events": "", "stateChanges": "", "hookActivity": "", "mood": "", "chapterType": "" },
            { "chapter": 3, "title": "b", "characters": "", "events": "", "stateChanges": "", "hookActivity": "", "mood": "", "chapterType": "" }
        ] });
        let input = ValidationInput { manifest: valid_manifest(), current_state: valid_current(), hooks: valid_hooks(), chapter_summaries: summaries };
        let issues = validate_runtime_state(&input);
        assert!(issues.iter().any(|i| i.code == "duplicate_summary_chapter"));
    }

    #[test]
    fn current_ahead_of_manifest() {
        let input = ValidationInput {
            manifest: valid_manifest(), // lastAppliedChapter: 5
            current_state: json!({ "chapter": 10, "facts": [] }), // 10 > 5
            hooks: valid_hooks(),
            chapter_summaries: valid_summaries(),
        };
        let issues = validate_runtime_state(&input);
        assert!(issues.iter().any(|i| i.code == "current_state_ahead_of_manifest"));
    }
}
