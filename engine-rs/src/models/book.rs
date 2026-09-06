//! 书籍配置模型。
//!
//! 移植自 `packages/core/src/models/book.ts`（107 行）。自包含（仅依赖 zod→手动校验）。

use serde::{Deserialize, Serialize};
#[cfg(feature = "export-bindings")]
use ts_rs::TS;

/// 发布平台。对齐 TS `PlatformSchema = z.enum(["tomato","feilu","qidian","other"])`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export, type = "\"tomato\" | \"feilu\" | \"qidian\" | \"other\""))]
pub enum Platform {
    #[serde(rename = "tomato")]
    Tomato,
    #[serde(rename = "feilu")]
    Feilu,
    #[serde(rename = "qidian")]
    Qidian,
    #[serde(rename = "other")]
    Other,
}

impl Platform {
    /// 序列化同值字符串（prompt 插值用，对齐 TS `${book.platform}`）。
    pub fn as_str(self) -> &'static str {
        match self {
            Platform::Tomato => "tomato",
            Platform::Feilu => "feilu",
            Platform::Qidian => "qidian",
            Platform::Other => "other",
        }
    }
}

/// 书籍状态。对齐 TS `BookStatusSchema`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(
    feature = "export-bindings",
    ts(export, type = "\"incubating\" | \"outlining\" | \"active\" | \"paused\" | \"completed\" | \"dropped\"")
)]
pub enum BookStatus {
    #[serde(rename = "incubating")]
    Incubating,
    #[serde(rename = "outlining")]
    Outlining,
    #[serde(rename = "active")]
    Active,
    #[serde(rename = "paused")]
    Paused,
    #[serde(rename = "completed")]
    Completed,
    #[serde(rename = "dropped")]
    Dropped,
}

/// 同人模式。对齐 TS `FanficModeSchema = z.enum(["canon","au","ooc","cp"])`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export, type = "\"canon\" | \"au\" | \"ooc\" | \"cp\""))]
pub enum FanficMode {
    #[serde(rename = "canon")]
    Canon,
    #[serde(rename = "au")]
    Au,
    #[serde(rename = "ooc")]
    Ooc,
    #[serde(rename = "cp")]
    Cp,
}

pub type ChapterReviewMode = ChapterReviewModeVal;
/// 章节审核模式。对齐 TS `type ChapterReviewMode = "auto" | "manual"`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export, type = "\"auto\" | \"manual\""))]
pub enum ChapterReviewModeVal {
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "manual")]
    Manual,
}

pub type RevisionGate = RevisionGateVal;
/// 手动修订门槛。对齐 TS `type RevisionGate = "strict" | "lenient" | "always"`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export, type = "\"strict\" | \"lenient\" | \"always\""))]
pub enum RevisionGateVal {
    #[serde(rename = "strict")]
    Strict,
    #[serde(rename = "lenient")]
    Lenient,
    #[serde(rename = "always")]
    Always,
}

/// 书籍 writing 子配置。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct BookWritingConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_mode: Option<ChapterReviewModeVal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision_gate: Option<RevisionGateVal>,
    /// 189 号：write-next 落盘后自动为本章沉淀时间线节拍（默认关）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_timeline_beats: Option<bool>,
}

/// 系列归属。对齐 TS `BookSeriesSchema`（180 号 C3-a）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct BookSeries {
    pub name: String,
    pub order: u32,
}

/// 书籍配置。对齐 TS `BookConfigSchema`。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct BookConfig {
    pub id: String,
    pub title: String,
    pub platform: Platform,
    pub genre: String,
    pub status: BookStatus,
    #[serde(default = "default_target_chapters")]
    pub target_chapters: u32,
    #[serde(default = "default_chapter_word_count", rename = "chapterWordCount")]
    pub chapter_word_count: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_book_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fanfic_mode: Option<FanficMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub series: Option<BookSeries>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub writing: Option<BookWritingConfig>,
}

fn default_target_chapters() -> u32 {
    200
}
fn default_chapter_word_count() -> u32 {
    3000
}

/// 把平台标识归一化为 [`Platform`]。
///
/// 规则（逐字移植 TS `normalizePlatformId`）：
/// - trim 后为空 → None
/// - compact（小写、去空白/_/-）匹配 tomato/fanqie/fanqienovel，或原文含「番茄」→ Tomato
/// - compact 匹配 qidian/qidianzhongwenwang，或含「起点」→ Qidian
/// - compact 匹配 feilu，或含「飞卢」→ Feilu
/// - compact 匹配 other/others，或含「其他/其它」→ Other
/// - 默认 → Other
pub fn normalize_platform_id(platform: &str) -> Option<Platform> {
    let raw = platform.trim();
    if raw.is_empty() {
        return None;
    }
    let lowered = raw.to_lowercase();
    let compact: String = lowered
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '_' && *c != '-')
        .collect();

    if compact == "tomato" || compact == "fanqie" || compact == "fanqienovel" || raw.contains("番茄") {
        return Some(Platform::Tomato);
    }
    if compact == "qidian" || compact == "qidianzhongwenwang" || raw.contains("起点") {
        return Some(Platform::Qidian);
    }
    if compact == "feilu" || raw.contains("飞卢") {
        return Some(Platform::Feilu);
    }
    if compact == "other" || compact == "others" || raw.contains("其他") || raw.contains("其它") {
        return Some(Platform::Other);
    }
    Some(Platform::Other)
}

/// 同 [`normalize_platform_id`]，None 时回退 [`Platform::Other`]。
pub fn normalize_platform_or_other(platform: &str) -> Platform {
    normalize_platform_id(platform).unwrap_or(Platform::Other)
}

/// 解析生效的章节审核模式：book 级覆盖 project 级，均未设回退 Auto。
pub fn resolve_chapter_review_mode(
    book_writing: Option<&BookWritingConfig>,
    project_review_mode: Option<ChapterReviewModeVal>,
) -> ChapterReviewModeVal {
    book_writing
        .and_then(|w| w.review_mode)
        .or(project_review_mode)
        .unwrap_or(ChapterReviewModeVal::Auto)
}

/// 解析生效的手动修订门槛：book 级覆盖 project 级，均未设回退 Strict。
pub fn resolve_revision_gate(
    book_writing: Option<&BookWritingConfig>,
    project_revision_gate: Option<RevisionGateVal>,
) -> RevisionGateVal {
    book_writing
        .and_then(|w| w.revision_gate)
        .or(project_revision_gate)
        .unwrap_or(RevisionGateVal::Strict)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 180 号 C3-a：series 缺省时序列化完全省略（双端 DTO 等价的兜底形态）；
    /// 给定值 roundtrip 保真。
    #[test]
    fn book_config_series_optional_and_roundtrip() {
        let base = r#"{
            "id":"b1","title":"书","platform":"other","genre":"xianxia","status":"active",
            "targetChapters":10,"chapterWordCount":3000,
            "createdAt":"2026-09-07T00:00:00.000Z","updatedAt":"2026-09-07T00:00:00.000Z"
        }"#;
        let config: BookConfig = serde_json::from_str(base).unwrap();
        assert!(config.series.is_none());
        let json = serde_json::to_value(&config).unwrap();
        assert!(json.get("series").is_none(), "缺省 series 必须省略字段");

        let with_series = r#"{
            "id":"b1","title":"书","platform":"other","genre":"xianxia","status":"active",
            "targetChapters":10,"chapterWordCount":3000,
            "createdAt":"2026-09-07T00:00:00.000Z","updatedAt":"2026-09-07T00:00:00.000Z",
            "series":{"name":"斗气大陆","order":3}
        }"#;
        let config: BookConfig = serde_json::from_str(with_series).unwrap();
        let series = config.series.clone().expect("series 应解析");
        assert_eq!(series.name, "斗气大陆");
        assert_eq!(series.order, 3);
        let round = serde_json::to_value(&config).unwrap();
        assert_eq!(round["series"]["name"], "斗气大陆");
        assert_eq!(round["series"]["order"], 3);
    }

    /// 189 号：writing.autoTimelineBeats 缺省省略 + roundtrip 保真。
    #[test]
    fn writing_config_auto_timeline_beats_optional_and_roundtrip() {
        let bare: BookWritingConfig = serde_json::from_str(r#"{"reviewMode":"auto"}"#).unwrap();
        assert_eq!(bare.auto_timeline_beats, None);
        let json = serde_json::to_value(&bare).unwrap();
        assert!(json.get("autoTimelineBeats").is_none(), "缺省 autoTimelineBeats 必须省略字段");

        let flagged: BookWritingConfig =
            serde_json::from_str(r#"{"reviewMode":"manual","autoTimelineBeats":true}"#).unwrap();
        assert_eq!(flagged.auto_timeline_beats, Some(true));
        let round = serde_json::to_value(&flagged).unwrap();
        assert_eq!(round["autoTimelineBeats"], serde_json::json!(true));
    }

    #[test]
    fn normalize_platform_aliases() {
        assert_eq!(normalize_platform_id("番茄小说"), Some(Platform::Tomato));
        assert_eq!(normalize_platform_id("FanqieNovel"), Some(Platform::Tomato));
        assert_eq!(normalize_platform_id("tomato"), Some(Platform::Tomato));
        assert_eq!(normalize_platform_id("起点中文网"), Some(Platform::Qidian));
        assert_eq!(normalize_platform_id("QIDIAN"), Some(Platform::Qidian));
        assert_eq!(normalize_platform_id("飞卢"), Some(Platform::Feilu));
        assert_eq!(normalize_platform_id("Other"), Some(Platform::Other));
        assert_eq!(normalize_platform_id("其他"), Some(Platform::Other));
        assert_eq!(normalize_platform_id("未知平台"), Some(Platform::Other)); // 默认
        assert_eq!(normalize_platform_id("   "), None);
    }

    #[test]
    fn normalize_compact_strips_separators() {
        // 注意：compact 是精确匹配。"qi dian"→"qidian" 匹配；"tomato-novel"→"tomatonovel" 不匹配 "tomato"
        assert_eq!(normalize_platform_id("qi dian"), Some(Platform::Qidian));
        assert_eq!(normalize_platform_id("tomato-novel"), Some(Platform::Other));
        assert_eq!(normalize_platform_id("fan_qie"), Some(Platform::Tomato)); // compact="fanqie"
    }

    #[test]
    fn resolve_review_mode_precedence() {
        // book 覆盖 project
        assert_eq!(
            resolve_chapter_review_mode(Some(&BookWritingConfig { review_mode: Some(ChapterReviewModeVal::Manual), revision_gate: None, auto_timeline_beats: None }), Some(ChapterReviewModeVal::Auto)),
            ChapterReviewModeVal::Manual
        );
        // book 未设 → project
        assert_eq!(
            resolve_chapter_review_mode(Some(&BookWritingConfig::default()), Some(ChapterReviewModeVal::Manual)),
            ChapterReviewModeVal::Manual
        );
        // 均未设 → auto
        assert_eq!(resolve_chapter_review_mode(None, None), ChapterReviewModeVal::Auto);
    }

    #[test]
    fn resolve_revision_gate_precedence() {
        assert_eq!(
            resolve_revision_gate(Some(&BookWritingConfig { review_mode: None, revision_gate: Some(RevisionGateVal::Always), auto_timeline_beats: None }), Some(RevisionGateVal::Strict)),
            RevisionGateVal::Always
        );
        assert_eq!(resolve_revision_gate(None, None), RevisionGateVal::Strict);
    }
}
