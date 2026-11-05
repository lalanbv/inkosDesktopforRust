//! 短篇提示词构造（889 号 M3c）：`packages/core/src/prompts/short-fiction.ts`
//! 忠实移植——7 个构造函数全部确定性拼接，双端逐字金标准（同输入同字节）。
//!
//! 旧 `crate::prompts::short_fiction`（78 号 tagged 链）已于 895 号随旧 runner
//! 退役删除（对照证据见变更记录 895 号）。

use serde::Serialize;
use serde_json::Value;

use super::short_fiction::ShortFictionMeasurements;

pub type ShortFictionLanguage = WritingLanguageAlias;

/// zh/en 别名（对齐 TS `ShortFictionLanguage`；复用 WritingLanguage 语义）。
pub type WritingLanguageAlias = crate::utils::language::WritingLanguage;

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortFictionReferencePromptInput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// `{workId,artifactId,revisionId,checksum}`（JSON.stringify 进 prompt 的
    /// source 面——BTreeMap 键序无契约语义，提示词面非存储面）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<ShortFictionReferenceSource>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortFictionReferenceSource {
    pub work_id: String,
    pub artifact_id: String,
    pub revision_id: String,
    pub checksum: String,
}

pub struct OutlinePromptInput<'a> {
    pub title: Option<&'a str>,
    pub direction: &'a str,
    pub chapter_count: u32,
    pub chars_per_chapter: u32,
    pub reference: Option<&'a ShortFictionReferencePromptInput>,
}

pub struct DraftPromptInput<'a> {
    pub title: Option<&'a str>,
    pub opening_hook_chars: Option<u32>,
    pub direction: &'a str,
    pub outline_markdown: &'a str,
    pub chapter_count: u32,
    pub chars_per_chapter: u32,
    pub max_chapter_length: Option<u32>,
    pub min_chapter_length: Option<u32>,
    pub chapter_numbers: Option<&'a [u32]>,
    pub previous_draft_markdown: Option<&'a str>,
}

pub struct DraftReviewPromptInput<'a> {
    pub base: DraftPromptInput<'a>,
    pub draft_markdown: &'a str,
    pub reference: Option<&'a ShortFictionReferencePromptInput>,
    pub revision_request: Option<&'a str>,
    pub review_scope: Option<&'a str>,
    pub measurements: Option<&'a ShortFictionMeasurements>,
    /// TS `comparison?: unknown`——宿主核验对照面原样 JSON。
    pub comparison: Option<&'a serde_json::Value>,
}

pub struct PackagePromptInput<'a> {
    pub direction: &'a str,
    pub draft_markdown: &'a str,
    pub draft_title: &'a str,
}

/// TS `buildShortFictionOutlineSystemPrompt`。
pub fn build_short_fiction_outline_system_prompt(language: WritingLanguageAlias) -> String {
    if language == crate::utils::language::WritingLanguage::En {
        "Create the complete short-story plan using the activated short-writing Skill and only the supplied material. Submit it through the outline tool.".into()
    } else {
        "按已激活的短篇写作 Skill 和用户材料生成完整短篇方案，通过方案工具提交。".into()
    }
}

/// TS `buildShortFictionOutlineUserPrompt`。
pub fn build_short_fiction_outline_user_prompt(
    input: &OutlinePromptInput<'_>,
    language: WritingLanguageAlias,
) -> String {
    let en = language == crate::utils::language::WritingLanguage::En;
    let reference = input.reference.and_then(|reference| reference.text.as_deref()).map(str::trim).filter(|text| !text.is_empty());
    let mut parts: Vec<String> = vec![
        (if en { "## Creative Direction" } else { "## 创作方向" }).into(),
        input.direction.to_string(),
    ];
    if let Some(title) = input.title {
        parts.push(if en {
            format!("Confirmed title: {title}. Preserve it exactly.")
        } else {
            format!("已确定书名：{title}。书名保持原样。")
        });
    }
    parts.push(String::new());
    parts.push((if en { "## Target" } else { "## 目标" }).into());
    parts.push(if en {
        format!("{} chapters; about {} words per chapter.", input.chapter_count, input.chars_per_chapter)
    } else {
        format!("{} 章；每章约 {} 字。", input.chapter_count, input.chars_per_chapter)
    });
    if let (Some(reference_text), Some(reference)) = (reference, input.reference) {
        parts.push(String::new());
        parts.push(
            (if en { "## Reference material (source data)" } else { "## 参考材料（来源数据）" }).into(),
        );
        if let Some(source) = &reference.source {
            parts.push(serde_json::to_string(source).unwrap_or_default());
        }
        parts.push(reference_text.to_string());
    }
    if let Some(notes) = input.reference.and_then(|reference| reference.notes.as_deref()).map(str::trim).filter(|notes| !notes.is_empty()) {
        parts.push(String::new());
        parts.push((if en { "## Reference instructions" } else { "## 参考使用要求" }).into());
        parts.push(notes.to_string());
    }
    parts.join("\n")
}

/// TS `buildShortFictionWriterSystemPrompt`。
pub fn build_short_fiction_writer_system_prompt(language: WritingLanguageAlias) -> String {
    if language == crate::utils::language::WritingLanguage::En {
        "Write exactly the requested chapter batch using the activated short-writing Skill and the full plan. Submit complete chapters through the draft tool.".into()
    } else {
        "按已激活的短篇写作 Skill 和完整方案写本批指定章节，通过初稿工具提交完整章节。".into()
    }
}

/// TS `requestedShortFictionChapters`。
fn requested_short_fiction_chapters(
    chapter_numbers: Option<&[u32]>,
    chapter_count: u32,
) -> Vec<u32> {
    let requested: Option<Vec<u32>> = chapter_numbers.map(|numbers| {
        let unique: std::collections::BTreeSet<u32> = numbers
            .iter()
            .copied()
            .filter(|number| (1..=chapter_count).contains(number))
            .collect();
        // BTreeSet 迭代即升序（TS Set 去重后 sort 升序同构）。
        unique.into_iter().collect()
    });
    match requested {
        Some(list) if !list.is_empty() => list,
        _ => (1..=chapter_count).collect(),
    }
}

/// TS `buildShortFictionWriterUserPrompt`。
pub fn build_short_fiction_writer_user_prompt(
    input: &DraftPromptInput<'_>,
    language: WritingLanguageAlias,
) -> String {
    let en = language == crate::utils::language::WritingLanguage::En;
    let chapters = requested_short_fiction_chapters(input.chapter_numbers, input.chapter_count);
    let chapter_list = chapters
        .iter()
        .map(|number| number.to_string())
        .collect::<Vec<_>>()
        .join(if en { ", " } else { "、" });
    let previous = input.previous_draft_markdown.map(str::trim).filter(|text| !text.is_empty());
    let mut parts: Vec<String> = vec![
        (if en { "## Task" } else { "## 任务" }).into(),
        if en {
            format!(
                "Write only chapters {chapter_list} of {}; about {} words each.",
                input.chapter_count, input.chars_per_chapter
            )
        } else {
            format!(
                "只写第 {chapter_list} 章；全篇 {} 章，每章约 {} 字。",
                input.chapter_count, input.chars_per_chapter
            )
        },
    ];
    if let Some(title) = input.title {
        parts.push(if en {
            format!("Confirmed title: {title}. Preserve it exactly.")
        } else {
            format!("已确定书名：{title}。书名保持原样。")
        });
    }
    if let Some(hook_chars) = input.opening_hook_chars {
        if chapters.contains(&1) {
            parts.push(if en {
                format!("Also submit an independent opening scene of about {hook_chars} words before chapter one. Keep the complete first chapter separate.")
            } else {
                format!("另交约 {hook_chars} 字的正文前独立开篇场面，放在 openingHook 字段；第一章仍须完整，不能用开篇钩子代替。")
            });
        }
    }
    if let Some(max) = input.max_chapter_length {
        parts.push(if en {
            format!("Maximum per chapter: {max} words.")
        } else {
            format!("每章正文最多 {max} 个非空白字符（含标点）。")
        });
    }
    if let Some(min) = input.min_chapter_length {
        parts.push(if en {
            format!("Minimum per chapter: {min} words.")
        } else {
            format!("每章正文至少 {min} 个非空白字符（含标点）。")
        });
    }
    parts.push(String::new());
    parts.push((if en { "## Direction" } else { "## 创作方向" }).into());
    parts.push(input.direction.to_string());
    parts.push(String::new());
    parts.push((if en { "## Plan" } else { "## 故事方案" }).into());
    parts.push(input.outline_markdown.to_string());
    if let Some(previous) = previous {
        parts.push(String::new());
        parts.push((if en { "## Persisted previous chapters" } else { "## 已落盘前文章节" }).into());
        parts.push(previous.to_string());
    }
    parts.join("\n")
}

/// TS `buildShortFictionDraftReviewSystemPrompt`。
pub fn build_short_fiction_draft_review_system_prompt(language: WritingLanguageAlias) -> String {
    if language == crate::utils::language::WritingLanguage::En {
        "Review the persisted draft with the activated short-writing Skill. Attribute every finding to its sourceId and select a short inclusive startLine/endLine range from the numbered source; the host copies the exact excerpt. Distinguish outline differences from manuscript contradictions. Submit evidence-backed observations and a concise summary through the review tool. An empty observations array is valid.".into()
    } else {
        "按已激活的短篇写作 Skill 审查已落盘成稿。每项观察指定 sourceId 和该编号来源内一小段连续的 startLine/endLine（含首尾行），系统按行号截取原文。区分大纲差异与正文内部矛盾。通过审稿工具提交有证据的观察和简短总结；observations 为空是合法结果。".into()
    }
}

/// TS `buildShortFictionDraftReviewUserPrompt`。
#[allow(clippy::too_many_lines)]
pub fn build_short_fiction_draft_review_user_prompt(
    input: &DraftReviewPromptInput<'_>,
    language: WritingLanguageAlias,
) -> String {
    let en = language == crate::utils::language::WritingLanguage::En;
    let base = &input.base;
    let mut parts: Vec<String> = vec![
        (if en { "## Direction" } else { "## 创作方向" }).into(),
        base.direction.to_string(),
        String::new(),
        (if en {
            "## Source: outline (plan, not manuscript)"
        } else {
            "## Source: outline（大纲）"
        })
        .into(),
        base.outline_markdown.to_string(),
    ];
    let reference_text = input
        .reference
        .and_then(|reference| reference.text.as_deref())
        .map(str::trim)
        .filter(|text| !text.is_empty());
    if let (Some(reference_text), Some(reference)) = (reference_text, input.reference) {
        parts.push(String::new());
        parts.push("## Source: reference-material".into());
        parts.push(
            serde_json::to_string(&reference.source).unwrap_or_else(|_| "null".into()),
        );
        parts.push(
            if en {
                "These are the bound original samples and their deconstructions. Compare craft against their actual passages; sample facts are not facts of the new story. Cite this source for claims about the samples. Missing evidence remains unavailable."
            } else {
                "以下为作品绑定的原始样本及拆稿。对照实际段落判断写法；样本事实不是新故事事实。关于样本的判断须引用此来源，缺少的依据如实留白。"
            }
            .into(),
        );
        parts.push(reference_text.to_string());
    }
    parts.push(String::new());
    parts.push((if en { "## Review scope" } else { "## 审查范围" }).into());
    parts.push(input.review_scope.unwrap_or("whole-story").to_string());
    if let Some(revision_request) = input.revision_request {
        parts.push(String::new());
        parts.push((if en { "## Latest revision request" } else { "## 最近修改请求" }).into());
        parts.push(revision_request.to_string());
    }
    parts.push(String::new());
    parts.push((if en { "## Requested manuscript constraints" } else { "## 成稿约束" }).into());
    parts.push(
        serde_json::to_string(&serde_json::json!({
            "chapterCount": base.chapter_count,
            "targetChapterLength": base.chars_per_chapter,
            "minChapterLength": base.min_chapter_length,
            "maxChapterLength": base.max_chapter_length,
            "openingHookTarget": base.opening_hook_chars,
            "title": base.title,
            "unit": input.measurements.map(|measurements| measurements.unit),
        }))
        .unwrap_or_default(),
    );
    if let Some(comparison) = input.comparison {
        parts.push(String::new());
        parts.push((if en { "## Verified revision comparison" } else { "## 已核验的修订对照" }).into());
        parts.push(serde_json::to_string(comparison).unwrap_or_default());
        parts.push(
            if en {
                "Compare the requested scope with these verified changes. Baseline sources supply the earlier text for changed chapters. Unchanged chapters were compared in full; do not infer a change from a missing duplicate baseline source. Cite both baseline and current sources for a claimed unauthorized change."
            } else {
                "按请求范围核验这些已确认的变化。baseline 来源提供发生变化章节的旧文；标为未变的章节已经过全文比较，不要因未重复提供旧文而推断其发生变化。判断越界改动时同时引用旧版与当前来源。"
            }
            .into(),
        );
    }
    if let Some(measurements) = input.measurements {
        parts.push(String::new());
        parts.push(
            (if en {
                "## Host-verified manuscript measurements"
            } else {
                "## 宿主核验的成稿计量"
            })
            .into(),
        );
        // TS `JSON.stringify({contentScope, chapterLengthScope, ...input.measurements})`
        // ——measurements 字段**平铺**进对象（spread 语义）。
        let measurements_value =
            serde_json::to_value(measurements).unwrap_or(serde_json::Value::Null);
        let mut flat = serde_json::Map::new();
        flat.insert("contentScope".into(), Value::String("complete_manuscript".into()));
        flat.insert(
            "chapterLengthScope".into(),
            Value::String("prose_excluding_chapter_headings".into()),
        );
        if let Value::Object(entries) = measurements_value {
            for (key, item) in entries {
                flat.insert(key, item);
            }
        }
        parts.push(serde_json::to_string(&Value::Object(flat)).unwrap_or_default());
        parts.push(
            if en {
                "Use these measured lengths. Numbered source lines below contain the complete supplied manuscript; line numbering does not mean an excerpt."
            } else {
                "篇幅判断采用以上实测值。以下编号来源包含所提供的完整正文；编号用于引用，不表示节选。"
            }
            .into(),
        );
    }
    parts.push(String::new());
    parts.push((if en { "## Draft under review" } else { "## 待审正文" }).into());
    parts.push(input.draft_markdown.to_string());
    parts.join("\n")
}

/// TS `buildShortFictionPackageSystemPrompt`。
pub fn build_short_fiction_package_system_prompt(language: WritingLanguageAlias) -> String {
    if language == crate::utils::language::WritingLanguage::En {
        "Package the persisted draft using the activated short-writing Skill. Preserve its actual title and plot, then submit through the package tool.".into()
    } else {
        "按已激活的短篇写作 Skill 包装已落盘正文，保留实际标题与剧情，通过包装工具提交。".into()
    }
}

/// TS `buildShortFictionPackageUserPrompt`。
pub fn build_short_fiction_package_user_prompt(
    input: &PackagePromptInput<'_>,
    language: WritingLanguageAlias,
) -> String {
    let en = language == crate::utils::language::WritingLanguage::En;
    [
        (if en { "## Direction" } else { "## 创作方向" }).to_string(),
        input.direction.to_string(),
        String::new(),
        (if en { "## Persisted draft" } else { "## 已落盘正文" }).to_string(),
        input.draft_markdown.trim().to_string(),
        String::new(),
        (if en { "## Existing title" } else { "## 当前标题" }).to_string(),
        input.draft_title.to_string(),
    ]
    .join("\n")
}
