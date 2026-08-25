//! 静态前端面（124 号）——TS sidecar `startStudioServer` 静态服务的 Rust 对应物。
//!
//! TS 语义（server.ts L6820-6857）逐字对齐：
//! - `GET /assets/*`：`staticDir + 请求路径` 读文件；扩展名映射 content-type
//!   （js/css/svg/png/ico/json，其余 `application/octet-stream`）；读失败 404。
//! - SPA 回退：`GET *` 非 `/api/v1/` 路径 → index.html（**启动时读一次缓存**；
//!   index 缺失则不注册回退）；`/api/v1/*` 一律 404（不吞 API 未知路由）。
//!
//! 差异防护：路径分量含 `..` 直接 404（Rust 侧安全加固，TS 端 URL 解析已归一
//! 的等价面）；asset 路径 percent 解码后落盘。
//!
//! 装配：`with_static_face(router, Some(dir))` 合并到既有全量路由——bin 经
//! `INKOS_STATIC_DIR` 注入；未设/目录缺失为纯 API 服务（Tauri 壳内嵌前端场景）。

use std::path::PathBuf;

use axum::extract::State;
use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;

use crate::server::sse::BroadcastHub;
use crate::server::{AppState, write_next_route::WriteNextRuntime};

/// 静态面共享态：目录 + index.html 启动期缓存（TS 同款一次性读取）。
///
/// index 缓存为 [`Bytes`]（170 号 W-B1 修正）：SPA 回退每次请求都要回体，
/// `Vec<u8>` 逐请求整页 memcpy，`Bytes` 克隆为引用计数——读取时 `Vec → Bytes`
/// 转移所有权零拷贝，此后零分配。
struct StaticFace {
    dir: PathBuf,
    index: Option<axum::body::Bytes>,
}

/// 给全量路由挂静态前端面。
///
/// `static_dir` 为 None 时原样返回（纯 API 模式）。目录存在但无 index.html
/// 时仍注册 `/assets/*` 与回退（回退走 404 分支）——与 TS「assets 路由无条件
/// 注册、index 缺失不注册回退」的可观察差异仅为未知 GET 路径 404，行为等价。
pub fn with_static_face(router: Router, static_dir: Option<PathBuf>) -> Router {
    let Some(dir) = static_dir else {
        return router;
    };
    let index = std::fs::read(dir.join("index.html")).ok().map(axum::body::Bytes::from);
    let face = std::sync::Arc::new(StaticFace { dir, index });
    let static_router: Router = Router::new()
        .route("/assets/*path", get(serve_asset))
        .fallback(spa_fallback)
        .with_state(face);
    router.merge(static_router)
}

/// TS contentTypes 映射（server.ts L6832-6838）逐字。
fn asset_content_type(file_name: &str) -> &'static str {
    let extension = file_name.rsplit('.').next().unwrap_or_default();
    match extension {
        "js" => "application/javascript",
        "css" => "text/css",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "json" => "application/json",
        _ => "application/octet-stream",
    }
}

async fn serve_asset(
    State(face): State<std::sync::Arc<StaticFace>>,
    axum::extract::Path(path): axum::extract::Path<String>,
) -> Response {
    let decoded = percent_encoding::percent_decode_str(&path).decode_utf8_lossy();
    if decoded.split('/').any(|segment| segment == "..") {
        return (StatusCode::NOT_FOUND, "404 Not Found").into_response();
    }
    // TS joinPath(staticDir, "/assets/<path>")：wild 段落回 assets/ 子目录。
    let file = face.dir.join("assets").join(decoded.as_ref());
    match tokio::fs::read(&file).await {
        Ok(bytes) => {
            let name = file
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            (
                StatusCode::OK,
                [
                    (header::CONTENT_TYPE, asset_content_type(name)),
                    (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
                    (header::REFERRER_POLICY, "no-referrer"),
                    (header::X_FRAME_OPTIONS, "DENY"),
                ],
                bytes,
            )
                .into_response()
        }
        Err(_) => (StatusCode::NOT_FOUND, "404 Not Found").into_response(),
    }
}

async fn spa_fallback(
    State(face): State<std::sync::Arc<StaticFace>>,
    method: axum::http::Method,
    uri: Uri,
) -> Response {
    // TS app.get("*")：非 GET 不落入回退（Hono 404）。
    if method != axum::http::Method::GET && method != axum::http::Method::HEAD {
        return (StatusCode::NOT_FOUND, "404 Not Found").into_response();
    }
    // TS 回退内显式排除 API 前缀（server.ts L6853）。
    if uri.path().starts_with("/api/v1/") {
        return (StatusCode::NOT_FOUND, "404 Not Found").into_response();
    }
    match &face.index {
        Some(html) => (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, "text/html; charset=utf-8"),
                (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
                (header::REFERRER_POLICY, "no-referrer"),
                (header::X_FRAME_OPTIONS, "DENY"),
            ],
            // Bytes 克隆 = 引用计数递增（170 号 W-B1：替代 Vec 整页 memcpy）。
            html.clone(),
        )
            .into_response(),
        None => (StatusCode::NOT_FOUND, "404 Not Found").into_response(),
    }
}

/// 组装带静态面的完整路由（bin 复用；测试亦直接构造）。
pub fn router_books_with_static(
    state: AppState,
    hub: std::sync::Arc<BroadcastHub>,
    write_next: WriteNextRuntime,
    audit: crate::server::audit_route::AuditRuntime,
    books: crate::server::books_routes::BooksRuntime,
    static_dir: Option<PathBuf>,
) -> Router {
    with_static_face(crate::server::router_books(state, hub, write_next, audit, books), static_dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    fn base_router() -> Router {
        crate::server::router(AppState { version: "test".into() })
    }

    fn dist() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("assets")).unwrap();
        std::fs::write(dir.path().join("index.html"), "<!doctype html><title>t</title>").unwrap();
        std::fs::write(dir.path().join("assets").join("app.js"), b"console.log(1)").unwrap();
        std::fs::write(dir.path().join("assets").join("style.css"), b"body{}").unwrap();
        std::fs::write(dir.path().join("assets").join("logo.svg"), b"<svg/>").unwrap();
        std::fs::write(dir.path().join("assets").join("data.bin"), b"\x00\x01").unwrap();
        dir
    }

    async fn response_parts(
        app: &mut Router,
        method: &axum::http::Method,
        uri: &str,
    ) -> (u16, Option<String>, Vec<u8>) {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method.clone())
                    .uri(uri)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status().as_u16();
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(String::from);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec();
        (status, content_type, bytes)
    }

    #[tokio::test]
    async fn assets_content_type_map_matches_ts() {
        let dir = dist();
        let mut app = with_static_face(base_router(), Some(dir.path().to_path_buf()));
        for (name, expected) in [
            ("app.js", "application/javascript"),
            ("style.css", "text/css"),
            ("logo.svg", "image/svg+xml"),
            ("data.bin", "application/octet-stream"),
        ] {
            let (status, content_type, bytes) =
                response_parts(&mut app, &axum::http::Method::GET, &format!("/assets/{name}"))
                    .await;
            assert_eq!(status, 200, "{name}");
            assert_eq!(content_type.as_deref(), Some(expected), "{name}");
            assert!(!bytes.is_empty(), "{name}");
        }
    }

    #[tokio::test]
    async fn missing_asset_is_404() {
        let dir = dist();
        let mut app = with_static_face(base_router(), Some(dir.path().to_path_buf()));
        let (status, _, _) =
            response_parts(&mut app, &axum::http::Method::GET, "/assets/missing.js").await;
        assert_eq!(status, 404);
    }

    #[tokio::test]
    async fn traversal_is_rejected() {
        let dir = dist();
        let mut app = with_static_face(base_router(), Some(dir.path().to_path_buf()));
        let (status, _, _) = response_parts(
            &mut app,
            &axum::http::Method::GET,
            "/assets/..%2F..%2Fsecrets.json",
        )
        .await;
        assert_eq!(status, 404);
    }

    #[tokio::test]
    async fn spa_fallback_serves_index_for_non_api_get() {
        let dir = dist();
        let mut app = with_static_face(base_router(), Some(dir.path().to_path_buf()));
        for uri in ["/", "/editor/chapter/2"] {
            let (status, content_type, bytes) =
                response_parts(&mut app, &axum::http::Method::GET, uri).await;
            assert_eq!(status, 200, "{uri}");
            assert_eq!(content_type.as_deref(), Some("text/html; charset=utf-8"));
            assert_eq!(bytes, b"<!doctype html><title>t</title>");
        }
    }

    #[tokio::test]
    async fn api_paths_fall_through_to_404_not_index() {
        let dir = dist();
        let mut app = with_static_face(base_router(), Some(dir.path().to_path_buf()));
        let (status, _, bytes) =
            response_parts(&mut app, &axum::http::Method::GET, "/api/v1/nonexistent").await;
        assert_eq!(status, 404);
        assert_ne!(bytes, b"<!doctype html><title>t</title>");
        // 已注册 API 路由不受回退干扰。
        let (status, _, _) =
            response_parts(&mut app, &axum::http::Method::GET, "/api/v1/health").await;
        assert_eq!(status, 200);
    }

    #[tokio::test]
    async fn non_get_fallback_is_404() {
        let dir = dist();
        let mut app = with_static_face(base_router(), Some(dir.path().to_path_buf()));
        let (status, _, _) =
            response_parts(&mut app, &axum::http::Method::POST, "/some/route").await;
        assert_eq!(status, 404);
    }

    #[tokio::test]
    async fn none_dir_keeps_router_pure_api() {
        let mut app = with_static_face(base_router(), None);
        let (status, _, _) = response_parts(&mut app, &axum::http::Method::GET, "/").await;
        assert_eq!(status, 404);
    }

    #[tokio::test]
    async fn missing_index_still_serves_assets() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("assets")).unwrap();
        std::fs::write(dir.path().join("assets").join("app.js"), b"x").unwrap();
        let mut app = with_static_face(base_router(), Some(dir.path().to_path_buf()));
        let (status, _, _) =
            response_parts(&mut app, &axum::http::Method::GET, "/assets/app.js").await;
        assert_eq!(status, 200);
        let (status, _, _) = response_parts(&mut app, &axum::http::Method::GET, "/").await;
        assert_eq!(status, 404);
    }

    /// 170 号 W-A3：静态面（assets + SPA 回退）三安全头；API 面不加（契约不扰动）。
    #[tokio::test]
    async fn static_face_carries_security_headers() {
        let dir = dist();
        let mut app = with_static_face(base_router(), Some(dir.path().to_path_buf()));

        for uri in ["/assets/app.js", "/", "/editor/chapter/2"] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(uri)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status().as_u16(), 200, "{uri}");
            assert_eq!(
                response.headers().get(header::X_CONTENT_TYPE_OPTIONS).and_then(|v| v.to_str().ok()),
                Some("nosniff"),
                "{uri}"
            );
            assert_eq!(
                response.headers().get(header::REFERRER_POLICY).and_then(|v| v.to_str().ok()),
                Some("no-referrer"),
                "{uri}"
            );
            assert_eq!(
                response.headers().get(header::X_FRAME_OPTIONS).and_then(|v| v.to_str().ok()),
                Some("DENY"),
                "{uri}"
            );
        }

        // API 面对照：健康端点不带静态面安全头（保持既有契约响应面）。
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), 200);
        assert!(response.headers().get(header::X_FRAME_OPTIONS).is_none());
    }
}
