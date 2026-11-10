//! 翻译项目存储（run-store.ts）：manifest / 章节 / 术语表读写与合并。
//!
//! 970 号 P4：存储根 = `works/<id>/source/`（TS 2.0 `translationProjectDir`）；
//! `translations/<id>/…` 旧布局在读面**一次性自动搬迁**（目录重命名到新根，搬前
//! 不存在 work.json 才搬——纯 Rust 时代产物；迁移失败原样回退旧根读，不阻断）。

use std::path::{Path, PathBuf};

use crate::translation::types::*;

/// TS 2.0 `translationProjectDir`（works/<id>/source）。
pub fn translation_project_dir(project_root: &Path, project_id: &str) -> PathBuf {
    project_root.join("works").join(project_id).join("source")
}

/// 旧布局根（970 号前）。
fn legacy_project_dir(project_root: &Path, project_id: &str) -> PathBuf {
    project_root.join("translations").join(project_id)
}

/// 一次性迁移：旧根在、新 manifest 不在、works/<id> 无 work.json（纯 Rust 时代产物）
/// → 按子目录搬迁（旧 `source/`、`translated/`、散文件 → 新根直下）。任何失败静默
/// 返回 None（调用方回退旧根读，下次调用重试；部分搬迁无碍——已搬文件在新根可见）。
async fn migrate_legacy_dir(project_root: &Path, project_id: &str) -> Option<()> {
    let legacy = legacy_project_dir(project_root, project_id);
    let target = translation_project_dir(project_root, project_id);
    if !tokio::fs::metadata(&legacy).await.ok()?.is_dir() {
        return None;
    }
    if tokio::fs::try_exists(translation_manifest_path(project_root, project_id))
        .await
        .unwrap_or(false)
    {
        return None;
    }
    let work_json = project_root.join("works").join(project_id).join("work.json");
    if tokio::fs::try_exists(&work_json).await.unwrap_or(true) {
        return None;
    }
    tokio::fs::create_dir_all(&target).await.ok()?;
    if let Ok(mut dirents) = tokio::fs::read_dir(&legacy).await {
        while let Ok(Some(entry)) = dirents.next_entry().await {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            // work.json 属 works 域产物——不该出现在旧布局，出现即不迁（保守）。
            if name == "work.json" {
                return None;
            }
            let from = legacy.join(&*name);
            let to = target.join(&*name);
            if tokio::fs::try_exists(&to).await.unwrap_or(false) {
                continue;
            }
            tokio::fs::rename(&from, &to).await.ok()?;
        }
    }
    // 迁移收尾：合成 work.json（TS works/<id> 恒有 work.json——列表扫描
    // `profileId=translation` 过滤依赖它；970 前纯 Rust 项目无此文件）。
    let manifest_raw = tokio::fs::read_to_string(target.join("manifest.json")).await.ok()?;
    let manifest: serde_json::Value = serde_json::from_str(&manifest_raw).ok()?;
    let work = serde_json::json!({
        "version": 2,
        "id": project_id,
        "title": manifest.get("title").cloned().unwrap_or_else(|| serde_json::json!(project_id)),
        "profileId": "translation",
        "language": manifest.get("targetLanguage").cloned().unwrap_or_else(|| serde_json::json!("zh")),
        "status": "active",
        "lineage": [],
        "artifacts": [],
        "metadata": {
            "sourceLanguage": manifest.get("sourceLanguage").cloned().unwrap_or_else(|| serde_json::json!("en")),
            "targetLanguage": manifest.get("targetLanguage").cloned().unwrap_or_else(|| serde_json::json!("zh")),
            "sourceOrigin": "migrated",
        },
        "createdAt": manifest.get("createdAt").cloned().unwrap_or_else(|| serde_json::json!("1970-01-01T00:00:00.000Z")),
        "updatedAt": crate::utils::utc_time::utc_now_iso(),
    });
    crate::utils::atomic_file_set::write_file_atomic(
        &project_root.join("works").join(project_id).join("work.json"),
        &format!("{}\n", serde_json::to_string_pretty(&work).unwrap_or_default()),
    )
    .await
    .ok()?;
    let _ = tokio::fs::remove_dir(&legacy).await; // 空了才删得掉，非空保守保留
    Some(())
}

async fn ensure_layout(project_root: &Path, project_id: &str) -> PathBuf {
    if tokio::fs::try_exists(translation_manifest_path(project_root, project_id))
        .await
        .unwrap_or(false)
    {
        return translation_project_dir(project_root, project_id);
    }
    migrate_legacy_dir(project_root, project_id).await;
    if tokio::fs::try_exists(translation_manifest_path(project_root, project_id))
        .await
        .unwrap_or(false)
    {
        return translation_project_dir(project_root, project_id);
    }
    legacy_project_dir(project_root, project_id)
}

pub fn translation_manifest_path(project_root: &Path, project_id: &str) -> PathBuf {
    translation_project_dir(project_root, project_id).join("manifest.json")
}

/// manifest 加载错误：NotFound（端点 404）与坏载荷（500）。
#[derive(Debug)]
pub enum LoadManifestError {
    NotFound,
    BadPayload(String),
}

pub async fn load_translation_manifest(
    project_root: &Path,
    project_id: &str,
) -> Result<TranslationProjectManifest, LoadManifestError> {
    let dir = ensure_layout(project_root, project_id).await;
    let raw = tokio::fs::read_to_string(dir.join("manifest.json"))
        .await
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                LoadManifestError::NotFound
            } else {
                LoadManifestError::BadPayload(e.to_string())
            }
        })?;
    serde_json::from_str(&raw).map_err(|e| LoadManifestError::BadPayload(e.to_string()))
}

pub async fn save_translation_manifest(
    project_root: &Path,
    manifest: &TranslationProjectManifest,
) -> std::io::Result<()> {
    let path = translation_manifest_path(project_root, &manifest.id);
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let payload = format!("{}\n", serde_json::to_string_pretty(manifest).unwrap_or_default());
    crate::utils::atomic_file_set::write_file_atomic(&path, &payload).await
}

pub async fn load_translation_chapter(
    project_root: &Path,
    chapter_path: &str,
) -> Result<TranslationChapterFile, String> {
    let raw = tokio::fs::read_to_string(project_root.join(chapter_path))
        .await
        .map_err(|e| e.to_string())?;
    serde_json::from_str(&raw).map_err(|e| e.to_string())
}

pub async fn save_translation_chapter(
    project_root: &Path,
    chapter_path: &str,
    chapter: &TranslationChapterFile,
) -> Result<(), String> {
    let path = project_root.join(chapter_path);
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|e| e.to_string())?;
    }
    let payload = format!("{}\n", serde_json::to_string_pretty(chapter).unwrap_or_default());
    crate::utils::atomic_file_set::write_file_atomic(&path, &payload)
        .await
        .map_err(|e| e.to_string())
}

/// 术语表读取（坏文件 → 空表——TS catch 同语义）。
pub async fn load_translation_glossary(
    project_root: &Path,
    project_id: &str,
) -> Vec<TranslationGlossaryTerm> {
    let dir = ensure_layout(project_root, project_id).await;
    let Ok(raw) = tokio::fs::read_to_string(dir.join("glossary.json")).await else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return Vec::new();
    };
    value
        .get("terms")
        .and_then(|terms| terms.as_array())
        .map(|terms| {
            terms
                .iter()
                .filter_map(|term| {
                    let source = term.get("source")?.as_str()?.to_string();
                    let target = term.get("target")?.as_str()?.to_string();
                    Some(TranslationGlossaryTerm {
                        source,
                        target,
                        note: term
                            .get("note")
                            .and_then(|note| note.as_str())
                            .map(String::from),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// TS `saveTranslationProgress`（142 号）：章译文 + 术语表**同事务**原子
/// 落盘（commitAtomicFileSet 双写——翻译进行中的两个真值文件要么都在、
/// 要么都不在）。
pub async fn save_translation_progress(
    project_root: &Path,
    project_id: &str,
    chapter_path: &str,
    chapter: &TranslationChapterFile,
    terms: &[TranslationGlossaryTerm],
) -> Result<(), String> {
    use crate::utils::atomic_file_set::{
        commit_atomic_file_set, AtomicFileSet, AtomicFileWrite, FileContent,
    };
    let merged = merge_glossary_terms(terms);
    let glossary_path = to_posix_path(
        project_root,
        &translation_project_dir(project_root, project_id).join("glossary.json"),
    );
    commit_atomic_file_set(&AtomicFileSet {
        root_dir: project_root,
        writes: vec![
            AtomicFileWrite {
                relative_path: chapter_path.to_string(),
                content: FileContent::Text(format!(
                    "{}\n",
                    serde_json::to_string_pretty(chapter).unwrap_or_default()
                )),
            },
            AtomicFileWrite {
                relative_path: glossary_path,
                content: FileContent::Text(format!(
                    "{}\n",
                    serde_json::to_string_pretty(&serde_json::json!({ "terms": merged }))
                        .unwrap_or_default()
                )),
            },
        ],
        deletes: Vec::new(),
    })
    .await
    .map_err(|e| e.to_string())
}

pub async fn save_translation_glossary(
    project_root: &Path,
    project_id: &str,
    terms: &[TranslationGlossaryTerm],
) -> Result<(), String> {
    let merged = merge_glossary_terms(terms);
    let payload = format!(
        "{}\n",
        serde_json::to_string_pretty(&serde_json::json!({ "terms": merged })).unwrap_or_default()
    );
    crate::utils::atomic_file_set::write_file_atomic(
        &translation_project_dir(project_root, project_id).join("glossary.json"),
        &payload,
    )
    .await
    .map_err(|e| e.to_string())
}

/// `mergeGlossaryTerms`：source.trim().lower() 去重（后写覆盖）。
pub fn merge_glossary_terms(terms: &[TranslationGlossaryTerm]) -> Vec<TranslationGlossaryTerm> {
    let mut map: std::collections::BTreeMap<String, TranslationGlossaryTerm> =
        std::collections::BTreeMap::new();
    for term in terms {
        let key = term.source.trim().to_lowercase();
        if key.is_empty() {
            continue;
        }
        let note = term.note.as_deref().map(str::trim).filter(|n| !n.is_empty());
        map.insert(
            key,
            TranslationGlossaryTerm {
                source: term.source.trim().to_string(),
                target: term.target.trim().to_string(),
                note: note.map(String::from),
            },
        );
    }
    map.into_values().collect()
}

/// posix 相对路径（toPosixPath + relative）。
pub fn to_posix_path(project_root: &Path, path: &Path) -> String {
    path.strip_prefix(project_root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}


#[cfg(test)]
mod progress_tests {
    use super::*;
    use crate::translation::types::{TranslationChapterFile, TranslationGlossaryTerm, TranslationSegment};

    fn chapter() -> TranslationChapterFile {
        TranslationChapterFile {
            number: 3,
            title: "夜巡".into(),
            source_language: "en".into(),
            target_language: "zh".into(),
            segments: vec![TranslationSegment {
                index: 0,
                source: "Night patrol".into(),
                target: Some("夜巡".into()),
                notes: None,
            }],
        }
    }

    #[tokio::test]
    async fn progress_writes_chapter_and_glossary_atomically() {
        let dir = tempfile::tempdir().unwrap();
        let terms = vec![
            TranslationGlossaryTerm { source: "patrol".into(), target: "巡逻".into(), note: None },
            TranslationGlossaryTerm { source: "Patrol".into(), target: "巡逻（覆盖）".into(), note: None },
        ];
        save_translation_progress(
            dir.path(),
            "proj",
            "works/proj/source/translated/0003.json",
            &chapter(),
            &terms,
        )
        .await
        .unwrap();
        let chapter_raw = std::fs::read_to_string(
            dir.path().join("works/proj/source/translated/0003.json"),
        )
        .unwrap();
        assert!(chapter_raw.ends_with("}\n"), "pretty + 尾换行");
        assert!(chapter_raw.contains("\"title\": \"夜巡\""));
        let glossary_raw = std::fs::read_to_string(
            dir.path().join("works/proj/source/glossary.json"),
        )
        .unwrap();
        assert!(glossary_raw.ends_with("}\n"));
        assert!(
            glossary_raw.contains("\"target\": \"巡逻（覆盖）\""),
            "trim+lower 去重后写覆盖：{glossary_raw}"
        );
        assert_eq!(
            glossary_raw.matches("\"source\":").count(),
            1,
            "去重单条（保留后写原文形态）"
        );
    }

    #[tokio::test]
    async fn progress_rejects_escape_paths_with_zero_writes() {
        let dir = tempfile::tempdir().unwrap();
        let err = save_translation_progress(
            dir.path(),
            "proj",
            "../escape.json",
            &chapter(),
            &[],
        )
        .await;
        assert!(err.is_err(), "越界路径必须拒：{err:?}");
        assert!(!dir.path().join("translations").exists(), "零写入");
    }
}
