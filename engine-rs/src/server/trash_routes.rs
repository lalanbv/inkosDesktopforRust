//! 回收站路由（R60/1679 号）——此前回收站只进不出：回滚与删书往里放，没有列出 / 恢复 /
//! 清空的面。六条路由与错误码和 Node 回退腿 `server.ts` 的同名路由同口径：
//!
//! - 书级：`GET /api/v1/trash/books`、`POST …/restore`、`POST …/purge`
//! - 章级：`GET /api/v1/books/:id/trash`、`POST …/restore`、`POST …/purge`
//!
//! 回收站是「把文件找回来」，不是撤销：目标已在原位 → 409 `TRASH_TARGET_EXISTS`，一个
//! 字节都不覆盖；条目不在 → 404 `TRASH_ENTRY_NOT_FOUND`；请求不成形 → 400
//! `INVALID_TRASH_REQUEST`。书级回收站两腿共用一个目录，本腿只认自己删出来的形态。

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};

use super::books_routes::BooksRuntime;
use crate::state::trash::{
    commit_planned_restore, list_book_trash as scan_book_trash, list_chapter_trash,
    plan_chapter_trash_restore, purge_book_trash, purge_chapter_trash, restore_book_from_trash,
    ChapterTrashEntry, TrashError,
};

/// 请求体必须是一个 JSON 对象；不是就按「请求不合法」处理（与 Node 腿 `trashBody` 同文案）。
fn body_object(bytes: &Bytes) -> Result<serde_json::Map<String, Value>, TrashError> {
    match serde_json::from_slice::<Value>(bytes) {
        Ok(Value::Object(object)) => Ok(object),
        _ => Err(TrashError::Invalid("Expected a JSON object body")),
    }
}

fn refused(error: &TrashError) -> Response {
    let mut body = json!({ "error": { "code": error.code(), "message": error.message() } });
    if let TrashError::TargetExists { conflicts, .. } = error {
        body["error"]["conflicts"] = json!(conflicts);
    }
    (
        StatusCode::from_u16(error.status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
        Json(body),
    )
        .into_response()
}

fn book_not_found(book_id: &str) -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(json!({ "error": format!("Book \"{book_id}\" not found") })),
    )
        .into_response()
}

fn invalid_book_id(book_id: &str) -> Response {
    let escaped = serde_json::to_string(book_id).unwrap_or_else(|_| "\"?\"".to_string());
    (
        StatusCode::BAD_REQUEST,
        Json(json!({
            "error": { "code": "INVALID_BOOK_ID", "message": format!("Invalid book ID: {escaped}") }
        })),
    )
        .into_response()
}

/// 书在不在（书配置读得出才算在）。
async fn book_is_there(runtime: &BooksRuntime, book_id: &str) -> bool {
    runtime.state.load_book_config(book_id).await.is_ok()
}

// ── 启动清扫（R60 第三批/1685 号）────────────────────────────────────

/// 服务启动装配点的一次性清扫（bin 的 `main` 在 bind 之前调用）。读端点零写
///（1682 号定线）——过保留期的自动清空只在装配点做。保留天数读 raw inkos.json
///（缺失/损坏按缺省 30——清扫绝不因配置缺失/损坏而拒跑）；书目录跟随书架同一
/// 枚举面（`StateManager::list_books`）——本腿只清自己列得出来的书。清扫失败
/// 记日志返回零值，永不阻塞启动。
pub async fn startup_trash_sweep(
    project_root: &std::path::Path,
) -> crate::state::trash::SweptTrash {
    let raw = crate::server::project_config_routes::load_raw_config(project_root)
        .await
        .unwrap_or(Value::Null);
    let days = crate::state::trash::trash_retention_days_from_raw(&raw);
    let state = crate::state::manager::StateManager::new(project_root.to_path_buf());
    let book_dirs: Vec<std::path::PathBuf> = state
        .list_books()
        .await
        .iter()
        .map(|book_id| state.book_dir(book_id))
        .collect();
    match crate::state::trash::sweep_expired_trash(
        project_root,
        &book_dirs,
        days,
        crate::state::trash::now_millis(),
    )
    .await
    {
        Ok(swept) => swept,
        Err(error) => {
            eprintln!("inkos-engine-server: trash sweep skipped: {error}");
            crate::state::trash::SweptTrash {
                books: 0,
                batches: 0,
            }
        }
    }
}

// ── GET /api/v1/trash/books ─────────────────────────────────────────

pub async fn list_book_trash(State(runtime): State<BooksRuntime>) -> impl IntoResponse {
    match scan_book_trash(runtime.state.project_root()).await {
        Ok(entries) => (StatusCode::OK, Json(json!({ "entries": entries }))).into_response(),
        Err(error) => refused(&TrashError::Io(format!("Trash operation failed: {error}"))),
    }
}

// ── POST /api/v1/trash/books/restore ────────────────────────────────

pub async fn restore_trashed_book(
    State(runtime): State<BooksRuntime>,
    bytes: Bytes,
) -> impl IntoResponse {
    let restored = match body_object(&bytes) {
        Ok(body) => match body.get("entry").and_then(Value::as_str) {
            Some(entry) => restore_book_from_trash(runtime.state.project_root(), entry)
                .await
                .map(|book_id| (entry.to_string(), book_id)),
            None => Err(TrashError::Invalid("Expected { entry }")),
        },
        Err(error) => Err(error),
    };
    match restored {
        Ok((entry, book_id)) => {
            runtime
                .hub
                .broadcast("book:restored", &json!({ "bookId": book_id }));
            (
                StatusCode::OK,
                Json(json!({ "ok": true, "entry": entry, "bookId": book_id })),
            )
                .into_response()
        }
        Err(error) => refused(&error),
    }
}

// ── POST /api/v1/trash/books/purge ──────────────────────────────────

pub async fn purge_trashed_book(
    State(runtime): State<BooksRuntime>,
    bytes: Bytes,
) -> impl IntoResponse {
    let purged = match body_object(&bytes) {
        Ok(body) => match body.get("entry").and_then(Value::as_str) {
            Some(entry) => purge_book_trash(runtime.state.project_root(), entry)
                .await
                .map(|()| entry.to_string()),
            None => Err(TrashError::Invalid("Expected { entry }")),
        },
        Err(error) => Err(error),
    };
    match purged {
        Ok(entry) => (StatusCode::OK, Json(json!({ "ok": true, "entry": entry }))).into_response(),
        Err(error) => refused(&error),
    }
}

// ── GET /api/v1/books/:id/trash ─────────────────────────────────────

pub async fn list_chapter_trash_route(
    State(runtime): State<BooksRuntime>,
    Path(book_id): Path<String>,
) -> impl IntoResponse {
    if !crate::utils::is_safe_book_id(&book_id) {
        return invalid_book_id(&book_id);
    }
    if !book_is_there(&runtime, &book_id).await {
        return book_not_found(&book_id);
    }
    let entries: Vec<ChapterTrashEntry> =
        match list_chapter_trash(&runtime.state.book_dir(&book_id)).await {
            Ok(entries) => entries,
            Err(error) => {
                return refused(&TrashError::Io(format!("Trash operation failed: {error}")))
            }
        };
    (StatusCode::OK, Json(json!({ "entries": entries }))).into_response()
}

// ── POST /api/v1/books/:id/trash/restore ────────────────────────────

pub async fn restore_trashed_chapter(
    State(runtime): State<BooksRuntime>,
    Path(book_id): Path<String>,
    bytes: Bytes,
) -> impl IntoResponse {
    if !crate::utils::is_safe_book_id(&book_id) {
        return invalid_book_id(&book_id);
    }
    if !book_is_there(&runtime, &book_id).await {
        return book_not_found(&book_id);
    }
    let book_dir = runtime.state.book_dir(&book_id);
    match body_object(&bytes) {
        Ok(body) => {
            let (path, batch) = (
                body.get("path")
                    .filter(|value| value.is_string())
                    .and_then(Value::as_str),
                body.get("batch")
                    .filter(|value| value.is_string())
                    .and_then(Value::as_str),
            );
            let planned = match plan_chapter_trash_restore(&book_dir, path, batch).await {
                Ok(planned) => planned,
                Err(error) => return refused(&error),
            };
            // 先写目录、后搬文件：目录写不下来就什么都不搬，回收站里原样留着、书架照常。
            // 目录里缺的章号才补。状态与快照不动——找回的是文件，不是撤销一次回滚。
            if let Err(error) = runtime
                .state
                .register_restored_chapters(&book_id, &planned)
                .await
            {
                return refused(&TrashError::Io(format!("Trash operation failed: {error}")));
            }
            match commit_planned_restore(&book_dir, &planned).await {
                Ok(restored) => (
                    StatusCode::OK,
                    Json(json!({ "ok": true, "restored": restored })),
                )
                    .into_response(),
                Err(error) => refused(&error),
            }
        }
        Err(error) => refused(&error),
    }
}

// ── POST /api/v1/books/:id/trash/purge ──────────────────────────────

pub async fn purge_trashed_chapter(
    State(runtime): State<BooksRuntime>,
    Path(book_id): Path<String>,
    bytes: Bytes,
) -> impl IntoResponse {
    if !crate::utils::is_safe_book_id(&book_id) {
        return invalid_book_id(&book_id);
    }
    if !book_is_there(&runtime, &book_id).await {
        return book_not_found(&book_id);
    }
    let book_dir = runtime.state.book_dir(&book_id);
    let removed = match body_object(&bytes) {
        Ok(body) => {
            // 与 serde `Option<Value>` 同语义：字段不在、null 或类型不对都当没给。
            let text = |key: &str| body.get(key).and_then(Value::as_str);
            let all = matches!(body.get("all"), Some(Value::Bool(true)));
            purge_chapter_trash(&book_dir, text("path"), text("batch"), all).await
        }
        Err(error) => Err(error),
    };
    match removed {
        Ok(removed) => (
            StatusCode::OK,
            Json(json!({ "ok": true, "removed": removed })),
        )
            .into_response(),
        Err(error) => refused(&error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use crate::pipeline::merged_audit::RevisionGate;
    use crate::server::sse::BroadcastHub;
    use crate::state::manager::StateManager;
    use std::sync::Arc;
    use tower::util::ServiceExt;

    fn test_key() -> String {
        std::env::var("INKOS_TEST_LLM_KEY").unwrap_or_else(|_| "test-key-placeholder".to_string())
    }

    fn runtime_for(root: &std::path::Path) -> BooksRuntime {
        BooksRuntime {
            hub: Arc::new(BroadcastHub::new()),
            state: Arc::new(StateManager::new(root)),
            router: Arc::new(AgentRouter::new(
                LlmEndpointConfig {
                    context_window_tokens: 128_000,
                    base_url: "http://127.0.0.1:9".into(),
                    api_key: test_key(),
                    model: "m".into(),
                    max_tokens: 1024,
                    extra_headers: Default::default(),
                },
                Default::default(),
            )),
            builtin_genres_dir: root.to_path_buf(),
            revision_gate: RevisionGate::default(),
        }
    }

    fn app(runtime: BooksRuntime) -> axum::Router {
        axum::Router::new()
            .route("/api/v1/trash/books", axum::routing::get(list_book_trash))
            .route(
                "/api/v1/trash/books/restore",
                axum::routing::post(restore_trashed_book),
            )
            .route(
                "/api/v1/trash/books/purge",
                axum::routing::post(purge_trashed_book),
            )
            .route(
                "/api/v1/books/:id/trash",
                axum::routing::get(list_chapter_trash_route),
            )
            .route(
                "/api/v1/books/:id/trash/restore",
                axum::routing::post(restore_trashed_chapter),
            )
            .route(
                "/api/v1/books/:id/trash/purge",
                axum::routing::post(purge_trashed_chapter),
            )
            .with_state(runtime)
    }

    fn request(
        method: &str,
        uri: &str,
        body: Option<&str>,
    ) -> axum::http::Request<axum::body::Body> {
        let mut builder = axum::http::Request::builder().method(method).uri(uri);
        if body.is_some() {
            builder = builder.header("content-type", "application/json");
        }
        builder
            .body(axum::body::Body::from(body.unwrap_or("").to_string()))
            .unwrap()
    }

    async fn json_body(response: axum::http::Response<axum::body::Body>) -> Value {
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    /// 书级回收站里第一个条目的名字。
    async fn first_entry(app: &axum::Router) -> String {
        call(app, "GET", "/api/v1/trash/books", None).await.1["entries"][0]["entry"]
            .as_str()
            .unwrap()
            .to_string()
    }

    async fn call(
        app: &axum::Router,
        method: &str,
        uri: &str,
        body: Option<&str>,
    ) -> (StatusCode, Value) {
        let response = app
            .clone()
            .oneshot(request(method, uri, body))
            .await
            .unwrap();
        let status = response.status();
        (status, json_body(response).await)
    }

    fn write(path: &std::path::Path, content: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    /// 一本书：三章、带索引、每章都有快照（回滚要靠它恢复状态）。
    fn seed_book(root: &std::path::Path, id: &str, title: &str) -> std::path::PathBuf {
        let book = root.join("books").join(id);
        write(
            &book.join("book.json"),
            &format!(
                r#"{{"id":"{id}","title":"{title}","platform":"other","genre":"other","status":"active","targetChapters":10,"chapterWordCount":3000,"language":"zh","createdAt":"2026-08-17T00:00:00.000Z","updatedAt":"2026-08-17T00:00:00.000Z"}}"#
            ),
        );
        let stamp = "2026-08-17T00:00:00.000Z";
        let mut index = String::from("[");
        for (offset, name) in ["风起", "云涌", "收束"].iter().enumerate() {
            let number = offset + 1;
            write(
                &book.join("chapters").join(format!("000{number}_{name}.md")),
                &format!("# 第{number}章 {name}\n\n正文{number}。"),
            );
            if offset > 0 {
                index.push(',');
            }
            index.push_str(&format!(
                r#"{{"number":{number},"title":"{name}","status":"approved","wordCount":5,"createdAt":"{stamp}","updatedAt":"{stamp}","auditIssues":[],"lengthWarnings":[]}}"#
            ));
            let snapshot = book
                .join("story")
                .join("snapshots")
                .join(format!("{}", number - 1));
            write(
                &snapshot.join("current_state.md"),
                &format!("state-{}", number - 1),
            );
            write(
                &snapshot.join("pending_hooks.md"),
                &format!("hooks-{}", number - 1),
            );
        }
        index.push(']');
        write(&book.join("chapters").join("index.json"), &index);
        write(&book.join("story").join("current_state.md"), "state-3");
        book
    }

    #[tokio::test]
    async fn book_trash_lists_restores_and_purges() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        seed_book(root, "b1", "回来的书");
        let app = app(runtime_for(root));

        let (status, body) = call(&app, "GET", "/api/v1/trash/books", None).await;
        assert_eq!((status, body), (StatusCode::OK, json!({ "entries": [] })));

        runtime_for(root)
            .state
            .trash_book("b1", crate::state::trash::now_millis())
            .await
            .unwrap();
        let (status, body) = call(&app, "GET", "/api/v1/trash/books", None).await;
        assert_eq!(status, StatusCode::OK);
        let listed = &body["entries"][0];
        let entry = listed["entry"].as_str().unwrap().to_string();
        assert!(entry.starts_with("b1--"));
        // 整本书都在：书配置 + 三章 + 目录 + 三份快照各两件 + 当前状态 = 12 个文件。
        assert_eq!(
            (
                listed["bookId"].clone(),
                listed["fileCount"].clone(),
                listed["title"].clone()
            ),
            (json!("b1"), json!(12), json!("回来的书"))
        );
        assert!(listed["bytes"].as_u64().unwrap() > 100);
        assert!(listed["trashedAt"].as_str().unwrap().ends_with('Z'));

        let (status, body) = call(
            &app,
            "POST",
            "/api/v1/trash/books/restore",
            Some(&format!(r#"{{ "entry": "{entry}" }}"#)),
        )
        .await;
        assert_eq!(
            (status, body),
            (
                StatusCode::OK,
                json!({ "ok": true, "entry": entry, "bookId": "b1" })
            )
        );
        assert!(root.join("books").join("b1").join("book.json").is_file());
        let (_, body) = call(&app, "GET", "/api/v1/trash/books", None).await;
        assert_eq!(body, json!({ "entries": [] }));

        // 清空：真空删除，只删点名的那一个。
        runtime_for(root)
            .state
            .trash_book("b1", crate::state::trash::now_millis())
            .await
            .unwrap();
        let entry = first_entry(&app).await;
        let (status, body) = call(
            &app,
            "POST",
            "/api/v1/trash/books/purge",
            Some(&format!(r#"{{ "entry": "{entry}" }}"#)),
        )
        .await;
        assert_eq!(
            (status, body),
            (StatusCode::OK, json!({ "ok": true, "entry": entry }))
        );
        assert_eq!(
            std::fs::read_dir(root.join(".trash").join("books"))
                .unwrap()
                .count(),
            0
        );
        let (status, body) = call(
            &app,
            "POST",
            "/api/v1/trash/books/purge",
            Some(&format!(r#"{{ "entry": "{entry}" }}"#)),
        )
        .await;
        assert_eq!(
            (status, body["error"]["code"].clone()),
            (StatusCode::NOT_FOUND, json!("TRASH_ENTRY_NOT_FOUND"))
        );
    }

    #[tokio::test]
    async fn book_restore_never_overwrites_and_rejects_what_it_does_not_know() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        seed_book(root, "b1", "第一版");
        let state = runtime_for(root).state.clone();
        state
            .trash_book("b1", crate::state::trash::now_millis())
            .await
            .unwrap();
        let entry = std::fs::read_dir(root.join(".trash").join("books"))
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .next()
            .unwrap();
        // 删掉之后又建了一本同 id 的书。
        seed_book(root, "b1", "第二版");
        // Node 腿删的书：根上是 work.json 加 source/。本腿不认。
        let foreign = root.join(".trash").join("books").join("t1--20261125093007");
        write(&foreign.join("work.json"), r#"{"id":"t1"}"#);
        let app = app(runtime_for(root));

        let (status, body) = call(
            &app,
            "POST",
            "/api/v1/trash/books/restore",
            Some(&format!(r#"{{ "entry": "{entry}" }}"#)),
        )
        .await;
        assert_eq!(
            (
                status,
                body["error"]["code"].clone(),
                body["error"]["conflicts"].clone()
            ),
            (
                StatusCode::CONFLICT,
                json!("TRASH_TARGET_EXISTS"),
                json!(["books/b1"])
            )
        );
        assert!(root.join(".trash").join("books").join(&entry).is_dir());
        assert!(root.join("books").join("b1").join("book.json").is_file());

        for (entry_name, code) in [
            ("t1--20261125093007", "TRASH_ENTRY_NOT_FOUND"),
            ("nope--20261125093007", "TRASH_ENTRY_NOT_FOUND"),
        ] {
            let (status, body) = call(
                &app,
                "POST",
                "/api/v1/trash/books/restore",
                Some(&format!(r#"{{ "entry": "{entry_name}" }}"#)),
            )
            .await;
            assert_eq!(
                (status, body["error"]["code"].clone()),
                (StatusCode::NOT_FOUND, json!(code))
            );
        }
        let (status, body) = call(
            &app,
            "POST",
            "/api/v1/trash/books/purge",
            Some(r#"{"entry":"t1--20261125093007"}"#),
        )
        .await;
        assert_eq!(
            (status, body["error"]["code"].clone()),
            (StatusCode::NOT_FOUND, json!("TRASH_ENTRY_NOT_FOUND"))
        );
        assert!(foreign.join("work.json").is_file());

        for body in [
            r#"{"entry":"../b1--20261125093007"}"#,
            r#"{"entry":"notes.txt"}"#,
            "{}",
            r#"{"entry":7}"#,
            "{not json",
        ] {
            let (status, body) =
                call(&app, "POST", "/api/v1/trash/books/restore", Some(body)).await;
            assert_eq!(
                (status, body["error"]["code"].clone()),
                (StatusCode::BAD_REQUEST, json!("INVALID_TRASH_REQUEST")),
                "{body}"
            );
        }
        let (status, _) = call(&app, "POST", "/api/v1/trash/books/restore", None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn chapter_trash_lists_restores_and_purges() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let book = seed_book(root, "b1", "驳回过的书");
        let runtime = runtime_for(root);
        let app = app(runtime_for(root));

        let (status, body) = call(&app, "GET", "/api/v1/books/b1/trash", None).await;
        assert_eq!((status, body), (StatusCode::OK, json!({ "entries": [] })));

        // 驳回第 3 章 = 回滚到第 2 章：被废的内容整批进回收站。
        runtime.state.rollback_to_chapter("b1", 2).await.unwrap();
        let (status, body) = call(&app, "GET", "/api/v1/books/b1/trash", None).await;
        assert_eq!(status, StatusCode::OK);
        let entries = body["entries"].as_array().unwrap();
        let mut originals: Vec<&str> = entries
            .iter()
            .map(|entry| entry["originalPath"].as_str().unwrap())
            .collect();
        originals.sort();
        assert_eq!(
            originals,
            [
                "chapters/0003_收束.md",
                "chapters/index.json",
                "story/current_state.md"
            ]
        );
        assert!(entries.iter().all(|entry| entry["batch"]
            .as_str()
            .unwrap()
            .starts_with("rollback-")
            && entry["trashedAt"].as_str().unwrap().ends_with('Z')));

        // 找回第 3 章正文：带着回滚前的目录条目回来，并登记进目录；状态文件一个字节没动。
        let chapter = entries
            .iter()
            .find(|entry| entry["originalPath"] == "chapters/0003_收束.md")
            .unwrap()["path"]
            .as_str()
            .unwrap()
            .to_string();
        let (status, body) = call(
            &app,
            "POST",
            "/api/v1/books/b1/trash/restore",
            Some(&json!({ "path": chapter }).to_string()),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["ok"], true);
        assert_eq!(body["restored"][0]["originalPath"], "chapters/0003_收束.md");
        assert_eq!(body["restored"][0]["chapter"]["number"], 3);
        assert_eq!(body["restored"][0]["chapter"]["meta"]["title"], "收束");
        assert_eq!(body["restored"][0]["chapter"]["meta"]["status"], "approved");
        assert_eq!(
            std::fs::read_to_string(book.join("chapters").join("0003_收束.md")).unwrap(),
            "# 第3章 收束\n\n正文3。"
        );
        let index = runtime.state.load_chapter_index("b1").await.unwrap();
        let listed: Vec<(u32, serde_json::Value)> = index
            .iter()
            .map(|meta| (meta.number, serde_json::to_value(meta.status).unwrap()))
            .collect();
        assert_eq!(
            listed,
            vec![
                (1, json!("approved")),
                (2, json!("approved")),
                (3, json!("approved"))
            ]
        );
        assert_eq!(
            std::fs::read_to_string(book.join("story").join("current_state.md")).unwrap(),
            "state-2"
        );
        // 不会重复登记：平铺的另一个第 3 章回到原位，目录还是三条。
        write(
            &book.join("chapters").join(".trash").join("0003_另一份.md"),
            "# 第3章 另一份\n\n正文。",
        );
        let (status, _) = call(
            &app,
            "POST",
            "/api/v1/books/b1/trash/restore",
            Some(r#"{"path":"chapters/.trash/0003_另一份.md"}"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            runtime.state.load_chapter_index("b1").await.unwrap().len(),
            3
        );

        // 目标已在原位：409，一个都不移。
        let (status, body) = call(&app, "GET", "/api/v1/books/b1/trash", None).await;
        assert_eq!(status, StatusCode::OK);
        let state = body["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["originalPath"] == "story/current_state.md")
            .unwrap();
        let (status, body) = call(
            &app,
            "POST",
            "/api/v1/books/b1/trash/restore",
            Some(&json!({ "path": state["path"] }).to_string()),
        )
        .await;
        assert_eq!(
            (
                status,
                body["error"]["code"].clone(),
                body["error"]["conflicts"].clone()
            ),
            (
                StatusCode::CONFLICT,
                json!("TRASH_TARGET_EXISTS"),
                json!(["story/current_state.md"])
            )
        );
        let (status, body) = call(
            &app,
            "POST",
            "/api/v1/books/b1/trash/restore",
            Some(&json!({ "batch": state["batch"] }).to_string()),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        let conflicts = body["error"]["conflicts"].as_array().unwrap();
        assert!(conflicts.contains(&json!("chapters/index.json")));
        assert!(conflicts.contains(&json!("story/current_state.md")));
        let (_, body) = call(&app, "GET", "/api/v1/books/b1/trash", None).await;
        assert_eq!(body["entries"].as_array().unwrap().len(), 2);

        // 清空：一个条目 → 400 / 404 的边界 → 全部。
        let (status, body) = call(
            &app,
            "POST",
            "/api/v1/books/b1/trash/purge",
            Some(&json!({ "path": state["path"] }).to_string()),
        )
        .await;
        assert_eq!(
            (status, body),
            (StatusCode::OK, json!({ "ok": true, "removed": 1 }))
        );
        for body in [
            r#"{"path":"chapters/0001_风起.md"}"#,
            r#"{"path":"chapters/.trash/../0001_风起.md"}"#,
            r#"{"batch":".."}"#,
            r#"{"path":"x","all":true}"#,
            "{}",
            r#"{"all":"yes"}"#,
        ] {
            let (status, body) =
                call(&app, "POST", "/api/v1/books/b1/trash/purge", Some(body)).await;
            assert_eq!(
                (status, body["error"]["code"].clone()),
                (StatusCode::BAD_REQUEST, json!("INVALID_TRASH_REQUEST")),
                "{body}"
            );
        }
        let (status, _) = call(
            &app,
            "POST",
            "/api/v1/books/b1/trash/purge",
            Some(r#"{"path":"chapters/.trash/0009_没有.md"}"#),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, body) = call(
            &app,
            "POST",
            "/api/v1/books/b1/trash/purge",
            Some(r#"{"all":true}"#),
        )
        .await;
        assert_eq!(
            (status, body),
            (StatusCode::OK, json!({ "ok": true, "removed": 1 }))
        );
        assert_eq!(
            std::fs::read_dir(book.join("chapters").join(".trash"))
                .unwrap()
                .count(),
            0
        );
        for name in [
            "0001_风起.md",
            "0002_云涌.md",
            "0003_收束.md",
            "0003_另一份.md",
        ] {
            assert!(book.join("chapters").join(name).is_file(), "{name}");
        }
    }

    /// 1679 号：先写目录、后搬文件——目录写不下来就什么都不搬（先搬后写的话文件回位而目录
    /// 不列，重试又撞上「条目不在」，没法自愈）。
    #[tokio::test]
    async fn chapter_restore_writes_the_index_before_moving_any_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let book = seed_book(root, "b1", "驳回过的书");
        runtime_for(root)
            .state
            .rollback_to_chapter("b1", 2)
            .await
            .unwrap();
        let app = app(runtime_for(root));
        let (_, body) = call(&app, "GET", "/api/v1/books/b1/trash", None).await;
        let before: Vec<String> = body["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["path"].as_str().unwrap().to_string())
            .collect();
        let chapter = before
            .iter()
            .find(|path| path.ends_with("chapters/0003_收束.md"))
            .unwrap()
            .clone();
        // 目录文件的位置上是一个目录：读不出来（按「没有目录」从文件重建），也写不回去。
        std::fs::remove_file(book.join("chapters").join("index.json")).unwrap();
        std::fs::create_dir(book.join("chapters").join("index.json")).unwrap();

        let (status, body) = call(
            &app,
            "POST",
            "/api/v1/books/b1/trash/restore",
            Some(&json!({ "path": chapter }).to_string()),
        )
        .await;
        assert_eq!(
            (status, body["error"]["code"].clone()),
            (StatusCode::INTERNAL_SERVER_ERROR, json!("TRASH_IO_ERROR")),
            "{body}"
        );
        // 一个都没搬：第 3 章的文件还在回收站里。
        assert!(!book.join("chapters").join("0003_收束.md").exists());
        let (_, body) = call(&app, "GET", "/api/v1/books/b1/trash", None).await;
        let after: Vec<String> = body["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["path"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(after, before);
    }

    #[tokio::test]
    async fn chapter_routes_answer_404_for_a_missing_book_and_400_for_an_unsafe_id() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let app = app(runtime_for(root));
        let (status, body) = call(&app, "GET", "/api/v1/books/nowhere/trash", None).await;
        assert_eq!(
            (status, body),
            (
                StatusCode::NOT_FOUND,
                json!({ "error": "Book \"nowhere\" not found" })
            )
        );
        for method in ["POST"] {
            for route in ["restore", "purge"] {
                let (status, body) = call(
                    &app,
                    method,
                    &format!("/api/v1/books/nowhere/trash/{route}"),
                    Some("{}"),
                )
                .await;
                assert_eq!(
                    (status, body["error"].clone()),
                    (StatusCode::NOT_FOUND, json!("Book \"nowhere\" not found"))
                );
            }
        }
        let (status, body) = call(&app, "GET", "/api/v1/books/a%2Fb/trash", None).await;
        assert_eq!(
            (status, body["error"]["code"].clone()),
            (StatusCode::BAD_REQUEST, json!("INVALID_BOOK_ID"))
        );
    }

    #[tokio::test]
    async fn startup_sweep_reads_config_and_enumerates_books() {
        // 装配面：inkos.json 里的 retentionDays=1 决定窗口；书目录走书架枚举面
        //（books/<id>）。条目名用生产函数按真实时钟构造（2 天前 = 过期，1 天窗口）。
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("inkos.json"), r#"{"trash":{"retentionDays":1}}"#).unwrap();
        let book = root.join("books").join("b1");
        std::fs::create_dir_all(book.join("chapters").join(".trash")).unwrap();
        std::fs::write(book.join("book.json"), r#"{"id":"b1"}"#).unwrap();
        let old = crate::state::trash::compact_timestamp(
            crate::state::trash::now_millis() - 2 * 86_400_000,
        );
        let fresh = crate::state::trash::compact_timestamp(crate::state::trash::now_millis());
        let expired_batch = format!("rollback-{old}-3");
        std::fs::create_dir_all(
            book.join("chapters")
                .join(".trash")
                .join(&expired_batch)
                .join("chapters"),
        )
        .unwrap();
        std::fs::write(
            book.join("chapters")
                .join(".trash")
                .join(&expired_batch)
                .join("chapters")
                .join("0003_回.md"),
            "旧正文",
        )
        .unwrap();
        let fresh_batch = format!("rollback-{fresh}-3");
        std::fs::create_dir_all(book.join("chapters").join(".trash").join(&fresh_batch)).unwrap();
        std::fs::write(
            book.join("chapters")
                .join(".trash")
                .join(&fresh_batch)
                .join("0004_留.md"),
            "留",
        )
        .unwrap();
        std::fs::write(
            book.join("chapters").join(".trash").join("0009_旧形态.md"),
            "平铺",
        )
        .unwrap();
        let gone = format!("gone--{old}");
        std::fs::create_dir_all(root.join(".trash").join("books").join(&gone)).unwrap();
        std::fs::write(
            root.join(".trash")
                .join("books")
                .join(&gone)
                .join("book.json"),
            r#"{"id":"gone"}"#,
        )
        .unwrap();

        let swept = startup_trash_sweep(root).await;
        assert_eq!(
            swept,
            crate::state::trash::SweptTrash {
                books: 1,
                batches: 1
            }
        );
        assert!(!book
            .join("chapters")
            .join(".trash")
            .join(&expired_batch)
            .exists());
        assert!(book
            .join("chapters")
            .join(".trash")
            .join(&fresh_batch)
            .is_dir());
        assert!(book
            .join("chapters")
            .join(".trash")
            .join("0009_旧形态.md")
            .is_file());
        assert!(!root.join(".trash").join("books").join(&gone).exists());
    }
}
