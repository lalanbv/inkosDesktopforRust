//! 1669 号：模型调用面的 LLM 配置前置守卫。
//!
//! 「模型调用**之前**就能检出的配置问题」是客户端可纠正的配置错误，不是内部
//! 错误：双端统一回 `400 { error: { code: "LLM_CONFIG_ERROR", message } }`。
//! 此前 Rust 全仓没有这一错误类——项目配置不可用时 `effective_router` 静默
//! 回退启动路由器，未配置只在实际请求时以连接拒绝的传输错误暴露（500）。
//!
//! 判定 = 项目配置不可用 **且** 启动路由器是未配置占位（bin 的 env 缺省，
//! 见 `LlmEndpointConfig::is_unconfigured_placeholder`）。env 配置的引擎
//! （开发/冒烟脚本经 `INKOS_LLM_*` 起引擎）在项目配置缺失时照常回退，不拦。

use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::MethodRouter;
use axum::Json;

use crate::llm::agent_router::AgentRouter;
use crate::server::books_routes::{resolve_project_llm, BooksRuntime, ProjectLlmState};

pub const LLM_CONFIG_ERROR_CODE: &str = "LLM_CONFIG_ERROR";

/// UI 文案表（`packages/core/src/llm/error-copy.ts`）的逐字锚文本——与 TS
/// `LLMConfigurationError("MISSING_API_KEY")` 同句，改写即失去本地化。
pub const LLM_CONFIG_ERROR_MISSING_KEY: &str =
    "Studio LLM API key not set. Open Studio services and save an API key for the selected service.";
/// 无配置文件 / 配置里没有 llm 对象。
pub const LLM_CONFIG_ERROR_NO_SERVICE: &str =
    "Studio LLM service not configured. Open Studio services and select a model.";

/// 守卫状态：项目根 + 启动路由器（两者共同决定「是否真的未配置」）。
#[derive(Clone)]
pub struct LlmConfigGuard {
    root: std::path::PathBuf,
    startup_router: Arc<AgentRouter>,
}

impl LlmConfigGuard {
    pub fn new(root: &std::path::Path, startup_router: Arc<AgentRouter>) -> Self {
        Self {
            root: root.to_path_buf(),
            startup_router,
        }
    }

    /// `Ok(())` = 有可用端点（项目配置，或 env 配置的启动路由器）；
    /// `Err(message)` = 两头都没有。
    pub async fn check(&self) -> Result<(), String> {
        let state = resolve_project_llm(&self.root).await;
        if matches!(state, ProjectLlmState::Usable(_)) {
            return Ok(());
        }
        if !self
            .startup_router
            .default_endpoint()
            .is_unconfigured_placeholder()
        {
            return Ok(());
        }
        Err(match state {
            ProjectLlmState::Missing => LLM_CONFIG_ERROR_NO_SERVICE,
            _ => LLM_CONFIG_ERROR_MISSING_KEY,
        }
        .to_string())
    }
}

/// 400 信封（与 TS 全局 `onError` 的 `LLM_CONFIG_ERROR` 映射同形）。返回
/// `(StatusCode, Json)` 元组——`-> impl IntoResponse` 的处理器各分支须同型。
pub fn llm_config_error(message: &str) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({
            "error": { "code": LLM_CONFIG_ERROR_CODE, "message": message }
        })),
    )
}

/// axum 中间件形态：不通过 → 400，通过 → 放行到处理器。
pub async fn llm_config_guard(
    State(guard): State<LlmConfigGuard>,
    request: Request,
    next: Next,
) -> Response {
    match guard.check().await {
        Ok(()) => next.run(request).await,
        Err(message) => llm_config_error(&message).into_response(),
    }
}

/// 给方法路由挂守卫（`route_layer` 只包命中的方法处理器，未命中方法的
/// 405 不经过守卫）。
pub fn llm_guarded<S>(router: MethodRouter<S>, guard: LlmConfigGuard) -> MethodRouter<S>
where
    S: Clone + Send + Sync + 'static,
{
    router.route_layer(axum::middleware::from_fn_with_state(
        guard,
        llm_config_guard,
    ))
}

/// `post(handler)` + 配置守卫 + 绑定 `BooksRuntime` 状态的简写（1670 号：组合层
/// 批量接线）。守卫在处理器自身的一切校验之前——未配置的项目上，坏载荷也
/// 先报配置问题（两者都是 400）。
pub fn guarded_post<H, T>(handler: H, runtime: &BooksRuntime) -> MethodRouter
where
    H: axum::handler::Handler<T, BooksRuntime> + Clone + Send + Sync + 'static,
    T: 'static,
{
    llm_guarded(
        axum::routing::post(handler),
        LlmConfigGuard::new(runtime.state.project_root(), runtime.router.clone()),
    )
    .with_state(runtime.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use crate::server::books_routes::BooksRuntime;
    use crate::server::sse::BroadcastHub;
    use crate::state::manager::StateManager;
    use std::collections::HashMap;

    /// 测试键：env 注入 + 非凭据回退（源码不落可用凭据字面量）。
    fn test_key() -> String {
        std::env::var("INKOS_TEST_LLM_KEY").unwrap_or_else(|_| "test-key-placeholder".to_string())
    }

    fn router(base_url: &str, api_key: &str) -> Arc<AgentRouter> {
        Arc::new(AgentRouter::new(
            LlmEndpointConfig {
                base_url: base_url.into(),
                api_key: api_key.into(),
                model: "default".into(),
                max_tokens: 8192,
                extra_headers: HashMap::new(),
                context_window_tokens: 128_000,
            },
            HashMap::new(),
        ))
    }

    fn books(root: &std::path::Path, router: Arc<AgentRouter>) -> BooksRuntime {
        BooksRuntime {
            hub: Arc::new(BroadcastHub::new()),
            state: Arc::new(StateManager::new(root.to_path_buf())),
            router,
            builtin_genres_dir: root.join("assets").join("genres"),
            revision_gate: Default::default(),
        }
    }

    fn write_config(root: &std::path::Path, base_url: &str) {
        std::fs::write(
            root.join("inkos.json"),
            format!(
                r#"{{"version":"0.1.0","name":"守卫单测","language":"zh","llm":{{"services":{{"custom:Guard":{{"service":"custom","name":"Guard","baseUrl":"{base_url}","apiFormat":"chat"}}}},"defaultModel":"guard-model"}},"notify":[]}}"#
            ),
        )
        .unwrap();
    }

    fn write_secrets(root: &std::path::Path) {
        std::fs::create_dir_all(root.join(".inkos")).unwrap();
        let service_key = test_key();
        std::fs::write(
            root.join(".inkos").join("secrets.json"),
            serde_json::json!({ "services": { "custom:Guard": { "apiKey": service_key } } })
                .to_string(),
        )
        .unwrap();
    }

    /// 未配置占位 = bin 的 env 缺省（discard 端点 + 空 key）——带 key 或换端点
    /// 都不算（测试与开发惯用的「127.0.0.1:9 + k」路由器不得被误拦）。
    #[test]
    fn placeholder_is_exactly_the_bin_default() {
        let unconfigured = crate::llm::agent_router::LLM_UNCONFIGURED_BASE_URL;
        assert!(router(unconfigured, "")
            .default_endpoint()
            .is_unconfigured_placeholder());
        assert!(!router(unconfigured, "k")
            .default_endpoint()
            .is_unconfigured_placeholder());
        assert!(!router("http://127.0.0.1:9/v1", "")
            .default_endpoint()
            .is_unconfigured_placeholder());
        assert!(!router("http://127.0.0.1:9/v1", "k")
            .default_endpoint()
            .is_unconfigured_placeholder());
    }

    /// 产品路径（桌壳直启不设 env）：无项目配置 + 未配置引擎 → 两头都没有。
    #[tokio::test]
    async fn no_project_config_on_an_unconfigured_engine_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = books(
            dir.path(),
            router(crate::llm::agent_router::LLM_UNCONFIGURED_BASE_URL, ""),
        );
        assert_eq!(
            runtime.require_llm_config().await.unwrap_err(),
            LLM_CONFIG_ERROR_NO_SERVICE
        );
        assert!(runtime.startup_router_unconfigured());
    }

    /// 项目配置在但缺 key → 文案与 TS `MISSING_API_KEY` 逐字同句（UI 文案表锚）。
    #[tokio::test]
    async fn keyless_project_config_is_rejected_with_the_ts_anchor_text() {
        let dir = tempfile::tempdir().unwrap();
        write_config(dir.path(), "https://example.invalid/v1");
        let runtime = books(
            dir.path(),
            router(crate::llm::agent_router::LLM_UNCONFIGURED_BASE_URL, ""),
        );
        assert_eq!(
            runtime.require_llm_config().await.unwrap_err(),
            // 故意写全句而非引用常量：常量被改写时这里先红。
            "Studio LLM API key not set. Open Studio services and save an API key for the selected service."
        );
    }

    /// env 配置的引擎（开发/冒烟脚本）：项目配置缺失照常回退启动路由器，不拦。
    #[tokio::test]
    async fn env_configured_engine_is_not_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = books(dir.path(), router("http://127.0.0.1:9/v1", "env-key"));
        assert!(!runtime.startup_router_unconfigured());
        runtime.require_llm_config().await.unwrap();
    }

    /// 项目配置可用 → 放行。
    #[tokio::test]
    async fn usable_project_config_passes() {
        let dir = tempfile::tempdir().unwrap();
        write_config(dir.path(), "https://example.invalid/v1");
        write_secrets(dir.path());
        let runtime = books(
            dir.path(),
            router(crate::llm::agent_router::LLM_UNCONFIGURED_BASE_URL, ""),
        );
        runtime.require_llm_config().await.unwrap();
        // 热解析照旧落到项目路由器。
        assert!(!Arc::ptr_eq(
            &runtime.effective_router().await,
            &runtime.router
        ));
    }

    /// 拆分不得改动回退语义：项目配置不可用时 `effective_router` 仍是启动路由器。
    #[tokio::test]
    async fn effective_router_still_falls_back_to_the_startup_router() {
        let dir = tempfile::tempdir().unwrap();
        write_config(dir.path(), "https://example.invalid/v1");
        let runtime = books(dir.path(), router("http://127.0.0.1:9/v1", "env-key"));
        assert!(Arc::ptr_eq(
            &runtime.effective_router().await,
            &runtime.router
        ));
    }
}
