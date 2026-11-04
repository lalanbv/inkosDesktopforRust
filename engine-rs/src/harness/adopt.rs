//! adopt 能力环（884 号 M1b）：`POST /works/:id/artifacts/:aid/revisions/:rid/adopt`
//! 的服务端执行环——TS 侧 = `executeExplicitCapabilityTool`（单工具能力注册表 +
//! CreativeHarnessRuntime episode 编排 + runWithAgentTrajectory 纯 ALS 包裹）+
//! `createAdoptWorkRevisionTool`。881 §3.2③ 判定：adopt 路径的 trajectory 包裹
//! 不含 LLM 轨迹，Rust 直接执行工具体；episode 事件序列与归一化语义逐字对齐。
//!
//! 事件序列（成功）：episode-started → action-started → work-artifacts-synced →
//! action-completed → episode-completed。失败：action-failed → episode-failed
//!（finish 幂等失败吞掉、保留原错误——TS 同）。

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use serde::Serialize;
use serde_json::{json, Value};
use tokio::sync::Mutex;
use uuid::Uuid;

use super::action_error::ActionError;
use super::artifact_edit_policy::assert_generic_artifact_editable;
use super::artifact_reader::read_artifact_revision_bytes;
use super::artifact_validation::validated_artifact_writes;
use super::contracts::{CreativeEpisode, EpisodeStatus, HARNESS_VERSION, WorkManifest, WorkProfile};
use super::episode_store::CreativeEpisodeStore;
use super::profile_registry::create_built_in_work_profile_registry;
use super::source_sync::{hex_sha256, sync_work_source_artifacts, SyncEvidence, SyncWorkSourceInput};
use super::work_store::{load_work_manifest, work_directory};
use crate::utils::atomic_file_set::{AtomicFileWrite, FileContent};

/// TS binding：`{ capabilityId: "workspace", actionId: "adopt_work_revision",
/// profileId: work.profileId, risk: "recoverable-write" }`。
const ADOPT_CAPABILITY_ID: &str = "workspace";
const ADOPT_ACTION_ID: &str = "adopt_work_revision";
const ADOPT_RISK: &str = "recoverable-write";
/// `normalizeToolResult(context, …, summary = tool.label || tool.name)`。
const ADOPT_SUMMARY: &str = "Adopt work revision";

/// 进程内 per-work 写互斥（TS `runInWorkMutationQueue` 键 = projectRoot\0workId；
/// Rust 侧沿用 write_next 惯例：单进程引擎内 per-work tokio 锁，跨进程一致性由
/// atomic-file-set 原子提交保证——state/manager.rs 文件锁为暂缓件，先例同源）。
static WORK_MUTATION_LOCKS: std::sync::Mutex<Option<HashMap<String, Arc<Mutex<()>>>>> =
    std::sync::Mutex::new(None);

/// 取 per-work 锁句柄；调用方持有 Arc 同时 `lock().await`（guard 生命周期锚定
/// 本地 Arc，二者同作用域存活）。
fn acquire_work_mutation_lock(project_root: &Path, work_id: &str) -> Arc<Mutex<()>> {
    let key = format!("{}\0{work_id}", project_root.display());
    let mut guards = WORK_MUTATION_LOCKS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    guards
        .get_or_insert_with(HashMap::new)
        .entry(key)
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

// ── ActionResult（contracts.ts ActionResultSchema 的 adopt 面子集）───────

#[derive(Debug, Clone, Serialize)]
pub struct ActionArtifactRef {
    #[serde(rename = "workId")]
    pub work_id: String,
    #[serde(rename = "artifactId")]
    pub artifact_id: String,
    #[serde(rename = "revisionId", skip_serializing_if = "Option::is_none")]
    pub revision_id: Option<String>,
    #[serde(rename = "path", skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionResult {
    pub status: String,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    pub artifacts: Vec<ActionArtifactRef>,
    pub observations: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_receipts: Option<Vec<Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct AdoptError(pub ActionError);

pub struct AdoptOutcome {
    pub action_result: ActionResult,
    pub episode_id: String,
}

/// adopt 工具体（`createAdoptWorkRevisionTool.execute` 忠实移植）。
/// 返回 (details, 同步器 evidence——executeAction 的 evidence sink 将其转写为
/// `work-artifacts-synced` episode 事件)。
#[allow(clippy::type_complexity)]
async fn execute_adopt_tool(
    project_root: &Path,
    work_id: &str,
    artifact_id: &str,
    revision_id: &str,
    expected_current_revision_id: Option<&str>,
) -> Result<(Value, SyncEvidence), ActionError> {
    let work = load_work_manifest(project_root, work_id)
        .await
        .map_err(|error| ActionError::plain(error.to_string()))?;
    let profiles = create_built_in_work_profile_registry(project_root)
        .map_err(|error| ActionError::plain(error.to_string()))?;
    let profile: &WorkProfile = profiles
        .require(&work.profile_id)
        .map_err(|error| ActionError::plain(error.to_string()))?;
    let artifact = work.artifacts.iter().find(|item| item.id == artifact_id);
    let revision = artifact
        .and_then(|item| item.revisions.iter().find(|candidate| candidate.id == revision_id));
    let (Some(artifact), Some(revision)) = (artifact, revision) else {
        return Err(ActionError::coded("Unknown artifact revision", "ARTIFACT_NOT_FOUND"));
    };
    assert_generic_artifact_editable(&work, &revision.path, profile)?;
    if artifact.current_revision_id.as_deref() != expected_current_revision_id {
        return Err(ActionError::coded("Current revision changed", "ARTIFACT_REVISION_CONFLICT"));
    }
    if !revision.path.starts_with("source/") {
        return Err(ActionError::coded("Use this domain's revision action", "ARTIFACT_DERIVED"));
    }
    let work_root = work_directory(project_root, work_id)
        .map_err(|error| ActionError::plain(error.to_string()))?;
    let snapshot_file = revision.snapshot_path.clone().unwrap_or_else(|| revision.path.clone());
    let bytes = tokio::fs::read(work_root.join(&snapshot_file))
        .await
        .map_err(|error| ActionError::plain(error.to_string()))?;
    if format!("sha256:{}", hex_sha256(&bytes)) != revision.checksum {
        return Err(ActionError::coded(
            "Revision snapshot does not match recorded content",
            "ARTIFACT_SNAPSHOT_UNAVAILABLE",
        ));
    }
    let current_bytes: Option<Vec<u8>> = match &artifact.current_revision_id {
        Some(current_revision_id) => Some(
            read_artifact_revision_bytes(project_root, work_id, artifact_id, Some(current_revision_id))
                .await
                .map_err(|error| ActionError::plain(error.to_string()))?,
        ),
        None => None,
    };
    // TS bytes.toString("utf8") 是有损解码——lossy 对齐。
    let writes: Vec<AtomicFileWrite> =
        if revision.content_type.starts_with("text/") || revision.content_type == "application/json"
        {
            validated_artifact_writes(
                &work,
                &revision.path,
                &String::from_utf8_lossy(&bytes),
                Some(profile),
                current_bytes
                    .as_deref()
                    .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
                    .as_deref(),
            )?
        } else {
            vec![AtomicFileWrite {
                relative_path: format!("works/{}/{}", work_id, revision.path),
                content: FileContent::Bytes(bytes),
            }]
        };
    // TS：revision.path === "source/final/short-story.json" 时以文档 storyTitle
    // 更新 work 标题——该路径在 validatedArtifactWrites 已被 884 号 fail-closed
    // 边界拒绝，title 覆写不可达；M3 移植 authority 再生后随分支接通。
    let sync = sync_work_source_artifacts(SyncWorkSourceInput {
        project_root,
        work_id,
        accept: true,
        accept_paths: Some(vec![revision.path.clone()]),
        episode_id: None,
        updated_at: None,
        writes,
        title: None,
        status: None,
    })
    .await
    .map_err(|error| ActionError::plain(error.to_string()))?;
    let details = json!({
        "kind": "work_revision_adopted",
        "workId": work_id,
        "artifactId": artifact_id,
        "revisionId": revision_id,
        "previousRevisionId": artifact.current_revision_id,
    });
    Ok((details, sync.evidence))
}

/// `executeExplicitCapabilityTool` 的 adopt 面等价（episode 环 + 归一化）。
pub async fn adopt_work_revision(
    project_root: &Path,
    work_id: &str,
    artifact_id: &str,
    revision_id: &str,
    expected_current_revision_id: Option<&str>,
) -> Result<AdoptOutcome, AdoptError> {
    let store_path = project_root.join(".inkos").join("harness.sqlite");
    let episodes = CreativeEpisodeStore::new(&store_path)
        .map_err(|error| AdoptError(ActionError::plain(error.to_string())))?;
    let outcome =
        run_adopt_ring(project_root, &episodes, work_id, artifact_id, revision_id, expected_current_revision_id)
            .await;
    episodes.close();
    outcome
}

async fn run_adopt_ring(
    project_root: &Path,
    episodes: &CreativeEpisodeStore,
    work_id: &str,
    artifact_id: &str,
    revision_id: &str,
    expected_current_revision_id: Option<&str>,
) -> Result<AdoptOutcome, AdoptError> {
    let now = crate::utils::utc_time::utc_now_iso();
    // TS executeExplicitCapabilityTool：work/profile 解析（binding.profileId =
    // work.profileId → profile 恒为 work 的 profile）。
    let work = load_work_manifest(project_root, work_id)
        .await
        .map_err(|error| AdoptError(ActionError::plain(error.to_string())))?;
    let profiles = create_built_in_work_profile_registry(project_root)
        .map_err(|error| AdoptError(ActionError::plain(error.to_string())))?;
    let profile = profiles
        .require(&work.profile_id)
        .map_err(|error| AdoptError(ActionError::plain(error.to_string())))?
        .clone();

    // TS startEpisode：recoverInterruptedEpisodes 先行 → create → episode-started。
    episodes
        .recover_interrupted_episodes(&now, "studio-process-restarted")
        .map_err(|error| AdoptError(ActionError::plain(error)))?;
    let episode = CreativeEpisode {
        version: HARNESS_VERSION,
        id: format!("episode-{}", Uuid::new_v4()),
        work_id: Some(work_id.to_string()),
        profile_id: Some(profile.id.clone()),
        status: EpisodeStatus::Running,
        started_at: now,
        completed_at: None,
    };
    episodes
        .create(&episode)
        .map_err(|error| AdoptError(ActionError::plain(error.to_string())))?;
    let baseline = work;
    let started_payload = json!({
        "profileId": profile.id,
        "baseline": baseline.artifacts.iter().map(|artifact| json!({
            "artifactId": artifact.id,
            "revisionId": artifact.current_revision_id,
        })).collect::<Vec<_>>(),
    });
    episodes
        .append(
            &episode.id,
            "episode-started",
            episode.work_id.as_deref(),
            None,
            None,
            &started_payload,
            &episode.started_at,
        )
        .map_err(|error| AdoptError(ActionError::plain(error)))?;

    let action_execution_id = format!("action-{}", Uuid::new_v4());
    // TS executeAction：execute 前事件序 + evidence sink 转写 + 互斥写锁。
    let ring_result: Result<(Value, SyncEvidence), ActionError> = async {
        // 授权面：source=explicit + confirmed=true → isActionAuthorized 恒真，
        // 但 profile 缺能力仍拒（executeAction 首检查）。
        if !profile.capability_ids.iter().any(|id| id == ADOPT_CAPABILITY_ID) {
            return Err(ActionError::plain(format!(
                "Profile \"{}\" cannot use capability \"{ADOPT_CAPABILITY_ID}\"",
                profile.id
            )));
        }
        episodes
            .require_episode(&episode.id)
            .map_err(ActionError::plain)?;
        let work_lock = acquire_work_mutation_lock(project_root, work_id);
        let _guard = work_lock.lock().await;
        episodes
            .append(
                &episode.id,
                "action-started",
                episode.work_id.as_deref(),
                Some(ADOPT_CAPABILITY_ID),
                Some(ADOPT_ACTION_ID),
                &json!({
                    "actionExecutionId": action_execution_id,
                    "risk": ADOPT_RISK,
                    "source": "explicit",
                    "parameters": {
                        "artifactId": artifact_id,
                        "revisionId": revision_id,
                        "expectedCurrentRevisionId": expected_current_revision_id,
                    },
                }),
                &crate::utils::utc_time::utc_now_iso(),
            )
            .map_err(ActionError::plain)?;
        let (details, evidence) = execute_adopt_tool(
            project_root,
            work_id,
            artifact_id,
            revision_id,
            expected_current_revision_id,
        )
        .await?;
        Ok((details, evidence))
    }
    .await;

    match ring_result {
        Ok((details, evidence)) => {
            // evidence sink：work-artifacts-synced（payload + actionExecutionId）。
            let mut synced = super::source_sync::evidence_payload(&evidence);
            if let Value::Object(map) = &mut synced {
                map.insert("actionExecutionId".into(), Value::String(action_execution_id.clone()));
            }
            // adopt 路径 episode 已绑定 work（bindWork 仅对未绑定 episode 生效——TS 同）。
            episodes
                .append(
                    &episode.id,
                    "work-artifacts-synced",
                    episode.work_id.as_deref(),
                    Some(ADOPT_CAPABILITY_ID),
                    Some(ADOPT_ACTION_ID),
                    &synced,
                    &crate::utils::utc_time::utc_now_iso(),
                )
                .map_err(|error| AdoptError(ActionError::plain(error)))?;
            let after = load_work_manifest(project_root, work_id)
                .await
                .map_err(|error| AdoptError(ActionError::plain(error.to_string())))?;
            let artifacts = artifact_refs_between(Some(&baseline), &after);
            let result = ActionResult {
                status: "success".into(),
                summary: ADOPT_SUMMARY.into(),
                content: Some(format!("Adopted revision {revision_id}.")),
                artifacts,
                observations: Vec::new(),
                operation_receipts: None,
                data: Some(details),
            };
            episodes
                .append(
                    &episode.id,
                    "action-completed",
                    episode.work_id.as_deref(),
                    Some(ADOPT_CAPABILITY_ID),
                    Some(ADOPT_ACTION_ID),
                    &json!({
                        "actionExecutionId": action_execution_id,
                        "result": serde_json::to_value(&result).unwrap_or(Value::Null),
                        "status": result.status,
                        "summary": result.summary,
                        "artifactCount": result.artifacts.len(),
                        "observationCount": result.observations.len(),
                    }),
                    &crate::utils::utc_time::utc_now_iso(),
                )
                .map_err(|error| AdoptError(ActionError::plain(error)))?;
            finish_episode(episodes, &episode.id, "completed")
                .map_err(|error| AdoptError(ActionError::plain(error)))?;
            Ok(AdoptOutcome { action_result: result, episode_id: episode.id })
        }
        Err(error) => {
            // TS catch：action-failed（facts 提取合并）→ episode-failed（finish
            // 失败吞掉、保留原错误）。
            let mut payload = json!({
                "actionExecutionId": action_execution_id,
                "error": error.message,
            });
            if let Value::Object(facts) = error.failure_facts() {
                if let Value::Object(existing) = &mut payload {
                    for (key, value) in facts {
                        existing.entry(key).or_insert(value);
                    }
                }
            }
            let _ = episodes.append(
                &episode.id,
                "action-failed",
                episode.work_id.as_deref(),
                Some(ADOPT_CAPABILITY_ID),
                Some(ADOPT_ACTION_ID),
                &payload,
                &crate::utils::utc_time::utc_now_iso(),
            );
            let _ = finish_episode(episodes, &episode.id, "failed");
            Err(AdoptError(error))
        }
    }
}

/// TS `finishEpisode`：episode-<status> 事件 + finish 落库。
fn finish_episode(
    episodes: &CreativeEpisodeStore,
    episode_id: &str,
    status: &str,
) -> Result<(), String> {
    let now = crate::utils::utc_time::utc_now_iso();
    episodes.append(episode_id, &format!("episode-{status}"), None, None, None, &json!({}), &now)?;
    let final_status = match status {
        "completed" => EpisodeStatus::Completed,
        "failed" => EpisodeStatus::Failed,
        _ => EpisodeStatus::Cancelled,
    };
    episodes.finish(episode_id, final_status, &now)?;
    Ok(())
}

/// TS `normalizeToolResult` 的 artifact ref 组装（before/after currentRevisionId
/// 差异面——before = ring 起点清单快照）。
fn artifact_refs_between(before: Option<&WorkManifest>, after: &WorkManifest) -> Vec<ActionArtifactRef> {
    let prior_revisions: HashMap<&str, Option<&str>> = before
        .map(|work| {
            work.artifacts
                .iter()
                .map(|artifact| (artifact.id.as_str(), artifact.current_revision_id.as_deref()))
                .collect()
        })
        .unwrap_or_default();
    let mut refs = Vec::new();
    for artifact in &after.artifacts {
        if prior_revisions.get(artifact.id.as_str()).copied().flatten()
            == artifact.current_revision_id.as_deref()
        {
            continue;
        }
        let revision = artifact
            .revisions
            .iter()
            .find(|candidate| Some(&candidate.id) == artifact.current_revision_id.as_ref());
        let previous_revision = before.and_then(|work| {
            work.artifacts
                .iter()
                .find(|candidate| candidate.id == artifact.id)
                .and_then(|candidate| {
                    candidate
                        .revisions
                        .iter()
                        .find(|revision| {
                            Some(revision.id.as_str())
                                == prior_revisions.get(artifact.id.as_str()).copied().flatten()
                        })
                })
        });
        refs.push(ActionArtifactRef {
            work_id: after.id.clone(),
            artifact_id: artifact.id.clone(),
            revision_id: artifact.current_revision_id.clone(),
            path: revision
                .map(|revision| revision.path.clone())
                .or_else(|| previous_revision.map(|revision| revision.path.clone())),
        });
    }
    refs
}
