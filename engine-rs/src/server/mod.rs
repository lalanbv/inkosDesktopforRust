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
pub mod backup_routes;
pub mod books_routes;
pub mod book_create_routes;
pub mod fanfic_routes;
pub mod books_state_routes;
pub mod series_backfill_routes;
pub mod genre_routes;
pub mod loopback_guard;
pub mod ops_routes;
pub mod play_routes;
pub mod interactive_film_routes;
pub mod project_config_routes;
pub mod project_files_routes;
pub mod segment_guard;
pub mod service_routes;
pub mod session_routes;
pub mod skill_routes;
pub mod translation_routes;
pub mod upload_common;
pub mod sse;
pub mod static_routes;
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
    routing::{delete, get, post, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};

/// 共享应用状态（暂仅版本；后续接 ProjectManager / LLM client 句柄）。
#[derive(Clone, Default)]
pub struct AppState {
    pub version: String,
}

// ── 大载荷读取上限（273 号）─────────────────────────────────────
//
// axum 默认 2MB 请求体上限只约束 `Bytes`/`String`/`Json` 等**提取器**
//（axum-core：提取器走 limited body）；手工 `to_bytes(req.into_body(), n)`
// 不受限。TS 端（Hono）对 body 无统一上限、由各端点业务上限兜底——
// 此处读取上限 = TS 业务上限按 base64 膨胀（×4/3）取整；TS 无上限者给
// 64MB 安全上界（loopback 守卫已限定本机）。
/// 整本文本端点（导入/风格/同人 sourceText / agent 会话）——TS 无上限。
pub(crate) const BODY_CAP_LARGE_TEXT: usize = 64 * 1024 * 1024;
/// 翻译源上传——TS 解码上限 80MB，base64 膨胀后 ≥107MB。
pub(crate) const BODY_CAP_TRANSLATION_UPLOAD: usize = 128 * 1024 * 1024;
/// 正典上传——TS 解码上限 18MB，膨胀后 24MB。
pub(crate) const BODY_CAP_CANON_UPLOAD: usize = 32 * 1024 * 1024;
/// Skill 文件夹导入——TS 总限 8MB，膨胀后 ~10.7MB。
pub(crate) const BODY_CAP_SKILL_IMPORT: usize = 16 * 1024 * 1024;

/// 大载荷端点专用读取：`Bytes` 提取器会被 axum 默认 2MB 截断（413 纯文本），
/// 改走手工 `to_bytes` + 显式上限。超限返回 413，由调用方按各自错误形态包装。
pub(crate) async fn read_body_capped(
    req: axum::extract::Request,
    max: usize,
) -> Result<axum::body::Bytes, StatusCode> {
    axum::body::to_bytes(req.into_body(), max)
        .await
        .map_err(|_| StatusCode::PAYLOAD_TOO_LARGE)
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
    /// 后端标识（170 号 W-C1）：诊断面交叉核验「声明后端 vs 实际应答」。
    /// 加法字段——Node sidecar 无 `/api/v1/health`（Rust 超集端点，62 号），
    /// 不存在契约破坏面。
    pub backend: String,
}

// ── Handlers ────────────────────────────────────────────────────

async fn health(State(state): State<AppState>) -> Json<HealthResponse> {
    Json(HealthResponse { ok: true, version: state.version, backend: "rust-engine".to_string() })
}

/// 工具目录单条投影（debug 面）。
#[derive(Debug, Serialize)]
pub struct DebugToolEntry {
    pub name: String,
    pub description: String,
    /// 参数 schema 的规范化 sha256（serde_json BTreeMap 键序即规范化序；
    /// 双端目录机械对照用——差分器 tool-catalog 维度数据源）。
    pub parameters_sha256: String,
}

/// `GET /api/v1/debug/tools`（Rust 超集只读端点，health 62 号同位）：全族
/// 工具注册表投影（R38b）——名称/描述/参数哈希，声明序 = 分发链序。
/// loopback CORS 已由全局守卫限定本机（与 health 同级）。
async fn debug_tools() -> Json<Vec<DebugToolEntry>> {
    use sha2::{Digest, Sha256};
    Json(
        crate::interaction::registry::ToolRegistry::global()
            .all()
            .map(|def| {
                let parameters = def.parameters();
                let mut hasher = Sha256::new();
                hasher.update(serde_json::to_string(&parameters).unwrap_or_default());
                DebugToolEntry {
                    name: def.name().to_string(),
                    description: def.description(),
                    parameters_sha256: format!("{:x}", hasher.finalize()),
                }
            })
            .collect(),
    )
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
    req: axum::extract::Request,
) -> Result<Json<CountLengthResponse>, (StatusCode, String)> {
    // 273 号：`Json` 提取器 2MB 上限会截断整稿统计；改手工读（TS c.req.json() 无上限）。
    let body = read_body_capped(req, BODY_CAP_LARGE_TEXT)
        .await
        .map_err(|s| (s, "request body too large".to_string()))?;
    let req: CountLengthRequest = serde_json::from_slice(&body)
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("invalid JSON body: {e}")))?;
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
    req: axum::extract::Request,
) -> Result<Json<CapContextResponse>, (StatusCode, String)> {
    let body = read_body_capped(req, BODY_CAP_LARGE_TEXT)
        .await
        .map_err(|s| (s, "request body too large".to_string()))?;
    let req: CapContextRequest = serde_json::from_slice(&body)
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("invalid JSON body: {e}")))?;
    let opts = ContextCapOptions {
        label: req.label.as_str(),
        max_chars: req.max_chars,
        head_ratio: req.head_ratio,
    };
    Ok(Json(CapContextResponse { content: cap_context_block(&req.content, opts) }))
}

/// 回环 Origin 反射 CORS 层（173 号 W-A4a 收紧；原 125 号为 Hono `cors()`
/// 默认参 `Allow-Origin: *` 等价层）。allow_origin 判定与回环守卫同规则
/// （共享 [`loopback_guard::origin_is_allowed`]）：回环主机 Origin（端口
/// 不限）或 `INKOS_ENGINE_ALLOWED_ORIGINS` 白名单命中 → ACAO **反射请求
/// Origin**；其余（远端、`null`、无 Origin）→ 不发 ACAO。守卫仍在更外层
/// 403 远端 Origin，本层保证守卫被 `INKOS_ENGINE_LOOPBACK_GUARD=0` 关闭
/// 时 `*` 放大面也不复活。方法族 GET/HEAD/PUT/POST/DELETE/PATCH；头镜像
/// 请求的 `Access-Control-Request-Headers`；无 credentials / 无 expose /
/// 无 max-age；preflight 短路 200（tower-http 对不允许的 preflight 也回
/// 200、仅缺 ACAO——浏览器以无 ACAO 拒绝，与 Hono 204 无 ACAO 同效）。
pub fn sidecar_cors_layer() -> tower_http::cors::CorsLayer {
    sidecar_cors_layer_with_extras(loopback_guard::env_extra_origins())
}

/// [`sidecar_cors_layer`] 的显式白名单形态（供测试/嵌入装配；bin 走 env 版）。
pub fn sidecar_cors_layer_with_extras(
    extra_origins: Vec<String>,
) -> tower_http::cors::CorsLayer {
    use axum::http::Method;
    tower_http::cors::CorsLayer::new()
        .allow_origin(tower_http::cors::AllowOrigin::predicate(move |origin, _| {
            loopback_guard::origin_is_allowed(origin.to_str().unwrap_or(""), &extra_origins)
        }))
        .allow_methods([
            Method::GET,
            Method::HEAD,
            Method::PUT,
            Method::POST,
            Method::DELETE,
            Method::PATCH,
        ])
        .allow_headers(tower_http::cors::AllowHeaders::mirror_request())
}

/// 构造路由（供 Tauri 命令 / 独立 bin 复用）。
pub fn router(state: AppState) -> Router {    Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/debug/tools", get(debug_tools))
        .route("/api/v1/utils/derive-book-id", post(derive_book_id))
        .route("/api/v1/utils/count-length", post(count_length))
        .route("/api/v1/utils/cap-context", post(cap_context))
        .with_state(state)
        .layer(axum::middleware::from_fn(api_no_store))
}

/// 带运行时句柄的组合路由（utility + SSE + write-next）。
pub fn router_with_runtime(state: AppState, hub: std::sync::Arc<sse::BroadcastHub>, write_next: write_next_route::WriteNextRuntime) -> Router {
    router(state)
        // SSE 快照对账用引擎项目根（前端 EventSource 只传 sessionId；
        // write-next 运行时持有同一根——检查点落盘面与恢复面同源）。
        .route(
            "/api/v1/events",
            get(sse::events_handler).with_state(sse::EventsState {
                hub,
                project_root: write_next.project_root.clone(),
            }),
        )
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
        // 443 号：R18 备份双端点移植（386 备案随 164 号默认引擎切换失效后翻案）。
        .route("/api/v1/backup/export", get(backup_routes::export).with_state(books.clone()))
        .route(
            "/api/v1/backup/import",
            post(backup_routes::import)
                .layer(axum::extract::DefaultBodyLimit::max(512 * 1024 * 1024))
                .with_state(books.clone()),
        )
        // 48 号：状态端点组（列表/详情/更新/删除 + 章节读 + approve/reject +
        // truth 文件 + chapter-review-mode）。
        .route("/api/v1/books", get(books_state_routes::list_books).with_state(books.clone()))
        .route(
            "/api/v1/books/:id/timeline",
            get(books_state_routes::get_timeline).put(books_state_routes::put_timeline).with_state(books.clone()),
        )
        .route(
            "/api/v1/books/:id/series-backfill/extract",
            post(series_backfill_routes::extract).with_state(books.clone()),
        )
        .route(
            "/api/v1/books/:id/series-backfill/apply",
            post(series_backfill_routes::apply).with_state(books.clone()),
        )
        .route(
            "/api/v1/books/:id/series-backfill/existing",
            get(series_backfill_routes::existing).with_state(books.clone()),
        )
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
        // 273 号：正典上传与 canon-file 导入（UI ImportManager 调用面）。
        .route(
            "/api/v1/import/canon/upload",
            post(style_routes::upload_canon).with_state(books.clone()),
        )
        .route(
            "/api/v1/books/:id/import/canon-file",
            post(style_routes::import_canon_file).with_state(books.clone()),
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
        .route(
            "/api/v1/books/:id/best-of-n",
            get(books_state_routes::get_best_of_n)
                .put(books_state_routes::put_best_of_n)
                .with_state(books.clone()),
        )
        .route(
            "/api/v1/books/:id/series-id",
            get(books_state_routes::get_series_id)
                .put(books_state_routes::put_series_id)
                .with_state(books.clone()),
        )
        .route(
            "/api/v1/books/:id/codex",
            get(books_state_routes::get_codex).put(books_state_routes::put_codex).with_state(books.clone()),
        )
        // R21/391 号：Context Lens——上下文装配透明回放（纯读）。
        .route(
            "/api/v1/books/:id/context-lens",
            get(books_state_routes::get_context_lens_chapters).with_state(books.clone()),
        )
        .route(
            "/api/v1/books/:id/context-lens/:chapter",
            get(books_state_routes::get_context_lens).with_state(books.clone()),
        )
        // 189 号：时间线节拍自动沉淀开关（书籍级，默认关）。
        .route(
            "/api/v1/books/:id/timeline-auto-beats",
            get(books_state_routes::get_timeline_auto_beats)
                .put(books_state_routes::put_timeline_auto_beats)
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
            post(translation_routes::export_translation).with_state(books.clone()),
        )
        // 71 号：play 域（互动世界游玩面）。
        .route(
            "/api/v1/play/runs/:worldId/:runId",
            get(play_routes::get_play_run).with_state(books.clone()),
        )
        .route(
            "/api/v1/play/runs/:worldId/:runId/image-settings",
            put(play_routes::put_play_image_settings).with_state(books.clone()),
        )
        .route(
            "/api/v1/play/runs/:worldId/:runId/generate-image",
            post(play_routes::post_play_generate_image).with_state(books.clone()),
        )
        .route(
            "/api/v1/play/runs/:worldId/:runId/images/:file",
            get(play_routes::get_play_image).with_state(books.clone()),
        )
        // 72 号：运维面（daemon / logs / doctor / radar）+ 架构稿修订。
        .route(
            "/api/v1/daemon",
            get(ops_routes::get_daemon).with_state(books.clone()),
        )
        .route(
            "/api/v1/daemon/start",
            post(ops_routes::post_daemon_start).with_state(books.clone()),
        )
        .route(
            "/api/v1/daemon/stop",
            post(ops_routes::post_daemon_stop).with_state(books.clone()),
        )
        .route(
            "/api/v1/logs",
            get(ops_routes::get_logs).with_state(books.clone()),
        )
        .route(
            "/api/v1/doctor",
            get(ops_routes::get_doctor).with_state(books.clone()),
        )
        .route(
            "/api/v1/radar/scan",
            post(ops_routes::post_radar_scan).with_state(books.clone()),
        )
        // G14a/335 号：选后再析——免费扫榜 + 勾选范围分析。
        .route(
            "/api/v1/radar/rankings",
            post(ops_routes::post_radar_rankings).with_state(books.clone()),
        )
        .route(
            "/api/v1/radar/analyze",
            post(ops_routes::post_radar_analyze).with_state(books.clone()),
        )
        .route(
            "/api/v1/radar/history",
            get(ops_routes::get_radar_history).with_state(books.clone()),
        )
        // R26/403 号：调用级运行遥测（进程内环形缓冲纯读投影）。
        .route("/api/v1/run-log", get(ops_routes::get_run_log))
        // R13/411 号：写作数据行面（前端 aggregateWritingStats 聚合）。
        .route(
            "/api/v1/writing-stats-rows",
            get(ops_routes::get_writing_stats_rows).with_state(books.clone()),
        )
        .route(
            "/api/v1/books/:id/quality-debts",
            get(ops_routes::get_quality_debts).with_state(books.clone()),
        )
        .route(
            "/api/v1/books/:id/tension-curve",
            get(ops_routes::get_tension_curve).with_state(books.clone()),
        )
        .route(
            "/api/v1/books/:id/quality-trend",
            get(ops_routes::get_quality_trend).with_state(books.clone()),
        )
        // R4/363 号：三库资产（种子兜底 + CRUD + 便携包导入导出）。
        .route(
            "/api/v1/asset-library/:kind",
            get(ops_routes::get_asset_library).with_state(books.clone()),
        )
        .route(
            "/api/v1/asset-library/:kind/assets",
            put(ops_routes::put_asset_library_asset).with_state(books.clone()),
        )
        .route(
            "/api/v1/asset-library/:kind/assets/:id",
            delete(ops_routes::delete_asset_library_asset).with_state(books.clone()),
        )
        .route(
            "/api/v1/asset-library/:kind/export",
            get(ops_routes::export_asset_library).with_state(books.clone()),
        )
        .route(
            "/api/v1/asset-library/:kind/import",
            post(ops_routes::import_asset_library).with_state(books.clone()),
        )
        .route(
            "/api/v1/asset-library/:kind/import-preview",
            post(ops_routes::preview_asset_library_import).with_state(books.clone()),
        )
        .route(
            "/api/v1/books/:id/adopt-library-assets",
            post(ops_routes::adopt_library_assets).with_state(books.clone()),
        )
        // R20/390 号：系列正典共享（.inkos/series/{seriesId}.json）。
        .route(
            "/api/v1/series/:seriesId/canon",
            get(ops_routes::get_series_canon)
                .put(ops_routes::put_series_canon)
                .with_state(books.clone()),
        )
        // R5/366 号：书级反AI规则 + G13 经验条目。
        .route(
            "/api/v1/books/:id/anti-ai-rules",
            get(ops_routes::get_anti_ai_rules).with_state(books.clone()),
        )
        .route(
            "/api/v1/books/:id/anti-ai-rules",
            put(ops_routes::put_anti_ai_rules).with_state(books.clone()),
        )
        .route(
            "/api/v1/books/:id/experience",
            get(ops_routes::get_experience_entries).with_state(books.clone()),
        )
        .route(
            "/api/v1/books/:id/experience",
            put(ops_routes::put_experience_entries).with_state(books.clone()),
        )
        .route(
            "/api/v1/books/:id/experience/:entryId",
            delete(ops_routes::delete_experience_entry).with_state(books.clone()),
        )
        .route(
            "/api/v1/books/:id/promises",
            get(ops_routes::get_promises).with_state(books.clone()),
        )
        // G1/412 号补注册：混合召回（召回测试面板消费；函数已在 349 号实现但路由漏挂）。
        .route(
            "/api/v1/books/:id/hybrid-search",
            post(ops_routes::post_hybrid_search).with_state(books.clone()),
        )
        // G5/351 号：统一拆书面。
        .route(
            "/api/v1/books/:id/deconstruct",
            post(ops_routes::post_deconstruct).with_state(books.clone()),
        )
        // G7b/343 号：名册候选确认卡 + 三选写回。
        .route(
            "/api/v1/books/:id/roster-candidates",
            get(ops_routes::get_roster_candidates).with_state(books.clone()),
        )
        .route(
            "/api/v1/books/:id/roster/confirm",
            post(ops_routes::post_roster_confirm).with_state(books.clone()),
        )
        // G16/346 号：项目级任务路由读写。
        .route(
            "/api/v1/task-routing",
            get(ops_routes::get_task_routing).with_state(books.clone()),
        )
        .route(
            "/api/v1/task-routing",
            put(ops_routes::put_task_routing).with_state(books.clone()),
        )
        // G6/354 号：方向候选批量生成（灵感卡 → LLM）。
        .route(
            "/api/v1/director/directions",
            post(ops_routes::post_direction_candidates).with_state(books.clone()),
        )
        // G6/353 号：导演会话读写（含 G9 续跑建议）。
        .route(
            "/api/v1/books/:id/director",
            get(ops_routes::get_director).with_state(books.clone()),
        )
        .route(
            "/api/v1/books/:id/director",
            put(ops_routes::put_director).with_state(books.clone()),
        )
        // G4/340 号：写法档案池 + 书级绑定。
        .route(
            "/api/v1/style-profiles",
            get(ops_routes::list_style_profiles).with_state(books.clone()),
        )
        .route(
            "/api/v1/style-profiles",
            post(ops_routes::save_style_profile).with_state(books.clone()),
        )
        .route(
            "/api/v1/books/:id/style-binding",
            get(ops_routes::get_style_binding).with_state(books.clone()),
        )
        .route(
            "/api/v1/books/:id/style-binding",
            put(ops_routes::save_style_binding).with_state(books.clone()),
        )
        .route(
            "/api/v1/books/:id/foundation/revise",
            post(book_create_routes::revise_foundation).with_state(books),
        )
        // 路径段守卫（201 号）：books/projects 的 id 段 percent-decode 后统一过
        // is_safe_book_id，拦 `..%2F` 注入穿越——对齐 Node 侧
        // `/api/v1/books/:id*` 中间件，补齐默认引擎的文件面纵深。
        .layer(axum::middleware::from_fn(segment_guard::guard))
        // 445 号：API 响应统一 no-store——读取/遥测面（run-log 等）响应缺
        // 缓存头时浏览器启发式缓存会把跨会话陈旧响应当新鲜用
        // （RunLogPanel 间歇消失实测机制）。
        .layer(axum::middleware::from_fn(api_no_store))
}

/// API 响应统一 `Cache-Control: no-store`（445 号）。
async fn api_no_store(
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let mut response = next.run(req).await;
    response.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    response
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

    /// R38b：debug/tools 端点活体投影——34 件全表、声明序、哈希稳定。
    #[tokio::test]
    async fn debug_tools_projects_full_registry() {
        let resp = app()
            .oneshot(Request::builder().uri("/api/v1/debug/tools").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_string(resp.into_body()).await;
        let entries: Vec<serde_json::Value> = serde_json::from_str(&body).unwrap();
        assert_eq!(entries.len(), 34, "全表投影（与 registry 总数断言同源）");
        assert_eq!(entries[0]["name"], "set_world_anchor", "声明序 = 分发链序首族首件");
        assert!(entries[0]["parameters_sha256"].as_str().unwrap().len() == 64, "sha256 hex");
        let names: Vec<&str> = entries.iter().filter_map(|e| e["name"].as_str()).collect();
        assert!(names.contains(&"read") && names.contains(&"ingest_material"));
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
        assert!(body.contains(r#""backend":"rust-engine""#), "170 号：应答应含后端标识");
    }

    /// 445 号：API 响应统一 no-store——防浏览器启发式缓存跨会话陈旧遥测
    /// （RunLogPanel 间歇消失实测机制）。
    #[tokio::test]
    async fn api_responses_carry_no_store() {
        let resp = app()
            .oneshot(Request::builder().uri("/api/v1/health").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(
            resp.headers().get(axum::http::header::CACHE_CONTROL).map(|v| v.to_str().unwrap()),
            Some("no-store")
        );
    }

    #[tokio::test]
    async fn cors_layer_reflects_loopback_origins_only() {
        use axum::http::HeaderValue;
        let app = router(AppState { version: "0.0.1-test".into() })
            .layer(sidecar_cors_layer());

        // 回环 Origin（端口不限）：ACAO 反射请求 Origin（173 号）。
        for origin in ["http://localhost:5173", "http://127.0.0.1:7788", "tauri://localhost"] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri("/api/v1/health")
                        .header("origin", origin)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status().as_u16(), 200, "{origin}");
            assert_eq!(
                response.headers().get("access-control-allow-origin"),
                Some(&HeaderValue::from_str(origin).unwrap()),
                "{origin} 应反射"
            );
        }

        // 远端 Origin：不发 ACAO（守卫缺位时本层兜底不放大；bin 装配下守卫
        // 会先行 403——本用例锁定纯 CORS 层语义）。
        for origin in ["http://duel.local", "null"] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri("/api/v1/health")
                        .header("origin", origin)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status().as_u16(), 200, "{origin}");
            assert!(
                response.headers().get("access-control-allow-origin").is_none(),
                "{origin} 不应发 ACAO"
            );
        }

        // 无 Origin 的普通请求：不发 ACAO（原 125 号 `*` 恒设语义废止）；
        // preflight 专属头不出现。
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(response.headers().get("access-control-allow-origin").is_none());
        assert!(response.headers().get("access-control-allow-methods").is_none());
        assert!(response.headers().get("access-control-allow-headers").is_none());

        // Preflight（回环 Origin）：短路 2xx，ACAO 反射，方法族含 POST，
        // 头镜像请求的 content-type。
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("OPTIONS")
                    .uri("/api/v1/health")
                    .header("origin", "http://localhost:5173")
                    .header("access-control-request-method", "POST")
                    .header("access-control-request-headers", "content-type")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(response.status().is_success());
        assert_eq!(
            response.headers().get("access-control-allow-origin"),
            Some(&HeaderValue::from_static("http://localhost:5173"))
        );
        let methods = response
            .headers()
            .get("access-control-allow-methods")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_ascii_lowercase();
        assert!(methods.contains("post"), "方法族应含 POST: {methods}");
        assert_eq!(
            response.headers().get("access-control-allow-headers").cloned(),
            Some(HeaderValue::from_str("content-type").unwrap())
        );

        // Preflight（远端 Origin）：tower-http 仍 200 但不发 ACAO——浏览器
        // 以无 ACAO 拒绝（与 Hono 204 无 ACAO 同效）。
        let response = app
            .oneshot(
                Request::builder()
                    .method("OPTIONS")
                    .uri("/api/v1/health")
                    .header("origin", "http://duel.local")
                    .header("access-control-request-method", "POST")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(response.status().is_success());
        assert!(response.headers().get("access-control-allow-origin").is_none());
    }

    #[tokio::test]
    async fn cors_layer_extra_origins_reflected() {
        use axum::http::HeaderValue;
        let app = router(AppState { version: "0.0.1-test".into() })
            .layer(sidecar_cors_layer_with_extras(vec!["https://embed.example".into()]));

        // env 白名单（非回环）命中：同样反射（与守卫白名单同源共享）。
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/health")
                    .header("origin", "https://embed.example")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), 200);
        assert_eq!(
            response.headers().get("access-control-allow-origin"),
            Some(&HeaderValue::from_static("https://embed.example"))
        );

        // 白名单前缀相近但不精确命中：不反射。
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/health")
                    .header("origin", "https://embed.example.evil")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(response.headers().get("access-control-allow-origin").is_none());
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
