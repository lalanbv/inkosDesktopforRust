//! 执行状态与交互事件。
//!
//! 移植自 `packages/core/src/interaction/events.ts`。

use serde::{Deserialize, Serialize};
#[cfg(feature = "export-bindings")]
use ts_rs::TS;

/// 执行状态（11 态）。对齐 TS `ExecutionStatusSchema`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(
    feature = "export-bindings",
    ts(export, type = "\"idle\" | \"planning\" | \"composing\" | \"writing\" | \"assessing\" | \"repairing\" | \"persisting\" | \"waiting_human\" | \"blocked\" | \"completed\" | \"failed\"")
)]
pub enum ExecutionStatus {
    #[serde(rename = "idle")] Idle,
    #[serde(rename = "planning")] Planning,
    #[serde(rename = "composing")] Composing,
    #[serde(rename = "writing")] Writing,
    #[serde(rename = "assessing")] Assessing,
    #[serde(rename = "repairing")] Repairing,
    #[serde(rename = "persisting")] Persisting,
    #[serde(rename = "waiting_human")] WaitingHuman,
    #[serde(rename = "blocked")] Blocked,
    #[serde(rename = "completed")] Completed,
    #[serde(rename = "failed")] Failed,
}

/// 是否终态（completed/failed/blocked）。
pub fn is_terminal_execution_status(status: ExecutionStatus) -> bool {
    matches!(status, ExecutionStatus::Completed | ExecutionStatus::Failed | ExecutionStatus::Blocked)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ExecutionState {
    pub status: ExecutionStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub book_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chapter_number: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage_label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct InteractionEvent {
    pub kind: String,
    pub timestamp: u64,
    pub status: ExecutionStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub book_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chapter_number: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_statuses() {
        assert!(is_terminal_execution_status(ExecutionStatus::Completed));
        assert!(is_terminal_execution_status(ExecutionStatus::Failed));
        assert!(is_terminal_execution_status(ExecutionStatus::Blocked));
        assert!(!is_terminal_execution_status(ExecutionStatus::Writing));
        assert!(!is_terminal_execution_status(ExecutionStatus::Idle));
    }

    #[test]
    fn status_serializes() {
        assert_eq!(serde_json::to_string(&ExecutionStatus::WaitingHuman).unwrap(), "\"waiting_human\"");
    }
}
