//! 路径段守卫（201 号）——books/projects 路由的 id 段统一校验中间件。
//!
//! 威胁模型：axum `Path<String>` 提取时对路径段做 percent-decode，`..%2F..`
//! 注入 `:id` 后经 `root.join("books").join(book_id)` 即越出项目根（读/写
//! 项目根之外的任意文件）。回环守卫（170 号）管网络面（远端 Origin / DNS
//! rebinding），本层补文件面纵深：守卫关闭（env）或本机进程直连时，
//! 路径穿越也被 id 段校验拦在 handler 之外。
//!
//! 双端对齐：Node studio 侧早有同语义中间件（`app.use("/api/v1/books/:id*")`
//! 过 `isSafeBookId` 后 400 INVALID_BOOK_ID）；本层为 Rust 默认引擎补齐同一
//! 水位，并顺带覆盖 `/api/v1/projects/{id}`（interactive-films / translation
//! 等 projects 前缀路由同样以 id 拼路径）。handler 内既有 `is_safe_book_id`
//! 点状校验（books_state / translation / interactive_film）保留——中间件是
//! 全路由单点，点状校验在非 HTTP 入口（内部调用）继续生效，构成纵深。
//!
//! 规则：
//! - 提取 `/api/v1/{books|projects}/{seg}` 的原始（未解码）seg；
//! - percent-decode 失败（非法 UTF-8）→ 400；
//! - 解码后 [`crate::utils::is_safe_book_id`] 不通过 → 400；
//! - 保留段 `create`（`/api/v1/books/create` 是创建端点而非 id）放行；
//! - 无 seg（`/api/v1/books` 列表）与非 books/projects 前缀放行。
//!
//! 拦截响应不带 CORS 头（本层挂在 CORS 之内、路由之外，短路响应不经过
//! CORS 层装饰——与 Node 侧 Hono 中间件行为一致，浏览器面由回环守卫
//! 先行 403，正常跨源请求不会触达本层 400）。

use axum::extract::Request;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use percent_encoding::percent_decode_str;

/// `/api/v1/books/` 命名空间下的非 id 保留段（创建端点）。
const RESERVED_BOOK_SEGMENTS: [&str; 1] = ["create"];

/// 从请求路径提取需校验的（命名空间, id 段）——id 段为原始未解码形态。
///
/// 返回 `None`：非 books/projects 命名空间、或前缀后无段（列表端点）。
fn guarded_segment(path: &str) -> Option<(&'static str, &str)> {
    let mut it = path.split('/');
    let _ = it.next()?; // ""（前导斜杠）
    let _ = it.next()?; // "api"
    let _ = it.next()?; // "v1"
    let namespace = it.next()?;
    let ns = match namespace {
        "books" => "books",
        "projects" => "projects",
        _ => return None,
    };
    let seg = it.next()?;
    if seg.is_empty() {
        return None;
    }
    Some((ns, seg))
}

/// 守卫中间件本体（`from_fn` 装配，无状态）。
pub async fn guard(req: Request, next: Next) -> Response {
    let Some((ns, raw_seg)) = guarded_segment(req.uri().path()) else {
        return next.run(req).await;
    };
    if ns == "books" && RESERVED_BOOK_SEGMENTS.contains(&raw_seg) {
        return next.run(req).await;
    }
    // percent-decode 与 axum Path 提取同源（percent-encoding crate）——
    // 中间件校验的解码值与 handler 最终拿到的 Path<String> 一致。
    let decoded = percent_decode_str(raw_seg).decode_utf8();
    let safe = decoded.as_ref().is_ok_and(|id| crate::utils::is_safe_book_id(id));
    if !safe {
        let raw = decoded.as_deref().unwrap_or(raw_seg);
        return invalid_segment(ns, raw);
    }
    next.run(req).await
}

fn invalid_segment(namespace: &str, raw: &str) -> Response {
    // 消息中的原始值做 JSON 转义（对齐 assert_safe_book_id 的防注入转义）；
    // 错误码对齐各命名空间既有契约：books=INVALID_BOOK_ID（Node 中间件）/
    // projects=INVALID_ID（双端 story-graph 端点既有码）。
    let (code, label) = if namespace == "projects" {
        ("INVALID_ID", "project id")
    } else {
        ("INVALID_BOOK_ID", "book ID")
    };
    let escaped = serde_json::to_string(raw).unwrap_or_else(|_| format!("\"{raw}\""));
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({
            "error": { "code": code, "message": format!("Invalid {label}: {escaped}") }
        })),
    )
        .into_response()
}

// 装挂点唯一：server/mod.rs 路由表手写 `.layer(from_fn(guard))`（需与
// api_no_store 保持洋葱序，不便抽象）——537 号扫出便捷装配函数零引用后
// 删除，勿再引入单用途包装。

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Method, Request};
    use axum::routing::{get, post};
    use axum::Router;
    use tower::ServiceExt;

    fn app() -> Router {
        Router::new()
            .route("/api/v1/books", get(|| async { "list" }))
            .route("/api/v1/books/create", post(|| async { "create" }))
            .route("/api/v1/books/:id", get(|| async { "book" }))
            .route("/api/v1/books/:id/timeline", get(|| async { "timeline" }))
            .route("/api/v1/projects/:id/story-graph", get(|| async { "graph" }))
            .route("/api/v1/health", get(|| async { "health" }))
            .layer(axum::middleware::from_fn(guard))
    }

    async fn body_string(body: axum::body::Body) -> String {
        let bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    fn req(method: Method, uri: &str) -> Request<Body> {
        Request::builder().method(method).uri(uri).body(Body::empty()).unwrap()
    }

    #[tokio::test]
    async fn percent_encoded_traversal_is_rejected() {
        for (uri, code) in [
            ("/api/v1/books/..%2F..%2Fusers", "INVALID_BOOK_ID"),
            ("/api/v1/books/..%2Fx", "INVALID_BOOK_ID"),
            ("/api/v1/books/.", "INVALID_BOOK_ID"),
            ("/api/v1/books/b%20", "INVALID_BOOK_ID"),
            ("/api/v1/books/%FF%FE", "INVALID_BOOK_ID"),
            ("/api/v1/projects/..%2F..%2Fetc", "INVALID_ID"),
        ] {
            let resp = app().oneshot(req(Method::GET, uri)).await.unwrap();
            assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "{uri}");
            let body = body_string(resp.into_body()).await;
            assert!(body.contains(code), "{uri}: {body}");
        }
    }

    #[tokio::test]
    async fn safe_ids_and_reserved_routes_pass() {
        // 合法 id（含中文 percent-encoded）放行到假路由。
        let resp = app().oneshot(req(Method::GET, "/api/v1/books/b1")).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        // 中文 id：URL 编码形态「夜港账本」。
        let resp = app()
            .oneshot(req(Method::GET, "/api/v1/books/%E5%A4%9C%E6%B8%AF%E8%B4%A6%E6%9C%AC/timeline"))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        // 列表、创建保留段、非 books 前缀不受影响。
        let resp = app().oneshot(req(Method::GET, "/api/v1/books")).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let resp = app().oneshot(req(Method::POST, "/api/v1/books/create")).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let resp = app().oneshot(req(Method::GET, "/api/v1/health")).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let resp = app().oneshot(req(Method::GET, "/api/v1/projects/p1/story-graph")).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn guard_segment_extraction() {
        assert_eq!(guarded_segment("/api/v1/books/b1/x"), Some(("books", "b1")));
        assert_eq!(guarded_segment("/api/v1/books/b1"), Some(("books", "b1")));
        assert_eq!(guarded_segment("/api/v1/books"), None);
        assert_eq!(guarded_segment("/api/v1/books/"), None);
        assert_eq!(guarded_segment("/api/v1/projects/p1"), Some(("projects", "p1")));
        assert_eq!(guarded_segment("/api/v1/health"), None);
        assert_eq!(guarded_segment("/health"), None);
    }
}
