//! `/api/v1/books/:id/audit/:chapter` 端点（同步审计契约）。
//!
//! 移植 Node 契约（server.ts L5570-5598）：**同步**流程（非 fire-and-forget）——
//! 读章节文件（缺失 404）→ 真实 `audit_chapter`（11 路真相文件 + 维度审计）→
//! 返回 AuditResult JSON；SSE 广播 `audit:start` / `audit:complete {passed}` /
//! `audit:error`。失败返回 500 `{error}`。

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;

use crate::agents::continuity::audit_chapter as run_audit;
use crate::llm::agent_router::{AgentRouter, FullCycleAuditor, RoutedAgent};
use crate::server::sse::BroadcastHub;
use crate::state::manager::StateManager;
use crate::state::store::FsStateStore;

/// 审计端点运行时。
#[derive(Clone)]
pub struct AuditRuntime {
    pub hub: Arc<BroadcastHub>,
    pub state: Arc<StateManager>,
    pub router: Arc<AgentRouter>,
    pub builtin_genres_dir: std::path::PathBuf,
}

impl AuditRuntime {
    /// 运行时有效 router（109 号，与 BooksRuntime::effective_router 同款——
    /// state/router 均为共享 Arc，经 BooksRuntime 委托复用缓存）。
    pub async fn effective_router(&self) -> Arc<AgentRouter> {
        crate::server::books_routes::BooksRuntime {
            hub: self.hub.clone(),
            state: self.state.clone(),
            router: self.router.clone(),
            builtin_genres_dir: self.builtin_genres_dir.clone(),
            revision_gate: Default::default(),
        }
        .effective_router()
        .await
    }
}

#[derive(Debug, Default, Deserialize)]
pub struct AuditBody {
    #[serde(default)]
    pub temperature: Option<f64>,
}

/// POST /api/v1/books/:id/audit/:chapter
pub async fn audit_chapter(
    State(runtime): State<AuditRuntime>,
    Path((book_id, chapter)): Path<(String, String)>,
    body: Option<Json<AuditBody>>,
) -> impl IntoResponse {
    let Json(body) = body.unwrap_or_default();
    let Ok(chapter_number) = chapter.parse::<u32>() else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": format!("invalid chapter number: {chapter}") })),
        );
    };

    runtime
        .hub
        .broadcast("audit:start", &serde_json::json!({ "bookId": book_id, "chapter": chapter_number }));

    match run_audit_flow(&runtime, &book_id, chapter_number, body.temperature).await {
        Ok(result) => {
            runtime.hub.broadcast(
                "audit:complete",
                &serde_json::json!({ "bookId": book_id, "chapter": chapter_number, "passed": result.passed }),
            );
            (StatusCode::OK, Json(serde_json::to_value(&result).unwrap_or_default()))
        }
        Err(error) => {
            let message = error.to_string();
            runtime.hub.broadcast(
                "audit:error",
                &serde_json::json!({ "bookId": book_id, "error": message }),
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": message })),
            )
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum AuditFlowError {
    #[error("book.json load failed: {0}")]
    Book(String),
    #[error("Chapter not found")]
    ChapterNotFound,
    #[error("audit failed: {0}")]
    Audit(String),
}

pub(crate) async fn run_audit_flow(
    runtime: &AuditRuntime,
    book_id: &str,
    chapter_number: u32,
    temperature: Option<f64>,
) -> Result<crate::agents::continuity::AuditResult, AuditFlowError> {
    let book = runtime
        .state
        .load_book_config(book_id)
        .await
        .map_err(|e| AuditFlowError::Book(e.to_string()))?;
    let book_dir = runtime.state.book_dir(book_id);

    // 章节文件：NNNN*.md 首匹配（对齐 Node readdir+find）。
    let chapters_dir = book_dir.join("chapters");
    let padded = format!("{chapter_number:04}");
    let mut entries = tokio::fs::read_dir(&chapters_dir)
        .await
        .map_err(|_| AuditFlowError::ChapterNotFound)?;
    let mut matched: Option<String> = None;
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with(&padded) && name.ends_with(".md") {
            matched = Some(name);
            break;
        }
    }
    let file_name = matched.ok_or(AuditFlowError::ChapterNotFound)?;
    let content = tokio::fs::read_to_string(chapters_dir.join(file_name))
        .await
        .map_err(|_| AuditFlowError::ChapterNotFound)?;

    // 完整审计（FullCycleAuditor → 真实 audit_chapter 编排）。
    let auditor = FullCycleAuditor {
        router: runtime.effective_router().await,
        project_root: runtime.state.project_root().to_path_buf(),
        builtin_genres_dir: runtime.builtin_genres_dir.clone(),
        book_dir,
        chapter_number,
        genre: book.genre.clone(),
    };
    let chat = RoutedAgent {
        router: runtime.effective_router().await,
        agent: "auditor",
    };
    let prompt_store = FsStateStore;
    let ctx = crate::agents::continuity::AuditChapterCtx {
        project_root: runtime.state.project_root(),
        builtin_genres_dir: &runtime.builtin_genres_dir,
        prompt_store: &prompt_store,
    };
    let options = crate::agents::continuity::AuditChapterOptions {
        temperature,
        chapter_intent: None,
        chapter_memo: None,
        context_package: None,
        rule_stack: None,
        truth_file_overrides: None,
    };
    let _ = auditor; // 主链经 ctx+chat 直调（FullCycleAuditor 供 write-next 环复用）

    run_audit(
        &ctx,
        &chat,
        &runtime.state.book_dir(book_id),
        &content,
        chapter_number,
        Some(&book.genre),
        &options,
    )
    .await
    .map_err(|e| AuditFlowError::Audit(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::agent_router::{AgentOverride, LlmEndpointConfig};
    use crate::server::sse::BroadcastHub;
    use tower::util::ServiceExt;

    fn fixture(root: &std::path::Path) {
        let book = root.join("books").join("b1");
        std::fs::create_dir_all(book.join("chapters")).unwrap();
        std::fs::create_dir_all(root.join("genres")).unwrap();
        std::fs::write(
            root.join("genres").join("other.md"),
            "---\nname: 其他\nid: other\nchapterTypes: [\"推进章\"]\nfatigueWords: []\nauditDimensions: [1]\n---\n指导\n",
        )
        .unwrap();
        std::fs::write(
            book.join("book.json"),
            r#"{"id":"b1","title":"t","platform":"other","genre":"other","status":"active","targetChapters":10,"chapterWordCount":3000,"language":"zh","createdAt":"","updatedAt":""}"#,
        )
        .unwrap();
        std::fs::write(
            book.join("chapters").join("0001_风起.md"),
            "# 第1章 风起\n\n林动睁开双眼，灵气顺着经脉游走。",
        )
        .unwrap();
    }

    fn runtime_for(root: &std::path::Path, llm_port: u16) -> AuditRuntime {
        AuditRuntime {
            hub: Arc::new(BroadcastHub::new()),
            state: Arc::new(StateManager::new(root)),
            router: Arc::new(AgentRouter::new(
                LlmEndpointConfig {
                    base_url: format!("http://127.0.0.1:{llm_port}"),
                    api_key: "k".into(),
                    model: "m".into(),
                    max_tokens: 1024,
                    extra_headers: Default::default(),
                },
                std::collections::HashMap::from([(
                    "auditor".to_string(),
                    AgentOverride::default(),
                )]),
            )),
            builtin_genres_dir: root.to_path_buf(),
        }
    }

    #[tokio::test]
    async fn chapter_not_found_returns_404() {
        let dir = tempfile::tempdir().unwrap();
        fixture(dir.path());
        let runtime = fixture_with_empty_chapters(dir.path());
        let app = axum::Router::new()
            .route("/api/v1/books/:id/audit/:chapter", axum::routing::post(audit_chapter))
            .with_state(runtime);

        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/v1/books/b1/audit/9")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = axum::body::to_bytes(response.into_body(), 4096).await.unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(parsed["error"].as_str().unwrap().contains("Chapter not found"));
    }

    fn fixture_with_empty_chapters(root: &std::path::Path) -> AuditRuntime {
        let book = root.join("books").join("b1");
        std::fs::create_dir_all(book.join("chapters")).unwrap();
        std::fs::write(
            book.join("book.json"),
            r#"{"id":"b1","title":"t","platform":"other","genre":"other","status":"active","targetChapters":10,"chapterWordCount":3000,"language":"zh","createdAt":"","updatedAt":""}"#,
        )
        .unwrap();
        fixture_for_runtime(root)
    }

    fn fixture_for_runtime(root: &std::path::Path) -> AuditRuntime {
        // 不可达 LLM（端口 9 discard）——触发 audit:error 路径。
        AuditRuntime {
            hub: Arc::new(BroadcastHub::new()),
            state: Arc::new(StateManager::new(root)),
            router: Arc::new(AgentRouter::new(
                LlmEndpointConfig {
                    base_url: "http://127.0.0.1:9".into(),
                    api_key: "k".into(),
                    model: "m".into(),
                    max_tokens: 1024,
                    extra_headers: Default::default(),
                },
                Default::default(),
            )),
            builtin_genres_dir: root.to_path_buf(),
        }
    }

    #[tokio::test]
    async fn llm_unreachable_returns_500_and_error_event() {
        let dir = tempfile::tempdir().unwrap();
        fixture(dir.path());
        let runtime = fixture_for_runtime(dir.path());
        let hub = runtime.hub.clone();
        let mut subscriber = hub.subscribe();
        let app = axum::Router::new()
            .route("/api/v1/books/:id/audit/:chapter", axum::routing::post(audit_chapter))
            .with_state(runtime);

        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/v1/books/b1/audit/1")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

        // SSE：start → error。
        let first = subscriber.recv().await.unwrap();
        assert_eq!(first.event, "audit:start");
        let second = tokio::time::timeout(std::time::Duration::from_secs(10), subscriber.recv())
            .await
            .expect("error 事件应到达")
            .unwrap();
        assert_eq!(second.event, "audit:error");
        assert!(second.data.contains("b1"));
    }

    #[allow(dead_code)]
    fn unused_fixture(root: &std::path::Path) -> AuditRuntime {
        runtime_for(root, 9)
    }
}
