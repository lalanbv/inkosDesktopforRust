//! 本地回环守卫（170 号 W-A1）——引擎本地服务的信任边界中间件。
//!
//! 威胁模型（对标 Jupyter 4.3+ 默认 token 鉴权、VS Code 内置服务同款问题域）：
//! - T1 远端网页跨源读取 secrets / 调用写接口（CORS `Allow-Origin: *` 放大面）；
//! - T2 DNS rebinding（attacker.com 解析到 127.0.0.1，请求 Host 为攻击者域名）。
//! 非目标：本地恶意进程（其本可直读 `.inkos/secrets.json`，服务侧无增量手段）。
//!
//! 规则（deny-first，仅拦浏览器形态请求）：
//! 1. `Origin` 头存在：`scheme://host[:port]` 剥端口后的 host 必须是回环主机
//!    （localhost / 127.0.0.1 / [::1] / [::]，端口不限——Vite dev 4567 代理与
//!    `pick_free_port` 动态端口天然放行），或命中 env 追加白名单；否则 403。
//!    `tauri://localhost` 等 Tauri origin 因 host 为回环同获放行；`Origin: null`
//!    （file:// 沙箱页）无 `://` 结构，一律 403。
//! 2. `Host` 头存在：剥端口后必须是回环主机，否则 403（防 T2）。
//! 3. 两头皆缺（curl / 桌壳健康探测 / duel oneshot / supertest 形态）→ 放行。
//!    这是兼容性的关键：契约测试与非浏览器客户端零改动。
//!
//! 装配位置：仅由 `inkos-engine-server` bin 挂在最外层（包 CORS 之外），
//! `router_books` 本身不挂——duel/单测路由面零扰动。拦截响应不带 CORS 头
//! （守卫在 CORS 层之外先行短路）。
//!
//! env 开关：
//! - `INKOS_ENGINE_LOOPBACK_GUARD`：默认开；`0`/`false`/`off`（大小写不敏感）关。
//! - `INKOS_ENGINE_ALLOWED_ORIGINS`：逗号分隔的精确 Origin 白名单（追加项，
//!   供未来嵌入自定义 webview origin；不替换回环判定）。

use axum::extract::State;
use axum::http::{header, Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;

/// 守卫配置（Clone 供 `from_fn_with_state` 装配）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopbackGuardConfig {
    pub enabled: bool,
    /// 追加的精确 Origin 白名单（`INKOS_ENGINE_ALLOWED_ORIGINS`）。
    pub extra_origins: Vec<String>,
}

impl Default for LoopbackGuardConfig {
    fn default() -> Self {
        Self { enabled: true, extra_origins: Vec::new() }
    }
}

/// env 解析：`INKOS_ENGINE_LOOPBACK_GUARD`（默认 on）+
/// `INKOS_ENGINE_ALLOWED_ORIGINS`（逗号分隔，空白项剔除）。
impl LoopbackGuardConfig {
    pub fn from_env() -> Self {
        let enabled = std::env::var("INKOS_ENGINE_LOOPBACK_GUARD")
            .ok()
            .map(|v| !parse_disabled(&v))
            .unwrap_or(true);
        let extra_origins = std::env::var("INKOS_ENGINE_ALLOWED_ORIGINS")
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
        Self { enabled, extra_origins }
    }
}

/// `0`/`false`/`off`（大小写不敏感）视为关闭指令，其余任何值（含空串）视为开启。
fn parse_disabled(value: &str) -> bool {
    matches!(value.trim().to_ascii_lowercase().as_str(), "0" | "false" | "off")
}

/// Host:port 剥端口（IPv6 `[::1]:8787` 形态保留方括号整体）。
fn host_without_port(host: &str) -> &str {
    if host.starts_with('[') {
        match host.find(']') {
            Some(end) => &host[..=end],
            None => host,
        }
    } else {
        match host.rfind(':') {
            Some(idx) => &host[..idx],
            None => host,
        }
    }
}

/// 回环主机判定（含 IPv6 字面量两形态；`::1` 裸形态见于个别客户端 Host 头）。
fn is_loopback_host(host: &str) -> bool {
    matches!(
        host.to_ascii_lowercase().as_str(),
        "localhost" | "127.0.0.1" | "[::1]" | "[::]" | "::1"
    )
}

/// Origin 判定：白名单精确命中，或 `scheme://host[:port]` 的 host 为回环。
/// 无 `://` 结构（`null`、畸形值）→ 拒绝（不冒险）。
fn origin_is_allowed(origin: &str, extras: &[String]) -> bool {
    if extras.iter().any(|allowed| allowed == origin) {
        return true;
    }
    let Some(scheme_end) = origin.find("://") else {
        return false;
    };
    let rest = &origin[scheme_end + 3..];
    let host_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    is_loopback_host(host_without_port(&rest[..host_end]))
}

/// 守卫中间件本体（`from_fn_with_state` 装配）。
pub async fn guard(
    State(config): State<LoopbackGuardConfig>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Response {
    if !config.enabled {
        return next.run(req).await;
    }
    let headers = req.headers();
    if let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) {
        if !origin_is_allowed(origin, &config.extra_origins) {
            return forbidden("origin not allowed");
        }
    }
    if let Some(host) = headers.get(header::HOST).and_then(|v| v.to_str().ok()) {
        if !is_loopback_host(host_without_port(host)) {
            return forbidden("host not allowed");
        }
    }
    next.run(req).await
}

fn forbidden(message: &str) -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(serde_json::json!({ "error": message })),
    )
        .into_response()
}

/// 便捷装配：在既有路由（已含 CORS 层）之外再包守卫层——后加的 layer 在外，
/// 请求先过守卫再过 CORS，拦截响应不携带 CORS 头。
pub fn with_loopback_guard(router: axum::Router, config: LoopbackGuardConfig) -> axum::Router {
    router.layer(axum::middleware::from_fn_with_state(config, guard))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::routing::get;
    use axum::Router;
    use tower::ServiceExt;

    /// 测试形态：守卫（外）+ CORS（内）+ echo 路由——复刻 bin 装配顺序。
    fn app(config: LoopbackGuardConfig) -> Router {
        let inner = Router::new()
            .route("/api/v1/health", get(|| async { axum::Json(serde_json::json!({ "ok": true })) }))
            .layer(crate::server::sidecar_cors_layer());
        with_loopback_guard(inner, config)
    }

    async fn hit(
        app: &Router,
        decorate: impl FnOnce(axum::http::request::Builder) -> axum::http::request::Builder,
    ) -> (u16, Vec<(String, String)>) {
        let resp = app
            .clone()
            .oneshot(
                decorate(axum::http::Request::builder().uri("/api/v1/health"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = resp.status().as_u16();
        let headers = resp
            .headers()
            .iter()
            .map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap_or_default().to_string()))
            .collect();
        (status, headers)
    }

    #[test]
    fn parse_disabled_matrix() {
        assert!(parse_disabled("0"));
        assert!(parse_disabled("false"));
        assert!(parse_disabled("OFF"));
        assert!(!parse_disabled("1"));
        assert!(!parse_disabled("true"));
        assert!(!parse_disabled(""));
    }

    #[test]
    fn host_without_port_forms() {
        assert_eq!(host_without_port("localhost:8787"), "localhost");
        assert_eq!(host_without_port("127.0.0.1"), "127.0.0.1");
        assert_eq!(host_without_port("[::1]:8787"), "[::1]");
        assert_eq!(host_without_port("[::1]"), "[::1]");
        assert_eq!(host_without_port("example.com:80"), "example.com");
    }

    #[test]
    fn loopback_host_matrix() {
        for host in ["localhost", "LOCALHOST", "127.0.0.1", "[::1]", "[::]", "::1"] {
            assert!(is_loopback_host(host), "{host} 应为回环");
        }
        for host in ["example.com", "127.0.0.2", "0.0.0.0", "[2001:db8::1]", ""] {
            assert!(!is_loopback_host(host), "{host} 不应为回环");
        }
    }

    #[test]
    fn origin_matrix() {
        let empty: Vec<String> = Vec::new();
        for origin in [
            "http://localhost:4567",
            "http://127.0.0.1:7788",
            "https://localhost",
            "tauri://localhost",
            "http://[::1]:9000",
        ] {
            assert!(origin_is_allowed(origin, &empty), "{origin} 应放行");
        }
        for origin in [
            "http://evil.com",
            "https://attacker.example:443",
            "null",
            "",
            "http://127.0.0.2:8080",
        ] {
            assert!(!origin_is_allowed(origin, &empty), "{origin} 应拒绝");
        }
        // 白名单精确命中可放行非回环 Origin。
        let extras = vec!["https://custom.example".to_string()];
        assert!(origin_is_allowed("https://custom.example", &extras));
        assert!(!origin_is_allowed("https://custom.example.evil", &extras));
    }

    #[tokio::test]
    async fn remote_origin_gets_403_without_cors_headers() {
        let (status, headers) = hit(&app(LoopbackGuardConfig::default()), |b| {
            b.header("origin", "http://evil.com")
        })
        .await;
        assert_eq!(status, 403);
        assert!(
            !headers.iter().any(|(k, _)| k.starts_with("access-control-")),
            "拦截响应不应携带 CORS 头: {headers:?}"
        );
    }

    #[tokio::test]
    async fn loopback_origin_any_port_passes() {
        for origin in ["http://localhost:4567", "http://127.0.0.1:7788", "tauri://localhost"] {
            let (status, _) = hit(&app(LoopbackGuardConfig::default()), |b| {
                b.header("origin", origin)
            })
            .await;
            assert_eq!(status, 200, "Origin {origin} 应放行");
        }
    }

    #[tokio::test]
    async fn null_origin_rejected() {
        let (status, _) = hit(&app(LoopbackGuardConfig::default()), |b| b.header("origin", "null")).await;
        assert_eq!(status, 403);
    }

    #[tokio::test]
    async fn extra_origin_allowlist_passes() {
        let config = LoopbackGuardConfig {
            extra_origins: vec!["https://embed.example".to_string()],
            ..Default::default()
        };
        let (status, _) = hit(&app(config), |b| b.header("origin", "https://embed.example")).await;
        assert_eq!(status, 200);
    }

    #[tokio::test]
    async fn missing_headers_pass_through() {
        // duel oneshot / curl / 健康探测形态：无 Origin 无 Host。
        let (status, _) = hit(&app(LoopbackGuardConfig::default()), |b| b).await;
        assert_eq!(status, 200);
    }

    #[tokio::test]
    async fn dns_rebinding_host_rejected() {
        // Origin 缺席但 Host 为攻击者域名（DNS rebinding 形态）。
        let (status, _) = hit(&app(LoopbackGuardConfig::default()), |b| {
            b.header("host", "attacker.com:8787")
        })
        .await;
        assert_eq!(status, 403);
    }

    #[tokio::test]
    async fn loopback_hosts_pass() {
        for host in ["localhost:8787", "127.0.0.1:8787", "[::1]:8787"] {
            let (status, _) = hit(&app(LoopbackGuardConfig::default()), |b| b.header("host", host)).await;
            assert_eq!(status, 200, "Host {host} 应放行");
        }
    }

    #[tokio::test]
    async fn disabled_guard_passes_everything() {
        let config = LoopbackGuardConfig { enabled: false, ..Default::default() };
        let (status, _) = hit(&app(config), |b| {
            b.header("origin", "http://evil.com").header("host", "evil.com:1")
        })
        .await;
        assert_eq!(status, 200, "守卫关闭时应全放行");
    }

    #[tokio::test]
    async fn secrets_endpoint_blocked_for_remote_origin() {
        // 集成式复核：mini 路由挂真实 secrets handler 语义（此处以 echo 代替，
        // 守卫行为已由上方矩阵锁定——本用例锁定装配顺序：守卫先于路由执行）。
        let inner = Router::new().route(
            "/api/v1/services/:service/secret",
            get(|| async { axum::Json(serde_json::json!({ "apiKey": "sk-test" })) }),
        );
        let guarded = with_loopback_guard(inner, LoopbackGuardConfig::default());
        let evil = guarded
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri("/api/v1/services/openai/secret")
                    .header("origin", "http://evil.com")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(evil.status().as_u16(), 403, "远端 Origin 不得读取 secrets");
        let body = axum::body::to_bytes(evict_into_body(evil), usize::MAX).await.unwrap();
        assert!(!body.windows(7).any(|w| w == b"sk-test"), "403 体不得泄漏密钥");
    }

    fn evict_into_body(resp: axum::response::Response) -> Body {
        resp.into_body()
    }
}
