//! project 文件浏览面端点（server.ts L4270-L4320）。
//!
//! - `GET /project/files/*file`：生成图片预览（shorts/ covers/ interactive-films/
//!   前缀 + png/jpg/jpeg/webp），二进制回流 `Cache-Control: no-store`
//! - `GET /project/artifacts/*file`：文本工件读取（dramas/ storyboards/
//!   interactive-films/ shorts/ covers/ 前缀 + md/markdown/txt/json）
//! - `PUT /project/artifacts/*file`：文本工件写入（mkdir -p 父目录）
//!
//! 路径校验链逐字对齐 `resolveProjectImageFile` / `normalizeProjectGeneratedPath`：
//! 手动 `decodeURIComponent`（非法序列 400）→ 剥头部 `/` → 空/`\0`/绝对/`..` 段拒绝
//! → 前缀白名单 → ext → contentType → resolve+relative 再校验。
//! 错误形状：`{"error":{"code":..,"message":..}}`（ApiError onError 形状）。

use std::path::{Path, PathBuf};

use axum::body::Bytes;
use axum::extract::{Request, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};

use crate::server::books_routes::BooksRuntime;

type ApiErrorResponse = (StatusCode, Json<Value>);

fn api_error(status: StatusCode, code: &str, message: &str) -> ApiErrorResponse {
    (
        status,
        Json(json!({ "error": { "code": code, "message": message } })),
    )
}

/// `decodeURIComponent`：非法 percent 序列 / 非 UTF-8 → Err（TS throw → 400）。
///
/// `percent_decode_str` 对非法 `%xx` 是宽容透传，而 `decodeURIComponent` 严格
/// 抛错——先做严格 `%` 扫描（每个 `%` 必须后随两个 hex digit）再解码。
fn decode_uri_path(raw: &str) -> Result<String, ApiErrorResponse> {
    fn is_hex(b: u8) -> bool {
        b.is_ascii_hexdigit()
    }
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() || !is_hex(bytes[i + 1]) || !is_hex(bytes[i + 2]) {
                return Err(api_error(
                    StatusCode::BAD_REQUEST,
                    "INVALID_PROJECT_FILE_PATH",
                    "Invalid project file path",
                ));
            }
            i += 3;
        } else {
            i += 1;
        }
    }
    percent_encoding::percent_decode_str(raw)
        .decode_utf8()
        .map(|cow| cow.into_owned())
        .map_err(|_| {
            api_error(
                StatusCode::BAD_REQUEST,
                "INVALID_PROJECT_FILE_PATH",
                "Invalid project file path",
            )
        })
}

/// `replace(/^\/+/u, "")`：剥头部连续 `/`。
fn strip_leading_slashes(value: &str) -> &str {
    value.trim_start_matches('/')
}

/// `relPath.split(/[\\/]+/u).includes("..")`：`/` 与 `\` 分隔的 `..` 段。
fn has_parent_escape(rel_path: &str) -> bool {
    rel_path.split(['/', '\\']).any(|part| part == "..")
}

/// resolve + relative 的逃逸兜底：词法 join 后必须仍在 root 下。
fn resolve_within_root(root: &Path, rel_path: &str) -> Result<PathBuf, ApiErrorResponse> {
    let resolved = root.join(rel_path);
    let Ok(rel) = resolved.strip_prefix(root) else {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "INVALID_PROJECT_FILE_PATH",
            "Invalid project file path",
        ));
    };
    if rel.as_os_str().is_empty() {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "INVALID_PROJECT_FILE_PATH",
            "Invalid project file path",
        ));
    }
    Ok(resolved)
}

/// 通配段提取：剥 `/api/v1/{prefix}/` 前缀（uri.path() 保留 percent 编码，
/// 与 Hono `c.req.param` 返回 raw 段一致，解码由 [`decode_uri_path`] 手动做）。
fn wildcard_tail(req: &Request, prefix: &str) -> String {
    let path = req.uri().path();
    path.strip_prefix(prefix)
        .map(|s| s.to_string())
        .unwrap_or_default()
}

/// `normalizeProjectGeneratedPath`：解码 + 安全校验 + 前缀白名单 + 逃逸兜底。
fn normalize_generated_path(
    root: &Path,
    raw_path: &str,
    code: &str,
) -> Result<(String, PathBuf), ApiErrorResponse> {
    let invalid = |message: &str| api_error(StatusCode::BAD_REQUEST, code, message);
    let rel_path = decode_uri_path(raw_path).map_err(|_| {
        api_error(StatusCode::BAD_REQUEST, code, "Invalid project artifact path")
    })?;
    let rel_path = strip_leading_slashes(&rel_path);
    if rel_path.is_empty()
        || rel_path.contains('\0')
        || Path::new(rel_path).is_absolute()
        || has_parent_escape(rel_path)
    {
        return Err(invalid("Invalid project artifact path"));
    }
    const ALLOWED_ROOTS: [&str; 5] = [
        "dramas/",
        "storyboards/",
        "interactive-films/",
        "shorts/",
        "covers/",
    ];
    if !ALLOWED_ROOTS.iter().any(|prefix| rel_path.starts_with(prefix)) {
        return Err(invalid("Only generated writing artifacts can be opened"));
    }
    let resolved = resolve_within_root(root, rel_path)?;
    Ok((rel_path.to_string(), resolved))
}

/// `resolveProjectImageFile`：生成图片（前缀 + ext 白名单）。
fn resolve_project_image_file(
    root: &Path,
    raw_path: &str,
) -> Result<(PathBuf, &'static str), ApiErrorResponse> {
    let invalid = || {
        api_error(
            StatusCode::BAD_REQUEST,
            "INVALID_PROJECT_FILE_PATH",
            "Invalid project file path",
        )
    };
    let rel_path = decode_uri_path(raw_path)?;
    let rel_path = strip_leading_slashes(&rel_path).to_string();
    if rel_path.is_empty()
        || rel_path.contains('\0')
        || Path::new(&rel_path).is_absolute()
        || has_parent_escape(&rel_path)
    {
        return Err(invalid());
    }
    if !rel_path.starts_with("shorts/")
        && !rel_path.starts_with("covers/")
        && !rel_path.starts_with("interactive-films/")
    {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "INVALID_PROJECT_FILE_PATH",
            "Only generated shorts/, covers/, interactive-films/ images can be previewed",
        ));
    }
    // TS：relPath.split(".").pop()?.toLowerCase()（无点时为整串）
    let ext = rel_path.rsplit('.').next().unwrap_or_default().to_lowercase();
    let content_type = match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        _ => {
            return Err(api_error(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "UNSUPPORTED_PROJECT_FILE_TYPE",
                "Unsupported project file type",
            ))
        }
    };
    let resolved = resolve_within_root(root, &rel_path)?;
    Ok((resolved, content_type))
}

/// `resolveProjectTextArtifactFile`：文本工件（ext → contentType）。
fn resolve_project_text_artifact_file(
    root: &Path,
    raw_path: &str,
) -> Result<(String, PathBuf, &'static str), ApiErrorResponse> {
    let (rel_path, resolved) = normalize_generated_path(root, raw_path, "INVALID_PROJECT_ARTIFACT_PATH")?;
    let ext = rel_path.rsplit('.').next().unwrap_or_default().to_lowercase();
    let content_type = match ext.as_str() {
        "md" | "markdown" => "text/markdown; charset=utf-8",
        "txt" => "text/plain; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        _ => {
            return Err(api_error(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "UNSUPPORTED_PROJECT_ARTIFACT_TYPE",
                "Unsupported project artifact type",
            ))
        }
    };
    Ok((rel_path, resolved, content_type))
}

// ── GET /api/v1/project/files/*file ────────────────────────────

pub async fn get_project_file(
    State(runtime): State<BooksRuntime>,
    req: Request,
) -> Response {
    let root = runtime.state.project_root().to_path_buf();
    let raw = wildcard_tail(&req, "/api/v1/project/files/");
    let (resolved, content_type) = match resolve_project_image_file(&root, &raw) {
        Ok(file) => file,
        Err(response) => return response.into_response(),
    };
    match tokio::fs::read(&resolved).await {
        Ok(content) => {
            let mut response = Response::new(axum::body::Body::from(content));
            let headers = response.headers_mut();
            headers.insert(header::CONTENT_TYPE, content_type.parse().unwrap());
            headers.insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
            response
        }
        // TS c.notFound()：404 text/plain
        Err(_) => (
            StatusCode::NOT_FOUND,
            [(header::CONTENT_TYPE, "text/plain")],
            "404 Not Found",
        )
            .into_response(),
    }
}

// ── GET /api/v1/project/artifacts/*file ────────────────────────

pub async fn get_project_artifact(
    State(runtime): State<BooksRuntime>,
    req: Request,
) -> Response {
    let root = runtime.state.project_root().to_path_buf();
    let raw = wildcard_tail(&req, "/api/v1/project/artifacts/");
    let (rel_path, resolved, content_type) =
        match resolve_project_text_artifact_file(&root, &raw) {
            Ok(file) => file,
            Err(response) => return response.into_response(),
        };
    let content = match tokio::fs::read_to_string(&resolved).await {
        Ok(content) => content,
        Err(_) => {
            return (StatusCode::NOT_FOUND, "404 Not Found").into_response();
        }
    };
    let size = content.len();
    (
        StatusCode::OK,
        Json(json!({
            "path": rel_path,
            "content": content,
            "contentType": content_type,
            "size": size,
        })),
    )
        .into_response()
}

// ── PUT /api/v1/project/artifacts/*file ────────────────────────

pub async fn put_project_artifact(
    State(runtime): State<BooksRuntime>,
    req: Request,
) -> Response {
    let root = runtime.state.project_root().to_path_buf();
    let raw = wildcard_tail(&req, "/api/v1/project/artifacts/");
    let (rel_path, resolved, content_type) =
        match resolve_project_text_artifact_file(&root, &raw) {
            Ok(file) => file,
            Err(response) => return response.into_response(),
        };
    let body = match axum::body::to_bytes(req.into_body(), 16 * 1024 * 1024).await {
        Ok(bytes) => bytes,
        Err(_) => Bytes::new(),
    };
    let payload: Option<Value> = serde_json::from_slice(&body).ok();
    // TS：json().catch(() => null) + `payload && typeof === "object" && "content" in`
    let content = payload
        .as_ref()
        .and_then(Value::as_object)
        .and_then(|record| record.get("content"))
        .and_then(Value::as_str);
    let Some(content) = content else {
        return api_error(
            StatusCode::BAD_REQUEST,
            "INVALID_PROJECT_ARTIFACT_BODY",
            "content must be a string",
        )
        .into_response();
    };
    if let Some(parent) = resolved.parent() {
        if let Err(e) = tokio::fs::create_dir_all(parent).await {
            return internal_error(&e).into_response();
        }
    }
    if let Err(e) = tokio::fs::write(&resolved, content).await {
        return internal_error(&e).into_response();
    }
    (
        StatusCode::OK,
        Json(json!({
            "ok": true,
            "path": rel_path,
            "contentType": content_type,
            "size": content.len(),
        })),
    )
        .into_response()
}

fn internal_error(error: &std::io::Error) -> ApiErrorResponse {
    api_error(
        StatusCode::INTERNAL_SERVER_ERROR,
        "INTERNAL_ERROR",
        &error.to_string(),
    )
}
