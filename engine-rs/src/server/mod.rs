//! HTTP 服务（strangler 切换面）。
//!
//! 用 axum 挂载已移植的 engine-rs 函数为 `/api/v1/*` 端点，与 Node sidecar 同契约，
//! 前端无感切换。当前提供 utility 端点（由已移植 utils 驱动）；业务端点随 state/llm
//! 层就绪逐个接入。
//!
//! 设计：
//! - 所有 handler 接收 `axum::Json`，返回 `Result<Json<T>, StatusCode>`（错误统一 400/500）
//! - 路由集中导出 [`router`]，供 Tauri 命令或独立 bin 复用
//! - 测试用 `tower::ServiceExt::oneshot` 不绑端口

pub mod audit_route;
pub mod books_routes;
pub mod books_state_routes;
pub mod sse;
pub mod task_store;
pub mod write_next_route;

pub use write_next_route::{WriteNextRunner, WriteNextRuntime};

use crate::utils::{
    context_filter::{cap_context_block, ContextCapOptions},
    derive_book_id_from_title,
    length_metrics::count_chapter_length,
};
use crate::models::length_governance::LengthCountingMode;
use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};

/// 共享应用状态（暂仅版本；后续接 ProjectManager / LLM client 句柄）。
#[derive(Clone, Default)]
pub struct AppState {
    pub version: String,
}

// ── 请求/响应 DTO ───────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct DeriveBookIdRequest {
    pub title: String,
}
#[derive(Debug, Serialize)]
pub struct BookIdResponse {
    pub id: String,
}

#[derive(Debug, Deserialize)]
pub struct CountLengthRequest {
    pub content: String,
    pub mode: String, // "zh_chars" | "en_words"
}
#[derive(Debug, Serialize)]
pub struct CountLengthResponse {
    pub count: u32,
}

#[derive(Debug, Deserialize)]
pub struct CapContextRequest {
    pub content: String,
    pub label: String,
    #[serde(rename = "maxChars")]
    pub max_chars: usize,
    #[serde(rename = "headRatio", default)]
    pub head_ratio: Option<f64>,
}
#[derive(Debug, Serialize)]
pub struct CapContextResponse {
    pub content: String,
}

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub ok: bool,
    pub version: String,
}

// ── Handlers ────────────────────────────────────────────────────

async fn health(State(state): State<AppState>) -> Json<HealthResponse> {
    Json(HealthResponse { ok: true, version: state.version })
}

async fn derive_book_id(
    Json(req): Json<DeriveBookIdRequest>,
) -> Result<Json<BookIdResponse>, (StatusCode, String)> {
    if req.title.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "title 不能为空".into()));
    }
    Ok(Json(BookIdResponse { id: derive_book_id_from_title(&req.title) }))
}

async fn count_length(
    Json(req): Json<CountLengthRequest>,
) -> Result<Json<CountLengthResponse>, (StatusCode, String)> {
    let mode = match req.mode.as_str() {
        "zh_chars" => LengthCountingMode::ZhChars,
        "en_words" => LengthCountingMode::EnWords,
        other => {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("未知 mode: {other}（期望 zh_chars/en_words）"),
            ))
        }
    };
    Ok(Json(CountLengthResponse { count: count_chapter_length(&req.content, mode) }))
}

async fn cap_context(
    Json(req): Json<CapContextRequest>,
) -> Result<Json<CapContextResponse>, (StatusCode, String)> {
    let opts = ContextCapOptions {
        label: req.label.as_str(),
        max_chars: req.max_chars,
        head_ratio: req.head_ratio,
    };
    Ok(Json(CapContextResponse { content: cap_context_block(&req.content, opts) }))
}

/// 构造路由（供 Tauri 命令 / 独立 bin 复用）。
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/utils/derive-book-id", post(derive_book_id))
        .route("/api/v1/utils/count-length", post(count_length))
        .route("/api/v1/utils/cap-context", post(cap_context))
        .with_state(state)
}

/// 带运行时句柄的组合路由（utility + SSE + write-next）。
pub fn router_with_runtime(state: AppState, hub: std::sync::Arc<sse::BroadcastHub>, write_next: write_next_route::WriteNextRuntime) -> Router {
    router(state)
        .route("/api/v1/events", get(sse::events_handler).with_state(hub))
        .route("/api/v1/books/:id/write-next", post(write_next_route::write_next).with_state(write_next))
}

/// 完整组合路由（utility + SSE + write-next + audit）。
pub fn router_full(
    state: AppState,
    hub: std::sync::Arc<sse::BroadcastHub>,
    write_next: write_next_route::WriteNextRuntime,
    audit: audit_route::AuditRuntime,
) -> Router {
    router_with_runtime(state, hub, write_next)
        .route("/api/v1/books/:id/audit/:chapter", post(audit_route::audit_chapter).with_state(audit))
}

/// books 域全量组合路由（+ plan/settle/draft/revise）。
pub fn router_books(
    state: AppState,
    hub: std::sync::Arc<sse::BroadcastHub>,
    write_next: write_next_route::WriteNextRuntime,
    audit: audit_route::AuditRuntime,
    books: books_routes::BooksRuntime,
) -> Router {
    router_full(state, hub, write_next, audit)
        .route("/api/v1/books/:id/plan", post(books_routes::plan).with_state(books.clone()))
        .route("/api/v1/books/:id/settle", post(books_routes::settle).with_state(books.clone()))
        .route("/api/v1/books/:id/draft", post(books_routes::draft).with_state(books.clone()))
        .route("/api/v1/books/:id/revise/:chapter", post(books_routes::revise).with_state(books.clone()))
        .route("/api/v1/books/:id/rewrite/:chapter", post(books_routes::rewrite).with_state(books.clone()))
        .route("/api/v1/books/:id/resync/:chapter", post(books_routes::resync).with_state(books.clone()))
        .route("/api/v1/books/:id/compose", post(books_routes::compose).with_state(books.clone()))
        .route("/api/v1/books/:id/consolidate", post(books_routes::consolidate_endpoint).with_state(books.clone()))
        .route("/api/v1/books/:id/repair-state/:chapter", post(books_routes::repair_state).with_state(books.clone()))
        .route("/api/v1/books/:id/analytics", get(books_routes::analytics).with_state(books.clone()))
        .route("/api/v1/books/:id/eval", get(books_routes::eval).with_state(books.clone()))
        .route("/api/v1/books/:id/export", get(books_routes::export).with_state(books.clone()))
        // 48 号：状态端点组（列表/详情/更新/删除 + 章节读 + approve/reject +
        // truth 文件 + chapter-review-mode）。
        .route("/api/v1/books", get(books_state_routes::list_books).with_state(books.clone()))
        .route(
            "/api/v1/books/:id",
            get(books_state_routes::book_detail)
                .put(books_state_routes::update_book)
                .delete(books_state_routes::delete_book)
                .with_state(books.clone()),
        )
        .route(
            "/api/v1/books/:id/chapters/:num",
            get(books_state_routes::read_chapter)
                .put(books_state_routes::put_chapter)
                .delete(books_state_routes::delete_chapter)
                .with_state(books.clone()),
        )
        .route(
            "/api/v1/books/:id/chapters/:num/approve",
            post(books_state_routes::approve_chapter).with_state(books.clone()),
        )
        .route(
            "/api/v1/books/:id/chapters/:num/reject",
            post(books_state_routes::reject_chapter).with_state(books.clone()),
        )
        // 49 号：编辑事务域（workspace / brief / 版本读恢复）。
        .route(
            "/api/v1/books/:id/chapters/:num/workspace",
            get(books_state_routes::chapter_workspace).with_state(books.clone()),
        )
        .route(
            "/api/v1/books/:id/chapters/:num/workspace/brief",
            put(books_state_routes::put_workspace_brief).with_state(books.clone()),
        )
        .route(
            "/api/v1/books/:id/chapters/:num/versions/:versionId",
            get(books_state_routes::get_chapter_version).with_state(books.clone()),
        )
        .route(
            "/api/v1/books/:id/chapters/:num/versions/:versionId/restore",
            post(books_state_routes::restore_chapter_version).with_state(books.clone()),
        )
        // 50 号：交互运行时子集（export-save 落盘变体 + LLM 灵感卡）。
        .route(
            "/api/v1/books/:id/export-save",
            post(books_state_routes::export_save).with_state(books.clone()),
        )
        .route(
            "/api/v1/books/:id/chapters/:num/workspace/inspiration",
            post(books_state_routes::post_workspace_inspiration).with_state(books.clone()),
        )
        // 52 号：检测域（全章扫描 / 历史统计 / 单章检测，纯规则无 LLM）。
        .route(
            "/api/v1/books/:id/detect-all",
            post(books_state_routes::detect_all).with_state(books.clone()),
        )
        .route(
            "/api/v1/books/:id/detect/stats",
            get(books_state_routes::detect_stats).with_state(books.clone()),
        )
        .route(
            "/api/v1/books/:id/detect/:chapter",
            post(books_state_routes::detect_chapter).with_state(books.clone()),
        )
        .route("/api/v1/books/:id/truth", get(books_state_routes::truth_list).with_state(books.clone()))
        .route(
            "/api/v1/books/:id/truth/*file",
            get(books_state_routes::truth_file).with_state(books.clone()),
        )
        .route(
            "/api/v1/books/:id/chapter-review-mode",
            get(books_state_routes::get_review_mode)
                .put(books_state_routes::put_review_mode)
                .with_state(books),
        )
}

/// 启动 HTTP 服务（绑 127.0.0.1:port）。供独立 bin 调用。
pub async fn serve(port: u16, state: AppState) -> Result<(), std::io::Error> {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
    tracing::info!("engine-rs HTTP 服务监听 127.0.0.1:{port}");
    axum::serve(listener, router(state)).await
}

// IntoResponse 兼容层（保留以备 handler 返回 Result<Json, AppError> 扩展）
#[allow(dead_code)]
fn _ensure_into_response_impl<T: IntoResponse>() {}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Method, Request};
    use tower::ServiceExt;

    fn app() -> Router {
        router(AppState { version: "0.0.1-test".into() })
    }

    async fn body_string(body: axum::body::Body) -> String {
        let bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    #[tokio::test]
    async fn health_ok() {
        let resp = app()
            .oneshot(Request::builder().uri("/api/v1/health").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_string(resp.into_body()).await;
        assert!(body.contains(r#""ok":true"#));
        assert!(body.contains("0.0.1-test"));
    }

    #[tokio::test]
    async fn derive_book_id_endpoint() {
        let req = Request::builder()
            .method(Method::POST)
            .uri("/api/v1/utils/derive-book-id")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"title":" Harbor: Ledger! "}"#))
            .unwrap();
        let resp = app().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_string(resp.into_body()).await;
        assert!(body.contains(r#""id":"harbor-ledger""#), "实际: {body}");
    }

    #[tokio::test]
    async fn count_length_zh() {
        let req = Request::builder()
            .method(Method::POST)
            .uri("/api/v1/utils/count-length")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"content":"正文内容。","mode":"zh_chars"}"#))
            .unwrap();
        let resp = app().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_string(resp.into_body()).await;
        assert!(body.contains(r#""count":5"#), "实际: {body}");
    }

    #[tokio::test]
    async fn count_length_bad_mode_rejected() {
        let req = Request::builder()
            .method(Method::POST)
            .uri("/api/v1/utils/count-length")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"content":"x","mode":"bogus"}"#))
            .unwrap();
        let resp = app().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn cap_context_endpoint() {
        let req = Request::builder()
            .method(Method::POST)
            .uri("/api/v1/utils/cap-context")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"content":"short","label":"x","maxChars":100}"#))
            .unwrap();
        let resp = app().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_string(resp.into_body()).await;
        assert!(body.contains("short")); // 短内容不截断
    }

    // 静态断言 AppState: Clone（router with_state 需要）
    #[test]
    fn app_state_is_clone() {
        fn assert_clone<T: Clone>() {}
        assert_clone::<AppState>();
    }
}
