//! inkos-engine-server —— 独立 HTTP bin（strangler 切换面宿主）。
//!
//! 装配 `router_books` 全量路由：utility 端点 + SSE（/api/v1/events）+
//! write-next（POST /api/v1/books/:id/write-next，九路 LLM 端口经
//! [`AgentRouter`] 真实接线——**与其余写面同源走 `effective_router`
//! 热解析**：inkos.json 服务项 + secrets 优先，配置不可用才回退
//! `INKOS_LLM_*` 启动端点；bin 不持有独立 LLM 配置路径）。
//!
//! 配置来源（环境变量，与 Node sidecar 的 env 约定对齐）：
//! - `INKOS_PROJECT_ROOT`：项目根（books/、inkos.json；默认 CWD）
//! - `INKOS_LLM_BASE_URL` / `INKOS_LLM_API_KEY` / `INKOS_LLM_MODEL`：默认端点
//! - `INKOS_LLM_MAX_TOKENS`：默认 max_tokens（默认 8192）
//! - `INKOS_PORT`：监听端口（默认 8787，与 Node sidecar 同位替换时由壳层指定）
//! - `INKOS_STATIC_DIR`：静态前端面目录（`/assets/*` + SPA 回退；浏览器直连
//!   模式 = `packages/studio/dist`；未设为纯 API 服务——TS sidecar 无此开关、
//!   恒挂 dist，Rust 侧显式化供 Tauri 壳内嵌前端场景）
//!
//! agent 覆盖表暂经 env 前缀扩展（`INKOS_AGENT_<NAME>_MODEL`），完整
//! model-overrides 配置面随 project 配置端点接线（备案）。

use std::collections::HashMap;
use std::sync::Arc;

use inkos_engine::llm::agent_router::{AgentOverride, AgentRouter, LlmEndpointConfig};
use inkos_engine::pipeline::write_next::{write_next_chapter, WriteNextConfig};
use inkos_engine::server::sse::BroadcastHub;
use inkos_engine::server::{AppState, WriteNextRuntime};
use inkos_engine::state::manager::StateManager;

fn env(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

/// agent 名（覆盖表键）。
const AGENTS: &[&str] = &[
    "writer", "planner", "composer", "reviser", "auditor",
    "length-normalizer", "chapter-analyzer", "state-validator", "writer-settler",
];

fn build_router() -> axum::Router {
    let project_root = std::path::PathBuf::from(env("INKOS_PROJECT_ROOT", "."));
    let default = LlmEndpointConfig {
        base_url: env("INKOS_LLM_BASE_URL", "http://127.0.0.1:9"),
        api_key: env("INKOS_LLM_API_KEY", ""),
        model: env("INKOS_LLM_MODEL", "default"),
        max_tokens: env("INKOS_LLM_MAX_TOKENS", "8192").parse().unwrap_or(8192),
        extra_headers: HashMap::new(),
    };

    let mut overrides: HashMap<String, AgentOverride> = HashMap::new();
    for agent in AGENTS {
        let model = std::env::var(format!("INKOS_AGENT_{}_MODEL", agent.to_uppercase().replace('-', "_")))
            .ok();
        let base_url =
            std::env::var(format!("INKOS_AGENT_{}_BASE_URL", agent.to_uppercase().replace('-', "_")))
                .ok();
        let api_key =
            std::env::var(format!("INKOS_AGENT_{}_API_KEY", agent.to_uppercase().replace('-', "_")))
                .ok();
        let max_tokens = std::env::var(format!(
            "INKOS_AGENT_{}_MAX_TOKENS",
            agent.to_uppercase().replace('-', "_")
        ))
        .ok()
        .and_then(|v| v.parse().ok());
        if model.is_some() || base_url.is_some() || api_key.is_some() || max_tokens.is_some() {
            overrides.insert(
                (*agent).to_string(),
                AgentOverride { model, base_url, api_key, max_tokens },
            );
        }
    }

    // INKOS_LLM_API_FORMAT（106 号）/ INKOS_LLM_STREAM（108 号；TS parseBoolean：
    // "true"/"1"/"yes" → 流式，其余已设值 → 非流式，未设 → 缺省流式）。
    let env_stream = std::env::var("INKOS_LLM_STREAM").ok().map(|value| {
        value == "true" || value == "1" || value == "yes"
    });
    let router = AgentRouter::new(default, overrides)
        .with_api_format(
            match std::env::var("INKOS_LLM_API_FORMAT").as_deref() {
                Ok("responses") => inkos_engine::llm::providers::TransportApiFormat::Responses,
                _ => inkos_engine::llm::providers::TransportApiFormat::Chat,
            },
        )
        .with_stream(env_stream);
    let state = Arc::new(StateManager::new(project_root.clone()));
    let hub = Arc::new(BroadcastHub::new());

    // BooksRuntime 先建：write-next runner 复用其 effective_router 热解析
    // （inkos.json 服务项 + secrets 优先，配置不可用回退上面的启动 env router
    // ——与其余写面端点同源，bin 不再持有独立配置路径）。
    let builtin_genres_dir =
        std::path::PathBuf::from(env("INKOS_BUILTIN_GENRES_DIR", "assets/genres"));
    let books = inkos_engine::server::books_routes::BooksRuntime {
        hub: hub.clone(),
        state: state.clone(),
        router: std::sync::Arc::new(router.clone()),
        builtin_genres_dir: builtin_genres_dir.clone(),
        revision_gate: inkos_engine::pipeline::merged_audit::RevisionGate::parse(
            std::env::var("INKOS_REVISION_GATE").ok().as_deref(),
        ),
    };

    // runner：九路端口经共享装配（build_write_next_agents/ctx）+ 44 号全周期
    // 审计器挂有效 router（write_next_chapter 内 for_chapter 按章重绑）。
    let runner_books = books.clone();
    let runner: inkos_engine::server::WriteNextRunner = Arc::new(
        move |state, book_id, word_count, temperature| {
            let books = runner_books.clone();
            Box::pin(async move {
                let mut agents =
                    inkos_engine::server::books_routes::build_write_next_agents(&books).await;
                agents.full_auditor =
                    Some(inkos_engine::llm::agent_router::FullCycleAuditor {
                        router: (*books.effective_router().await).clone(),
                        project_root: books.state.project_root().to_path_buf(),
                        builtin_genres_dir: books.builtin_genres_dir.clone(),
                        book_dir: books.state.project_root().join("books").join(&book_id),
                        chapter_number: 0, // for_chapter 按章重绑
                        genre: String::new(),
                    });
                let ctx = inkos_engine::server::books_routes::build_write_next_ctx(&books);
                write_next_chapter(
                    &state,
                    &agents,
                    &ctx,
                    &WriteNextConfig::from_project(books.state.project_root()).await,
                    &book_id,
                    word_count,
                    temperature,
                    None,
                )
                .await
                .map_err(|e| e.to_string())
            })
        },
    );
    let runtime = WriteNextRuntime {
        hub: hub.clone(),
        state: state.clone(),
        runner,
        project_root: project_root.clone(),
    };
    let audit = inkos_engine::server::audit_route::AuditRuntime {
        hub: hub.clone(),
        state: state.clone(),
        router: std::sync::Arc::new(router.clone()),
        builtin_genres_dir,
    };
    // 静态前端面（124 号）：INKOS_STATIC_DIR 指向前端产物目录（浏览器直连
    // 模式 = packages/studio/dist）；未设为纯 API 服务（Tauri 壳内嵌前端）。
    let static_dir = std::env::var("INKOS_STATIC_DIR").ok().map(std::path::PathBuf::from);
    let app = inkos_engine::server::static_routes::router_books_with_static(
        AppState { version: env("CARGO_PKG_VERSION", "0.0.1") },
        hub,
        runtime,
        audit,
        books,
        static_dir,
    );
    // CORS（125 号）：Hono `app.use("/*", cors())` 等价——跨源前端
    // （Tauri 壳 tauri:// / vite dev server）经 API base 直连的场景。
    // 在最终组合路由（含静态面）之上一次性施加。
    app.layer(inkos_engine::server::sidecar_cors_layer())
}

#[tokio::main]
async fn main() -> Result<(), std::io::Error> {
    let port: u16 = env("INKOS_PORT", "8787").parse().unwrap_or(8787);
    let app = build_router();
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
    eprintln!("inkos-engine-server listening on 127.0.0.1:{port}");
    axum::serve(listener, app).await
}

