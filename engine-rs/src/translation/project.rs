//! 翻译项目创建（project.ts，970 号 P4 对齐 2.0 存储）：
//! 源提取 → 分章分段 → `works/<id>/source/` 布局 + `works/<id>/work.json`。
//!
//! 970 号：存储从 `translations/<id>/` 迁至 TS 2.0 的 `works/<id>/source/`
//! （workDirectory+work.json，WorkManifest version=2 profileId="translation"）——
//! 桌面「默认 Rust+故障回退 Node」双引擎从此共享同一项目（此前数据源不相交）。
//! work.json 的 artifacts 由写入集构造（TS createCurrentArtifact 同形：id=文件名
//! 去扩展换 '-'、kind=translationArtifactKind 逐字、checksum sha256、current 一枚）。
//! 章节/术语/报告布局与 manifest 字段不变，仅根路径变。

use std::path::Path;

use crate::harness::contracts::{ArtifactManifest, ArtifactRevision, ArtifactRevisionStatus, WorkManifest};
use crate::harness::source_sync::hex_sha256;
use crate::translation::run_store::to_posix_path;
use crate::translation::source::extract_translation_source;
use crate::translation::text::segment_translation_text_vec;
use crate::translation::types::*;

/// TS `translationArtifactKind` 逐字。
fn translation_artifact_kind(work_relative_path: &str) -> &'static str {
    if work_relative_path.ends_with("manifest.json") {
        return "translation-manifest";
    }
    if work_relative_path.ends_with("glossary.json") {
        return "glossary";
    }
    if work_relative_path.ends_with("review-report.md") {
        return "review";
    }
    if work_relative_path.contains("/translated/") {
        return "translation-chapter";
    }
    "source-chapter"
}

struct PendingWrite {
    /// `works/<id>/…`（project 根相对 posix）。
    work_root_path: String,
    /// `source/…`（work 根相对 posix，即 artifact.path）。
    artifact_path: String,
    /// `source/…`（baseDir=source 相对 posix，即 TS artifactId 源串）。
    source_relative: String,
    content: String,
}

impl PendingWrite {
    fn new(work_id: &str, work_root_path: String, content: String) -> Self {
        let prefix = format!("works/{work_id}/source/");
        let source_relative = work_root_path.strip_prefix(&prefix).unwrap_or(&work_root_path).to_string();
        let artifact_path = work_root_path
            .strip_prefix(&format!("works/{work_id}/"))
            .unwrap_or(&work_root_path)
            .to_string();
        Self { work_root_path, artifact_path, source_relative, content }
    }

    /// TS `artifactId`：baseDir 相对路径去扩展、`/`→`-`（manifest/glossary/
    /// review-report/source-chapter-N/translated-chapter-N——全 Work 唯一）。
    fn artifact_id(&self) -> String {
        match self.source_relative.rsplit_once('.') {
            Some((stem, extension)) if !extension.is_empty() => stem.replace('/', "-"),
            _ => self.source_relative.replace('/', "-"),
        }
    }
}

/// TS `createCurrentArtifact` 形（单 current 修订，snapshotPath None）。
fn current_artifact(write: &PendingWrite, now: &str) -> ArtifactManifest {
    let content_type = if write.work_root_path.ends_with(".md") { "text/markdown" } else { "application/json" };
    let bytes = write.content.as_bytes();
    ArtifactManifest {
        id: write.artifact_id(),
        kind: translation_artifact_kind(&write.work_root_path).to_string(),
        current_revision_id: Some("rev-1".into()),
        revisions: vec![ArtifactRevision {
            id: "rev-1".into(),
            parent_revision_id: None,
            path: write.artifact_path.clone(),
            snapshot_path: None,
            content_type: content_type.to_string(),
            status: ArtifactRevisionStatus::Current,
            checksum: format!("sha256:{}", hex_sha256(bytes)),
            byte_length: bytes.len() as u64,
            episode_id: None,
            created_at: now.to_string(),
        }],
        metadata: {
            let mut metadata = serde_json::Map::new();
            metadata.insert("sourcePath".into(), serde_json::json!(write.work_root_path));
            metadata
        },
    }
}

pub async fn create_translation_project_from_file(
    project_root: &Path,
    input: &CreateTranslationProjectInput<'_>,
) -> Result<TranslationProjectCreateResult, String> {
    let source = extract_translation_source(project_root, input).await?;
    let now = crate::utils::utc_time::utc_now_iso();
    let id = format!("{}-{}", now.replace([':', '.'], "-"), slug(&source.title));
    let base_dir_posix = format!("works/{id}/source");
    let base_dir_abs = project_root.join("works").join(&id).join("source");
    let mut writes: Vec<PendingWrite> = Vec::new();

    let mut chapters: Vec<TranslationChapterManifest> = Vec::new();
    for (index, chapter) in source.chapters.iter().enumerate() {
        let number = (index + 1) as u32;
        let file_stem = format!("chapter-{:04}", number);
        let source_chapter_posix = format!("{base_dir_posix}/source/{file_stem}.json");
        let translated_chapter_posix = format!("{base_dir_posix}/translated/{file_stem}.json");
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
        writes.push(PendingWrite::new(
            &id,
            source_chapter_posix.clone(),
            format!("{}\n", serde_json::to_string_pretty(&chapter_file).unwrap_or_default()),
        ));
        writes.push(PendingWrite::new(
            &id,
            translated_chapter_posix.clone(),
            format!(
                "{}\n",
                serde_json::to_string_pretty(&TranslationChapterFile { segments: Vec::new(), ..chapter_file })
                    .unwrap_or_default()
            ),
        ));
        chapters.push(TranslationChapterManifest {
            number,
            title: chapter.title.clone(),
            source_path: source_chapter_posix,
            translated_path: translated_chapter_posix,
            segment_count: segments.len(),
            char_count: chapter.content.chars().count() as u64,
            status: TranslationChapterStatus::Pending,
            translated_segments: 0,
            review_summary: None,
            observations: None,
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
        updated_at: now.clone(),
        source: TranslationSourceManifest {
            kind: source.kind,
            path: source.source_path,
            char_count: source.char_count,
            total_pages: source.total_pages,
        },
        chapters,
    };
    let manifest_path_posix = format!("{base_dir_posix}/manifest.json");
    writes.push(PendingWrite::new(
        &id,
        manifest_path_posix.clone(),
        format!("{}\n", serde_json::to_string_pretty(&manifest).unwrap_or_default()),
    ));
    writes.push(PendingWrite::new(
        &id,
        format!("{base_dir_posix}/glossary.json"),
        "{\n  \"terms\": []\n}\n".to_string(),
    ));
    writes.push(PendingWrite::new(
        &id,
        format!("{base_dir_posix}/review-report.md"),
        "# Translation Review\n\nPending.\n".to_string(),
    ));

    // work.json（TS `createWorkManifest`+artifacts 同形；WorkManifestSchema parse 面
    // 由 contracts validate 承载——version=2/profileId="translation"/language=目标语）。
    let work = WorkManifest {
        version: 2,
        id: id.clone(),
        title: manifest.title.clone(),
        profile_id: "translation".into(),
        language: manifest.target_language.clone(),
        status: crate::harness::contracts::WorkStatus::Active,
        lineage: Vec::new(),
        artifacts: writes.iter().map(|write| current_artifact(write, &now)).collect(),
        metadata: {
            let mut metadata = serde_json::Map::new();
            metadata.insert("sourceLanguage".into(), serde_json::json!(manifest.source_language));
            metadata.insert("targetLanguage".into(), serde_json::json!(manifest.target_language));
            metadata.insert("sourceOrigin".into(), serde_json::json!("file"));
            metadata
        },
        created_at: now.clone(),
        updated_at: now,
    };
    let mut commit: Vec<crate::utils::atomic_file_set::AtomicFileWrite> = writes
        .iter()
        .map(|write| crate::utils::atomic_file_set::AtomicFileWrite {
            relative_path: write.work_root_path.clone(),
            content: crate::utils::atomic_file_set::FileContent::Text(write.content.clone()),
        })
        .collect();
    commit.push(crate::utils::atomic_file_set::AtomicFileWrite {
        relative_path: format!("works/{id}/work.json"),
        content: crate::utils::atomic_file_set::FileContent::Text(format!(
            "{}\n",
            serde_json::to_string_pretty(&work).unwrap_or_default()
        )),
    });
    crate::utils::atomic_file_set::commit_atomic_file_set(&crate::utils::atomic_file_set::AtomicFileSet {
        root_dir: project_root,
        writes: commit,
        deletes: Vec::new(),
    })
    .await
    .map_err(|e| e.to_string())?;

    let manifest_path_abs = base_dir_abs.join("manifest.json");
    Ok(TranslationProjectCreateResult {
        project_dir: to_posix_path(project_root, &base_dir_abs),
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
