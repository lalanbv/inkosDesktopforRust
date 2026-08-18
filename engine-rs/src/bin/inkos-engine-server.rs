//! inkos-engine-server —— 独立 HTTP bin（strangler 切换面宿主）。
//!
//! 装配 `router_with_runtime`：utility 端点 + SSE（/api/v1/events）+
//! write-next（POST /api/v1/books/:id/write-next，九路 LLM 端口经
//! [`AgentRouter`] 真实接线）。
//!
//! 配置来源（环境变量，与 Node sidecar 的 env 约定对齐）：
//! - `INKOS_PROJECT_ROOT`：项目根（books/、inkos.json；默认 CWD）
//! - `INKOS_LLM_BASE_URL` / `INKOS_LLM_API_KEY` / `INKOS_LLM_MODEL`：默认端点
//! - `INKOS_LLM_MAX_TOKENS`：默认 max_tokens（默认 8192）
//! - `INKOS_PORT`：监听端口（默认 8787，与 Node sidecar 同位替换时由壳层指定）
//!
//! agent 覆盖表暂经 env 前缀扩展（`INKOS_AGENT_<NAME>_MODEL`），完整
//! model-overrides 配置面随 project 配置端点接线（备案）。

use std::collections::HashMap;
use std::sync::Arc;

use inkos_engine::llm::agent_router::{AgentOverride, AgentRouter, LlmEndpointConfig, RoutedAgent, RoutedSettler};
use inkos_engine::pipeline::write_next::{write_next_chapter, WriteNextAgents, WriteNextConfig, WriteNextCtx};
use inkos_engine::server::sse::BroadcastHub;
use inkos_engine::server::{AppState, WriteNextRuntime};
use inkos_engine::state::manager::StateManager;
use inkos_engine::state::store::FsStateStore;

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

    // runner：九路端口聚合构造 + write-next 执行（生命周期收敛在 BoxFuture 内）。
    let runner_state = state.clone();
    let runner_project = project_root.clone();
    let runner_router = router.clone();
    let runner: inkos_engine::server::WriteNextRunner = Arc::new(
        move |state, book_id, word_count, temperature| {
            let project = runner_project.clone();
            let llm = runner_router.clone();
            let _ = &runner_state;
            Box::pin(async move {
                // 九路端口：settler 持独立 ctx（static 生命周期经泄漏一次的
                // store/project 根——进程生命周期单例）。
                static STORE: std::sync::OnceLock<FsStateStore> = std::sync::OnceLock::new();
                let prompt_store = STORE.get_or_init(|| FsStateStore);
                let state_store = prompt_store;
                let project_ref: &'static std::path::Path =
                    Box::leak(project.clone().into_boxed_path());
                let builtin: &'static std::path::Path = Box::leak(
                    std::path::PathBuf::from(std::env::var("INKOS_BUILTIN_GENRES_DIR")
                        .unwrap_or_else(|_| "assets/genres".to_string()))
                    .into_boxed_path(),
                );

                let writer = Box::leak(Box::new(RoutedAgent { router: llm.clone(), agent: "writer" }));
                let planner = Box::leak(Box::new(RoutedAgent { router: llm.clone(), agent: "planner" }));
                let composer = Box::leak(Box::new(RoutedAgent { router: llm.clone(), agent: "composer" }));
                let reviser = Box::leak(Box::new(RoutedAgent { router: llm.clone(), agent: "reviser" }));
                let auditor = Box::leak(Box::new(RoutedAgent { router: llm.clone(), agent: "auditor" }));
                let normalizer =
                    Box::leak(Box::new(RoutedAgent { router: llm.clone(), agent: "length-normalizer" }));
                let analyzer =
                    Box::leak(Box::new(RoutedAgent { router: llm.clone(), agent: "chapter-analyzer" }));
                let state_validator =
                    Box::leak(Box::new(RoutedAgent { router: llm.clone(), agent: "state-validator" }));
                let settler = Box::leak(Box::new(RoutedSettler {
                    router: llm.clone(),
                    ctx: inkos_engine::agents::writer::WriterCtx {
                        project_root: project_ref,
                        builtin_genres_dir: builtin,
                        prompt_store,
                        state_store,
                    },
                    chapter_number: 0,
                }));

                let full_auditor =
                    inkos_engine::llm::agent_router::FullCycleAuditor {
                        router: llm.clone(),
                        project_root: project_ref.to_path_buf(),
                        builtin_genres_dir: builtin.to_path_buf(),
                        book_dir: project_ref.join("books").join(&book_id),
                        chapter_number: 0, // write-next 内部按需重建（见下）
                        genre: String::new(),
                    };
                let agents = WriteNextAgents {
                    writer,
                    planner,
                    composer,
                    reviser,
                    auditor,
                    full_auditor: Some(full_auditor),
                    normalizer,
                    analyzer,
                    state_validator,
                    settler,
                };
                let ctx = WriteNextCtx {
                    project_root: project_ref,
                    builtin_genres_dir: builtin,
                    prompt_store,
                    state_store,
                    context_budget: None,
                    notify: None,
                };
                write_next_chapter(
                    &state,
                    &agents,
                    &ctx,
                    &WriteNextConfig::from_project(project_ref).await,
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
        builtin_genres_dir: std::path::PathBuf::from(env(
            "INKOS_BUILTIN_GENRES_DIR",
            "assets/genres",
        )),
    };
    let books = inkos_engine::server::books_routes::BooksRuntime {
        hub: hub.clone(),
        state,
        router: std::sync::Arc::new(router.clone()),
        builtin_genres_dir: audit.builtin_genres_dir.clone(),
        revision_gate: inkos_engine::pipeline::merged_audit::RevisionGate::parse(
            std::env::var("INKOS_REVISION_GATE").ok().as_deref(),
        ),
    };
    inkos_engine::server::router_books(
        AppState { version: env("CARGO_PKG_VERSION", "0.0.1") },
        hub,
        runtime,
        audit,
        books,
    )
}

#[tokio::main]
async fn main() -> Result<(), std::io::Error> {
    let port: u16 = env("INKOS_PORT", "8787").parse().unwrap_or(8787);
    let app = build_router();
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
    eprintln!("inkos-engine-server listening on 127.0.0.1:{port}");
    axum::serve(listener, app).await
}

