//! play 域端点（71 号）——互动世界游玩面。
//!
//! 契约来源 `packages/studio/src/api/server.ts` L4551-L4700：
//! - `GET /play/runs/:worldId/:runId`（L4551）：transcript + currentState +
//!   world + graph 快照 + 插图 sidecar 合并（实体/当前场景 imageUrl）
//! - `PUT .../image-settings`（L4605）：三开关覆写
//! - `POST .../generate-image`（L4619）：entity/scene 目标提示词 + 生图
//!   （生图链未移植 → `needsCoverConfig` 兜底，偏差备案）
//! - `GET .../images/:file`（L4682）：插图文件回读（png/jpg）

use std::path::Path;

use axum::body::Bytes;
use axum::extract::{Path as AxumPath, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Map, Value};

use crate::play::{self, PlayWorldContext};
use crate::server::books_routes::BooksRuntime;
use crate::server::session_routes::normalize_api_book_id;

fn flat_error(status: StatusCode, message: impl Into<String>) -> Response {
    (
        status,
        Json(json!({ "error": message.into() })),
    )
        .into_response()
}

fn api_error(status: StatusCode, code: &str, message: impl Into<String>) -> Response {
    (
        status,
        Json(json!({ "error": { "code": code, "message": message.into() } })),
    )
        .into_response()
}

/// `normalizeApiBookId(param) ?? "default-{world|run}"`：非法段 400（TS throw）。
fn normalize_segment(value: &str, field: &str, fallback: &str) -> Result<String, Box<Response>> {
    match normalize_api_book_id(Some(&json!(value)), field) {
        Ok(Some(segment)) => Ok(segment),
        Ok(None) => Ok(fallback.to_string()),
        Err(response) => Err(Box::new(response.into_response())),
    }
}

fn image_url_for(world_id: &str, run_id: &str, file: &str) -> String {
    format!(
        "/api/v1/play/runs/{}/{}/images/{}",
        crate::server::task_store::js_encode_uri_component(world_id),
        crate::server::task_store::js_encode_uri_component(run_id),
        crate::server::task_store::js_encode_uri_component(file),
    )
}

// ── GET /play/runs/:worldId/:runId ──────────────────────────────

pub async fn get_play_run(
    State(runtime): State<BooksRuntime>,
    AxumPath((raw_world_id, raw_run_id)): AxumPath<(String, String)>,
) -> impl IntoResponse {
    let root = runtime.state.project_root();
    let (world_id, run_id) = match (
        normalize_segment(&raw_world_id, "worldId", "default-world"),
        normalize_segment(&raw_run_id, "runId", "default-run"),
    ) {
        (Ok(world_id), Ok(run_id)) => (world_id, run_id),
        (Err(response), _) | (_, Err(response)) => return *response,
    };
    let Ok(run_dir) = play::run_dir(root, &world_id, &run_id) else {
        return api_error(
            StatusCode::BAD_REQUEST,
            "INVALID_ID",
            format!("Unsafe play path segment: {raw_world_id}/{raw_run_id}"),
        );
    };

    let transcript = play::read_transcript(root, &world_id, &run_id).await;
    let current_state = play::load_current_state(root, &world_id, &run_id).await;
    let world = play::load_world(root, &world_id).await;
    let graph = play::play_graph_snapshot(&run_dir);

    // 插图 sidecar 合并：实体 ready 项注入 imageUrl；scene-turn-* 收集 URL 表。
    let manifest = play::read_play_image_manifest(&run_dir).await;
    let image_settings = play::read_play_image_settings(&run_dir).await;
    let image_url = |file: &str| image_url_for(&world_id, &run_id, file);
    let mut scene_image_urls = Map::new();
    for (key, entry) in &manifest {
        if !key.starts_with("scene-turn-") {
            continue;
        }
        let ready = entry.get("status").and_then(Value::as_str) == Some("ready");
        let Some(file) = entry.get("file").and_then(Value::as_str) else {
            continue;
        };
        if ready && !file.is_empty() {
            scene_image_urls.insert(key.clone(), json!(image_url(file)));
        }
    }
    let entities_with_images: Vec<Value> = graph["entities"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|mut entity| {
            let Some(entity_id) = entity.get("id").and_then(Value::as_str) else {
                return entity;
            };
            if let Some(entry) = manifest.get(entity_id) {
                let ready = entry.get("status").and_then(Value::as_str) == Some("ready");
                let file = entry.get("file").and_then(Value::as_str).unwrap_or_default();
                if ready && !file.is_empty() {
                    if let Some(obj) = entity.as_object_mut() {
                        obj.insert("imageUrl".into(), json!(image_url(file)));
                    }
                }
            }
            entity
        })
        .collect();

    let scene_turn = current_state
        .as_ref()
        .and_then(|state| state.get("turn"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let scene_key = format!("scene-turn-{scene_turn}");
    let scene_image_url = manifest
        .get(&scene_key)
        .filter(|entry| entry.get("status").and_then(Value::as_str) == Some("ready"))
        .and_then(|entry| entry.get("file").and_then(Value::as_str))
        .filter(|file| !file.is_empty())
        .map(image_url);

    let mut payload = json!({
        "worldId": world_id,
        "runId": run_id,
        "title": world.as_ref().and_then(|w| w.get("title")).cloned().unwrap_or(Value::Null),
        "transcript": transcript,
        "currentState": current_state,
        "graph": { "entities": entities_with_images,
                   "edges": graph["edges"].clone(),
                   "stateSlots": graph["stateSlots"].clone(),
                   "events": graph["events"].clone() },
        "imageSettings": image_settings,
        "sceneImageUrls": Value::Object(scene_image_urls),
    });
    if let Some(scene_image_url) = scene_image_url {
        payload["sceneImageUrl"] = json!(scene_image_url);
    }
    (StatusCode::OK, Json(payload)).into_response()
}

// ── PUT /play/runs/:worldId/:runId/image-settings ───────────────

pub async fn put_play_image_settings(
    State(runtime): State<BooksRuntime>,
    AxumPath((raw_world_id, raw_run_id)): AxumPath<(String, String)>,
    body: Bytes,
) -> impl IntoResponse {
    let root = runtime.state.project_root();
    let (world_id, run_id) = match (
        normalize_segment(&raw_world_id, "worldId", "default-world"),
        normalize_segment(&raw_run_id, "runId", "default-run"),
    ) {
        (Ok(world_id), Ok(run_id)) => (world_id, run_id),
        (Err(response), _) | (_, Err(response)) => return *response,
    };
    let payload: Value = serde_json::from_slice(&body).unwrap_or(json!({}));
    let flag = |name: &str| payload.get(name).and_then(Value::as_bool).unwrap_or(false);
    let settings = json!({
        "actors": flag("actors"),
        "moments": flag("moments"),
        "inventory": flag("inventory"),
    });
    let Ok(run_dir) = play::run_dir(root, &world_id, &run_id) else {
        return api_error(
            StatusCode::BAD_REQUEST,
            "INVALID_ID",
            format!("Unsafe play path segment: {raw_world_id}/{raw_run_id}"),
        );
    };
    if let Err(message) = play::write_play_image_settings(&run_dir, &settings).await {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR", message);
    }
    (
        StatusCode::OK,
        Json(json!({ "ok": true, "imageSettings": settings })),
    )
        .into_response()
}

// ── POST /play/runs/:worldId/:runId/generate-image ──────────────

pub async fn post_play_generate_image(
    State(runtime): State<BooksRuntime>,
    AxumPath((raw_world_id, raw_run_id)): AxumPath<(String, String)>,
    body: Bytes,
) -> impl IntoResponse {
    let root: &Path = runtime.state.project_root();
    let (world_id, run_id) = match (
        normalize_segment(&raw_world_id, "worldId", "default-world"),
        normalize_segment(&raw_run_id, "runId", "default-run"),
    ) {
        (Ok(world_id), Ok(run_id)) => (world_id, run_id),
        (Err(response), _) | (_, Err(response)) => return *response,
    };
    let payload: Value = serde_json::from_slice(&body).unwrap_or(json!({}));
    let target = payload.get("target").and_then(Value::as_str).unwrap_or("entity");
    let Ok(run_dir) = play::run_dir(root, &world_id, &run_id) else {
        return api_error(
            StatusCode::BAD_REQUEST,
            "INVALID_ID",
            format!("Unsafe play path segment: {raw_world_id}/{raw_run_id}"),
        );
    };

    let world = play::load_world(root, &world_id).await;
    let world_context = world.as_ref().map(|world| PlayWorldContext {
        premise: world.get("premise").and_then(Value::as_str),
        world_contract: world.get("worldContract").and_then(Value::as_str),
        visual_contract: world.get("visualContract").and_then(Value::as_str),
    });
    let current_state = play::load_current_state(root, &world_id, &run_id).await;

    let (_key, _prompt) = if target == "scene" {
        let body_scene = payload
            .get("sceneText")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let scene_text = match body_scene {
            Some(text) => text.to_string(),
            None => play::read_projection(root, &world_id, &run_id, "projections/scene.md")
                .await
                .unwrap_or_default()
                .trim()
                .to_string(),
        };
        if scene_text.is_empty() {
            return flat_error(StatusCode::BAD_REQUEST, "no current scene to illustrate");
        }
        let turn = current_state
            .as_ref()
            .and_then(|state| state.get("turn"))
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let key = payload
            .get("sceneKey")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
            .unwrap_or_else(|| format!("scene-turn-{turn}"));
        let prompt = play::build_play_scene_image_prompt(&scene_text, &world_context);
        (key, prompt)
    } else {
        let Some(entity_id) = payload
            .get("entityId")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
        else {
            return flat_error(
                StatusCode::BAD_REQUEST,
                "entityId is required for an entity image",
            );
        };
        let graph = play::play_graph_snapshot(&run_dir);
        let entity = graph["entities"]
            .as_array()
            .and_then(|entities| {
                entities
                    .iter()
                    .find(|entity| entity.get("id").and_then(Value::as_str) == Some(entity_id))
            })
            .cloned();
        let Some(entity) = entity else {
            return flat_error(StatusCode::NOT_FOUND, format!("entity not found: {entity_id}"));
        };
        let prompt = play::build_play_entity_image_prompt(
            entity.get("type").and_then(Value::as_str).unwrap_or(""),
            entity.get("label").and_then(Value::as_str).unwrap_or(""),
            entity.get("summary").and_then(Value::as_str),
            &world_context,
        );
        (entity_id.to_string(), prompt)
    };

    // 生图执行链（cover 基础设施）未移植：与 TS "cover API 未配置" catch 分支
    // 同形兜底（{error, needsCoverConfig:true} → 前端提示先配置；偏差备案）。
    (
        StatusCode::BAD_REQUEST,
        Json(json!({
            "error": "Image generation is not available in this engine build yet. Configure the cover API on the Node side or wait for the image pipeline port.",
            "needsCoverConfig": true,
        })),
    )
        .into_response()
}

// ── GET /play/runs/:worldId/:runId/images/:file ─────────────────

pub async fn get_play_image(
    State(runtime): State<BooksRuntime>,
    AxumPath((raw_world_id, raw_run_id, file)): AxumPath<(String, String, String)>,
) -> impl IntoResponse {
    let root = runtime.state.project_root();
    let (world_id, run_id) = match (
        normalize_segment(&raw_world_id, "worldId", "default-world"),
        normalize_segment(&raw_run_id, "runId", "default-run"),
    ) {
        (Ok(world_id), Ok(run_id)) => (world_id, run_id),
        (Err(response), _) | (_, Err(response)) => return *response,
    };
    if file.is_empty() || file.contains('/') || file.contains("..") || file.contains('\0') {
        return flat_error(StatusCode::BAD_REQUEST, "Invalid image file");
    }
    let Ok(run_dir) = play::run_dir(root, &world_id, &run_id) else {
        return flat_error(StatusCode::BAD_REQUEST, "Invalid image file");
    };
    match tokio::fs::read(run_dir.join("images").join(&file)).await {
        Ok(content) => {
            let ext = file.rsplit('.').next().unwrap_or("").to_lowercase();
            let content_type = if ext == "jpg" || ext == "jpeg" {
                "image/jpeg"
            } else {
                "image/png"
            };
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, content_type)],
                content,
            )
                .into_response()
        }
        Err(_) => flat_error(StatusCode::NOT_FOUND, "Not Found"),
    }
}
