//! work 源工件同步器（884 号 M1b）：`packages/core/src/harness/source-sync.ts`
//! `syncWorkSourceArtifacts` 忠实移植——work 域唯一的清单变更通道。
//!
//! 语义密点（与 TS 逐行对照，勿简化）：
//! - 源目录全量重扫描 + pending 写集合一（键 = source/ 相对 posix 路径）
//! - 修订 id = `rev-<checksum 前 16 hex>`；同 checksum 修订幂等复用（prior 分支）
//! - 快照补齐：legacy 当前修订无 snapshotPath 时回填（就地补登记 / 前值取证冲突）
//! - accept 面翻转：旧 current → superseded；removedAt/removedPath 淘汰检测
//! - kind 归一（SOURCE_KIND_VERSION="storage-path-v1" 门控 + legacy 默认 kind 搬迁）
//! - 原子提交 = pending 写集 + 快照写集 + work.json（结构体序列化保字段声明序）
//! - .db/.sqlite 源字节 = sqlite backup 快照读（WAL 未合并时的读一致性，TS 同）

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use super::contracts::{
    validate_work_resource_id, ArtifactManifest, ArtifactRevision, WorkManifest, WorkStatus,
    ArtifactRevisionStatus,
};
use super::work_store::{load_work_manifest, manifest_to_json, work_directory, WorkStoreError};
use crate::utils::atomic_file_set::{
    commit_atomic_file_set, AtomicFileSet, AtomicFileWrite, FileContent,
};

const SOURCE_KIND_VERSION: &str = "storage-path-v1";

#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    Conflict(String),
    #[error("work manifest not found: {0}")]
    NotFound(String),
    #[error("{0}")]
    Io(String),
    #[error("atomic commit failed: {0}")]
    Atomic(String),
}

impl From<WorkStoreError> for SyncError {
    fn from(error: WorkStoreError) -> Self {
        match error {
            WorkStoreError::NotFound(id) => SyncError::NotFound(id),
            WorkStoreError::Invalid(message) => SyncError::Invalid(message),
            WorkStoreError::Atomic(message) => SyncError::Atomic(message),
            WorkStoreError::Io(message) => SyncError::Io(message),
        }
    }
}

/// `recordExecutionEvidence("work-artifacts-synced", …)` 的载荷——adopt 环将其
/// 转写为 episode 事件（evidence 通道不落盘，TS 同）。
#[derive(Debug, Clone)]
pub struct SyncEvidence {
    pub work_id: String,
    pub profile_id: String,
    pub accepted: bool,
    pub artifacts: Vec<SyncEvidenceArtifact>,
}

#[derive(Debug, Clone)]
pub struct SyncEvidenceArtifact {
    pub artifact_id: String,
    pub current_revision_id: Option<String>,
    pub latest_revision_id: Option<String>,
}

pub struct SyncWorkSourceInput<'a> {
    pub project_root: &'a Path,
    pub work_id: &'a str,
    pub accept: bool,
    /// Work 内绝对 posix 路径（`source/…`）；None = 所有当前源修订都受 accept 面
    /// 管辖，Some = 只列出者（TS acceptPaths）。
    pub accept_paths: Option<Vec<String>>,
    pub episode_id: Option<&'a str>,
    pub updated_at: Option<String>,
    /// project 根相对路径（`works/<id>/source/…`）。
    pub writes: Vec<AtomicFileWrite>,
    pub title: Option<&'a str>,
    pub status: Option<WorkStatus>,
}

pub struct SyncWorkSourceOutput {
    pub manifest: WorkManifest,
    pub evidence: SyncEvidence,
}

pub async fn sync_work_source_artifacts(
    input: SyncWorkSourceInput<'_>,
) -> Result<SyncWorkSourceOutput, SyncError> {
    validate_work_resource_id(input.work_id).map_err(SyncError::Invalid)?;
    let manifest = load_work_manifest(input.project_root, input.work_id).await?;
    let root = work_directory(input.project_root, input.work_id)?;
    let source_root = root.join("source");

    // pending：键 = Work 源内相对路径（posix）；越界/逃逸写 = 错（TS 同文案）。
    // 值 = 字节（commit 面另行持有 input.writes 原件——TS pending 与提交写集
    // 是同一写集合的两处引用，Rust 侧以字节快照解耦所有权）。
    let mut pending: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for write in &input.writes {
        let path = relative_to_source(&format!("works/{}/source", input.work_id), &write.relative_path)?;
        pending.insert(path, file_content_bytes(&write.content));
    }
    // TS acceptPaths 逐个 toPosixPath 归一后再比对。
    let accept_paths: Option<Vec<String>> = input
        .accept_paths
        .as_ref()
        .map(|paths| paths.iter().map(|path| to_posix_path(path)).collect());

    // files = 磁盘扫描 ∪ pending 键（discovery 与 pending 必须共享一键再合并）。
    let mut union: BTreeSet<String> = list_files(&source_root)?.into_iter().collect();
    union.extend(pending.keys().cloned());
    let files: Vec<String> = union.into_iter().collect();

    let present_work_paths: BTreeSet<String> =
        files.iter().map(|file| format!("source/{file}")).collect();
    let updated_at =
        input.updated_at.clone().unwrap_or_else(crate::utils::utc_time::utc_now_iso);

    let mut artifacts: Vec<ArtifactManifest> = manifest.artifacts.clone();
    let mut snapshot_writes: Vec<AtomicFileWrite> = Vec::new();

    for file in &files {
        let work_path = format!("source/{file}");
        let accept_item =
            input.accept && accept_paths.as_ref().map(|paths| paths.iter().any(|p| p == &work_path)).unwrap_or(true);
        let bytes: Vec<u8> = match pending.get(file) {
            Some(bytes) => bytes.clone(),
            None => read_artifact_bytes(&source_root.join(file)).await?,
        };
        let checksum = format!("sha256:{}", hex_sha256(&bytes));
        let revision_id = revision_id_for(&checksum);
        let existing_index = artifacts
            .iter()
            .position(|artifact| artifact.revisions.iter().any(|revision| revision.path == work_path));
        let artifact_id = match existing_index {
            Some(index) => artifacts[index].id.clone(),
            None => artifact_id_for(file),
        };
        let snapshot_path = revision_snapshot_path(&artifact_id, &revision_id, file);
        let Some(index) = existing_index else {
            let revision = ArtifactRevision {
                id: revision_id.clone(),
                parent_revision_id: None,
                path: work_path.clone(),
                snapshot_path: Some(snapshot_path.clone()),
                content_type: content_type_for(file).to_string(),
                status: if accept_item {
                    ArtifactRevisionStatus::Current
                } else {
                    ArtifactRevisionStatus::Candidate
                },
                checksum: checksum.clone(),
                byte_length: bytes.len() as u64,
                episode_id: input.episode_id.map(str::to_string),
                created_at: updated_at.clone(),
            };
            let mut metadata = Map::new();
            metadata.insert(
                "sourcePath".into(),
                Value::String(format!("works/{}/{}", input.work_id, work_path)),
            );
            metadata.insert("kindSource".into(), Value::String(SOURCE_KIND_VERSION.into()));
            artifacts.push(ArtifactManifest {
                id: artifact_id.clone(),
                kind: artifact_kind_for(&work_path),
                current_revision_id: if accept_item { Some(revision.id.clone()) } else { None },
                revisions: vec![revision],
                metadata,
            });
            snapshot_writes.push(snapshot_write(input.work_id, &snapshot_path, bytes));
            continue;
        };

        let mut existing = artifacts[index].clone();
        // kind 归一：新分类器写入（kindSource 门控）或 legacy 默认 kind 搬迁。
        let native_source = existing
            .metadata
            .get("sourcePath")
            .and_then(Value::as_str)
            .map(|value| value == format!("works/{}/{}", input.work_id, work_path))
            .unwrap_or(false);
        let legacy_default_kind = (existing.id == artifact_id_for(file)
            && existing.kind == legacy_source_artifact_kind(file))
            || (existing.id == artifact_id_for(&work_path)
                && existing.kind == legacy_source_artifact_kind(&work_path));
        let kind_source = existing.metadata.get("kindSource").and_then(Value::as_str);
        if kind_source == Some(SOURCE_KIND_VERSION)
            || (kind_source.is_none() && native_source && legacy_default_kind)
        {
            existing.kind = artifact_kind_for(&work_path);
            existing
                .metadata
                .insert("kindSource".into(), Value::String(SOURCE_KIND_VERSION.into()));
            artifacts[index] = existing.clone();
        }
        let mut current = existing
            .revisions
            .iter()
            .find(|revision| Some(&revision.id) == existing.current_revision_id.as_ref())
            .cloned();
        if let Some(current_revision) = &current {
            if current_revision.checksum == checksum {
                if current_revision.snapshot_path.is_none() {
                    existing.revisions = existing
                        .revisions
                        .iter()
                        .map(|revision| {
                            if revision.id == current_revision.id {
                                let mut updated = revision.clone();
                                updated.snapshot_path = Some(snapshot_path.clone());
                                updated
                            } else {
                                revision.clone()
                            }
                        })
                        .collect();
                    artifacts[index] = existing;
                    snapshot_writes.push(snapshot_write(input.work_id, &snapshot_path, bytes));
                }
                continue;
            }
            // legacy 当前修订无快照且有 pending 写：先取证前值（checksum 不符即冲突）。
            if current_revision.snapshot_path.is_none() && pending.contains_key(file) {
                let previous_bytes = tokio::fs::read(source_root.join(file))
                    .await
                    .map_err(|error| SyncError::Io(error.to_string()))?;
                if format!("sha256:{}", hex_sha256(&previous_bytes)) != current_revision.checksum {
                    return Err(SyncError::Conflict(
                        "Source changed before revision commit".into(),
                    ));
                }
                let previous_snapshot = revision_snapshot_path(&artifact_id, &current_revision.id, file);
                snapshot_writes
                    .push(snapshot_write(input.work_id, &previous_snapshot, previous_bytes));
                existing.revisions = existing
                    .revisions
                    .iter()
                    .map(|item| {
                        if item.id == current_revision.id {
                            let mut updated = item.clone();
                            updated.snapshot_path = Some(previous_snapshot.clone());
                            updated
                        } else {
                            item.clone()
                        }
                    })
                    .collect();
                current = existing
                    .revisions
                    .iter()
                    .find(|revision| Some(&revision.id) == existing.current_revision_id.as_ref())
                    .cloned();
                artifacts[index] = existing.clone();
            }
        }

        let prior = existing
            .revisions
            .iter()
            .find(|revision| revision.id == revision_id)
            .cloned();
        let revision = prior.clone().unwrap_or(ArtifactRevision {
            id: revision_id.clone(),
            parent_revision_id: existing.current_revision_id.clone(),
            path: work_path.clone(),
            snapshot_path: Some(snapshot_path.clone()),
            content_type: content_type_for(file).to_string(),
            status: if accept_item {
                ArtifactRevisionStatus::Current
            } else {
                ArtifactRevisionStatus::Candidate
            },
            checksum: checksum.clone(),
            byte_length: bytes.len() as u64,
            episode_id: input.episode_id.map(str::to_string),
            created_at: updated_at.clone(),
        });
        let next_revisions: Vec<ArtifactRevision> = match &prior {
            Some(prior) => existing
                .revisions
                .iter()
                .map(|item| {
                    if item.id == prior.id {
                        let mut updated = item.clone();
                        if updated.snapshot_path.is_none() {
                            updated.snapshot_path = Some(snapshot_path.clone());
                        }
                        if accept_item {
                            updated.status = ArtifactRevisionStatus::Current;
                        }
                        return updated;
                    }
                    if accept_item && item.status == ArtifactRevisionStatus::Current {
                        let mut superseded = item.clone();
                        superseded.status = ArtifactRevisionStatus::Superseded;
                        return superseded;
                    }
                    item.clone()
                })
                .collect(),
            None => {
                let mut revisions = existing
                    .revisions
                    .iter()
                    .map(|item| {
                        if accept_item && item.status == ArtifactRevisionStatus::Current {
                            let mut superseded = item.clone();
                            superseded.status = ArtifactRevisionStatus::Superseded;
                            superseded
                        } else {
                            item.clone()
                        }
                    })
                    .collect::<Vec<_>>();
                revisions.push(revision.clone());
                revisions
            }
        };
        existing.current_revision_id = if accept_item {
            Some(revision.id.clone())
        } else {
            existing.current_revision_id.clone()
        };
        existing.revisions = next_revisions;
        artifacts[index] = existing;
        // TS `if (!prior?.snapshotPath)`：新修订或复用修订缺快照时补写。
        if prior.as_ref().map(|item| item.snapshot_path.is_none()).unwrap_or(true) {
            snapshot_writes.push(snapshot_write(input.work_id, &snapshot_path, bytes));
        }
    }

    // 淘汰检测：accept 面（或 acceptPaths 列出者）之外、且已不在源目录的当前
    // 修订 → superseded + removedAt/removedPath 元数据。
    for artifact in &mut artifacts {
        let current = artifact
            .revisions
            .iter()
            .find(|revision| Some(&revision.id) == artifact.current_revision_id.as_ref())
            .cloned();
        let Some(current) = current else { continue };
        // TS：`(acceptPaths && !acceptPaths.includes(current.path))`——acceptPaths
        // 未定义时该卫兵为 false（不跳过，淘汰检测照常执行）。
        let accept_paths_excludes = accept_paths
            .as_ref()
            .map(|paths| !paths.iter().any(|p| p == &current.path))
            .unwrap_or(false);
        if !input.accept
            || !current.path.starts_with("source/")
            || accept_paths_excludes
            || present_work_paths.contains(&current.path)
        {
            continue;
        }
        artifact.current_revision_id = None;
        artifact.revisions = artifact
            .revisions
            .iter()
            .map(|revision| {
                if revision.id == current.id && revision.status == ArtifactRevisionStatus::Current {
                    let mut superseded = revision.clone();
                    superseded.status = ArtifactRevisionStatus::Superseded;
                    superseded
                } else {
                    revision.clone()
                }
            })
            .collect();
        artifact.metadata.insert("removedAt".into(), Value::String(updated_at.clone()));
        artifact.metadata.insert("removedPath".into(), Value::String(current.path.clone()));
    }

    let next = WorkManifest {
        title: input.title.map(str::to_string).unwrap_or(manifest.title.clone()),
        status: input.status.unwrap_or(match manifest.status {
            WorkStatus::Archived => WorkStatus::Archived,
            _ if input.accept => WorkStatus::Active,
            other => other,
        }),
        artifacts,
        updated_at,
        ..manifest
    };
    next.validate().map_err(SyncError::Invalid)?;

    let mut writes: Vec<AtomicFileWrite> =
        Vec::with_capacity(input.writes.len() + snapshot_writes.len() + 1);
    writes.extend(input.writes);
    writes.extend(snapshot_writes);
    writes.push(AtomicFileWrite {
        relative_path: format!("works/{}/work.json", input.work_id),
        // 结构体序列化保字段声明序（与 TS JSON.stringify 字段序逐字对齐，M1a 同源）。
        content: FileContent::Text(manifest_to_json(&next)?),
    });
    commit_atomic_file_set(&AtomicFileSet {
        root_dir: input.project_root,
        writes,
        deletes: Vec::new(),
    })
    .await
    .map_err(|error| SyncError::Atomic(error.to_string()))?;

    let evidence = SyncEvidence {
        work_id: next.id.clone(),
        profile_id: next.profile_id.clone(),
        accepted: input.accept,
        artifacts: next
            .artifacts
            .iter()
            .map(|artifact| SyncEvidenceArtifact {
                artifact_id: artifact.id.clone(),
                current_revision_id: artifact.current_revision_id.clone(),
                latest_revision_id: artifact.revisions.last().map(|revision| revision.id.clone()),
            })
            .collect(),
    };
    Ok(SyncWorkSourceOutput { manifest: next, evidence })
}

/// TS `captureWorkSourceState`：源目录全量字节指纹（键 = `source/` 相对 posix
/// 路径）。源目录不存在 → 空表。.db/.sqlite 额外并入 `-wal` 字节——已提交的
/// SQLite 变更在 checkpoint 前驻留 WAL，只为观察「是否写过」不做备份快照（TS
/// 同源注释语义）。
pub async fn capture_work_source_state(
    project_root: &Path,
    work_id: &str,
) -> Result<BTreeMap<String, String>, SyncError> {
    let root = work_directory(project_root, work_id)
        .map_err(|error| SyncError::Invalid(error.to_string()))?
        .join("source");
    match std::fs::symlink_metadata(&root) {
        Ok(_) => {}
        // TS：lstat ENOENT → 空 Map；其余错误上抛。
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(error) => return Err(SyncError::Io(error.to_string())),
    }
    let files = list_files(&root)?;
    let mut state = BTreeMap::new();
    for file in files {
        let path = root.join(&file);
        let bytes = tokio::fs::read(&path)
            .await
            .map_err(|error| SyncError::Io(error.to_string()))?;
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let extension = path
            .extension()
            .map(|ext| ext.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if extension == "db" || extension == "sqlite" {
            if let Ok(wal) = tokio::fs::read(path_with_wal_suffix(&path)).await {
                hasher.update(&wal);
            }
        }
        let digest = hasher.finalize();
        state.insert(
            to_posix_path(&format!("source/{file}")),
            digest.iter().map(|byte| format!("{byte:02x}")).collect::<String>(),
        );
    }
    Ok(state)
}

fn path_with_wal_suffix(path: &Path) -> PathBuf {
    let os = path.to_string_lossy().into_owned();
    PathBuf::from(format!("{os}-wal"))
}

/// TS `changedWorkSourcePaths`：前后指纹并集中哈希不同的路径，全局排序。
pub async fn changed_work_source_paths(
    project_root: &Path,
    work_id: &str,
    before: &BTreeMap<String, String>,
) -> Result<Vec<String>, SyncError> {
    let after = capture_work_source_state(project_root, work_id).await?;
    let mut changed: Vec<String> = before
        .keys()
        .chain(after.keys())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|path| before.get(*path) != after.get(*path))
        .cloned()
        .collect();
    changed.sort();
    Ok(changed)
}

/// TS `relative(join("works", workId, "source"), relativePath)`：前缀剥离 + 归一化
/// 后的 `..` 段检查（TS path.relative 归一化输出再判 "../" 前缀的等价实现）。
fn relative_to_source(prefix: &str, relative_path: &str) -> Result<String, SyncError> {
    let source_prefix = format!("{prefix}/");
    let path = relative_path
        .strip_prefix(&source_prefix)
        .ok_or_else(|| SyncError::Invalid("Artifact write must remain inside Work source".to_string()))?;
    if to_posix_path(path).split('/').any(|segment| segment == "..") {
        return Err(SyncError::Invalid("Artifact write must remain inside Work source".to_string()));
    }
    Ok(to_posix_path(path))
}

pub fn to_posix_path(path: &str) -> String {
    path.replace('\\', "/")
}

fn file_content_bytes(content: &FileContent) -> Vec<u8> {
    match content {
        FileContent::Text(text) => text.as_bytes().to_vec(),
        FileContent::Bytes(bytes) => bytes.clone(),
    }
}

pub fn hex_sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// TS `revisionIdFor`：`rev-<checksum 前 16 hex>`。
fn revision_id_for(checksum: &str) -> String {
    format!("rev-{}", &checksum["sha256:".len().."sha256:".len() + 16])
}

/// TS `revisionSnapshotPath`：`revisions/<artifactId>/<revisionId><ext>`——ext 含
/// 点（extname 形态）；无扩展名文件无点。
fn revision_snapshot_path(artifact_id: &str, revision_id: &str, source_path: &str) -> String {
    let extension = extension_of(source_path).to_lowercase();
    if extension.is_empty() {
        format!("revisions/{artifact_id}/{revision_id}")
    } else {
        format!("revisions/{artifact_id}/{revision_id}.{extension}")
    }
}

fn snapshot_write(work_id: &str, snapshot_path: &str, content: Vec<u8>) -> AtomicFileWrite {
    AtomicFileWrite {
        relative_path: format!("works/{work_id}/{snapshot_path}"),
        content: FileContent::Bytes(content),
    }
}

/// Node `extname` 等价：最后一个不在首位的 `.` 之后（dotfile 无扩展名）。
fn extension_of(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    match name.rfind('.') {
        Some(index) if index > 0 => &name[index + 1..],
        _ => "",
    }
}

/// TS `artifactIdFor`：可读段清洗（保留 a-zA-Z0-9·CJK·._-；`giu` 使 a-z 类含大写）
/// + 连续非法段合并为 `-`（TS `+` 量词）+ 截 64 + `"artifact"` 兜底 + `-<hash 前 10 hex>`。
pub fn artifact_id_for(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    let stem = match extension_of(name) {
        "" => name.to_string(),
        extension => name[..name.len() - extension.len() - 1].to_string(),
    };
    let mut readable = String::with_capacity(stem.len());
    let mut last_separator = false;
    for ch in stem.chars() {
        let keep = ch.is_ascii_alphanumeric()
            || ch == '.'
            || ch == '_'
            || ch == '-'
            || ('\u{4e00}'..='\u{9fff}').contains(&ch);
        if keep {
            readable.push(ch);
            last_separator = false;
        } else if !last_separator {
            readable.push('-');
            last_separator = true;
        }
    }
    let trimmed = readable.trim_matches('-');
    let mut sliced: String = trimmed.chars().take(64).collect();
    if sliced.is_empty() {
        sliced = "artifact".to_string();
    }
    let hash = hex_sha256(path.as_bytes())[..10].to_string();
    format!("{sliced}-{hash}")
}

/// TS `ARTIFACT_FILE_KINDS`（legacy 分类器——迁移期宿主元数据解码专用）。
fn legacy_source_artifact_kind(path: &str) -> String {
    const NAMED: &[(&str, &str)] = &[
        ("manifest.json", "manifest"),
        ("glossary.json", "glossary"),
        ("review-report.md", "review"),
        ("script.md", "script"),
        ("storyboard.md", "storyboard"),
        ("image-prompts.md", "image-prompt"),
        ("assets.json", "asset-manifest"),
        ("story-graph.json", "story-graph"),
    ];
    for (name, kind) in NAMED {
        if path.ends_with(name) {
            return (*kind).to_string();
        }
    }
    if path.contains("translated") {
        return "translation-chapter".into();
    }
    if path.contains("source") {
        return "source-chapter".into();
    }
    if is_image_extension(path) {
        return "image".into();
    }
    "file".into()
}

fn is_image_extension(path: &str) -> bool {
    let extension = extension_of(path).to_lowercase();
    matches!(extension.as_str(), "png" | "jpg" | "jpeg" | "webp" | "gif")
}

/// TS `artifactKindFor`（注意 NAMED 是 Map 精确键匹配，非后缀匹配）。
pub fn artifact_kind_for(work_path: &str) -> String {
    let normalized = to_posix_path(work_path);
    let path = normalized
        .strip_prefix("source/")
        .map(str::to_string)
        .unwrap_or(normalized);
    const NAMED: &[(&str, &str)] = &[
        ("manifest.json", "manifest"),
        ("glossary.json", "glossary"),
        ("review-report.md", "review"),
        ("script.md", "script"),
        ("storyboard.md", "storyboard"),
        ("image-prompts.md", "image-prompt"),
        ("assets.json", "asset-manifest"),
        ("story-graph.json", "story-graph"),
    ];
    for (name, kind) in NAMED {
        if path == *name {
            return (*kind).to_string();
        }
    }
    if is_image_extension(&path) {
        return "image".into();
    }
    if path.starts_with("translated/") && path.ends_with(".json") {
        return "translation-chapter".into();
    }
    if path.starts_with("source/") && path.ends_with(".json") {
        return "source-chapter".into();
    }
    // `^chapters/\d+(?:_[^/]+)?\.md$`
    if let Some(file) = path.strip_prefix("chapters/") {
        if let Some(number) = file.strip_suffix(".md") {
            let number = number.split_once('_').map_or(number, |(head, _)| head);
            if !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit()) {
                return "chapter".into();
            }
        }
    }
    "file".into()
}

/// TS `contentTypeFor`。
pub fn content_type_for(path: &str) -> &'static str {
    match extension_of(path).to_lowercase().as_str() {
        "md" => "text/markdown",
        "txt" => "text/plain",
        "html" => "text/html",
        "ink" => "text/plain",
        "json" | "jsonl" => "application/json",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        _ => "application/octet-stream",
    }
}

/// TS `listFiles`：递归目录扫描；跳过 `.write.lock`/`-wal`/`-shm`/符号链接；
/// 键 = source 根相对 posix 路径。（收集顺序无关紧要——调用方 union 后全局排序，
/// TS 同：`[...new Set([...listFiles, ...pending])].sort()`。迭代栈遍历避免
/// async 递归装箱。）
fn list_files(root: &Path) -> Result<Vec<String>, SyncError> {
    let mut files = Vec::new();
    let mut stack: Vec<PathBuf> = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        let mut paths: Vec<PathBuf> = std::fs::read_dir(&directory)
            .map_err(|error| SyncError::Io(error.to_string()))?
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .collect();
        paths.sort_by_key(|path| path.file_name().map(|name| name.to_string_lossy().into_owned()));
        for path in paths {
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            if name == ".write.lock"
                || name.ends_with(".db-wal")
                || name.ends_with(".db-shm")
                || name.ends_with(".sqlite-wal")
                || name.ends_with(".sqlite-shm")
            {
                continue;
            }
            let meta = std::fs::symlink_metadata(&path)
                .map_err(|error| SyncError::Io(error.to_string()))?;
            if meta.file_type().is_symlink() {
                continue;
            }
            if meta.is_dir() {
                stack.push(path);
            } else if meta.is_file() {
                let relative = path
                    .strip_prefix(root)
                    .map_err(|_| SyncError::Invalid("source scan escaped root".into()))?;
                files.push(to_posix_path(&relative.to_string_lossy()));
            }
        }
    }
    Ok(files)
}

/// TS `readArtifactBytes`：.db/.sqlite = sqlite backup 快照读（临时库承接，
/// WAL/未 checkpoint 页随备份合并）；其余直读。
async fn read_artifact_bytes(path: &Path) -> Result<Vec<u8>, SyncError> {
    let extension = path
        .extension()
        .map(|ext| ext.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if extension != "db" && extension != "sqlite" {
        return tokio::fs::read(path)
            .await
            .map_err(|error| SyncError::Io(error.to_string()));
    }
    let source_path = path.to_string_lossy().into_owned();
    let snapshot_path = std::env::temp_dir().join(format!(
        "inkos-sqlite-snapshot-{}-{}.db",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    tokio::task::spawn_blocking(move || -> Result<Vec<u8>, SyncError> {
        let guard = SnapshotCleanup(&snapshot_path);
        let source = rusqlite::Connection::open_with_flags(
            &source_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .map_err(|error| SyncError::Io(error.to_string()))?;
        let mut destination = rusqlite::Connection::open(&snapshot_path)
            .map_err(|error| SyncError::Io(error.to_string()))?;
        {
            let backup = rusqlite::backup::Backup::new(&source, &mut destination)
                .map_err(|error| SyncError::Io(error.to_string()))?;
            backup
                .run_to_completion(5, std::time::Duration::from_millis(5), None)
                .map_err(|error| SyncError::Io(error.to_string()))?;
        }
        drop(source);
        drop(destination);
        let bytes = std::fs::read(&snapshot_path).map_err(|error| SyncError::Io(error.to_string()))?;
        drop(guard);
        Ok(bytes)
    })
    .await
    .map_err(|error| SyncError::Io(error.to_string()))?
}

/// 临时快照文件守卫（panic/早退也清理）。
struct SnapshotCleanup<'a>(&'a Path);

impl Drop for SnapshotCleanup<'_> {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(self.0);
    }
}

/// evidence 载荷（adopt 环的 `work-artifacts-synced` 事件体）。
pub fn evidence_payload(evidence: &SyncEvidence) -> Value {
    json!({
        "workId": evidence.work_id,
        "profileId": evidence.profile_id,
        "accepted": evidence.accepted,
        "artifacts": evidence
            .artifacts
            .iter()
            .map(|artifact| {
                json!({
                    "artifactId": artifact.artifact_id,
                    "currentRevisionId": artifact.current_revision_id,
                    "latestRevisionId": artifact.latest_revision_id,
                })
            })
            .collect::<Vec<_>>(),
    })
}
