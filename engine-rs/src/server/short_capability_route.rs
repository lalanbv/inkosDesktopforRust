//! 短篇 harness 能力动作面（894 号 M4）：TS `harness/tools/short-production.ts`
//! 四阶段动作（plan/draft/review/package_short_fiction）+ `revise_short_fiction`
//! （agent-tools.ts）的引擎侧路由——episode 环（adopt.rs 同款事件序）+ evidence
//! sink 转写 + profile 能力守卫 + work 互斥锁。
//!
//! TS 结果面逐字：
//! - stage：text = `${stage} ${stageStatus}: ${artifactPaths.join(", ")}`，
//!   details kind = short_stage_completed | short_stage_failed
//! - revise：text = `Revised short fiction "<id>" with N review observation(s).`，
//!   details kind = short_fiction_revised

use std::path::Path;
use std::sync::Arc;

use axum::extract::State;
use axum::response::IntoResponse;
use axum::Json;
use serde_json::{json, Value};

use crate::harness::contracts::{CreativeEpisode, EpisodeStatus, HARNESS_VERSION};
use crate::harness::episode_store::CreativeEpisodeStore;
use crate::harness::source_sync::SyncWorkSourceInput;
use crate::harness::work_store::load_work_manifest;
use crate::production::short_fiction_runner::{
    run_short_fiction_stage, EvidenceSink, ShortFictionRunOptions, ShortFictionRunRuntimes,
    ShortProductionStage, ShortRunExecutionScope, ShortRunError,
};
use crate::server::agent_production::short_production_agents;
use crate::server::books_routes::BooksRuntime;
use crate::utils::utc_time::utc_now_iso;

const CAPABILITY_ID: &str = "short-fiction";

/// 能力动作执行结果（ToolResult 面——text + details）。
pub struct CapabilityOutcome {
    pub text: String,
    pub details: Value,
}

/// 动作入口（HTTP 层）：episode 环 + 动作分发。
pub async fn run_short_capability_action(
    root: &Path,
    runtime: &BooksRuntime,
    work_id: &str,
    action_id: &str,
    params: &Value,
) -> Result<CapabilityOutcome, String> {
    let store_path = root.join(".inkos").join("harness.sqlite");
    let episodes = Arc::new(
        CreativeEpisodeStore::new(&store_path).map_err(|error| error.to_string())?,
    );
    let outcome = run_action_ring(root, runtime, &episodes, work_id, action_id, params).await;
    episodes.close();
    outcome
}

async fn run_action_ring(
    root: &Path,
    runtime: &BooksRuntime,
    episodes: &Arc<CreativeEpisodeStore>,
    work_id: &str,
    action_id: &str,
    params: &Value,
) -> Result<CapabilityOutcome, String> {
    let now = utc_now_iso();
    let work = load_work_manifest(root, work_id).await.map_err(|error| error.to_string())?;
    let profiles = crate::harness::profile_registry::create_built_in_work_profile_registry(root)
        .map_err(|error| error.to_string())?;
    let profile = profiles.require(&work.profile_id).map_err(|error| error.to_string())?.clone();
    if !profile.capability_ids.iter().any(|id| id == CAPABILITY_ID) {
        return Err(format!(
            "Profile \"{}\" cannot use capability \"{CAPABILITY_ID}\"",
            profile.id
        ));
    }
    episodes
        .recover_interrupted_episodes(&now, "studio-process-restarted")
        .map_err(|error| error.to_string())?;
    let episode = CreativeEpisode {
        version: HARNESS_VERSION,
        id: format!("episode-{}", uuid::Uuid::new_v4()),
        work_id: Some(work_id.to_string()),
        profile_id: Some(profile.id.clone()),
        status: EpisodeStatus::Running,
        started_at: now.clone(),
        completed_at: None,
    };
    episodes.create(&episode).map_err(|error| error.to_string())?;
    let started_payload = json!({
        "profileId": profile.id,
        "baseline": work.artifacts.iter().map(|artifact| json!({
            "artifactId": artifact.id,
            "revisionId": artifact.current_revision_id,
        })).collect::<Vec<_>>(),
    });
    episodes
        .append(&episode.id, "episode-started", episode.work_id.as_deref(), None, None, &started_payload, &episode.started_at)
        .map_err(|error| error.to_string())?;

    let action_execution_id = format!("action-{}", uuid::Uuid::new_v4());
    // evidence sink：观测点 → episode 事件（recordExecutionEvidence 转写面——
    // TS sink 的 append 等价；Arc 共享同一 store 连接）。
    let episodes_for_sink = Arc::clone(episodes);
    let episode_for_sink = episode.clone();
    let action_for_sink = action_id.to_string();
    let evidence_sink: EvidenceSink = Arc::new(move |evidence_type: &str, payload: &Value| {
        let _ = episodes_for_sink.append(
            &episode_for_sink.id,
            evidence_type,
            episode_for_sink.work_id.as_deref(),
            Some(CAPABILITY_ID),
            Some(&action_for_sink),
            payload,
            &utc_now_iso(),
        );
    });
    let execution_scope = ShortRunExecutionScope {
        author_request: params.get("instruction").and_then(Value::as_str).map(str::to_string),
        baseline_work: Some(Arc::new(load_work_manifest(root, work_id).await.map_err(|error| error.to_string())?)),
        evidence_sink: Some(evidence_sink),
    };

    let lock = crate::harness::adopt::acquire_work_mutation_lock(root, work_id);
    let _guard = lock.lock().await;
    let ring_result: Result<CapabilityOutcome, ShortRunError> = async {
        episodes
            .append(
                &episode.id,
                "action-started",
                episode.work_id.as_deref(),
                Some(CAPABILITY_ID),
                Some(action_id),
                &json!({
                    "actionExecutionId": action_execution_id,
                    "source": "explicit",
                    "parameters": params,
                }),
                &utc_now_iso(),
            )
            .map_err(|error| ShortRunError::new("EPISODE_EVENT_FAILED", error.to_string()))?;
        let outcome = execute_action(runtime, root, work_id, action_id, params, &execution_scope).await?;
        // evidence sink：work-artifacts-synced（动作后登记面转写）。
        let synced = crate::harness::source_sync::sync_work_source_artifacts(SyncWorkSourceInput {
            project_root: root,
            work_id,
            accept: false,
            accept_paths: None,
            episode_id: Some(&episode.id),
            updated_at: None,
            writes: Vec::new(),
            title: None,
            status: None,
        })
        .await
        .map_err(|error| ShortRunError::new("SOURCE_SYNC_FAILED", error.to_string()))?;
        let mut payload = crate::harness::source_sync::evidence_payload(&synced.evidence);
        if let Value::Object(map) = &mut payload {
            map.insert("actionExecutionId".into(), Value::String(action_execution_id.clone()));
        }
        episodes
            .append(
                &episode.id,
                "work-artifacts-synced",
                episode.work_id.as_deref(),
                Some(CAPABILITY_ID),
                Some(action_id),
                &payload,
                &utc_now_iso(),
            )
            .map_err(|error| ShortRunError::new("EPISODE_EVENT_FAILED", error.to_string()))?;
        Ok(outcome)
    }
    .await;

    match ring_result {
        Ok(outcome) => {
            let completed_at = utc_now_iso();
            episodes
                .append(
                    &episode.id,
                    "action-completed",
                    episode.work_id.as_deref(),
                    Some(CAPABILITY_ID),
                    Some(action_id),
                    &json!({ "actionExecutionId": action_execution_id, "status": "success" }),
                    &completed_at,
                )
                .map_err(|error| error.to_string())?;
            finish_episode(episodes, &episode, &completed_at)?;
            Ok(outcome)
        }
        Err(error) => {
            let completed_at = utc_now_iso();
            episodes
                .append(
                    &episode.id,
                    "action-completed",
                    episode.work_id.as_deref(),
                    Some(CAPABILITY_ID),
                    Some(action_id),
                    &json!({ "actionExecutionId": action_execution_id, "status": "error", "error": error.message }),
                    &completed_at,
                )
                .map_err(|e| e.to_string())?;
            finish_episode(episodes, &episode, &completed_at)?;
            Err(error.message)
        }
    }
}

fn finish_episode(
    episodes: &CreativeEpisodeStore,
    episode: &CreativeEpisode,
    completed_at: &str,
) -> Result<(), String> {
    episodes
        .append(
            &episode.id,
            "episode-completed",
            episode.work_id.as_deref(),
            None,
            None,
            &json!({}),
            completed_at,
        )
        .map_err(|error| error.to_string())?;
    episodes
        .finish(&episode.id, EpisodeStatus::Completed, completed_at)
        .map_err(|error| error.to_string())
        .map(|_| ())
}

// ── 参数契约（895 号 M4 收尾）────────────────────────────────────
// TS per-action schema（short-production.ts OutlineParameters=
// Omit(Parameters,['maxChaptersPerCall','reviewScope']) / DraftParameters=
// Omit(Parameters,['workId','reviewScope']) / ReviewParameters=
// Pick(Parameters,['workId','reviewScope']) / PackageParameters=
// Pick(Parameters,['workId','direction']) + agent-tools.ts ShortRevisionParams）
// 的 Value.Decode 语义：声明键类型+minimum 强制，未声明键透传（TypeBox
// additionalProperties 缺省开放）。错误文案镜像 capability-registry.ts
// decode 失败形态（path: message 分号拼接）。

fn capability_params_error(action_id: &str, violations: &[String]) -> ShortRunError {
    ShortRunError::new(
        "CAPABILITY_PARAMS_INVALID",
        format!(
            "Capability {CAPABILITY_ID}.{action_id} params decode failed: {}",
            violations.join("; ")
        ),
    )
}

fn require_uint_min1(params: &Value, key: &str, violations: &mut Vec<String>) {
    if let Some(value) = params.get(key) {
        match value.as_u64() {
            Some(n) if n >= 1 => {}
            _ => violations.push(format!("/{key}: Expected integer >= 1")),
        }
    }
}

/// `Type.Union([Type.Integer({minimum:1}),Type.Null()])`——null 合法（继承面）。
fn require_length_or_null(params: &Value, key: &str, violations: &mut Vec<String>) {
    if let Some(value) = params.get(key) {
        if value.is_null() {
            return;
        }
        match value.as_u64() {
            Some(n) if n >= 1 => {}
            _ => violations.push(format!("/{key}: Expected integer >= 1 or null")),
        }
    }
}

fn require_string(params: &Value, key: &str, violations: &mut Vec<String>) {
    if let Some(value) = params.get(key) {
        if value.as_str().is_none() {
            violations.push(format!("/{key}: Expected string"));
        }
    }
}

fn validate_stage_params(
    stage: ShortProductionStage,
    action_id: &str,
    params: &Value,
) -> Result<(), ShortRunError> {
    let mut violations: Vec<String> = Vec::new();
    match stage {
        ShortProductionStage::Outline | ShortProductionStage::Draft => {
            require_string(params, "direction", &mut violations);
            require_uint_min1(params, "chapters", &mut violations);
            require_uint_min1(params, "charsPerChapter", &mut violations);
            require_length_or_null(params, "minChapterLength", &mut violations);
            require_length_or_null(params, "openingHookChars", &mut violations);
            require_length_or_null(params, "maxChapterLength", &mut violations);
            if let Some(value) = params.get("coverRequired") {
                if !value.is_boolean() && !value.is_null() {
                    violations.push("/coverRequired: Expected boolean or null".to_string());
                }
            }
            if stage == ShortProductionStage::Draft {
                require_uint_min1(params, "maxChaptersPerCall", &mut violations);
            }
        }
        ShortProductionStage::Review => {
            require_string(params, "reviewScope", &mut violations);
        }
        ShortProductionStage::Package => {
            require_string(params, "direction", &mut violations);
        }
    }
    if violations.is_empty() {
        Ok(())
    } else {
        Err(capability_params_error(action_id, &violations))
    }
}

fn validate_revise_params(params: &Value) -> Result<(), ShortRunError> {
    const ACTION_ID: &str = "revise_short_fiction";
    let mut violations: Vec<String> = Vec::new();
    for key in ["instruction", "resumeOperationId"] {
        if let Some(value) = params.get(key) {
            match value.as_str() {
                Some(text) if !text.is_empty() => {}
                _ => violations.push(format!("/{key}: Expected string minLength 1")),
            }
        }
    }
    if let Some(value) = params.get("restartPendingRevision") {
        if !value.is_boolean() {
            violations.push("/restartPendingRevision: Expected boolean".to_string());
        }
    }
    for key in ["chapterCount", "charsPerChapter", "maxChapterLength"] {
        require_uint_min1(params, key, &mut violations);
    }
    if let Some(value) = params.get("chapterNumbers") {
        let valid = value
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .all(|item| item.as_u64().map(|n| n >= 1).unwrap_or(false))
            })
            .unwrap_or(false);
        if !valid {
            violations.push("/chapterNumbers: Expected array of integer >= 1".to_string());
        }
    }
    if violations.is_empty() {
        Ok(())
    } else {
        Err(capability_params_error(ACTION_ID, &violations))
    }
}

async fn execute_action(
    runtime: &BooksRuntime,
    root: &Path,
    work_id: &str,
    action_id: &str,
    params: &Value,
    execution_scope: &ShortRunExecutionScope,
) -> Result<CapabilityOutcome, ShortRunError> {
    match action_id {
        "plan_short_fiction" => {
            stage_action(runtime, root, work_id, action_id, ShortProductionStage::Outline, params, execution_scope).await
        }
        "draft_short_fiction" => {
            stage_action(runtime, root, work_id, action_id, ShortProductionStage::Draft, params, execution_scope).await
        }
        "review_short_fiction" => {
            stage_action(runtime, root, work_id, action_id, ShortProductionStage::Review, params, execution_scope).await
        }
        "package_short_fiction" => {
            stage_action(runtime, root, work_id, action_id, ShortProductionStage::Package, params, execution_scope).await
        }
        "revise_short_fiction" => revise_action(runtime, root, work_id, params, execution_scope).await,
        _ => Err(ShortRunError::new(
            "CAPABILITY_ACTION_UNKNOWN",
            format!("Unknown short-fiction capability action: {action_id}"),
        )),
    }
}

async fn stage_runtimes(runtime: &BooksRuntime) -> ShortFictionRunRuntimes {
    let router = runtime.effective_router().await;
    ShortFictionRunRuntimes {
        planner: short_production_agents(&router, "short-outline"),
        writer: short_production_agents(&router, "short-writer"),
        draft_review: short_production_agents(&router, "short-draft-review"),
        package: short_production_agents(&router, "short-package"),
    }
}

/// revise 链 runtimes（TS createShortFictionReviseTool L1836：writer 上下文 =
/// `short-reviser`——与 stage 链的 short-writer 分模型/温度面）。
async fn revise_runtimes(runtime: &BooksRuntime) -> ShortFictionRunRuntimes {
    let router = runtime.effective_router().await;
    ShortFictionRunRuntimes {
        planner: short_production_agents(&router, "short-outline"),
        writer: short_production_agents(&router, "short-reviser"),
        draft_review: short_production_agents(&router, "short-draft-review"),
        package: short_production_agents(&router, "short-package"),
    }
}

#[allow(clippy::too_many_arguments)]
async fn stage_action(
    runtime: &BooksRuntime,
    root: &Path,
    work_id: &str,
    action_id: &str,
    stage: ShortProductionStage,
    params: &Value,
    execution_scope: &ShortRunExecutionScope,
) -> Result<CapabilityOutcome, ShortRunError> {
    validate_stage_params(stage, action_id, params)?;
    let work = load_work_manifest(root, work_id).await?;
    let language = if work.language == "en" {
        crate::utils::language::WritingLanguage::En
    } else {
        crate::utils::language::WritingLanguage::Zh
    };
    // direction 回退链（TS L45 逐字）：params.direction 在场即用（含空串——
    // ?? 不穿透空值）；缺席 → production-state intent（含空串）→ work.metadata.
    // intent → ""。outline 阶段全链空 = TS "An outline needs a creative direction"。
    let direction = match params.get("direction") {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Null) | None => {
            let source_dir = root.join("works").join(work_id).join("source");
            match crate::production::short_fiction::read_short_production_state(root, &source_dir) {
                Ok(Some(state)) => state.intent,
                Ok(None) => work
                    .metadata
                    .get("intent")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                Err(error) => return Err(ShortRunError::new("PRODUCTION_STATE_INVALID", error)),
            }
        }
        Some(_) => {
            return Err(capability_params_error(
                action_id,
                &["/direction: Expected string".to_string()],
            ))
        }
    };
    if stage == ShortProductionStage::Outline && direction.trim().is_empty() {
        return Err(ShortRunError::new(
            "SHORT_OUTLINE_DIRECTION_REQUIRED",
            "An outline needs a creative direction",
        ));
    }
    let field_u32 = |name: &str| params.get(name).and_then(Value::as_u64).map(|v| v as u32);
    let runtimes = stage_runtimes(runtime).await;
    let mut options = ShortFictionRunOptions::new(root, &direction, &runtimes);
    options.story_id = Some(work_id);
    options.title = Some(&work.title);
    // production 面（TS `stage==='outline'||stage==='draft' ? params : {}`）：
    // review/package 阶段不读产出参数（TS execute production={} 同语义）。
    let production = matches!(stage, ShortProductionStage::Outline | ShortProductionStage::Draft);
    options.chapter_count = production.then(|| field_u32("chapters")).flatten();
    options.chars_per_chapter = production.then(|| field_u32("charsPerChapter")).flatten();
    options.min_chapter_length = production.then(|| field_u32("minChapterLength")).flatten();
    options.max_chapter_length = production.then(|| field_u32("maxChapterLength")).flatten();
    options.opening_hook_chars = production.then(|| field_u32("openingHookChars")).flatten();
    options.max_chapters_per_call = production.then(|| field_u32("maxChaptersPerCall")).flatten();
    options.cover_required = production
        .then(|| params.get("coverRequired").and_then(Value::as_bool))
        .flatten();
    // reviewScope 全阶段透传（TS `'reviewScope' in params ? params.reviewScope : undefined`）。
    options.review_scope = params.get("reviewScope").and_then(Value::as_str);
    options.retry_stages = match stage {
        ShortProductionStage::Review => vec![crate::production::short_fiction_runner::ShortRetryStage::Review],
        ShortProductionStage::Package => vec![crate::production::short_fiction_runner::ShortRetryStage::Package],
        _ => Vec::new(),
    };
    options.language = Some(language);
    options.cover_model = None;
    options.execution = execution_scope.clone();
    let result = run_short_fiction_stage(&options, stage).await?;
    let stage_label = match stage {
        ShortProductionStage::Outline => "outline",
        ShortProductionStage::Draft => "draft",
        ShortProductionStage::Review => "review",
        ShortProductionStage::Package => "package",
    };
    let status_label = match result.stage_status {
        crate::production::short_fiction::ShortStageStatus::Completed => "completed",
        crate::production::short_fiction::ShortStageStatus::Failed => "failed",
    };
    let text = format!("{stage_label} {status_label}: {}", result.artifact_paths.join(", "));
    let mut details = serde_json::to_value(&result).unwrap_or(Value::Null);
    if let Some(map) = details.as_object_mut() {
        map.insert("workId".into(), json!(result.story_id));
        map.insert(
            "kind".into(),
            json!(if status_label == "failed" { "short_stage_failed" } else { "short_stage_completed" }),
        );
    }
    Ok(CapabilityOutcome { text, details })
}

async fn revise_action(
    runtime: &BooksRuntime,
    root: &Path,
    work_id: &str,
    params: &Value,
    execution_scope: &ShortRunExecutionScope,
) -> Result<CapabilityOutcome, ShortRunError> {
    validate_revise_params(params)?;
    let work = load_work_manifest(root, work_id).await?;
    let language = if work.language == "en" {
        crate::utils::language::WritingLanguage::En
    } else {
        crate::utils::language::WritingLanguage::Zh
    };
    let field_u32 = |name: &str| params.get(name).and_then(Value::as_u64).map(|v| v as u32);
    let direction = params.get("instruction").and_then(Value::as_str).unwrap_or_default();
    let runtimes = revise_runtimes(runtime).await;
    let mut options = ShortFictionRunOptions::new(root, direction, &runtimes);
    options.story_id = Some(work_id);
    // TS ShortRevisionParams：chapterCount（非 stage 面的 chapters）。
    options.chapter_count = field_u32("chapterCount");
    options.chars_per_chapter = field_u32("charsPerChapter");
    options.max_chapter_length = field_u32("maxChapterLength");
    options.cover = Some(false);
    options.resume_operation_id = params.get("resumeOperationId").and_then(Value::as_str);
    options.restart_pending_revision =
        params.get("restartPendingRevision").and_then(Value::as_bool).unwrap_or(false);
    options.revision_chapter_numbers = params
        .get("chapterNumbers")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_u64).map(|v| v as u32).collect::<Vec<_>>());
    options.language = Some(language);
    options.cover_model = None;
    options.execution = execution_scope.clone();
    let result =
        crate::production::short_fiction_runner::revise_short_fiction_production_locked(&options, root, work_id)
            .await?;
    let text = format!(
        "Revised short fiction \"{}\" with {} review observation(s).",
        work_id,
        result.observations.len()
    );
    let mut details = serde_json::to_value(&result).unwrap_or(Value::Null);
    if let Some(map) = details.as_object_mut() {
        map.insert("kind".into(), json!("short_fiction_revised"));
        map.insert("workId".into(), json!(work_id));
    }
    Ok(CapabilityOutcome { text, details })
}

/// POST /api/v1/works/:workId/capabilities/short-fiction/:actionId。
pub async fn post_short_capability_action(
    State(runtime): State<BooksRuntime>,
    axum::extract::Path((work_id, action_id)): axum::extract::Path<(String, String)>,
    Json(params): Json<Value>,
) -> axum::response::Response {
    let root = runtime.state.project_root().to_path_buf();
    match run_short_capability_action(&root, &runtime, &work_id, &action_id, &params).await {
        Ok(outcome) => (
            axum::http::StatusCode::OK,
            Json(json!({ "content": outcome.text, "details": outcome.details })),
        )
            .into_response(),
        Err(message) => (
            axum::http::StatusCode::BAD_GATEWAY,
            Json(json!({ "error": { "code": "CAPABILITY_ACTION_FAILED", "message": message } })),
        )
            .into_response(),
    }
}
