//! 1672 号：`GET /api/v1/play/runs/:worldId/:runId` 对不存在的世界 / 运行回 404。
//!
//! 与 TS 腿同码同形（上游 2.0 自 7e60e13f 起）：世界不存在 →
//! `PLAY_WORLD_NOT_FOUND`；运行不存在 → `PLAY_RUN_NOT_FOUND`。Rust 端口移植时
//! （2026-08-16）TS 回的是 200 空壳，上游改后没跟——空壳会把「数据在另一棵树上」
//! （产品引擎 `worlds/` ↔ TS 腿 `works/<id>/source/`）伪装成「全新的空世界」。
//!
//! 运行：`cargo test --test play_run_not_found`。

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use serde_json::{json, Value};
use tower::ServiceExt;

use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig, LLM_UNCONFIGURED_BASE_URL};
use inkos_engine::server::audit_route::AuditRuntime;
use inkos_engine::server::books_routes::BooksRuntime;
use inkos_engine::server::sse::BroadcastHub;
use inkos_engine::server::write_next_route::WriteNextRuntime;
use inkos_engine::server::{router_books, AppState};
use inkos_engine::state::manager::StateManager;

fn app(root: &Path) -> axum::Router {
    // 读取面不调模型：启动路由器用 bin 的未配置占位（空 key）。
    let placeholder = Arc::new(AgentRouter::new(
        LlmEndpointConfig {
            base_url: LLM_UNCONFIGURED_BASE_URL.into(),
            api_key: String::new(),
            model: "default".into(),
            max_tokens: 8192,
            extra_headers: HashMap::new(),
            context_window_tokens: 128_000,
        },
        HashMap::new(),
    ));
    let hub = Arc::new(BroadcastHub::new());
    let state = Arc::new(StateManager::new(root.to_path_buf()));
    let runner: inkos_engine::server::WriteNextRunner =
        Arc::new(|_, _, _, _, _, _| Box::pin(async { Err("unused".to_string()) }));
    let write_next = WriteNextRuntime {
        hub: hub.clone(),
        state: state.clone(),
        runner,
        project_root: root.to_path_buf(),
    };
    let audit = AuditRuntime {
        hub: hub.clone(),
        state: state.clone(),
        router: placeholder.clone(),
        builtin_genres_dir: root.join("assets").join("genres"),
    };
    let books = BooksRuntime {
        hub,
        state,
        router: placeholder,
        builtin_genres_dir: root.join("assets").join("genres"),
        revision_gate: Default::default(),
    };
    router_books(
        AppState {
            version: "not-found".to_string(),
        },
        BroadcastHub::new().into(),
        write_next,
        audit,
        books,
    )
}

async fn get(app: axum::Router, uri: &str) -> (StatusCode, Value) {
    let request = Request::builder()
        .method(Method::GET)
        .uri(uri)
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn world_input<'a>(id: &'a str, title: &'a str) -> inkos_engine::play::PlayWorldInput<'a> {
    inkos_engine::play::PlayWorldInput {
        id,
        title,
        premise: "雾港的渡轮只在午夜靠岸。",
        world_contract: "",
        visual_contract: "",
        mode: "open",
        language: "zh",
    }
}

#[tokio::test]
async fn a_missing_world_is_404_not_an_empty_shell() {
    let dir = tempfile::tempdir().unwrap();
    let (status, body) = get(app(dir.path()), "/api/v1/play/runs/ghost/main").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    // 与 TS 腿同形：`{ error: { code } }`，不带 message。
    assert_eq!(body, json!({ "error": { "code": "PLAY_WORLD_NOT_FOUND" } }));
    // 只读：不存在的世界不得被建出来。
    assert!(!dir.path().join("worlds").join("ghost").exists());
}

#[tokio::test]
async fn a_missing_run_is_404() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    inkos_engine::play::create_world(root, &world_input("w1", "世界一"))
        .await
        .unwrap();
    let (status, body) = get(app(root), "/api/v1/play/runs/w1/ghost").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body, json!({ "error": { "code": "PLAY_RUN_NOT_FOUND" } }));
    assert!(!root.join("worlds").join("w1").join("runs").exists());
}

#[tokio::test]
async fn an_existing_run_still_answers_200() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    inkos_engine::play::create_world(root, &world_input("w1", "世界一"))
        .await
        .unwrap();
    inkos_engine::play::ensure_run(root, "w1", "main")
        .await
        .unwrap();
    inkos_engine::play::append_transcript_turn(root, "w1", "main", "user", "第一问")
        .await
        .unwrap();
    let (status, body) = get(app(root), "/api/v1/play/runs/w1/main").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["title"], "世界一");
    assert_eq!(body["worldId"], "w1");
    assert_eq!(body["runId"], "main");
    assert_eq!(body["transcript"].as_array().map(Vec::len), Some(1));
    assert_eq!(body["graph"]["entities"].as_array().map(Vec::len), Some(0));
}

#[tokio::test]
async fn an_unsafe_segment_is_still_rejected_first() {
    let dir = tempfile::tempdir().unwrap();
    let (status, _) = get(app(dir.path()), "/api/v1/play/runs/%2e%2e/main").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
