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
