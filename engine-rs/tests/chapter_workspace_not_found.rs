//! 1668 号：章节工作区「不存在」契约——1664 号立案的双端同形欠账 (a)(b) 清偿。
//!
//! (b) `POST …/versions/:versionId/restore` 的**查找**失败（非法章节号 / 非法版本
//!     id / 缺文件）→ 404，与 `GET …/versions/:versionId` 及 TS 腿同码。此前 500
//!     ——两条腿都是忠实移植 1.x TS「readChapterVersion 的 assert 错误落在外层
//!     catch」的形态。替换事务失败仍 500（真内部错误）。
//! (a) `PUT …/workspace/brief` 对不存在的书 → 404。此前直接落盘，在不存在的书下
//!     写出一棵只有 brief 的目录树（幽灵书写入）。
//!
//! 成功路径同锁——防「一律 404」的假绿。既有成功面回归见
//! `e2e_write_next_contract.rs` 的 books49_e2e（brief 持久化 / restore 广播）。
//! 运行：`cargo test --test chapter_workspace_not_found`。

use std::collections::HashMap;
use std::sync::Arc;

use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
use inkos_engine::server::books_routes::BooksRuntime;
use inkos_engine::server::books_state_routes::{
    get_chapter_version, put_workspace_brief, restore_chapter_version,
};
use inkos_engine::server::sse::BroadcastHub;
use inkos_engine::state::manager::StateManager;
use serde_json::{json, Value};

const VERSION_ID: &str = "1782864000000_manual_11111111-1111-4111-8111-111111111111";

fn runtime(root: &std::path::Path) -> BooksRuntime {
    BooksRuntime {
        hub: Arc::new(BroadcastHub::new()),
        state: Arc::new(StateManager::new(root.to_path_buf())),
        router: Arc::new(AgentRouter::new(
            LlmEndpointConfig {
                context_window_tokens: 128_000,
                base_url: "http://127.0.0.1:9".into(),
                api_key: "k".into(),
                model: "m".into(),
                max_tokens: 4096,
                extra_headers: HashMap::new(),
            },
            HashMap::new(),
        )),
        builtin_genres_dir: root.join("assets").join("genres"),
        revision_gate: Default::default(),
    }
}

/// books49_e2e::fixture49 同构：两章书 + 第 1 章快照 + 第 2 章 runtime 工件 + 归档版本。
fn fixture(root: &std::path::Path) {
    let book = root.join("books").join("b1");
    let story = book.join("story");
    std::fs::create_dir_all(book.join("chapters").join(".versions").join("0002")).unwrap();
    std::fs::create_dir_all(story.join("runtime")).unwrap();
    std::fs::create_dir_all(story.join("snapshots").join("1")).unwrap();
    std::fs::write(
        book.join("book.json"),
        r#"{"id":"b1","title":"测试书","platform":"other","genre":"xianxia","status":"active","targetChapters":100,"chapterWordCount":3000,"language":"zh","createdAt":"","updatedAt":""}"#,
    )
    .unwrap();
    std::fs::write(
        book.join("chapters").join("0001_风起.md"),
        "# 第1章 风起\n\n林动睁开双眼。",
    )
    .unwrap();
    std::fs::write(
        book.join("chapters").join("0002_云涌.md"),
        "# 第2章 云涌\n\n正文二。",
    )
    .unwrap();
    std::fs::write(story.join("current_state.md"), "状态v2").unwrap();
    std::fs::write(story.join("pending_hooks.md"), "| 伏笔 | 状态 |").unwrap();
    std::fs::write(
        story.join("snapshots").join("1").join("current_state.md"),
        "状态v1",
    )
    .unwrap();
    std::fs::write(
        story.join("snapshots").join("1").join("pending_hooks.md"),
        "钩子v1",
    )
    .unwrap();
    std::fs::write(story.join("runtime").join("chapter-0002.plan.md"), "# plan").unwrap();
    std::fs::write(
        book.join("chapters")
            .join(".versions")
            .join("0002")
            .join(format!("{VERSION_ID}.md")),
        "# 第2章 旧稿\n\n旧正文。",
    )
    .unwrap();
    let now = "2026-01-01T00:00:00.000Z";
    let meta = |number: u32| {
        json!({
            "number": number, "title": format!("第{number}章"), "status": "ready-for-review",
            "wordCount": 10, "auditIssues": [], "lengthWarnings": [],
            "createdAt": now, "updatedAt": now,
        })
    };
    std::fs::write(
        book.join("chapters").join("index.json"),
        serde_json::to_string(&vec![meta(1), meta(2)]).unwrap(),
    )
    .unwrap();
}

fn app(runtime: BooksRuntime) -> axum::Router {
    axum::Router::new()
        .route(
            "/api/v1/books/:id/chapters/:num/workspace/brief",
            axum::routing::put(put_workspace_brief),
        )
        .route(
            "/api/v1/books/:id/chapters/:num/versions/:versionId",
            axum::routing::get(get_chapter_version),
        )
        .route(
            "/api/v1/books/:id/chapters/:num/versions/:versionId/restore",
            axum::routing::post(restore_chapter_version),
        )
        .with_state(runtime)
}

async fn call(
    app: axum::Router,
    method: &str,
    uri: &str,
    body: Option<&str>,
) -> (axum::http::StatusCode, Value) {
    use tower::ServiceExt;
    let mut builder = axum::http::Request::builder().method(method).uri(uri);
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    let request = builder
        .body(axum::body::Body::from(body.unwrap_or("").to_string()))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .unwrap();
    let parsed = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, parsed)
}

#[tokio::test]
async fn restore_lookup_failures_are_not_found_and_leave_the_chapter_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    fixture(&root);
    let chapter = root
        .join("books")
        .join("b1")
        .join("chapters")
        .join("0002_云涌.md");
    let original = std::fs::read_to_string(&chapter).unwrap();

    // 非法版本 id（格式不符）→ 404，与 GET 同码。
    let (status, body) = call(
        app(runtime(&root)),
        "POST",
        "/api/v1/books/b1/chapters/2/versions/v1/restore",
        None,
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::NOT_FOUND, "body: {body}");
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .contains("Invalid chapter version id"),
        "body: {body}"
    );
    let (get_status, _) = call(
        app(runtime(&root)),
        "GET",
        "/api/v1/books/b1/chapters/2/versions/v1",
        None,
    )
    .await;
    assert_eq!(get_status, axum::http::StatusCode::NOT_FOUND);

    // 格式合法但不存在 → 404。
    let (status, body) = call(
        app(runtime(&root)),
        "POST",
        "/api/v1/books/b1/chapters/2/versions/1782864000001_restore_22222222-2222-4222-8222-222222222222/restore",
        None,
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::NOT_FOUND, "body: {body}");
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .contains("chapter version not found"),
        "body: {body}"
    );

    // 非法章节号 → 404。
    let (status, body) = call(
        app(runtime(&root)),
        "POST",
        "/api/v1/books/b1/chapters/abc/versions/v1/restore",
        None,
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::NOT_FOUND, "body: {body}");
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .contains("Invalid chapter number"),
        "body: {body}"
    );

    // 三次查找失败都不得触碰正文。
    assert_eq!(std::fs::read_to_string(&chapter).unwrap(), original);

    // 成功路径仍 200——防「一律 404」的假绿。
    let (status, body) = call(
        app(runtime(&root)),
        "POST",
        &format!("/api/v1/books/b1/chapters/2/versions/{VERSION_ID}/restore"),
        None,
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK, "body: {body}");
    assert_eq!(body["ok"], true);
    // 替换事务为正文补尾换行（与 books49_e2e 的 restore 断言同形）。
    assert_eq!(
        std::fs::read_to_string(&chapter).unwrap(),
        "# 第2章 旧稿\n\n旧正文。\n"
    );
}

#[tokio::test]
async fn workspace_brief_on_a_missing_book_is_not_found_and_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    fixture(&root);

    let (status, body) = call(
        app(runtime(&root)),
        "PUT",
        "/api/v1/books/ghost/chapters/1/workspace/brief",
        Some(r#"{ "brief": "给一本不存在的书写提示。" }"#),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::NOT_FOUND, "body: {body}");
    assert_eq!(body, json!({ "error": "Book not found" }));
    // 此前会写出 books/ghost/story/runtime/chapter-0001.user-brief.md。
    assert!(!root.join("books").join("ghost").exists());

    // 既有书照常落盘。
    let (status, body) = call(
        app(runtime(&root)),
        "PUT",
        "/api/v1/books/b1/chapters/1/workspace/brief",
        Some(r#"{ "brief": "收紧节奏" }"#),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK, "body: {body}");
    assert_eq!(body["brief"], "收紧节奏");
    assert_eq!(
        std::fs::read_to_string(
            root.join("books")
                .join("b1")
                .join("story")
                .join("runtime")
                .join("chapter-0001.user-brief.md")
        )
        .unwrap(),
        "收紧节奏\n"
    );
}
