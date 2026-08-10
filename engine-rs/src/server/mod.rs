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
    routing::{get, post},
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
