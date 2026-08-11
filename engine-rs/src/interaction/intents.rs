//! 交互意图与请求。
//!
//! 移植自 `packages/core/src/interaction/intents.ts`。

use super::modes::AutomationMode;
use serde::{Deserialize, Serialize};
#[cfg(feature = "export-bindings")]
use ts_rs::TS;

/// 交互意图类型（22 种）。对齐 TS `InteractionIntentTypeSchema`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
pub enum InteractionIntentType {
    #[serde(rename = "develop_book")] DevelopBook,
    #[serde(rename = "show_book_draft")] ShowBookDraft,
    #[serde(rename = "create_book")] CreateBook,
    #[serde(rename = "discard_book_draft")] DiscardBookDraft,
    #[serde(rename = "list_books")] ListBooks,
    #[serde(rename = "select_book")] SelectBook,
    #[serde(rename = "continue_book")] ContinueBook,
    #[serde(rename = "write_next")] WriteNext,
    #[serde(rename = "pause_book")] PauseBook,
    #[serde(rename = "resume_book")] ResumeBook,
    #[serde(rename = "revise_chapter")] ReviseChapter,
    #[serde(rename = "rewrite_chapter")] RewriteChapter,
    #[serde(rename = "patch_chapter_text")] PatchChapterText,
    #[serde(rename = "replace_chapter_text")] ReplaceChapterText,
    #[serde(rename = "edit_truth")] EditTruth,
    #[serde(rename = "rename_entity")] RenameEntity,
    #[serde(rename = "update_focus")] UpdateFocus,
    #[serde(rename = "update_author_intent")] UpdateAuthorIntent,
    #[serde(rename = "chat")] Chat,
    #[serde(rename = "explain_status")] ExplainStatus,
    #[serde(rename = "explain_failure")] ExplainFailure,
    #[serde(rename = "export_book")] ExportBook,
}

/// 导出格式。对齐 TS `z.enum(["txt","md","epub"])`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export, type = "\"txt\" | \"md\" | \"epub\""))]
pub enum ExportFormat {
    #[serde(rename = "txt")] Txt,
    #[serde(rename = "md")] Md,
    #[serde(rename = "epub")] Epub,
}

/// 交互请求（intent + 可选上下文字段）。对齐 TS `InteractionRequestSchema`。
/// 所有字段除 intent 外可选；camelCase 对齐 TS JSON。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct InteractionRequest {
    pub intent: Option<InteractionIntentType>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub book_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub chapter_number: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub genre: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub platform: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub chapter_word_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub target_chapters: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub blurb: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub world_premise: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub setting_notes: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub protagonist: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub supporting_cast: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub conflict_core: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub volume_outline: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub constraints: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub author_intent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub current_focus: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub file_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub format: Option<ExportFormat>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub approved_only: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub output_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub old_value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub new_value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub target_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub replacement_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub full_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub instruction: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub mode: Option<AutomationMode>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intent_serializes() {
        assert_eq!(serde_json::to_string(&InteractionIntentType::WriteNext).unwrap(), "\"write_next\"");
        assert_eq!(serde_json::to_string(&InteractionIntentType::PatchChapterText).unwrap(), "\"patch_chapter_text\"");
    }

    #[test]
    fn request_minimal_deserialize() {
        let json = r#"{"intent":"chat"}"#;
        let req: InteractionRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.intent, Some(InteractionIntentType::Chat));
        assert!(req.book_id.is_none());
    }
}
