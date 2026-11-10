//! 翻译域类型（types.ts 逐字段；全 camelCase——磁盘形态对齐 TS）。

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::models::observation::{Observation, ObservationAssessment};

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
            summary: "Translation review completed.".to_string(),
            observations: Vec::new(),
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
    /// 968 号：TS 2.0 `chapterTitle`（译后章节标题，trim 非空才有值）。
    pub chapter_title: Option<String>,
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
    /// 969 号：译后标题（TS `chapterTitle`——现译面身份）。
    pub chapter_title: &'a str,
    /// 969 号：原文标题（TS `sourceChapterTitle?`——独立源读 chapterTitle 面）。
    pub source_chapter_title: Option<&'a str>,
    pub segments: &'a [TranslationSegment],
    pub glossary: &'a [TranslationGlossaryTerm],
}

/// 969 号：审校 2.0 输出（TS `{summary, observations}`——passed 由「无 issue 级观察」
/// 派生：passed=observations 无 assessment=issue；968 前的 issues:[String] 面退役）。
pub struct ReviewChapterOutput {
    pub summary: String,
    pub observations: Vec<Observation>,
}

impl ReviewChapterOutput {
    /// TS 章节状态推进等价：有 issue 级发现 = 未通过（章停 translated）。
    pub fn passed(&self) -> bool {
        !self.observations.iter().any(|observation| observation.assessment == Some(ObservationAssessment::Issue))
    }
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
/// pendingSegments/observations）；969 号：+`reviewSourcePaths`（本轮完成有源审校的
/// 章译文路径，TS 同名）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunTranslationProjectResult {
    pub kind: &'static str,
    pub work_id: String,
    pub project_id: String,
    pub translated_segments: usize,
    pub reviewed_chapters: usize,
    pub review_source_paths: Vec<String>,
    pub report_path: String,
    pub total_segments: usize,
    pub completed_segments: usize,
    pub pending_segments: usize,
    /// 967 号：回执 observations=manifest 全章审校观察（TS flatMap 口径——非仅本次运行）。
    pub observations: Vec<Value>,
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
