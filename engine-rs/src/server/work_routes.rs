//! work 域路由（884 号 M1b）：`packages/studio/src/api/server.ts` works/episodes/
//! profiles 九条端点的 Rust 腿（L1 读写面——881 §一 价值链）。
//!
//! 错误面契约（逐端点对齐 TS，勿统一抽象）：
//! - works/:id、artifacts、revisions GET = **局部 catch**：ENOENT→404、其余→400，
//!   形状 `{ error: "<message>" }`（**纯字符串**——revisions 端点里 ApiError(409)
//!   会被局部 catch 降级为 400，TS 同款行为）。
//! - /works、/episodes/:id、/profiles、adopt = **无局部 catch**：工具/存储错误 →
//!   全局 handler → 500 `{ error: { code: "INTERNAL_ERROR", message: "Unexpected
//!   server error." } }`（固定文案，TS onError 同款）。

use std::path::{Component, Path, PathBuf};

use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use base64::Engine as _;
use serde_json::{json, Value};

use super::books_routes::BooksRuntime;
use crate::harness::adopt::{adopt_work_revision, ActionResult};
use crate::harness::contracts::WorkManifest;
use crate::harness::episode_store::{CreativeEpisodeStore, ListEpisodesOptions};
use crate::harness::profile_registry::{
    create_built_in_work_profile_registry, parse_work_profile_json, save_work_profile,
};
use crate::harness::source_sync::hex_sha256;
use crate::harness::work_store::{list_work_manifests, load_work_manifest, work_directory, WorkStoreError};

/// TS 全局 onError 非 ApiError 分支的固定形状。
fn internal_error_response() -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({
            "error": {
                "code": "INTERNAL_ERROR",
                "message": "Unexpected server error.",
            }
        })),
    )
        .into_response()
}

/// TS 局部 catch 的纯字符串形状（works GET 域）。
fn local_error(status: StatusCode, message: impl Into<String>) -> Response {
    (status, Json(json!({ "error": message.into() }))).into_response()
}

fn work_store_error_response(error: WorkStoreError) -> Response {
    if error.is_not_found() {
        local_error(StatusCode::NOT_FOUND, error.to_string())
    } else {
        local_error(StatusCode::BAD_REQUEST, error.to_string())
    }
}

/// GET /api/v1/works?profileId=
pub async fn list_works(
    State(runtime): State<BooksRuntime>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Response {
    let root = runtime.state.project_root();
    let profile_id = params
        .get("profileId")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    match list_work_manifests(root, profile_id.as_deref()).await {
        Ok(works) => {
            let items: Vec<Value> = works
                .iter()
                .map(|work| {
                    // TS `{...work, artifactCount}`：清单字段 + 末尾 artifactCount。
                    let mut value = serde_json::to_value(work).unwrap_or(Value::Null);
                    if let Value::Object(map) = &mut value {
                        map.insert("artifactCount".into(), json!(work.artifacts.len()));
                    }
                    value
                })
                .collect();
            (StatusCode::OK, Json(json!({ "works": items }))).into_response()
        }
        Err(_) => internal_error_response(),
    }
}

/// GET /api/v1/works/:id —— {work, episodes(limit 50)}。
pub async fn get_work(
    State(runtime): State<BooksRuntime>,
    AxumPath(work_id): AxumPath<String>,
) -> Response {
    let root = runtime.state.project_root();
    let work: WorkManifest = match load_work_manifest(root, &work_id).await {
        Ok(work) => work,
        Err(error) => return work_store_error_response(error),
    };
    let episodes = match CreativeEpisodeStore::new(&root.join(".inkos").join("harness.sqlite")) {
        Ok(store) => store,
        Err(error) => return local_error(StatusCode::BAD_REQUEST, error.to_string()),
    };
    let listed = episodes.list_episodes(&ListEpisodesOptions {
        work_id: Some(work_id.clone()),
        profile_id: None,
        status: None,
        limit: Some(50),
    });
    episodes.close();
    match listed {
        Ok(episodes) => (
            StatusCode::OK,
            Json(json!({
                "work": serde_json::to_value(&work).unwrap_or(Value::Null),
                "episodes": serde_json::to_value(&episodes).unwrap_or(Value::Null),
            })),
        )
            .into_response(),
        Err(error) => local_error(StatusCode::BAD_REQUEST, error),
    }
}

/// GET /api/v1/works/:id/artifacts —— {workId, artifacts}。
pub async fn list_work_artifacts(
    State(runtime): State<BooksRuntime>,
    AxumPath(work_id): AxumPath<String>,
) -> Response {
    let root = runtime.state.project_root();
    match load_work_manifest(root, &work_id).await {
        Ok(work) => (
            StatusCode::OK,
            Json(json!({
                "workId": work_id,
                "artifacts": serde_json::to_value(&work.artifacts).unwrap_or(Value::Null),
            })),
        )
            .into_response(),
        Err(error) => work_store_error_response(error),
    }
}

/// GET /api/v1/works/:id/artifacts/:artifactId/revisions/:revisionId[?download=1]。
#[allow(clippy::too_many_lines)]
pub async fn get_artifact_revision(
    State(runtime): State<BooksRuntime>,
    AxumPath((work_id, artifact_id, revision_id)): AxumPath<(String, String, String)>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Response {
    let root = runtime.state.project_root();
    let work = match load_work_manifest(root, &work_id).await {
        Ok(work) => work,
        Err(error) => return work_store_error_response(error),
    };
    let artifact = work.artifacts.iter().find(|candidate| candidate.id == artifact_id);
    let revision = artifact
        .and_then(|candidate| candidate.revisions.iter().find(|revision| revision.id == revision_id));
    let (Some(artifact), Some(revision)) = (artifact, revision) else {
        return local_error(StatusCode::NOT_FOUND, "Artifact revision not found");
    };
    // TS safeChildPath(workDirectory(root, id), snapshotPath ?? path)：词法收容
    // 检查（不解析符号链接），穿越 → 局部 catch 400。
    let work_root = match work_directory(root, &work_id) {
        Ok(path) => path,
        Err(error) => return work_store_error_response(error),
    };
    let snapshot_file = revision
        .snapshot_path
        .clone()
        .unwrap_or_else(|| revision.path.clone());
    let bytes = match safe_child_path(&work_root, &snapshot_file) {
        Ok(path) => match tokio::fs::read(&path).await {
            Ok(bytes) => bytes,
            Err(error) => {
                let status = if error.kind() == std::io::ErrorKind::NotFound {
                    StatusCode::NOT_FOUND
                } else {
                    StatusCode::BAD_REQUEST
                };
                return local_error(status, error.to_string());
            }
        },
        Err(error) => return local_error(StatusCode::BAD_REQUEST, error.to_string()),
    };
    // checksum 不符：TS 抛 ApiError(409, ARTIFACT_SNAPSHOT_UNAVAILABLE) 被局部
    // catch 降级为 400 + 纯字符串（TS 同款行为，勿"修复"为 409）。
    if format!("sha256:{}", hex_sha256(&bytes)) != revision.checksum {
        return local_error(
            StatusCode::BAD_REQUEST,
            "Revision content does not match its recorded checksum",
        );
    }
    if params.get("download").map(String::as_str) == Some("1") {
        let file_name = revision
            .path
            .rsplit('/')
            .next()
            .filter(|name| !name.is_empty())
            .unwrap_or(&artifact.id);
        return download_response(&revision.content_type, file_name, bytes);
    }
    let text_like =
        revision.content_type.starts_with("text/") || revision.content_type == "application/json";
    let payload = if text_like {
        json!({
            "workId": work_id,
            "artifactId": artifact.id,
            "revision": serde_json::to_value(revision).unwrap_or(Value::Null),
            "content": String::from_utf8_lossy(&bytes),
        })
    } else {
        json!({
            "workId": work_id,
            "artifactId": artifact.id,
            "revision": serde_json::to_value(revision).unwrap_or(Value::Null),
            "dataUrl": format!(
                "data:{};base64,{}",
                revision.content_type,
                base64::engine::general_purpose::STANDARD.encode(&bytes)
            ),
        })
    };
    (StatusCode::OK, Json(payload)).into_response()
}

/// POST /api/v1/works/:id/artifacts/:artifactId/revisions/:revisionId/adopt。
/// 无局部 catch（TS 同）：任何工具失败 → 500 INTERNAL_ERROR 固定文案。
pub async fn post_adopt_work_revision(
    State(runtime): State<BooksRuntime>,
    AxumPath((work_id, artifact_id, revision_id)): AxumPath<(String, String, String)>,
    body: Option<Json<AdoptRequestBody>>,
) -> Response {
    let root = runtime.state.project_root();
    // TS `await c.req.json()` 无 body 时抛错 → 500 固定文案；缺 expectedCurrent
    // 字段与 null 语义不同（strict !== 比较）——double option 保真。
    let Some(Json(body)) = body else {
        return internal_error_response();
    };
    let expected = flatten_double_option(body.expected_current_revision_id);
    match adopt_work_revision(root, &work_id, &artifact_id, &revision_id, expected.as_deref()).await {
        Ok(outcome) => {
            let action: ActionResult = outcome.action_result;
            (StatusCode::OK, Json(json!({ "action": serde_json::to_value(action).unwrap_or(Value::Null) })))
                .into_response()
        }
        Err(_) => internal_error_response(),
    }
}

#[derive(serde::Deserialize)]
pub struct AdoptRequestBody {
    #[serde(
        rename = "expectedCurrentRevisionId",
        default,
        deserialize_with = "deserialize_double_option"
    )]
    pub expected_current_revision_id: Option<Option<String>>,
}

fn deserialize_double_option<'de, D>(deserializer: D) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    serde::de::Deserialize::deserialize(deserializer).map(Some)
}

fn flatten_double_option(value: Option<Option<String>>) -> Option<String> {
    value.flatten()
}

/// GET /api/v1/episodes?workId&profileId&status&limit。
pub async fn list_episodes(
    State(runtime): State<BooksRuntime>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Response {
    let root = runtime.state.project_root();
    let trim_non_empty = |key: &str| -> Option<String> {
        params
            .get(key)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    };
    // TS `Number(query ?? "100")` + Number.isFinite 回落 100；listEpisodes 内部
    // 再钳制 1..=500（M1a store 同参）。
    let limit = params
        .get("limit")
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| value.is_finite())
        .map(|value| value as u64)
        .unwrap_or(100);
    let store = match CreativeEpisodeStore::new(&root.join(".inkos").join("harness.sqlite")) {
        Ok(store) => store,
        Err(error) => return local_error(StatusCode::BAD_REQUEST, error.to_string()),
    };
    let listed = store.list_episodes(&ListEpisodesOptions {
        work_id: trim_non_empty("workId"),
        profile_id: trim_non_empty("profileId"),
        status: trim_non_empty("status"),
        limit: Some(limit),
    });
    store.close();
    match listed {
        Ok(episodes) => (
            StatusCode::OK,
            Json(json!({
                "episodes": serde_json::to_value(&episodes).unwrap_or(Value::Null),
            })),
        )
            .into_response(),
        Err(error) => local_error(StatusCode::BAD_REQUEST, error),
    }
}

/// GET /api/v1/episodes/:id —— 无局部 catch：未知 episode → 500 固定文案（TS 同，
/// requireEpisode 抛普通 Error 落全局 handler；勿"修复"为 404）。
pub async fn get_episode(
    State(runtime): State<BooksRuntime>,
    AxumPath(episode_id): AxumPath<String>,
) -> Response {
    let root = runtime.state.project_root();
    let store = match CreativeEpisodeStore::new(&root.join(".inkos").join("harness.sqlite")) {
        Ok(store) => store,
        Err(_) => return internal_error_response(),
    };
    let result = store.require_episode(&episode_id).and_then(|episode| {
        store
            .list_events(&episode_id)
            .map(|events| (episode, events))
    });
    store.close();
    match result {
        Ok((episode, events)) => (
            StatusCode::OK,
            Json(json!({
                "episode": serde_json::to_value(&episode).unwrap_or(Value::Null),
                "events": serde_json::to_value(&events).unwrap_or(Value::Null),
            })),
        )
            .into_response(),
        Err(_) => internal_error_response(),
    }
}

/// GET /api/v1/profiles —— 内置 9 + `.inkos/profiles/*.json` 归并；无局部 catch。
pub async fn list_profiles(State(runtime): State<BooksRuntime>) -> Response {
    let root = runtime.state.project_root();
    match create_built_in_work_profile_registry(root) {
        Ok(registry) => {
            let profiles: Vec<Value> = registry
                .list()
                .iter()
                .map(|profile| serde_json::to_value(profile).unwrap_or(Value::Null))
                .collect();
            (StatusCode::OK, Json(json!({ "profiles": profiles }))).into_response()
        }
        Err(_) => internal_error_response(),
    }
}

/// POST /api/v1/profiles —— 创作配置新建（885 号 M2 写面）。
/// TS：WorkProfileSchema.parse 失败=zod 异常无局部 catch → 500 固定文案；
/// 重复 id（内置或自定义，registry 全集判断）→ 409 PROFILE_EXISTS。
pub async fn post_profile(
    State(runtime): State<BooksRuntime>,
    body: Option<Json<Value>>,
) -> Response {
    let Some(Json(body)) = body else {
        return internal_error_response();
    };
    let root = runtime.state.project_root();
    let raw = serde_json::to_string(&body).unwrap_or_default();
    let profile = match parse_work_profile_json(&raw) {
        Ok(profile) => profile,
        Err(_) => return internal_error_response(),
    };
    match create_built_in_work_profile_registry(root) {
        Ok(registry) => {
            if registry.get(&profile.id).is_some() {
                return (
                    StatusCode::CONFLICT,
                    Json(json!({
                        "error": { "code": "PROFILE_EXISTS", "message": "Choose a new profile ID" }
                    })),
                )
                    .into_response();
            }
        }
        Err(_) => return internal_error_response(),
    }
    match save_work_profile(root, &profile).await {
        Ok(_) => (
            StatusCode::CREATED,
            Json(json!({ "profile": serde_json::to_value(&profile).unwrap_or(Value::Null) })),
        )
            .into_response(),
        Err(_) => internal_error_response(),
    }
}

/// PUT /api/v1/profiles/:id —— 创作配置覆盖（内置亦可覆盖写盘——TS 同，
/// registry 归并时自定义文件 replace 内置）。id 不可变 → 400 PROFILE_ID_MISMATCH。
pub async fn put_profile(
    State(runtime): State<BooksRuntime>,
    AxumPath(profile_id): AxumPath<String>,
    body: Option<Json<Value>>,
) -> Response {
    let Some(Json(body)) = body else {
        return internal_error_response();
    };
    let root = runtime.state.project_root();
    let raw = serde_json::to_string(&body).unwrap_or_default();
    let profile = match parse_work_profile_json(&raw) {
        Ok(profile) => profile,
        Err(_) => return internal_error_response(),
    };
    if profile.id != profile_id {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": { "code": "PROFILE_ID_MISMATCH", "message": "Profile identity cannot change" }
            })),
        )
            .into_response();
    }
    match save_work_profile(root, &profile).await {
        Ok(_) => (
            StatusCode::OK,
            Json(json!({ "profile": serde_json::to_value(&profile).unwrap_or(Value::Null) })),
        )
            .into_response(),
        Err(_) => internal_error_response(),
    }
}

/// TS `safeChildPath`：词法 resolve + 收容检查（不解析符号链接；rel === "" 允许，
/// TS 同——请求恰为根目录不算穿越）。
fn safe_child_path(root: &Path, requested: &str) -> Result<PathBuf, String> {
    let requested_path = PathBuf::from(requested);
    let resolved = if requested_path.is_absolute() {
        lexical_normalize(&requested_path)
    } else {
        lexical_normalize(&root.join(&requested_path))
    };
    if resolved.strip_prefix(root).is_ok() {
        Ok(resolved)
    } else {
        Err(format!("Path traversal blocked: {requested}"))
    }
}

/// path.resolve 等价：组件级归一（`.` 消除、`..` 弹栈；不触及文件系统）。
fn lexical_normalize(path: &Path) -> PathBuf {
    let mut stack: Vec<Component> = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                // 根上弹不出栈（TS resolve 不上逃）——保留原语义即丢弃。
                if matches!(stack.last(), Some(Component::Normal(_))) {
                    stack.pop();
                }
            }
            other => stack.push(other),
        }
    }
    let mut resolved = PathBuf::new();
    for component in stack {
        resolved.push(component.as_os_str());
    }
    resolved
}

/// TS `attachmentDisposition`。
fn attachment_disposition(file_name: &str) -> String {
    let safe_ascii: String = file_name
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') { ch } else { '_' })
        .collect();
    let safe_ascii = if safe_ascii.is_empty() { "download".to_string() } else { safe_ascii };
    format!(
        "attachment; filename=\"{safe_ascii}\"; filename*=UTF-8''{}",
        crate::harness::artifact_validation::encode_uri_component(file_name)
    )
}

fn download_response(content_type: &str, file_name: &str, bytes: Vec<u8>) -> Response {
    let disposition = attachment_disposition(file_name);
    let content_type_header =
        HeaderValue::from_str(content_type).unwrap_or(HeaderValue::from_static("application/octet-stream"));
    let disposition_header = HeaderValue::from_str(&disposition)
        .unwrap_or(HeaderValue::from_static("attachment"));
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, content_type_header),
            (header::CONTENT_DISPOSITION, disposition_header),
        ],
        bytes,
    )
        .into_response()
}
