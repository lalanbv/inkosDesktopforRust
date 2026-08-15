//! 翻译项目创建（project.ts）：源提取 → 分章分段 → 项目目录落盘。

use std::path::Path;

use crate::translation::run_store::to_posix_path;
use crate::translation::source::extract_translation_source;
use crate::translation::text::segment_translation_text_vec;
use crate::translation::types::*;

pub async fn create_translation_project_from_file(
    project_root: &Path,
    input: &CreateTranslationProjectInput<'_>,
) -> Result<TranslationProjectCreateResult, String> {
    let source = extract_translation_source(project_root, input).await?;
    let now = crate::utils::utc_time::utc_now_iso();
    let id = format!("{}-{}", now.replace([':', '.'], "-"), slug(&source.title));
    let project_dir_abs = project_root.join("translations").join(&id);
    let source_dir_abs = project_dir_abs.join("source");
    let translated_dir_abs = project_dir_abs.join("translated");
    tokio::fs::create_dir_all(&source_dir_abs).await.map_err(|e| e.to_string())?;
    tokio::fs::create_dir_all(&translated_dir_abs).await.map_err(|e| e.to_string())?;

    let mut chapters: Vec<TranslationChapterManifest> = Vec::new();
    for (index, chapter) in source.chapters.iter().enumerate() {
        let number = (index + 1) as u32;
        let file_stem = format!("chapter-{:04}", number);
        let source_chapter_abs = source_dir_abs.join(format!("{file_stem}.json"));
        let translated_chapter_abs = translated_dir_abs.join(format!("{file_stem}.json"));
        // TS 默认 1200 字/段。
        let segments: Vec<TranslationSegment> = segment_translation_text_vec(
            &chapter.content,
            input.segment_max_chars.unwrap_or(1200),
        )
        .into_iter()
        .enumerate()
        .map(|(segment_index, source)| TranslationSegment {
            index: (segment_index + 1) as u32,
            source,
            target: None,
            notes: None,
        })
        .collect();
        let chapter_file = TranslationChapterFile {
            number,
            title: chapter.title.clone(),
            source_language: input.source_language.to_string(),
            target_language: input.target_language.to_string(),
            segments: segments.clone(),
        };
        let payload = format!("{}\n", serde_json::to_string_pretty(&chapter_file).unwrap_or_default());
        tokio::fs::write(&source_chapter_abs, &payload).await.map_err(|e| e.to_string())?;
        let translated_empty = TranslationChapterFile { segments: Vec::new(), ..chapter_file };
        tokio::fs::write(
            &translated_chapter_abs,
            format!("{}\n", serde_json::to_string_pretty(&translated_empty).unwrap_or_default()),
        )
        .await
        .map_err(|e| e.to_string())?;
        chapters.push(TranslationChapterManifest {
            number,
            title: chapter.title.clone(),
            source_path: to_posix_path(project_root, &source_chapter_abs),
            translated_path: to_posix_path(project_root, &translated_chapter_abs),
            segment_count: segments.len(),
            char_count: chapter.content.chars().count() as u64,
            status: TranslationChapterStatus::Pending,
        });
    }

    let manifest = TranslationProjectManifest {
        id: id.clone(),
        title: input
            .title
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(String::from)
            .unwrap_or_else(|| source.title.clone()),
        source_language: input.source_language.to_string(),
        target_language: input.target_language.to_string(),
        created_at: now.clone(),
        updated_at: now,
        source: TranslationSourceManifest {
            kind: source.kind,
            path: source.source_path,
            char_count: source.char_count,
            total_pages: source.total_pages,
        },
        chapters,
    };
    let manifest_path_abs = project_dir_abs.join("manifest.json");
    tokio::fs::write(
        &manifest_path_abs,
        format!("{}\n", serde_json::to_string_pretty(&manifest).unwrap_or_default()),
    )
    .await
    .map_err(|e| e.to_string())?;
    tokio::fs::write(
        project_dir_abs.join("glossary.json"),
        "{\n  \"terms\": []\n}\n",
    )
    .await
    .map_err(|e| e.to_string())?;
    tokio::fs::write(
        project_dir_abs.join("review-report.md"),
        "# Translation Review\n\nPending.\n",
    )
    .await
    .map_err(|e| e.to_string())?;

    Ok(TranslationProjectCreateResult {
        project_dir: to_posix_path(project_root, &project_dir_abs),
        manifest_path: to_posix_path(project_root, &manifest_path_abs),
        manifest,
    })
}

/// 测试暴露面。
pub mod tests_support {
    pub fn slug_pub(value: &str) -> String {
        super::slug(value)
    }
}

/// `slug`：lower + 非 Unicode 字母数字折叠 '-' + 去首尾 + 80 上限。
fn slug(value: &str) -> String {
    let lowered = value.trim().to_lowercase();
    let mut folded = String::with_capacity(lowered.len());
    let mut prev_dash = false;
    for ch in lowered.chars() {
        let keep = ch.is_alphanumeric();
        if keep {
            folded.push(ch);
            prev_dash = false;
        } else if !prev_dash {
            folded.push('-');
            prev_dash = true;
        }
    }
    let trimmed = folded.trim_matches('-').to_string();
    let mut out = String::new();
    let mut units = 0usize;
    for ch in trimmed.chars() {
        let ch_units = ch.len_utf16();
        if units + ch_units > 80 {
            break;
        }
        out.push(ch);
        units += ch_units;
    }
    if out.is_empty() {
        "translation".to_string()
    } else {
        out
    }
}
