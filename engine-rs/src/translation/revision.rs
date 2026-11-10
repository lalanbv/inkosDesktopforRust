//! 段落修订（revision.ts，971 号 P5）：选段 → 端口 reviseSegment → 变更才落盘
//! （sync_work_source_artifacts accept 同事务：章译文+manifest 清审校面）→ 回执哈希
//! 与摘要面（TS 逐字段）。
//!
//! TS 语义逐条：paragraph=`last` 或 1 基人类段号（非 0 基数组下标）；审校面
//! （reviewSummary/observations）随修订清空（TS 以 undefined 覆写）；未变更不写盘
//! 不清审校。回执 hash=sha256（"sha256:" 前缀），excerpt=前 400 字。

use std::path::Path;

use serde_json::json;

use crate::translation::run_store::*;
use crate::translation::types::*;

/// TS `reviseTranslationSegment`。`paragraph`：None=`last`，Some(n)=1 基段号。
pub async fn revise_translation_segment(
    project_root: &Path,
    project_id: &str,
    chapter_number: u32,
    paragraph: Option<u32>,
    instruction: &str,
    model: &dyn TranslationModelPort,
) -> Result<serde_json::Value, String> {
    if let Some(number) = paragraph {
        if number == 0 {
            return Err(coded("TRANSLATION_PARAGRAPH_REQUIRED", "Select last or a paragraph number starting at 1"));
        }
    }
    let manifest = load_translation_manifest(project_root, project_id)
        .await
        .map_err(|e| match e {
            LoadManifestError::NotFound => coded("TRANSLATION_CHAPTER_NOT_FOUND", "Translation chapter not found"),
            LoadManifestError::BadPayload(message) => coded("TRANSLATION_MANIFEST_INVALID", &message),
        })?;
    let Some(info) = manifest.chapters.iter().find(|chapter| chapter.number == chapter_number) else {
        return Err(coded("TRANSLATION_CHAPTER_NOT_FOUND", "Translation chapter not found"));
    };
    let source = load_translation_chapter(project_root, &info.source_path).await?;
    let chapter = load_translation_chapter(project_root, &info.translated_path).await?;
    let paragraph_number = paragraph.unwrap_or(source.segments.len() as u32);
    let Some(source_segment) = source.segments.get((paragraph_number as usize).checked_sub(1).unwrap_or(usize::MAX)) else {
        return Err(coded("TRANSLATION_SEGMENT_NOT_FOUND", "Translation source paragraph not found"));
    };
    let Some(current) = chapter.segments.iter().find(|segment| segment.index == source_segment.index) else {
        return Err(coded("TRANSLATION_SEGMENT_NOT_FOUND", "Translation source paragraph not found"));
    };
    let Some(current_target) = current.target.as_deref().map(str::trim).filter(|t| !t.is_empty()) else {
        return Err(coded("TRANSLATION_TARGET_MISSING", "Translate the target paragraph before revising it"));
    };
    let position = source.segments.iter().position(|segment| segment.index == source_segment.index).unwrap_or_default();
    let neighbors: Vec<TranslationSegment> = source
        .segments
        .get(position.saturating_sub(1)..=(position + 1).min(source.segments.len().saturating_sub(1)))
        .map(|range| {
            range
                .iter()
                .filter(|segment| segment.index != source_segment.index)
                .map(|segment| TranslationSegment {
                    target: chapter.segments.iter().find(|c| c.index == segment.index).and_then(|c| c.target.clone()),
                    ..segment.clone()
                })
                .collect()
        })
        .unwrap_or_default();
    let glossary = load_translation_glossary(project_root, project_id).await;
    let current_target = current_target.to_string();
    let outcome = model
        .revise_segment(ReviseSegmentInput {
            source_language: &manifest.source_language,
            target_language: &manifest.target_language,
            chapter_title: &chapter.title,
            segment: &TranslationSegment { target: Some(current_target.clone()), ..source_segment.clone() },
            neighbors: &neighbors,
            glossary: &glossary,
            instruction,
        })
        .await?;
    if outcome.target.trim().is_empty() {
        return Err(coded("TRANSLATION_TARGET_EMPTY", "A revised paragraph cannot be empty"));
    }
    let next_target = outcome.target.trim().to_string();
    let changed = next_target != current_target;
    if changed {
        let next_chapter = TranslationChapterFile {
            segments: chapter
                .segments
                .iter()
                .map(|segment| {
                    if segment.index == current.index {
                        TranslationSegment { target: Some(next_target.clone()), ..segment.clone() }
                    } else {
                        segment.clone()
                    }
                })
                .collect(),
            ..chapter.clone()
        };
        let mut next_manifest = manifest.clone();
        next_manifest.updated_at = crate::utils::utc_time::utc_now_iso();
        for chapter_info in &mut next_manifest.chapters {
            if chapter_info.number == info.number {
                // TS 以 undefined 覆写（969 号 serde skip 形同）。
                chapter_info.review_summary = None;
                chapter_info.observations = None;
            }
        }
        // 971 号：sync_work_source_artifacts accept（工件修订账随写生效）。run 阶段已在
        // 结尾按 changed-paths 对过账（与 TS runner 同款），此处 checksum 与磁盘一致，
        // 无 legacy 取证窗口。
        crate::harness::source_sync::sync_work_source_artifacts(crate::harness::source_sync::SyncWorkSourceInput {
            project_root,
            work_id: project_id,
            accept: true,
            accept_paths: None,
            episode_id: None,
            updated_at: Some(next_manifest.updated_at.clone()),
            writes: vec![
                crate::utils::atomic_file_set::AtomicFileWrite {
                    relative_path: info.translated_path.clone(),
                    content: crate::utils::atomic_file_set::FileContent::Text(format!(
                        "{}\n",
                        serde_json::to_string_pretty(&next_chapter).unwrap_or_default()
                    )),
                },
                crate::utils::atomic_file_set::AtomicFileWrite {
                    relative_path: format!("works/{project_id}/source/manifest.json"),
                    content: crate::utils::atomic_file_set::FileContent::Text(format!(
                        "{}\n",
                        serde_json::to_string_pretty(&next_manifest).unwrap_or_default()
                    )),
                },
            ],
            title: None,
            status: None,
        })
        .await
        .map_err(|error| error.to_string())?;
    }
    let excerpt = |text: &str| text.chars().take(400).collect::<String>();
    let _ = excerpt; // 占位避免未用告警（excerpt 用于 JSON 组装下方）
    Ok(json!({
        "projectId": project_id,
        "chapterNumber": info.number,
        "paragraphNumber": paragraph_number,
        "paragraphCount": source.segments.len(),
        "sourceSegmentIndex": current.index,
        "isLastParagraph": paragraph_number == source.segments.len() as u32,
        "changed": changed,
        "previousTargetHash": sha256_tagged(&current_target),
        "targetHash": sha256_tagged(&next_target),
        "sourcePreserved": true,
        "otherSegmentsPreserved": true,
        "sourceExcerpt": excerpt(&source_segment.source),
        "previousTargetExcerpt": excerpt(&current_target),
        "targetExcerpt": excerpt(&next_target),
        "reviewRequired": changed,
        "exportRequired": changed,
    }))
}

fn coded(code: &str, message: &str) -> String {
    format!("{code}: {message}")
}

fn sha256_tagged(text: &str) -> String {
    format!("sha256:{}", crate::harness::source_sync::hex_sha256(text.as_bytes()))
}
