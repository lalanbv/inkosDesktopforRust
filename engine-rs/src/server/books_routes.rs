//! books 域同步端点批量挂载（/plan、/settle、/draft、/revise/:chapter）。
//!
//! 契约来源 `packages/studio/src/api/server.ts`：
//! - `POST /books/:id/plan`（L3579）：**同步**，body `{context?}` →
//!   `{bookId, chapterNumber, intentPath, goal, conflicts: []}`（TS
//!   planChapter 的 PlanChapterResult 形状）
//! - `POST /books/:id/settle`：Node 侧无此端点（Node 走 /repair-state）；
//!   Rust 侧补齐——body `{chapter, title, content, allowReapply?}` →
//!   同步结算，返回落盘摘要
//! - `POST /books/:id/draft`（L3532）：**fire-and-forget**，body
//!   `{wordCount?, context?}` → `{status:"drafting", bookId}` + SSE
//!   draft:start/complete/error；复用 write-next manual 模式（写完即停 =
//!   draft 语义，变更记录备案）
//! - `POST /books/:id/revise/:chapter`（L5604）：**同步**，body
//!   `{mode?, brief?}`（默认 spot-fix）；章节缺失 404；SSE
//!   revise:start/complete/error；返回 ReviseOutput JSON

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::agents::reviser::{revise_chapter, ReviseMode, ReviseOptions, ReviseOutput};
use crate::llm::agent_router::{AgentRouter, RoutedAgent, RoutedSettler};
use crate::llm::provider::LLMMessage;
use crate::pipeline::persisted_governed_plan::relative_to_book_dir;
use crate::pipeline::write_next::{
    write_next_chapter, ChapterReviewMode, WriteNextAgents, WriteNextConfig, WriteNextCtx,
};
use crate::server::sse::BroadcastHub;
use crate::state::manager::StateManager;
use crate::state::store::FsStateStore;

/// 端点运行时（与 AuditRuntime 同款装配面）。
#[derive(Clone)]
pub struct BooksRuntime {
    pub hub: Arc<BroadcastHub>,
    pub state: Arc<StateManager>,
    pub router: Arc<AgentRouter>,
    pub builtin_genres_dir: std::path::PathBuf,
}

// ── POST /api/v1/books/:id/plan ─────────────────────────────────

#[derive(Debug, Default, Deserialize)]
pub struct PlanBody {
    #[serde(default)]
    pub context: Option<String>,
}

/// TS `PlanChapterResult` 形状。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanChapterResult {
    pub book_id: String,
    pub chapter_number: u32,
    pub intent_path: String,
    pub goal: String,
    pub conflicts: Vec<String>,
}

pub async fn plan(
    State(runtime): State<BooksRuntime>,
    Path(book_id): Path<String>,
    body: Option<Json<PlanBody>>,
) -> impl IntoResponse {
    let Json(body) = body.unwrap_or_default();
    match run_plan(&runtime, &book_id, body.context.as_deref()).await {
        Ok(result) => (StatusCode::OK, Json(serde_json::to_value(&result).unwrap_or_default())),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error.to_string() })),
        ),
    }
}

async fn run_plan(
    runtime: &BooksRuntime,
    book_id: &str,
    context: Option<&str>,
) -> Result<PlanChapterResult, crate::pipeline::write_next::WriteNextError> {
    runtime.state.ensure_control_documents(book_id, None).await?;
    let book = runtime.state.load_book_config(book_id).await?;
    let book_dir = runtime.state.book_dir(book_id);
    let chapter_number = runtime.state.get_next_chapter_number(book_id).await?;

    let planner: &'static RoutedAgent =
        Box::leak(Box::new(RoutedAgent { router: (*runtime.router).clone(), agent: "planner" }));
    let plan = crate::agents::planner::plan_chapter(
        planner,
        &crate::agents::planner::PlanChapterInput {
            book_language: book.language.as_deref().unwrap_or("zh"),
            book_dir: &book_dir,
            chapter_number,
            external_context: context,
        },
    )
    .await?;

    // 持久化（对齐 runner resolveGovernedPlan：plan.md 供后续 compose/write 复用）。
    let _ = crate::pipeline::persisted_governed_plan::save_persisted_plan(&book_dir, &plan).await;

    Ok(PlanChapterResult {
        book_id: book_id.to_string(),
        chapter_number,
        intent_path: relative_to_book_dir(&book_dir, &plan.runtime_path),
        goal: plan.intent.goal.clone(),
        conflicts: Vec::new(),
    })
}

// ── POST /api/v1/books/:id/settle（Rust 侧补齐端点）──────────────

#[derive(Debug, Deserialize)]
pub struct SettleBody {
    pub chapter: u32,
    pub title: String,
    pub content: String,
    #[serde(default)]
    pub allow_reapply: Option<bool>,
}

/// settle 落盘摘要（标题 + 字数 + 真相文件确认）。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettleResult {
    pub chapter_number: u32,
    pub title: String,
    pub word_count: u32,
    pub updated_state: String,
    pub updated_hooks: String,
}

pub async fn settle(
    State(runtime): State<BooksRuntime>,
    Path(book_id): Path<String>,
    Json(body): Json<SettleBody>,
) -> impl IntoResponse {
    match run_settle(&runtime, &book_id, &body).await {
        Ok(result) => (StatusCode::OK, Json(serde_json::to_value(&result).unwrap_or_default())),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error })),
        ),
    }
}

async fn run_settle(runtime: &BooksRuntime, book_id: &str, body: &SettleBody) -> Result<SettleResult, String> {
    let book = runtime
        .state
        .load_book_config(book_id)
        .await
        .map_err(|e| e.to_string())?;
    let book_dir = runtime.state.book_dir(book_id);

    let writer: &'static RoutedAgent =
        Box::leak(Box::new(RoutedAgent { router: (*runtime.router).clone(), agent: "writer" }));
    let ctx: &'static crate::agents::writer::WriterCtx =
        Box::leak(Box::new(crate::agents::writer::WriterCtx {
            project_root: Box::leak(runtime.state.project_root().to_path_buf().into_boxed_path()),
            builtin_genres_dir: Box::leak(runtime.builtin_genres_dir.clone().into_boxed_path()),
            prompt_store: Box::leak(Box::new(FsStateStore)),
            state_store: Box::leak(Box::new(FsStateStore)),
        }));
    let output = crate::agents::writer::settle_chapter_state(
        ctx,
        writer,
        &crate::agents::writer::SettleChapterStateInput {
            book: &book,
            book_dir: &book_dir,
            chapter_number: body.chapter,
            title: &body.title,
            content: &body.content,
            allow_reapply: body.allow_reapply,
            chapter_intent: None,
            context_package: None,
            rule_stack: None,
            validation_feedback: None,
        },
    )
    .await
    .map_err(|e| e.to_string())?;

    Ok(SettleResult {
        chapter_number: body.chapter,
        title: body.title.clone(),
        word_count: output.word_count,
        updated_state: output.updated_state,
        updated_hooks: output.updated_hooks,
    })
}

// ── POST /api/v1/books/:id/draft（fire-and-forget）──────────────

#[derive(Debug, Default, Deserialize)]
pub struct DraftBody {
    #[serde(rename = "wordCount", default)]
    pub word_count: Option<u32>,
    #[serde(default)]
    pub context: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DraftResponse {
    pub status: &'static str,
    #[serde(rename = "bookId")]
    pub book_id: String,
}

pub async fn draft(
    State(runtime): State<BooksRuntime>,
    Path(book_id): Path<String>,
    body: Option<Json<DraftBody>>,
) -> impl IntoResponse {
    let Json(body) = body.unwrap_or_default();
    runtime.hub.broadcast("draft:start", &serde_json::json!({ "bookId": book_id }));

    let task_runtime = runtime.clone();
    let task_book_id = book_id.clone();
    tokio::spawn(async move {
        // draft 语义 = 写完即停（manual 审核模式），结算照走。
        let result = run_draft(&task_runtime, &task_book_id, &body).await;
        match result {
            Ok(outcome) => {
                task_runtime.hub.broadcast(
                    "draft:complete",
                    &serde_json::json!({
                        "bookId": task_book_id,
                        "chapterNumber": outcome.chapter_number,
                        "title": outcome.title,
                        "wordCount": outcome.word_count,
                    }),
                );
            }
            Err(error) => {
                let message = error.to_string();
                task_runtime.hub.broadcast(
                    "draft:error",
                    &serde_json::json!({ "bookId": task_book_id, "error": message }),
                );
            }
        }
    });

    (
        StatusCode::OK,
        Json(DraftResponse { status: "drafting", book_id }),
    )
}

async fn run_draft(
    runtime: &BooksRuntime,
    book_id: &str,
    body: &DraftBody,
) -> Result<crate::pipeline::write_next::ChapterPipelineResult, crate::pipeline::write_next::WriteNextError> {
    let agents = build_write_next_agents(runtime);
    let ctx = build_write_next_ctx(runtime);
    write_next_chapter(
        &runtime.state,
        &agents,
        &ctx,
        &WriteNextConfig {
            chapter_review_mode: ChapterReviewMode::Manual,
            ..Default::default()
        },
        book_id,
        body.word_count,
        None,
        body.context.as_deref(),
    )
    .await
}

// ── POST /api/v1/books/:id/revise/:chapter（同步）────────────────

#[derive(Debug, Default, Deserialize)]
pub struct ReviseBody {
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub brief: Option<String>,
}

pub async fn revise(
    State(runtime): State<BooksRuntime>,
    Path((book_id, chapter)): Path<(String, String)>,
    body: Option<Json<ReviseBody>>,
) -> impl IntoResponse {
    let Json(body) = body.unwrap_or_default();
    let Ok(chapter_number) = chapter.parse::<u32>() else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": format!("invalid chapter number: {chapter}") })),
        );
    };

    runtime.hub.broadcast(
        "revise:start",
        &serde_json::json!({ "bookId": book_id, "chapter": chapter_number }),
    );

    match run_revise(&runtime, &book_id, chapter_number, &body).await {
        Ok(result) => {
            runtime.hub.broadcast(
                "revise:complete",
                &serde_json::json!({ "bookId": book_id, "chapter": chapter_number }),
            );
            (StatusCode::OK, Json(serde_json::to_value(&result).unwrap_or_default()))
        }
        Err(error) => {
            let message = error.to_string();
            runtime.hub.broadcast(
                "revise:error",
                &serde_json::json!({ "bookId": book_id, "error": message }),
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": message })),
            )
        }
    }
}

async fn run_revise(
    runtime: &BooksRuntime,
    book_id: &str,
    chapter_number: u32,
    body: &ReviseBody,
) -> Result<ReviseOutput, String> {
    let book = runtime
        .state
        .load_book_config(book_id)
        .await
        .map_err(|e| e.to_string())?;
    let book_dir = runtime.state.book_dir(book_id);

    // 章节文件：NNNN*.md 首匹配（404 文案对齐 Node）。
    let chapters_dir = book_dir.join("chapters");
    let padded = format!("{chapter_number:04}");
    let mut entries = tokio::fs::read_dir(&chapters_dir)
        .await
        .map_err(|_| "Chapter not found".to_string())?;
    let mut matched: Option<String> = None;
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with(&padded) && name.ends_with(".md") {
            matched = Some(name);
            break;
        }
    }
    let file_name = matched.ok_or_else(|| "Chapter not found".to_string())?;
    let content = tokio::fs::read_to_string(chapters_dir.join(file_name))
        .await
        .map_err(|_| "Chapter not found".to_string())?;

    // Node 默认 spot-fix。
    let mode = match body.mode.as_deref() {
        Some("polish") => ReviseMode::Polish,
        Some("rewrite") => ReviseMode::Rewrite,
        Some("rework") => ReviseMode::Rework,
        Some("anti-detect") => ReviseMode::AntiDetect,
        _ => ReviseMode::SpotFix,
    };
    let brief = body.brief.as_deref();

    let reviser: &'static RoutedAgent =
        Box::leak(Box::new(RoutedAgent { router: (*runtime.router).clone(), agent: "reviser" }));
    let ctx: &'static crate::agents::reviser::ReviserCtx =
        Box::leak(Box::new(crate::agents::reviser::ReviserCtx {
            project_root: Box::leak(runtime.state.project_root().to_path_buf().into_boxed_path()),
            builtin_genres_dir: Box::leak(runtime.builtin_genres_dir.clone().into_boxed_path()),
            prompt_store: Box::leak(Box::new(FsStateStore)),
        }));
    revise_chapter(
        reviser,
        ctx,
        &book_dir,
        &content,
        chapter_number,
        // 空问题清单 + brief 作为外部上下文（Node reviseDraft 内部先审后修；
        // Rust 首版直接以 brief 驱动修稿——变更记录备案差异）。
        &[],
        mode,
        Some(&book.genre),
        &ReviseOptions {
            chapter_intent: None,
            chapter_memo: None,
            chapter_intent_data: None,
            context_package: None,
            rule_stack: None,
            length_spec: None,
        },
    )
    .await
    .map_err(|e| {
        let _ = brief;
        e.to_string()
    })
}

// ── 共享装配 ─────────────────────────────────────────────────────

fn build_write_next_agents(runtime: &BooksRuntime) -> WriteNextAgents<'static> {
    let leak = |agent: &'static str| -> &'static RoutedAgent {
        Box::leak(Box::new(RoutedAgent { router: (*runtime.router).clone(), agent }))
    };
    let settler: &'static RoutedSettler = Box::leak(Box::new(RoutedSettler {
        router: (*runtime.router).clone(),
        ctx: crate::agents::writer::WriterCtx {
            project_root: Box::leak(runtime.state.project_root().to_path_buf().into_boxed_path()),
            builtin_genres_dir: Box::leak(runtime.builtin_genres_dir.clone().into_boxed_path()),
            prompt_store: Box::leak(Box::new(FsStateStore)),
            state_store: Box::leak(Box::new(FsStateStore)),
        },
        chapter_number: 0,
    }));
    WriteNextAgents {
        writer: leak("writer"),
        planner: leak("planner"),
        composer: leak("composer"),
        reviser: leak("reviser"),
        auditor: leak("auditor"),
        full_auditor: None,
        normalizer: leak("length-normalizer"),
        analyzer: leak("chapter-analyzer"),
        state_validator: leak("state-validator"),
        settler,
    }
}

fn build_write_next_ctx(runtime: &BooksRuntime) -> WriteNextCtx<'static> {
    let prompt_store: &'static FsStateStore = Box::leak(Box::new(FsStateStore));
    WriteNextCtx {
        project_root: Box::leak(runtime.state.project_root().to_path_buf().into_boxed_path()),
        builtin_genres_dir: Box::leak(runtime.builtin_genres_dir.clone().into_boxed_path()),
        prompt_store,
        state_store: prompt_store,
        context_budget: None,
        notify: None,
    }
}

/// LLMMessage 引用锚（防未来 use 清理误删）。
#[allow(dead_code)]
fn _message_anchor(messages: Vec<LLMMessage>) -> Vec<LLMMessage> {
    messages
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::agent_router::LlmEndpointConfig;
    use tower::util::ServiceExt;

    fn runtime_for(root: &std::path::Path) -> BooksRuntime {
        BooksRuntime {
            hub: Arc::new(BroadcastHub::new()),
            state: Arc::new(StateManager::new(root)),
            router: Arc::new(AgentRouter::new(
                LlmEndpointConfig {
                    base_url: "http://127.0.0.1:9".into(), // 不可达——错误路径
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

    fn fixture(root: &std::path::Path) {
        let book = root.join("books").join("b1");
        std::fs::create_dir_all(book.join("chapters")).unwrap();
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

    fn app(runtime: BooksRuntime) -> axum::Router {
        axum::Router::new()
            .route("/api/v1/books/:id/plan", axum::routing::post(plan))
            .route("/api/v1/books/:id/settle", axum::routing::post(settle))
            .route("/api/v1/books/:id/draft", axum::routing::post(draft))
            .route("/api/v1/books/:id/revise/:chapter", axum::routing::post(revise))
            .with_state(runtime)
    }

    fn post(uri: &str, body: &str) -> axum::http::Request<axum::body::Body> {
        axum::http::Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json")
            .body(axum::body::Body::from(body.to_string()))
            .unwrap()
    }

    #[tokio::test]
    async fn plan_unreachable_llm_returns_500() {
        let dir = tempfile::tempdir().unwrap();
        fixture(dir.path());
        let runtime = runtime_for(dir.path());
        let response = app(runtime)
            .oneshot(post("/api/v1/books/b1/plan", "{}"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = axum::body::to_bytes(response.into_body(), 4096).await.unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(parsed["error"].as_str().unwrap().contains("chat"));
    }

    #[tokio::test]
    async fn draft_returns_drafting_immediately_and_errors_via_sse() {
        let dir = tempfile::tempdir().unwrap();
        fixture(dir.path());
        let runtime = runtime_for(dir.path());
        let hub = runtime.hub.clone();
        let mut subscriber = hub.subscribe();
        let response = app(runtime)
            .oneshot(post("/api/v1/books/b1/draft", "{}"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 4096).await.unwrap();
        let body = body.to_vec();
        #[derive(Deserialize)]
        struct TestDto {
            status: String,
            #[serde(rename = "bookId")]
            book_id: String,
        }
        let parsed: TestDto = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed.status, "drafting");
        assert_eq!(parsed.book_id, "b1");

        let first = subscriber.recv().await.unwrap();
        assert_eq!(first.event, "draft:start");
        let second = tokio::time::timeout(std::time::Duration::from_secs(10), subscriber.recv())
            .await
            .expect("draft:error 应到达")
            .unwrap();
        assert_eq!(second.event, "draft:error");
    }

    #[tokio::test]
    async fn settle_missing_book_returns_500() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = runtime_for(dir.path());
        let response = app(runtime)
            .oneshot(post(
                "/api/v1/books/missing/settle",
                r#"{"chapter":1,"title":"t","content":"c"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn revise_missing_chapter_returns_500_with_node_message() {
        let dir = tempfile::tempdir().unwrap();
        fixture(dir.path());
        let runtime = runtime_for(dir.path());
        let response = app(runtime)
            .oneshot(post("/api/v1/books/b1/revise/9", "{}"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = axum::body::to_bytes(response.into_body(), 4096).await.unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed["error"], "Chapter not found");
    }

    #[tokio::test]
    async fn revise_invalid_chapter_returns_400() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = runtime_for(dir.path());
        let response = app(runtime)
            .oneshot(post("/api/v1/books/b1/revise/abc", "{}"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
