//! 翻译域类型（types.ts 逐字段；全 camelCase——磁盘形态对齐 TS）。

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TranslationSourceKind {
    Text,
    Markdown,
    Pdf,
    Epub,
}

impl TranslationSourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Markdown => "markdown",
            Self::Pdf => "pdf",
            Self::Epub => "epub",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TranslationExportFormat {
    Txt,
    Md,
    Epub,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TranslationChapterStatus {
    Pending,
    Translated,
    Reviewed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationSourceManifest {
    pub kind: TranslationSourceKind,
    pub path: String,
    pub char_count: u64,
    /// UTF-16 码元计数（JS .length 语义）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_pages: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationChapterManifest {
    pub number: u32,
    pub title: String,
    pub source_path: String,
    pub translated_path: String,
    pub segment_count: usize,
    pub char_count: u64,
    pub status: TranslationChapterStatus,
    /// 967 号：TS 2.0 manifest 章节三字段（translatedSegments 必有；reviewSummary/
    /// observations 可选，形同 TS ObservationSchema）——serde default 读旧 manifest，
    /// 随运行落盘。TranslationManager 读 `manifest.chapters[].translatedSegments`。
    #[serde(default)]
    pub translated_segments: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observations: Option<Vec<Value>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationProjectManifest {
    pub id: String,
    pub title: String,
    pub source_language: String,
    pub target_language: String,
    pub created_at: String,
    pub updated_at: String,
    pub source: TranslationSourceManifest,
    pub chapters: Vec<TranslationChapterManifest>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationSegment {
    pub index: u32,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationChapterFile {
    pub number: u32,
    pub title: String,
    pub source_language: String,
    pub target_language: String,
    pub segments: Vec<TranslationSegment>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationGlossaryTerm {
    pub source: String,
    pub target: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// 翻译模型口（LLM 或测试注入）。
#[async_trait::async_trait]
pub trait TranslationModelPort: Send + Sync {
    async fn translate_segments(
        &self,
        input: TranslateSegmentsInput<'_>,
    ) -> Result<TranslateSegmentsOutput, String>;

    async fn review_chapter(
        &self,
        _input: ReviewChapterInput<'_>,
    ) -> Result<ReviewChapterOutput, String> {
        Ok(ReviewChapterOutput {
            passed: true,
            summary: "Translation review completed.".to_string(),
            issues: Vec::new(),
        })
    }
}

pub struct TranslateSegmentsInput<'a> {
    pub source_language: &'a str,
    pub target_language: &'a str,
    pub chapter_title: &'a str,
    pub segments: &'a [TranslationSegment],
    pub glossary: &'a [TranslationGlossaryTerm],
}

#[derive(Debug, Clone, Default)]
pub struct TranslateSegmentsOutput {
    pub segments: Vec<TranslatedSegmentItem>,
    pub glossary: Vec<TranslationGlossaryTerm>,
}

#[derive(Debug, Clone)]
pub struct TranslatedSegmentItem {
    pub index: u32,
    pub target: String,
    pub notes: Option<String>,
}

pub struct ReviewChapterInput<'a> {
    pub source_language: &'a str,
    pub target_language: &'a str,
    pub chapter_title: &'a str,
    pub segments: &'a [TranslationSegment],
    pub glossary: &'a [TranslationGlossaryTerm],
}

pub struct ReviewChapterOutput {
    pub passed: bool,
    pub summary: String,
    pub issues: Vec<String>,
}

/// 提取出的源（extractTranslationSource 产物）。
pub struct ExtractedTranslationSource {
    pub title: String,
    pub kind: TranslationSourceKind,
    /// 项目根相对 posix 路径。
    pub source_path: String,
    pub char_count: u64,
    pub total_pages: Option<u32>,
    pub chapters: Vec<crate::translation::text::TranslationTextChapter>,
}

/// 创建项目入参。
pub struct CreateTranslationProjectInput<'a> {
    pub file_path: &'a str,
    pub source_language: &'a str,
    pub target_language: &'a str,
    pub title: Option<&'a str>,
    pub segment_max_chars: Option<usize>,
}

/// 创建项目产物。
pub struct TranslationProjectCreateResult {
    pub project_dir: String,
    pub manifest_path: String,
    pub manifest: TranslationProjectManifest,
}

/// run 产物（RunTranslationProjectResult）。
///
/// 967 号：补 TS 2.0 回执形（harness/tools/translation.ts：kind/workId/total·completed·
/// pendingSegments/observations）——`reviewSourcePaths` 属审校 2.0（P3）不在此期。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunTranslationProjectResult {
    pub kind: &'static str,
    pub work_id: String,
    pub project_id: String,
    pub translated_segments: usize,
    pub reviewed_chapters: usize,
    pub report_path: String,
    pub total_segments: usize,
    pub completed_segments: usize,
    pub pending_segments: usize,
    pub observations: Vec<Value>,
}

/// 967 号：审校问题 → TS ObservationSchema 形（strict 键集内：code/summary/evidence/
/// category/assessment）；空白问题不产出（TS summary min(1)）。
pub fn review_issue_observation(issue: &str) -> Option<Value> {
    let summary = issue.trim();
    if summary.is_empty() {
        return None;
    }
    Some(serde_json::json!({
        "code": "translation-review",
        "summary": summary,
        "evidence": [],
        "category": "quality",
        "assessment": "issue",
    }))
}

/// 967 号：章节已译段数（源段在译文中有非空 target 的个数——TS translatedSegments 语义）。
pub fn count_translated_segments(source: &[TranslationSegment], translated: &[TranslationSegment]) -> usize {
    let done: std::collections::HashSet<u32> = translated
        .iter()
        .filter(|segment| segment.target.as_deref().map(str::trim).is_some_and(|t| !t.is_empty()))
        .map(|segment| segment.index)
        .collect();
    source.iter().filter(|segment| done.contains(&segment.index)).count()
}

/// export 产物（TranslationExportResult）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationExportResult {
    pub output_path: String,
    pub format: TranslationExportFormat,
    pub chapters_exported: usize,
}

pub type Json = Value;
