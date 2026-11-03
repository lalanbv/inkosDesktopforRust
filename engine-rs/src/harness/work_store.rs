//! work 清单存储（882 号 M1a）：`packages/core/src/harness/work-store.ts` 移植。
//! 存储 = `works/<id>/work.json`（atomic-file-set 原子提交，尾部换行与 TS
//! `JSON.stringify(parsed, null, 2) + "\n"` 逐字对齐）。

use std::path::{Path, PathBuf};

use super::contracts::{validate_work_resource_id, WorkManifest};
use crate::utils::atomic_file_set::{AtomicFileSet, AtomicFileWrite};

pub const WORKS_DIRECTORY: &str = "works";
pub const WORK_MANIFEST_FILE: &str = "work.json";

#[derive(Debug, Clone, thiserror::Error)]
pub enum WorkStoreError {
    #[error("{0}")]
    Invalid(String),
    #[error("work manifest not found: {0}")]
    NotFound(String),
    #[error("{0}")]
    Io(String),
    #[error("atomic commit failed: {0}")]
    Atomic(String),
}

impl WorkStoreError {
    /// Node `error.code === "ENOENT"` 的对应判别（调用方按 404 分流）。
    pub fn is_not_found(&self) -> bool {
        matches!(self, WorkStoreError::NotFound(_))
    }
}

fn to_io_string(error: std::io::Error) -> String {
    error.to_string()
}

pub fn work_directory(project_root: &Path, work_id: &str) -> Result<PathBuf, WorkStoreError> {
    validate_work_resource_id(work_id).map_err(WorkStoreError::Invalid)?;
    Ok(project_root.join(WORKS_DIRECTORY).join(work_id))
}

pub fn work_manifest_path(project_root: &Path, work_id: &str) -> Result<PathBuf, WorkStoreError> {
    work_directory(project_root, work_id).map(|dir| dir.join(WORK_MANIFEST_FILE))
}

pub struct CreateWorkManifestInput<'a> {
    pub id: &'a str,
    pub title: &'a str,
    pub profile_id: &'a str,
    pub language: &'a str,
    pub status: Option<super::contracts::WorkStatus>,
    pub now: &'a str,
}

/// TS `createWorkManifest`：parse 即校验，产出空 lineage/artifacts/metadata 的
/// draft 清单。
pub fn create_work_manifest(
    input: &CreateWorkManifestInput<'_>,
) -> Result<WorkManifest, WorkStoreError> {
    let manifest = WorkManifest {
        version: 2,
        id: input.id.to_string(),
        title: input.title.to_string(),
        profile_id: input.profile_id.to_string(),
        language: input.language.to_string(),
        status: input.status.unwrap_or(super::contracts::WorkStatus::Draft),
        lineage: Vec::new(),
        artifacts: Vec::new(),
        metadata: serde_json::Map::new(),
        created_at: input.now.to_string(),
        updated_at: input.now.to_string(),
    };
    manifest.validate().map_err(WorkStoreError::Invalid)?;
    Ok(manifest)
}

pub fn manifest_to_json(manifest: &WorkManifest) -> Result<String, WorkStoreError> {
    manifest.validate().map_err(WorkStoreError::Invalid)?;
    // 直接序列化结构体（字段声明序 = schema 序）——不经 Value 中转（默认
    // BTreeMap 会把键重排成字母序，与 TS 字段声明序的落盘形态漂移）。
    let mut text =
        serde_json::to_string_pretty(manifest).map_err(|e| WorkStoreError::Io(e.to_string()))?;
    text.push('\n');
    Ok(text)
}

pub async fn load_work_manifest(
    project_root: &Path,
    work_id: &str,
) -> Result<WorkManifest, WorkStoreError> {
    let path = work_manifest_path(project_root, work_id)?;
    match tokio::fs::read_to_string(&path).await {
        Ok(raw) => parse_work_manifest_json(&raw),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Err(WorkStoreError::NotFound(work_id.to_string()))
        }
        Err(error) => Err(WorkStoreError::Io(to_io_string(error))),
    }
}

/// JSON 文本 → 校验后的清单（TS `WorkManifestSchema.parse(JSON.parse(raw))` 对应）。
/// zod strict 拒未知键 ↔ deny_unknown_fields；解析错误统一为 Invalid。
pub fn parse_work_manifest_json(raw: &str) -> Result<WorkManifest, WorkStoreError> {
    let manifest: WorkManifest =
        serde_json::from_str(raw).map_err(|error| WorkStoreError::Invalid(error.to_string()))?;
    manifest.validate().map_err(WorkStoreError::Invalid)?;
    Ok(manifest)
}

pub async fn save_work_manifest(
    project_root: &Path,
    manifest: &WorkManifest,
) -> Result<(), WorkStoreError> {
    let content = manifest_to_json(manifest)?;
    let commit = AtomicFileSet {
        root_dir: project_root,
        writes: vec![AtomicFileWrite {
            relative_path: format!("{WORKS_DIRECTORY}/{}/{}", manifest.id, WORK_MANIFEST_FILE),
            content: crate::utils::atomic_file_set::FileContent::Text(content),
        }],
        deletes: Vec::new(),
    };
    crate::utils::atomic_file_set::commit_atomic_file_set(&commit)
        .await
        .map_err(|error| WorkStoreError::Atomic(error.to_string()))
}

/// TS `listWorkManifests`：单遍读 works/ 目录（ENOENT=空集），无 work.json 的
/// 子目录跳过、其余错误上抛；profileId 过滤（Some=精确匹配）；updatedAt 降序、
/// id 升序 tiebreak。
pub async fn list_work_manifests(
    project_root: &Path,
    profile_id: Option<&str>,
) -> Result<Vec<WorkManifest>, WorkStoreError> {
    let mut entries = match tokio::fs::read_dir(project_root.join(WORKS_DIRECTORY)).await {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Vec::new());
        }
        Err(error) => return Err(WorkStoreError::Io(to_io_string(error))),
    };
    let mut ids: Vec<String> = Vec::new();
    while let Some(entry) = entries.next_entry().await.map_err(|e| WorkStoreError::Io(e.to_string()))? {
        if entry.file_type().await.map_err(|e| WorkStoreError::Io(e.to_string()))?.is_dir() {
            ids.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    ids.sort();

    let mut works: Vec<WorkManifest> = Vec::new();
    for id in &ids {
        match load_work_manifest(project_root, id).await {
            Ok(manifest) => {
                let matches = profile_id
                    .map(|profile| manifest.profile_id == profile)
                    .unwrap_or(true);
                if matches {
                    works.push(manifest);
                }
            }
            // TS：ENOENT 跳过（无 work.json 的目录），其余错误上抛。
            Err(WorkStoreError::NotFound(_)) => {}
            Err(error) => return Err(error),
        }
    }
    works.sort_by(|left, right| {
        right
            .updated_at
            .cmp(&left.updated_at)
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(works)
}

/// 供端点层把 id 校验/路径解析复用（M1b）。
pub fn validated_work_id(work_id: &str) -> Result<String, WorkStoreError> {
    validate_work_resource_id(work_id).map_err(WorkStoreError::Invalid)?;
    Ok(work_id.to_string())
}
