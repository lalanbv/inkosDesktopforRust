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

pub mod agent_production;
pub mod agent_route;
pub mod audit_route;
pub mod books_routes;
pub mod book_create_routes;
pub mod fanfic_routes;
pub mod books_state_routes;
pub mod genre_routes;
pub mod interactive_film_routes;
pub mod project_config_routes;
pub mod project_files_routes;
pub mod service_routes;
pub mod session_routes;
pub mod skill_routes;
pub mod translation_routes;
pub mod sse;
pub mod style_routes;
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
            get(books_state_routes::truth_file)
                .put(books_state_routes::write_truth_file)
                .with_state(books.clone()),
        )
        // 53 号：创建状态（磁盘判定分支）。
        .route(
            "/api/v1/books/:id/create-status",
            get(books_state_routes::create_status).with_state(books.clone()),
        )
        // 53 号：genres 域（列表/详情/创建/编辑/删除/内置复制）。
        .route("/api/v1/genres", get(genre_routes::list_genres).with_state(books.clone()))
        .route("/api/v1/genres/create", post(genre_routes::create_genre).with_state(books.clone()))
        .route(
            "/api/v1/genres/:id",
            get(genre_routes::genre_detail)
                .put(genre_routes::update_genre)
                .delete(genre_routes::delete_genre)
                .with_state(books.clone()),
        )
        .route("/api/v1/genres/:id/copy", post(genre_routes::copy_genre).with_state(books.clone()))
        // 54 号：project 配置域（inkos.json 轻量键值读写面）。
        .route(
            "/api/v1/project",
            get(project_config_routes::get_project)
                .put(project_config_routes::put_project)
                .with_state(books.clone()),
        )
        .route(
            "/api/v1/project/input-governance-mode",
            get(project_config_routes::get_input_governance_mode)
                .put(project_config_routes::put_input_governance_mode)
                .with_state(books.clone()),
        )
        .route(
            "/api/v1/project/detection",
            get(project_config_routes::get_detection)
                .put(project_config_routes::put_detection)
                .with_state(books.clone()),
        )
        .route(
            "/api/v1/project/model-overrides",
            get(project_config_routes::get_model_overrides)
                .put(project_config_routes::put_model_overrides)
                .with_state(books.clone()),
        )
        .route(
            "/api/v1/project/default-model",
            get(project_config_routes::get_default_model)
                .put(project_config_routes::put_default_model)
                .with_state(books.clone()),
        )
        .route(
            "/api/v1/project/research-search",
            get(project_config_routes::get_research_search)
                .put(project_config_routes::put_research_search)
                .with_state(books.clone()),
        )
        .route(
            "/api/v1/project/chapter-review-mode",
            get(project_config_routes::get_chapter_review_mode)
                .put(project_config_routes::put_chapter_review_mode)
                .with_state(books.clone()),
        )
        .route(
            "/api/v1/project/notify",
            get(project_config_routes::get_notify)
                .put(project_config_routes::put_notify)
                .with_state(books.clone()),
        )
        .route(
            "/api/v1/project/language",
            post(project_config_routes::post_language).with_state(books.clone()),
        )
        // 55 号：风格与导入域（指纹/文风向导/正典导入/番外正典读）。
        // 58 号：创建与导入链（staging 原子创建 + 章节导入回放）。
        .route("/api/v1/books/create", post(book_create_routes::create_book).with_state(books.clone()))
        .route(
            "/api/v1/books/:id/import/chapters",
            post(book_create_routes::import_chapters_endpoint).with_state(books.clone()),
        )
        // 59 号：同人/番外/仿写创建域。
        .route("/api/v1/fanfic/init", post(fanfic_routes::fanfic_init).with_state(books.clone()))
        .route(
            "/api/v1/books/:id/fanfic/refresh",
            post(fanfic_routes::fanfic_refresh).with_state(books.clone()),
        )
        .route("/api/v1/spinoff/init", post(fanfic_routes::spinoff_init).with_state(books.clone()))
        .route("/api/v1/imitation/init", post(fanfic_routes::imitation_init).with_state(books.clone()))
        .route("/api/v1/style/analyze", post(style_routes::style_analyze))
        .route(
            "/api/v1/books/:id/style/import",
            post(style_routes::style_import).with_state(books.clone()),
        )
        .route(
            "/api/v1/books/:id/import/canon",
            post(style_routes::import_canon_endpoint).with_state(books.clone()),
        )
        .route(
            "/api/v1/books/:id/fanfic",
            get(style_routes::fanfic_show).with_state(books.clone()),
        )
        .route(
            "/api/v1/books/:id/chapter-review-mode",
            get(books_state_routes::get_review_mode)
                .put(books_state_routes::put_review_mode)
                .with_state(books.clone()),
        )
        // 60 号：skills / prompt-packs 轻域。
        .route("/api/v1/skills", get(skill_routes::list_skills).with_state(books.clone()))
        .route(
            "/api/v1/skills/import",
            post(skill_routes::import_skill).with_state(books.clone()),
        )
        .route(
            "/api/v1/skills/:skillId",
            axum::routing::delete(skill_routes::delete_skill).with_state(books.clone()),
        )
        .route(
            "/api/v1/prompt-packs",
            get(skill_routes::list_prompt_packs).with_state(books.clone()),
        )
        .route(
            "/api/v1/prompt-packs/:promptId",
            put(skill_routes::put_prompt_pack)
                .delete(skill_routes::delete_prompt_pack)
                .with_state(books.clone()),
        )
        // 63 号：services / cover 域。
        .route("/api/v1/services", get(service_routes::list_services).with_state(books.clone()))
        .route(
            "/api/v1/services/config",
            get(service_routes::get_services_config)
                .put(service_routes::put_services_config)
                .with_state(books.clone()),
        )
        .route(
            "/api/v1/services/config/import-env",
            post(service_routes::import_env_config).with_state(books.clone()),
        )
        .route(
            "/api/v1/services/models",
            get(service_routes::list_services_models).with_state(books.clone()),
        )
        .route(
            "/api/v1/services/models/custom",
            get(service_routes::list_custom_services_models).with_state(books.clone()),
        )
        .route(
            "/api/v1/services/:service/models",
            get(service_routes::list_service_models).with_state(books.clone()),
        )
        .route(
            "/api/v1/services/:service/secret",
            get(service_routes::get_service_secret)
                .put(service_routes::put_service_secret)
                .with_state(books.clone()),
        )
        .route(
            "/api/v1/services/:service/test",
            post(service_routes::test_service).with_state(books.clone()),
        )
        .route(
            "/api/v1/services/:service",
            axum::routing::delete(service_routes::delete_service).with_state(books.clone()),
        )
        .route(
            "/api/v1/cover/config",
            get(service_routes::get_cover_config)
                .put(service_routes::put_cover_config)
                .with_state(books.clone()),
        )
        .route(
            "/api/v1/cover/secret/:service",
            get(service_routes::get_cover_secret)
                .put(service_routes::put_cover_secret)
                .with_state(books.clone()),
        )
        // 65 号：交互 agent 端点（直通聊天主路径；工具面随 66 号）。
        .route("/api/v1/agent", post(agent_route::post_agent).with_state(books.clone()))
        // 64 号：sessions / interaction 会话域。
        .route(
            "/api/v1/interaction/session",
            get(session_routes::get_interaction_session).with_state(books.clone()),
        )
        .route(
            "/api/v1/sessions",
            get(session_routes::list_sessions)
                .post(session_routes::create_session)
                .with_state(books.clone()),
        )
        .route(
            "/api/v1/sessions/:sessionId/play-mode",
            put(session_routes::put_session_play_mode).with_state(books.clone()),
        )
        .route(
            "/api/v1/sessions/:sessionId/abort",
            post(session_routes::abort_session).with_state(books.clone()),
        )
        .route(
            "/api/v1/sessions/:sessionId",
            get(session_routes::get_session)
                .put(session_routes::rename_session)
                .delete(session_routes::delete_session)
                .with_state(books.clone()),
        )
        // 61 号：project 文件浏览面（通配多段路径）。
        .route(
            "/api/v1/project/files/*file",
            get(project_files_routes::get_project_file).with_state(books.clone()),
        )
        .route(
            "/api/v1/project/artifacts/*file",
            get(project_files_routes::get_project_artifact)
                .put(project_files_routes::put_project_artifact)
                .with_state(books.clone()),
        )
        // 69 号：interactive-films / projects 域（故事图谱 + 三种导出 + tar.gz）。
        .route(
            "/api/v1/interactive-films",
            get(interactive_film_routes::list_interactive_films).with_state(books.clone()),
        )
        .route(
            "/api/v1/projects/:id/story-graph/delta",
            post(interactive_film_routes::post_story_graph_delta).with_state(books.clone()),
        )
        .route(
            "/api/v1/projects/:id/story-graph",
            get(interactive_film_routes::get_story_graph).with_state(books.clone()),
        )
        .route(
            "/api/v1/projects/:id/export",
            get(interactive_film_routes::get_project_export).with_state(books.clone()),
        )
        .route(
            "/api/v1/projects/:id/story-graph/validation",
            get(interactive_film_routes::get_story_graph_validation).with_state(books.clone()),
        )
        .route(
            "/api/v1/projects/:id/story-graph/analysis",
            get(interactive_film_routes::get_story_graph_analysis).with_state(books.clone()),
        )
        .route(
            "/api/v1/projects/:id/export/json",
            get(interactive_film_routes::get_export_json).with_state(books.clone()),
        )
        .route(
            "/api/v1/projects/:id/export/ink",
            get(interactive_film_routes::get_export_ink).with_state(books.clone()),
        )
        .route(
            "/api/v1/projects/:id/export/html",
            get(interactive_film_routes::get_export_html).with_state(books.clone()),
        )
        .route(
            "/api/v1/projects/:id/nodes/:nodeId/image",
            post(interactive_film_routes::post_node_image).with_state(books.clone()),
        )
        // 70 号：translations 域（翻译工作流六端点）。
        .route(
            "/api/v1/translations",
            get(translation_routes::list_translations).with_state(books.clone()),
        )
        .route(
            "/api/v1/translations/upload",
            post(translation_routes::upload_translation).with_state(books.clone()),
        )
        .route(
            "/api/v1/translations/create",
            post(translation_routes::create_translation).with_state(books.clone()),
        )
        .route(
            "/api/v1/translations/:id",
            get(translation_routes::get_translation_detail).with_state(books.clone()),
        )
        .route(
            "/api/v1/translations/:id/run",
            post(translation_routes::run_translation).with_state(books.clone()),
        )
        .route(
            "/api/v1/translations/:id/export",
            post(translation_routes::export_translation).with_state(books),
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
