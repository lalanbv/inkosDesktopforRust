//! genres 域端点（53 号）：题材画像 CRUD + 内置复制。
//!
//! 契约来源 `packages/studio/src/api/server.ts`：
//! - `GET /genres`（L3036）：`{genres: [{id, name, source, language}]}`——
//!   `listAvailableGenres` + 每条经 `readGenreProfile`（三级查找）附着
//!   language，失败回 "zh"
//! - `GET /genres/:id`（L5702）：`{profile, body}`；任何错误 404
//! - `POST /genres/create`（L6086）：frontmatter 拼装落盘 `genres/{id}.md`；
//!   缺 id/name → 400；id 含 `/` `\` `\0` 或 `..` → ApiError 400
//!   `{"error":{"code":"INVALID_GENRE_ID","message":...}}`
//! - `PUT /genres/:id`（L6130）：同 create，但 name/id 缺省回路径参数
//! - `DELETE /genres/:id`（L6166）：项目级删除；缺失 404
//! - `POST /genres/:id/copy`（L5713）：内置 → 项目复制
//!
//! frontmatter 拼装逐字对齐：`yamlScalar(v) = JSON.stringify(String(v ?? ""))`
//! （JSON 字符串转义即合法 YAML 双引号标量）、数组字段 `JSON.stringify(?? [])`
//! （serde_json 同款紧凑形态）、布尔 `?? false`。

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde_json::{json, Value};

use crate::agents::rules_reader::{list_available_genres, read_genre_profile};
use crate::server::books_routes::BooksRuntime;

/// id 安全校验：`/[/\\\0]/` 或包含 `..`（对齐 TS 正则 + includes）。
fn genre_id_is_unsafe(id: &str) -> bool {
    id.contains('/') || id.contains('\\') || id.contains('\0') || id.contains("..")
}

/// ApiError 400 响应（onError 形状逐字）。
fn invalid_genre_id(id: &str) -> (StatusCode, Json<Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "error": { "code": "INVALID_GENRE_ID", "message": format!("Invalid genre ID: \"{id}\"") } })),
    )
}

/// `JSON.stringify(String(v ?? ""))`：JSON 字符串转义。
fn yaml_scalar(value: Option<&Value>) -> String {
    let text = value.and_then(Value::as_str).unwrap_or("");
    serde_json::to_string(text).unwrap_or_else(|_| "\"\"".to_string())
}

/// `JSON.stringify(v ?? [])`：数组（或任意 JSON 值）紧凑序列化。
fn json_stringify(value: Option<&Value>) -> String {
    match value {
        Some(value) => serde_json::to_string(value).unwrap_or_else(|_| "[]".to_string()),
        None => "[]".to_string(),
    }
}

/// 布尔字面量（`v ?? false`）。
fn bool_literal(value: Option<&Value>) -> String {
    value.and_then(Value::as_bool).unwrap_or(false).to_string()
}

/// frontmatter 拼装（12 键固定顺序 + 正文，元素 join("\n")）。
/// `p` 为字段源（create 的 body 根 / update 的 profile）；
/// `name`/`id` 由调用方给定（create 已校验非空、update 缺省回路径参数）；
/// `language` 缺键回 "zh"（TS `?? "zh"`），`pacingRule` 缺省 ""。
fn build_genre_frontmatter(p: &Value, name: &Value, id: &Value, body: &str) -> String {
    let zh = json!("zh");
    let language = p.get("language").unwrap_or(&zh);
    [
        "---".to_string(),
        format!("name: {}", yaml_scalar(Some(name))),
        format!("id: {}", yaml_scalar(Some(id))),
        format!("language: {}", yaml_scalar(Some(language))),
        format!("chapterTypes: {}", json_stringify(p.get("chapterTypes"))),
        format!("fatigueWords: {}", json_stringify(p.get("fatigueWords"))),
        format!("numericalSystem: {}", bool_literal(p.get("numericalSystem"))),
        format!("powerScaling: {}", bool_literal(p.get("powerScaling"))),
        format!("eraResearch: {}", bool_literal(p.get("eraResearch"))),
        format!("pacingRule: {}", yaml_scalar(p.get("pacingRule"))),
        format!("satisfactionTypes: {}", json_stringify(p.get("satisfactionTypes"))),
        format!("auditDimensions: {}", json_stringify(p.get("auditDimensions"))),
        "---".to_string(),
        String::new(),
        body.to_string(),
    ]
    .join("\n")
}

// ── GET /api/v1/genres ───────────────────────────────────────────

pub async fn list_genres(State(runtime): State<BooksRuntime>) -> impl IntoResponse {
    let infos = list_available_genres(runtime.state.project_root(), &runtime.builtin_genres_dir).await;
    let mut genres: Vec<Value> = Vec::with_capacity(infos.len());
    for info in &infos {
        // TS：readGenreProfile 失败（catch）→ language "zh"；
        // `profile.language ?? "zh"` 仅对 null/undefined 回退（空串保持）。
        let language = read_genre_profile(
            runtime.state.project_root(),
            &info.id,
            &runtime.builtin_genres_dir,
        )
        .await
        .map(|parsed| parsed.profile.language)
        .unwrap_or_else(|_| "zh".to_string());
        genres.push(json!({
            "id": info.id,
            "name": info.name,
            "source": info.source,
            "language": language,
        }));
    }
    (StatusCode::OK, Json(json!({ "genres": genres })))
}

// ── GET /api/v1/genres/:id ───────────────────────────────────────

pub async fn genre_detail(
    State(runtime): State<BooksRuntime>,
    Path(genre_id): Path<String>,
) -> impl IntoResponse {
    match read_genre_profile(runtime.state.project_root(), &genre_id, &runtime.builtin_genres_dir).await {
        Ok(parsed) => (
            StatusCode::OK,
            Json(json!({ "profile": parsed.profile, "body": parsed.body })),
        ),
        Err(error) => (StatusCode::NOT_FOUND, Json(json!({ "error": error.to_string() }))),
    }
}

// ── POST /api/v1/genres/create ───────────────────────────────────

pub async fn create_genre(
    State(runtime): State<BooksRuntime>,
    body: Bytes,
) -> impl IntoResponse {
    let internal_error = || {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": { "code": "INTERNAL_ERROR", "message": "Unexpected server error." } })),
        )
    };
    let Ok(parsed) = serde_json::from_slice::<Value>(&body) else {
        return internal_error();
    };
    let id = parsed.get("id").and_then(Value::as_str).unwrap_or("");
    let name = parsed.get("name").and_then(Value::as_str).unwrap_or("");
    if id.is_empty() || name.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "id and name are required" })));
    }
    if genre_id_is_unsafe(id) {
        return invalid_genre_id(id);
    }
    let name = parsed.get("name").expect("name 已校验非空");
    let frontmatter = build_genre_frontmatter(
        &parsed,
        name,
        parsed.get("id").expect("id 已校验非空"),
        parsed.get("body").and_then(Value::as_str).unwrap_or(""),
    );
    let genres_dir = runtime.state.project_root().join("genres");
    if tokio::fs::create_dir_all(&genres_dir).await.is_err() {
        return internal_error();
    }
    match tokio::fs::write(genres_dir.join(format!("{id}.md")), frontmatter.as_bytes()).await {
        Ok(()) => (StatusCode::OK, Json(json!({ "ok": true, "id": id }))),
        Err(_) => internal_error(),
    }
}

// ── PUT /api/v1/genres/:id ───────────────────────────────────────

pub async fn update_genre(
    State(runtime): State<BooksRuntime>,
    Path(genre_id): Path<String>,
    body: Bytes,
) -> impl IntoResponse {
    let internal_error = || {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": { "code": "INTERNAL_ERROR", "message": "Unexpected server error." } })),
        )
    };
    if genre_id_is_unsafe(&genre_id) {
        return invalid_genre_id(&genre_id);
    }
    let Ok(parsed) = serde_json::from_slice::<Value>(&body) else {
        return internal_error();
    };
    // TS `const p = body.profile`（undefined.name → TypeError → 500）。
    let Some(profile) = parsed.get("profile") else {
        return internal_error();
    };
    let fallback = json!(genre_id);
    let frontmatter = build_genre_frontmatter(
        profile,
        profile.get("name").unwrap_or(&fallback),
        profile.get("id").unwrap_or(&fallback),
        parsed.get("body").and_then(Value::as_str).unwrap_or(""),
    );
    let genres_dir = runtime.state.project_root().join("genres");
    if tokio::fs::create_dir_all(&genres_dir).await.is_err() {
        return internal_error();
    }
    match tokio::fs::write(genres_dir.join(format!("{genre_id}.md")), frontmatter.as_bytes()).await {
        Ok(()) => (StatusCode::OK, Json(json!({ "ok": true, "id": genre_id }))),
        Err(_) => internal_error(),
    }
}

// ── DELETE /api/v1/genres/:id ────────────────────────────────────

pub async fn delete_genre(
    State(runtime): State<BooksRuntime>,
    Path(genre_id): Path<String>,
) -> impl IntoResponse {
    if genre_id_is_unsafe(&genre_id) {
        return invalid_genre_id(&genre_id);
    }
    let file_path = runtime.state.project_root().join("genres").join(format!("{genre_id}.md"));
    match tokio::fs::remove_file(&file_path).await {
        Ok(()) => (StatusCode::OK, Json(json!({ "ok": true, "id": genre_id }))),
        Err(_) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": format!("Genre \"{genre_id}\" not found in project") })),
        ),
    }
}

// ── POST /api/v1/genres/:id/copy ─────────────────────────────────

pub async fn copy_genre(
    State(runtime): State<BooksRuntime>,
    Path(genre_id): Path<String>,
) -> impl IntoResponse {
    if genre_id_is_unsafe(&genre_id) {
        return invalid_genre_id(&genre_id);
    }
    let builtin = runtime.builtin_genres_dir.join(format!("{genre_id}.md"));
    let project = runtime.state.project_root().join("genres");
    let copy = async {
        tokio::fs::create_dir_all(&project).await?;
        tokio::fs::copy(&builtin, project.join(format!("{genre_id}.md"))).await?;
        std::io::Result::Ok(())
    };
    match copy.await {
        Ok(()) => (StatusCode::OK, Json(json!({ "ok": true, "path": format!("genres/{genre_id}.md") }))),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": error.to_string() }))),
    }
}
