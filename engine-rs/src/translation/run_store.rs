//! 翻译项目存储（run-store.ts）：manifest / 章节 / 术语表读写与合并。

use std::path::{Path, PathBuf};

use crate::translation::types::*;

pub fn translation_project_dir(project_root: &Path, project_id: &str) -> PathBuf {
    project_root.join("translations").join(project_id)
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
    let raw = tokio::fs::read_to_string(translation_manifest_path(project_root, project_id))
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
    let payload = format!("{}\n", serde_json::to_string_pretty(manifest).unwrap_or_default());
    tokio::fs::write(&path, payload).await
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
    let payload = format!("{}\n", serde_json::to_string_pretty(chapter).unwrap_or_default());
    tokio::fs::write(project_root.join(chapter_path), payload)
        .await
        .map_err(|e| e.to_string())
}

/// 术语表读取（坏文件 → 空表——TS catch 同语义）。
pub async fn load_translation_glossary(
    project_root: &Path,
    project_id: &str,
) -> Vec<TranslationGlossaryTerm> {
    let Ok(raw) = tokio::fs::read_to_string(
        translation_project_dir(project_root, project_id).join("glossary.json"),
    )
    .await
    else {
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
    tokio::fs::write(
        translation_project_dir(project_root, project_id).join("glossary.json"),
        payload,
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
            "translations/proj/chapters/0003.json",
            &chapter(),
            &terms,
        )
        .await
        .unwrap();
        let chapter_raw = std::fs::read_to_string(
            dir.path().join("translations/proj/chapters/0003.json"),
        )
        .unwrap();
        assert!(chapter_raw.ends_with("}\n"), "pretty + 尾换行");
        assert!(chapter_raw.contains("\"title\": \"夜巡\""));
        let glossary_raw = std::fs::read_to_string(
            dir.path().join("translations/proj/glossary.json"),
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
