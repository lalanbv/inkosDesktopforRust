//! sessions / interaction 会话端点（server.ts L4536 / L4703-L4803）。
//!
//! - `GET /interaction/session`：项目会话 + activeBook 解析
//! - `GET /sessions`、`POST /sessions`（幂等创建 + safeSessionId）
//! - `GET /PUT/DELETE /sessions/:sessionId`（详情含对账快照 / 改名 / 双删）
//! - `PUT /sessions/:sessionId/play-mode`、`POST /sessions/:sessionId/abort`
//!
//! `POST /agent`（agent loop 大件）随 65 号交互运行时专项移植。
//! 错误形态：404 平铺 `{error:"Session not found"}`；校验走 ApiError
//! `{"error":{"code","message"}}`。

use std::collections::HashSet;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use axum::body::Bytes;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde_json::{json, Value};

use crate::interaction::book_session_store::{
    create_and_persist_book_session, delete_book_session, list_book_sessions,
    load_book_session, load_project_session, rename_book_session,
    resolve_session_active_book,
};
use crate::interaction::session::{is_safe_book_id, PlayMode, SessionKind};
use crate::server::books_routes::BooksRuntime;
use crate::server::task_store::{
    delete_studio_task_snapshot, load_studio_task_snapshot, save_studio_task_snapshot,
    StudioTaskExecutionStatus, StudioTaskSnapshot,
};

type ApiErrorResponse = (StatusCode, Json<Value>);

fn api_error(status: StatusCode, code: &str, message: impl Into<String>) -> ApiErrorResponse {
    (
        status,
        Json(json!({ "error": { "code": code, "message": message.into() } })),
    )
}

fn not_found() -> (StatusCode, Json<Value>) {
    (StatusCode::NOT_FOUND, Json(json!({ "error": "Session not found" })))
}

/// 已删除会话标记集（DELETE 先标记；POST 重建同 id 时复活）。
fn deleted_session_ids() -> &'static Mutex<HashSet<String>> {
    static SET: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    SET.get_or_init(|| Mutex::new(HashSet::new()))
}

/// `normalizeApiBookId`：undefined/null → None；非串 400；空/不安全 400。
pub(crate) fn normalize_api_book_id(value: Option<&Value>, field: &str) -> Result<Option<String>, ApiErrorResponse> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    let Some(book_id) = value.as_str() else {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "INVALID_BOOK_ID",
            format!("{field} must be a string"),
        ));
    };
    let trimmed = book_id.trim();
    if trimmed.is_empty() {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "INVALID_BOOK_ID",
            format!("{field} cannot be blank"),
        ));
    }
    if !is_safe_book_id(trimmed) {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "INVALID_BOOK_ID",
            format!("Invalid {field}: \"{trimmed}\""),
        ));
    }
    Ok(Some(trimmed.to_string()))
}

/// `normalizeStudioSessionKind`：空回退 fallback；非法 400 INVALID_SESSION_KIND。
pub(crate) fn normalize_studio_session_kind(
    value: Option<&Value>,
    fallback: SessionKind,
) -> Result<SessionKind, ApiErrorResponse> {
    let Some(text) = value.and_then(Value::as_str) else {
        return Ok(fallback);
    };
    if text.is_empty() {
        return Ok(fallback);
    }
    SessionKind::parse(text).ok_or_else(|| {
        api_error(
            StatusCode::BAD_REQUEST,
            "INVALID_SESSION_KIND",
            format!("Invalid sessionKind: {text}"),
        )
    })
}

/// `normalizeStudioPlayMode`：空/非法 → None（TS normalizeCorePlayMode 的
/// safeParse 失败返回 undefined——studio 侧静默回退）。
fn normalize_studio_play_mode(value: Option<&Value>) -> Option<PlayMode> {
    value
        .and_then(Value::as_str)
        .and_then(PlayMode::parse)
}

/// `loadReconciledTaskSnapshot`：running 快照且本进程无运行确认 → 改写终态
/// （64 号：进程内无 agent 执行体注册，running 快照一律视为旧进程遗留）。
async fn load_reconciled_task_snapshot(root: &Path, session_id: &str) -> Option<StudioTaskSnapshot> {
    let mut task = load_studio_task_snapshot(root, session_id).await?;
    let running = matches!(
        task.execution.status,
        StudioTaskExecutionStatus::Running | StudioTaskExecutionStatus::Processing
    );
    if !running {
        return Some(task);
    }
    let completed_at = crate::interaction::session::utc_now_ms() as f64;
    task.updated_at = completed_at;
    task.execution.status = StudioTaskExecutionStatus::Error;
    task.execution.error = Some(
        "任务已中断：Studio 服务在任务运行期间重启，任务未能继续。请重新发起。".to_string(),
    );
    task.execution.completed_at = Some(completed_at);
    let _ = save_studio_task_snapshot(root, &task).await;
    Some(task)
}

// ── GET /api/v1/interaction/session ────────────────────────────

pub async fn get_interaction_session(
    State(runtime): State<BooksRuntime>,
) -> impl IntoResponse {
    let root = runtime.state.project_root();
    let session = load_project_session(root).await;
    let active_book_id = resolve_session_active_book(root, &session).await;
    // session.activeBookId 与解析值不一致时以解析值覆盖（TS 展开语义）
    let mut session = session;
    if let Some(active) = active_book_id.as_deref() {
        let session_book = session.get("activeBookId").and_then(Value::as_str);
        if session_book != Some(active) {
            session
                .as_object_mut()
                .unwrap()
                .insert("activeBookId".to_string(), json!(active));
        }
    }
    (
        StatusCode::OK,
        Json(json!({ "session": session, "activeBookId": active_book_id })),
    )
}

// ── GET /api/v1/sessions ───────────────────────────────────────

pub async fn list_sessions(
    State(runtime): State<BooksRuntime>,
    Query(query): Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let root = runtime.state.project_root();
    let book_id = match query.get("bookId") {
        None => None,
        Some(value) if value.is_empty() || value == "null" => None,
        Some(value) => Some(value.clone()),
    };
    let sessions: Vec<Value> = list_book_sessions(root, book_id.as_deref())
        .await
        .iter()
        .map(|summary| summary.to_json())
        .collect();
    (StatusCode::OK, Json(json!({ "sessions": sessions })))
}

// ── GET /api/v1/sessions/:sessionId ────────────────────────────

pub async fn get_session(
    State(runtime): State<BooksRuntime>,
    AxumPath(session_id): AxumPath<String>,
) -> impl IntoResponse {
    let root = runtime.state.project_root();
    let Some(session) = load_book_session(root, &session_id).await else {
        return not_found();
    };
    let task = load_reconciled_task_snapshot(root, &session_id).await;
    let mut payload = json!({ "session": session.to_response_json() });
    if let Some(task) = task {
        payload
            .as_object_mut()
            .unwrap()
            .insert("task".to_string(), serde_json::to_value(&task).unwrap_or(Value::Null));
    }
    (StatusCode::OK, Json(payload))
}

// ── POST /api/v1/sessions ──────────────────────────────────────

pub async fn create_session(
    State(runtime): State<BooksRuntime>,
    body: Bytes,
) -> impl IntoResponse {
    let root = runtime.state.project_root();
    let payload: Value = serde_json::from_slice(&body).unwrap_or(json!({}));
    let book_id = match normalize_api_book_id(payload.get("bookId"), "bookId") {
        Ok(book_id) => book_id,
        Err(response) => return response.into_response(),
    };
    let fallback_kind = if book_id.is_some() {
        SessionKind::Book
    } else {
        SessionKind::Chat
    };
    let session_kind = match normalize_studio_session_kind(payload.get("sessionKind"), fallback_kind)
    {
        Ok(kind) => kind,
        Err(response) => return response.into_response(),
    };
    let play_mode = normalize_studio_play_mode(payload.get("playMode"));
    // sessionId 仅允许 timestamp-random 形态（防任意文件名注入）
    let safe_session_id = payload
        .get("sessionId")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .filter(|id| {
            let mut parts = id.splitn(2, '-');
            let timestamp = parts.next().unwrap_or_default();
            let random = parts.next().unwrap_or_default();
            !timestamp.is_empty()
                && timestamp.bytes().all(|b| b.is_ascii_digit())
                && !random.is_empty()
                && random.bytes().all(|b| b.is_ascii_digit() || b.is_ascii_lowercase())
        });
    let session = create_and_persist_book_session(
        root,
        book_id.as_deref(),
        safe_session_id,
        Some(session_kind),
        play_mode,
    )
    .await;
    // 同 id 重建 → 复活（清除删除标记）
    deleted_session_ids().lock().unwrap().remove(&session.session_id);
    (StatusCode::OK, Json(json!({ "session": session.to_response_json() }))).into_response()
}

// ── PUT /api/v1/sessions/:sessionId/play-mode ──────────────────

pub async fn put_session_play_mode(
    State(runtime): State<BooksRuntime>,
    AxumPath(session_id): AxumPath<String>,
    body: Bytes,
) -> impl IntoResponse {
    let root = runtime.state.project_root();
    let payload: Value = serde_json::from_slice(&body).unwrap_or(json!({}));
    let Some(play_mode) = normalize_studio_play_mode(payload.get("playMode")) else {
        return api_error(StatusCode::BAD_REQUEST, "INVALID_PLAY_MODE", "playMode is required")
            .into_response();
    };
    let Some(existing) = load_book_session(root, &session_id).await else {
        return not_found().into_response();
    };
    let session = create_and_persist_book_session(
        root,
        existing.book_id.as_deref(),
        Some(&existing.session_id),
        existing.session_kind,
        Some(play_mode),
    )
    .await;
    (StatusCode::OK, Json(json!({ "session": session.to_response_json() }))).into_response()
}

// ── PUT /api/v1/sessions/:sessionId ────────────────────────────

pub async fn rename_session(
    State(runtime): State<BooksRuntime>,
    AxumPath(session_id): AxumPath<String>,
    body: Bytes,
) -> impl IntoResponse {
    let root = runtime.state.project_root();
    let payload: Value = serde_json::from_slice(&body).unwrap_or(json!({}));
    let title = payload
        .get("title")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    if title.is_empty() {
        return api_error(
            StatusCode::BAD_REQUEST,
            "INVALID_SESSION_TITLE",
            "Session title is required",
        )
        .into_response();
    }
    match rename_book_session(root, &session_id, title).await {
        Some(session) => (
            StatusCode::OK,
            Json(json!({ "session": session.to_response_json() })),
        )
            .into_response(),
        None => not_found().into_response(),
    }
}

// ── DELETE /api/v1/sessions/:sessionId ─────────────────────────

pub async fn delete_session(
    State(runtime): State<BooksRuntime>,
    AxumPath(session_id): AxumPath<String>,
) -> impl IntoResponse {
    let root = runtime.state.project_root();
    // 先标记（中止后的错误持久化检查该标记）；运行中任务控制器随 65 号
    // agent 执行体接入注册表——当前无生产者，abort 为 no-op。
    deleted_session_ids().lock().unwrap().insert(session_id.clone());
    delete_book_session(root, &session_id).await;
    delete_studio_task_snapshot(root, &session_id).await;
    (StatusCode::OK, Json(json!({ "ok": true })))
}

// ── POST /api/v1/sessions/:sessionId/abort ─────────────────────

pub async fn abort_session(
    State(runtime): State<BooksRuntime>,
    AxumPath(session_id): AxumPath<String>,
    body: Bytes,
) -> impl IntoResponse {
    let payload: Value = serde_json::from_slice(&body).unwrap_or(json!({}));
    // scope=chat 只停聊天轮；默认 all（64 号两者均无运行执行体 → aborted=false）
    let _scope = if payload.get("scope") == Some(&json!("chat")) {
        "chat"
    } else {
        "all"
    };
    let aborted = false;
    runtime
        .hub
        .broadcast("agent:aborted", &json!({ "sessionId": session_id, "aborted": aborted }));
    (StatusCode::OK, Json(json!({ "ok": true, "aborted": aborted })))
}
