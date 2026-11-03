//! artifact 版本操作（882 号 M1a）：`packages/core/src/harness/artifact-revisions.ts`
//! 移植——stage（新版本落盘+manifest 更新）、promote（版本扶正）、读（checksum 校验）。
//! checksum = `sha256:<hex>`，写入前对字节计算；快照路径 `artifacts/<aid>/revisions/<rid><ext>`。

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::contracts::{
    ArtifactManifest, ArtifactRevision, ArtifactRevisionStatus, WorkManifest,
};
use super::work_store::{save_work_manifest, WorkStoreError, WORKS_DIRECTORY};

fn current_revision_id(manifest: &WorkManifest, artifact_id: &str) -> Option<String> {
    manifest
        .artifacts
        .iter()
        .find(|artifact| artifact.id == artifact_id)
        .and_then(|artifact| artifact.current_revision_id.clone())
}

fn upsert_artifact(
    artifacts: &[ArtifactManifest],
    next: ArtifactManifest,
) -> Vec<ArtifactManifest> {
    if artifacts.iter().any(|artifact| artifact.id == next.id) {
        artifacts
            .iter()
            .map(|artifact| {
                if artifact.id == next.id {
                    next.clone()
                } else {
                    artifact.clone()
                }
            })
            .collect()
    } else {
        let mut artifacts = artifacts.to_vec();
        artifacts.push(next);
        artifacts
    }
}

pub struct StagedRevision {
    pub manifest: WorkManifest,
    pub revision: ArtifactRevision,
    /// 快照文件的绝对路径（与 atomic 提交写入一致，供测试/调用方核对）。
    pub snapshot_path: PathBuf,
}

/// TS `stageArtifactRevision`：版本文件 + manifest 原子提交（promote=true 时
/// 旧 current 翻 superseded、新版本即 current）。
pub async fn stage_artifact_revision(input: &StageArtifactRevisionInput<'_>) -> Result<StagedRevision, WorkStoreError> {
    let manifest = clone_validated(input.manifest)?;
    let artifact_id = input.artifact_id.to_string();
    let revision_id = input.revision_id.to_string();
    let file_name = input.file_name;
    let extension = match file_name.rsplit_once('.') {
        // TS：extname + ^\.[a-z0-9._-]+$/i（extname 含点且非空）。
        Some((_, ext)) if !ext.is_empty() && ext.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-') => {
            format!(".{}", ext)
        }
        _ => {
            return Err(WorkStoreError::Invalid(format!(
                "Artifact revision fileName must have a safe extension: {file_name}"
            )));
        }
    };
    let bytes = input.content.as_bytes().to_vec();
    let relative = format!("artifacts/{artifact_id}/revisions/{revision_id}{extension}");
    let parent = current_revision_id(&manifest, &artifact_id);
    let created_at = input.created_at.unwrap_or(input.now);
    let revision = ArtifactRevision {
        id: revision_id.clone(),
        parent_revision_id: parent.clone(),
        path: relative.clone(),
        snapshot_path: None,
        content_type: input.content_type.to_string(),
        status: if input.promote {
            ArtifactRevisionStatus::Current
        } else {
            ArtifactRevisionStatus::Candidate
        },
        checksum: format!("sha256:{}", sha256_hex(&bytes)),
        byte_length: bytes.len() as u64,
        episode_id: input.episode_id.map(str::to_string),
        created_at: created_at.to_string(),
    };
    let existing = manifest
        .artifacts
        .iter()
        .find(|artifact| artifact.id == artifact_id)
        .cloned();
    let next_artifact = ArtifactManifest {
        id: artifact_id.clone(),
        kind: input.artifact_kind.to_string(),
        current_revision_id: if input.promote {
            Some(revision_id.clone())
        } else {
            parent.clone()
        },
        revisions: {
            let mut revisions = existing
                .as_ref()
                .map(|artifact| artifact.revisions.clone())
                .unwrap_or_default();
            if input.promote {
                for revision in &mut revisions {
                    if revision.status == ArtifactRevisionStatus::Current {
                        revision.status = ArtifactRevisionStatus::Superseded;
                    }
                }
            }
            revisions.push(revision.clone());
            revisions
        },
        metadata: existing.map(|artifact| artifact.metadata).unwrap_or_default(),
    };
    // TS：ArtifactManifestSchema.parse(nextArtifact) 触发三条不变量校验。
    next_artifact.validate().map_err(WorkStoreError::Invalid)?;
    let mut next_manifest = manifest.clone();
    next_manifest.artifacts = upsert_artifact(&next_manifest.artifacts, next_artifact.clone());
    next_manifest.updated_at = revision.created_at.clone();
    next_manifest.validate().map_err(WorkStoreError::Invalid)?;

    let snapshot_absolute = input.project_root.join(WORKS_DIRECTORY).join(&manifest.id).join(&relative);
    let commit = crate::utils::atomic_file_set::AtomicFileSet {
        root_dir: input.project_root,
        writes: vec![
            crate::utils::atomic_file_set::AtomicFileWrite {
                relative_path: format!("{}/{}/{}", WORKS_DIRECTORY, manifest.id, relative),
                content: crate::utils::atomic_file_set::FileContent::Bytes(bytes),
            },
            crate::utils::atomic_file_set::AtomicFileWrite {
                relative_path: format!("{}/{}/{}", WORKS_DIRECTORY, manifest.id, super::work_store::WORK_MANIFEST_FILE),
                content: crate::utils::atomic_file_set::FileContent::Text(
                    serde_json::to_string_pretty(&next_manifest)
                        .map_err(|e| WorkStoreError::Io(e.to_string()))?
                        + "\n",
                ),
            },
        ],
        deletes: Vec::new(),
    };
    crate::utils::atomic_file_set::commit_atomic_file_set(&commit)
        .await
        .map_err(|error| WorkStoreError::Atomic(error.to_string()))?;
    Ok(StagedRevision {
        manifest: next_manifest,
        revision,
        snapshot_path: snapshot_absolute,
    })
}

/// TS `promoteArtifactRevision`：既有版本扶正为 current（其余 current 翻 superseded）。
/// `updated_at` 由调用方传入 ISO 时刻（TS 侧默认 now，M1a 调用面显式传参）。
pub async fn promote_artifact_revision(
    project_root: &Path,
    manifest: &WorkManifest,
    artifact_id: &str,
    revision_id: &str,
    updated_at: &str,
) -> Result<WorkManifest, WorkStoreError> {
    let mut next = manifest.clone();
    next.validate().map_err(WorkStoreError::Invalid)?;
    {
        let artifact = next
            .artifacts
            .iter_mut()
            .find(|artifact| artifact.id == artifact_id)
            .ok_or_else(|| WorkStoreError::Invalid(format!("Unknown artifact: {artifact_id}")))?;
        if !artifact.revisions.iter().any(|revision| revision.id == revision_id) {
            return Err(WorkStoreError::Invalid(format!(
                "Unknown artifact revision: {artifact_id}.{revision_id}"
            )));
        }
        artifact.current_revision_id = Some(revision_id.to_string());
        for revision in &mut artifact.revisions {
            if revision.id == revision_id {
                revision.status = ArtifactRevisionStatus::Current;
            } else if revision.status == ArtifactRevisionStatus::Current {
                revision.status = ArtifactRevisionStatus::Superseded;
            }
        }
        artifact.validate().map_err(WorkStoreError::Invalid)?;
    }
    next.updated_at = updated_at.to_string();
    save_work_manifest(project_root, &next).await?;
    Ok(next)
}

/// TS `readArtifactRevision` 语义：读快照字节（TS 版同时校验 checksum——本函数
/// 返回字节与校验留给调用方比对，对齐 work-artifacts adopt 的显式冲突分支）。
pub async fn read_artifact_revision_bytes(
    project_root: &Path,
    manifest: &WorkManifest,
    artifact_id: &str,
    revision_id: &str,
) -> Result<(ArtifactRevision, Vec<u8>), WorkStoreError> {
    let artifact = manifest
        .artifacts
        .iter()
        .find(|artifact| artifact.id == artifact_id)
        .ok_or_else(|| WorkStoreError::NotFound(format!("{artifact_id}.{revision_id}")))?;
    let revision = artifact
        .revisions
        .iter()
        .find(|revision| revision.id == revision_id)
        .ok_or_else(|| WorkStoreError::NotFound(format!("{artifact_id}.{revision_id}")))?;
    let resolved = project_root
        .join(WORKS_DIRECTORY)
        .join(&manifest.id)
        .join(revision.snapshot_path.clone().unwrap_or_else(|| revision.path.clone()));
    let bytes = tokio::fs::read(&resolved)
        .await
        .map_err(|error| WorkStoreError::Io(error.to_string()))?;
    Ok((revision.clone(), bytes))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn clone_validated(manifest: &WorkManifest) -> Result<WorkManifest, WorkStoreError> {
    let manifest = manifest.clone();
    manifest.validate().map_err(WorkStoreError::Invalid)?;
    Ok(manifest)
}

pub struct StageArtifactRevisionInput<'a> {
    pub project_root: &'a Path,
    pub manifest: &'a WorkManifest,
    pub artifact_id: &'a str,
    pub artifact_kind: &'a str,
    pub revision_id: &'a str,
    pub content: &'a str,
    pub content_type: &'a str,
    pub file_name: &'a str,
    pub episode_id: Option<&'a str>,
    pub now: &'a str,
    pub created_at: Option<&'a str>,
    pub promote: bool,
}

