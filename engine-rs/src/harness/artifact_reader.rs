//! 工件修订读取器（884 号 M1b）：`packages/core/src/harness/artifact-reader.ts`
//! `readArtifactRevision` 移植——只读已注册修订，永不以最新 candidate 顶替。
//! realpath 收容 + checksum 强校验（缺一即显式错误码）。

use std::path::Path;

use super::contracts::{ArtifactManifest, ArtifactRevision, WorkManifest};
use super::source_sync::hex_sha256;
use super::work_store::{load_work_manifest, work_directory};

pub struct ReadArtifactRevision {
    pub work: WorkManifest,
    pub artifact: ArtifactManifest,
    pub revision: ArtifactRevision,
    pub bytes: Vec<u8>,
}

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct ArtifactReaderError {
    pub code: &'static str,
    pub message: String,
}

fn fail(code: &'static str, message: &str, work_id: &str) -> ArtifactReaderError {
    let _ = work_id;
    ArtifactReaderError { code, message: message.to_string() }
}

/// TS `readArtifactRevision`；`revision_id=None` = 当前修订。
pub async fn read_artifact_revision(
    project_root: &Path,
    work_id: &str,
    artifact_id: &str,
    revision_id: Option<&str>,
) -> Result<ReadArtifactRevision, ArtifactReaderError> {
    let work = load_work_manifest(project_root, work_id)
        .await
        .map_err(|error| fail("ARTIFACT_NOT_FOUND", &error.to_string(), work_id))?;
    let artifact = work.artifacts.iter().find(|item| item.id == artifact_id).cloned();
    let revision = artifact.as_ref().and_then(|item| {
        let wanted = revision_id.unwrap_or(item.current_revision_id.as_deref().unwrap_or(""));
        item.revisions.iter().find(|candidate| candidate.id == wanted).cloned()
    });
    let (Some(artifact), Some(revision)) = (artifact, revision) else {
        return Err(fail(
            "ARTIFACT_NOT_FOUND",
            "The requested artifact revision is not registered.",
            work_id,
        ));
    };
    let root = work_directory(project_root, work_id)
        .map_err(|error| fail("ARTIFACT_NOT_FOUND", &error.to_string(), work_id))?;
    // TS realpath(join(root, …))：目录不存在 → ENOENT → SNAPSHOT_UNAVAILABLE。
    let root_real = std::fs::canonicalize(&root).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            fail(
                "ARTIFACT_SNAPSHOT_UNAVAILABLE",
                "The registered revision has no readable snapshot.",
                work_id,
            )
        } else {
            ArtifactReaderError { code: "ARTIFACT_SNAPSHOT_UNAVAILABLE", message: error.to_string() }
        }
    })?;
    let target = root_real.join(revision.snapshot_path.as_deref().unwrap_or(&revision.path));
    let resolved = match std::fs::canonicalize(&target) {
        Ok(path) => path,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(fail(
                "ARTIFACT_SNAPSHOT_UNAVAILABLE",
                "The registered revision has no readable snapshot.",
                work_id,
            ));
        }
        Err(error) => {
            return Err(ArtifactReaderError { code: "ARTIFACT_PATH_OUTSIDE_WORK", message: error.to_string() })
        }
    };
    if !resolved.starts_with(&root_real) {
        return Err(fail(
            "ARTIFACT_PATH_OUTSIDE_WORK",
            "The revision resolves outside its Work.",
            work_id,
        ));
    }
    let bytes = tokio::fs::read(&resolved).await.map_err(|error| {
        ArtifactReaderError { code: "ARTIFACT_SNAPSHOT_UNAVAILABLE", message: error.to_string() }
    })?;
    if format!("sha256:{}", hex_sha256(&bytes)) != revision.checksum {
        return Err(fail(
            "ARTIFACT_REVISION_CONFLICT",
            "The stored bytes do not match the registered revision.",
            work_id,
        ));
    }
    Ok(ReadArtifactRevision {
        work,
        artifact,
        revision,
        bytes,
    })
}

/// adopt 工具体只需 bytes（TS `readArtifactRevision(…).bytes`）。
pub async fn read_artifact_revision_bytes(
    project_root: &Path,
    work_id: &str,
    artifact_id: &str,
    revision_id: Option<&str>,
) -> Result<Vec<u8>, ArtifactReaderError> {
    let read = read_artifact_revision(project_root, work_id, artifact_id, revision_id).await?;
    Ok(read.bytes)
}
