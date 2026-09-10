//! translations 域端点（70 号）——翻译工作流。
//!
//! 契约来源 `packages/studio/src/api/server.ts` L6521-L6665：
//! - `GET /translations`（L6521）：目录枚举 + manifest 摘要 + projectId 降序
//! - `POST /translations/upload`（L6553）：safeUploadFileName + dataUrl 解析 +
//!   80MB 上限 + `.inkos/uploads/translation/{毫秒}-{name}` 落盘。
//!   274 号起 dataUrl 解析收敛至 upload_common（TS `parseDataUrl` 逐字：
//!   仅收 base64；解析失败 400 `INVALID_ATTACHMENT_DATA_URL`——此前本地
//!   实现额外接受非 base64 data URL 且用严格 base64 解码，均与 Node
//!   宽松语义有偏差）
//! - `POST /translations/create`（L6563）：MISSING_FILE_PATH / MISSING_LANGUAGES
//!   400 → createTranslationProjectFromFile → 响应展开 + projectId/title
//! - `GET /translations/:id`（L6588）：manifest + review 报告 + 章节段级合并
//! - `POST /translations/:id/run`（L6625）：LLM 翻译模型 + 上游错误 502
//!   TRANSLATION_RUN_FAILED / 其余 500 同 code
//! - `POST /translations/:id/export`（L6655）：txt / md / epub

use std::path::Path;

use axum::body::Bytes;
use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};

use crate::interaction::session::is_safe_book_id;
use crate::server::books_routes::BooksRuntime;
use crate::translation::run_store::{
    load_translation_chapter, load_translation_manifest, translation_project_dir,
    LoadManifestError,
};
use crate::translation::types::*;

fn api_error(status: StatusCode, code: &str, message: impl Into<String>) -> Response {
    (
        status,
        Json(json!({ "error": { "code": code, "message": message.into() } })),
    )
        .into_response()
}

fn invalid_id(id: &str) -> Response {
    api_error(
        StatusCode::BAD_REQUEST,
        "INVALID_ID",
        format!("invalid translation id: {id}"),
    )
}

fn load_error(id: &str, e: LoadManifestError) -> Response {
    match e {
        LoadManifestError::NotFound => api_error(
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            format!("translation project not found for {id}"),
        ),
        LoadManifestError::BadPayload(message) => {
            api_error(StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR", message)
        }
    }
}

// ── GET /translations ───────────────────────────────────────────

pub async fn list_translations(
    State(runtime): State<BooksRuntime>,
) -> impl IntoResponse {
    let root = runtime.state.project_root();
    let translations_dir = root.join("translations");
    let mut entries: Vec<String> = Vec::new();
    if let Ok(mut dirents) = tokio::fs::read_dir(&translations_dir).await {
        while let Ok(Some(entry)) = dirents.next_entry().await {
            if tokio::fs::metadata(translations_dir.join(entry.file_name()))
                .await
                .map(|m| m.is_dir())
                .unwrap_or(false)
            {
                if let Ok(name) = entry.file_name().into_string() {
                    entries.push(name);
                }
            }
        }
    }
    let mut translations: Vec<Value> = Vec::new();
    for project_id in entries {
        if !is_safe_book_id(&project_id) {
            continue;
        }
        // 坏 manifest skip（TS catch 同语义）。
        let Ok(manifest) = load_translation_manifest(root, &project_id).await else {
            continue;
        };
        translations.push(json!({
            "projectId": manifest.id,
            "title": manifest.title,
            "sourceLanguage": manifest.source_language,
            "targetLanguage": manifest.target_language,
            "chapters": manifest.chapters.len(),
        }));
    }
    // TS `b.projectId.localeCompare(a)` 降序（新项目在前——id 前缀是时间戳）。
    translations.sort_by(|a, b| {
        let a = a["projectId"].as_str().unwrap_or_default();
        let b = b["projectId"].as_str().unwrap_or_default();
        b.cmp(a)
    });
    (StatusCode::OK, Json(json!({ "translations": translations })))
}

// ── POST /translations/upload ───────────────────────────────────

const MAX_TRANSLATION_UPLOAD_BYTES: usize = 80 * 1024 * 1024;

pub async fn upload_translation(
    State(runtime): State<BooksRuntime>,
    req: axum::extract::Request,
) -> impl IntoResponse {
    let root = runtime.state.project_root();
    // 273 号：TS 解码上限 80MB，base64 膨胀后 body ≥107MB——axum 默认 2MB
    // 提取器上限先行截断，改手工读取（BODY_CAP_TRANSLATION_UPLOAD=128MB）。
    let body = match crate::server::read_body_capped(req, crate::server::BODY_CAP_TRANSLATION_UPLOAD).await {
        Ok(body) => body,
        Err(status) => {
            return api_error(status, "TRANSLATION_UPLOAD_TOO_LARGE", "Translation upload body too large")
        }
    };
    // 321 号：落地逻辑收敛到 upload_common::store_project_upload（TS
    // storeProjectUpload 逐字：文件名清洗/缺 dataUrl 400/解析/80MB 上限/
    // .inkos/uploads/translation 落盘）。
    let payload: Value = serde_json::from_slice(&body).unwrap_or(json!({}));
    match crate::server::upload_common::store_project_upload(
        root,
        payload.get("filename").and_then(Value::as_str),
        payload.get("dataUrl").and_then(Value::as_str),
        "translation",
        "translation-source",
        MAX_TRANSLATION_UPLOAD_BYTES,
        "INVALID_TRANSLATION_UPLOAD",
    )
    .await
    {
        Ok(outcome) => (
            StatusCode::OK,
            Json(json!({
                "storedPath": outcome.stored_path,
                "size": outcome.size,
                "mimeType": outcome.mime_type,
            })),
        )
            .into_response(),
        Err((status, body)) => (status, body).into_response(),
    }
}

// ── POST /translations/create ───────────────────────────────────

pub async fn create_translation(
    State(runtime): State<BooksRuntime>,
    body: Bytes,
) -> impl IntoResponse {
    let payload: Value = serde_json::from_slice(&body).unwrap_or(json!({}));
    let field = |name: &str| {
        payload
            .get(name)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
    };
    let Some(file_path) = field("filePath") else {
        return api_error(
            StatusCode::BAD_REQUEST,
            "MISSING_FILE_PATH",
            "filePath is required",
        );
    };
    let (Some(source_language), Some(target_language)) =
        (field("sourceLanguage"), field("targetLanguage"))
    else {
        return api_error(
            StatusCode::BAD_REQUEST,
            "MISSING_LANGUAGES",
            "sourceLanguage and targetLanguage are required",
        );
    };
    let input = CreateTranslationProjectInput {
        file_path,
        source_language,
        target_language,
        title: field("title"),
        segment_max_chars: payload
            .get("segmentMaxChars")
            .and_then(Value::as_u64)
            .map(|v| v as usize),
    };
    let root = runtime.state.project_root();
    match crate::translation::project::create_translation_project_from_file(root, &input).await {
        Ok(result) => {
            let manifest_json =
                serde_json::to_value(&result.manifest).unwrap_or(Value::Null);
            let mut payload = json!({
                "projectDir": result.project_dir,
                "manifestPath": result.manifest_path,
                "manifest": manifest_json,
            });
            let obj = payload.as_object_mut().unwrap();
            obj.insert("projectId".into(), json!(result.manifest.id));
            obj.insert("title".into(), json!(result.manifest.title));
            (StatusCode::OK, Json(payload)).into_response()
        }
        Err(message) => api_error(StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR", message),
    }
}

// ── GET /translations/:id ───────────────────────────────────────

pub async fn get_translation_detail(
    State(runtime): State<BooksRuntime>,
    AxumPath(id): AxumPath<String>,
) -> impl IntoResponse {
    if !is_safe_book_id(&id) {
        return invalid_id(&id);
    }
    let root = runtime.state.project_root();
    let manifest = match load_translation_manifest(root, &id).await {
        Ok(manifest) => manifest,
        Err(e) => return load_error(&id, e),
    };
    let report = tokio::fs::read_to_string(translation_project_dir(root, &id).join("review-report.md"))
        .await
        .unwrap_or_default();
    let mut chapters: Vec<Value> = Vec::new();
    for chapter_info in &manifest.chapters {
        let Ok(source) = load_translation_chapter(root, &chapter_info.source_path).await else {
            continue;
        };
        let translated = load_translation_chapter(root, &chapter_info.translated_path)
            .await
            .unwrap_or_else(|_| TranslationChapterFile {
                segments: Vec::new(),
                ..source.clone()
            });
        let targets: std::collections::HashMap<u32, &TranslationSegment> = translated
            .segments
            .iter()
            .map(|segment| (segment.index, segment))
            .collect();
        chapters.push(json!({
            "number": chapter_info.number,
            "title": chapter_info.title,
            "status": chapter_status_str(chapter_info.status),
            "segments": source.segments.iter().map(|segment| {
                let target = targets.get(&segment.index);
                json!({
                    "index": segment.index,
                    "source": segment.source,
                    "target": target.and_then(|s| s.target.clone()).unwrap_or_default(),
                    "notes": target.and_then(|s| s.notes.clone()).unwrap_or_default(),
                })
            }).collect::<Vec<_>>(),
        }));
    }
    (
        StatusCode::OK,
        Json(json!({
            "manifest": serde_json::to_value(&manifest).unwrap_or(Value::Null),
            "report": report,
            "chapters": chapters,
        })),
    )
        .into_response()
}

fn chapter_status_str(status: TranslationChapterStatus) -> &'static str {
    match status {
        TranslationChapterStatus::Pending => "pending",
        TranslationChapterStatus::Translated => "translated",
        TranslationChapterStatus::Reviewed => "reviewed",
    }
}

// ── POST /translations/:id/run ──────────────────────────────────

fn is_upstream_error(message: &str) -> bool {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::Regex::new(
            // "error sending request" = reqwest 连接层错误（TS Node fetch 的
            // "fetch failed" 等价形态）。
            r"(?i)API|LLM|provider|upstream|temporarily unavailable|rate limit|quota|fetch failed|error sending request|ECONNREFUSED|ENOTFOUND|ETIMEDOUT|503|502|504",
        )
        .unwrap()
    });
    re.is_match(message)
}

pub async fn run_translation(
    State(runtime): State<BooksRuntime>,
    AxumPath(id): AxumPath<String>,
    body: Bytes,
) -> impl IntoResponse {
    if !is_safe_book_id(&id) {
        return invalid_id(&id);
    }
    let payload: Value = serde_json::from_slice(&body).unwrap_or(json!({}));
    let batch_size = payload
        .get("batchSize")
        .and_then(Value::as_u64)
        .map(|v| v as usize);
    let max_tokens = payload
        .get("maxTokens")
        .and_then(Value::as_u64)
        .map(|v| v as u32);
    let root = runtime.state.project_root();
    let model = crate::translation::llm_model::LlmTranslationModel {
        router: &*runtime.effective_router().await,
        max_tokens,
    };
    match crate::translation::runner::run_translation_project(root, &id, &model, batch_size)
        .await
    {
        Ok(result) => (
            StatusCode::OK,
            Json(serde_json::to_value(&result).unwrap_or(Value::Null)),
        )
            .into_response(),
        Err(message) => {
            let status = if is_upstream_error(&message) {
                StatusCode::BAD_GATEWAY
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            api_error(
                status,
                "TRANSLATION_RUN_FAILED",
                if message.is_empty() {
                    "Translation run failed.".to_string()
                } else {
                    message
                },
            )
        }
    }
}

// ── POST /translations/:id/export ───────────────────────────────

pub async fn export_translation(
    State(runtime): State<BooksRuntime>,
    AxumPath(id): AxumPath<String>,
    body: Bytes,
) -> impl IntoResponse {
    if !is_safe_book_id(&id) {
        return invalid_id(&id);
    }
    let payload: Value = serde_json::from_slice(&body).unwrap_or(json!({}));
    let format = match payload.get("format").and_then(Value::as_str) {
        None | Some("") => TranslationExportFormat::Md,
        Some("md") => TranslationExportFormat::Md,
        Some("txt") => TranslationExportFormat::Txt,
        Some("epub") => TranslationExportFormat::Epub,
        Some(other) => {
            return api_error(
                StatusCode::BAD_REQUEST,
                "INVALID_FORMAT",
                format!("unsupported translation export format: {other}"),
            )
        }
    };
    let output_path = payload
        .get("outputPath")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty());
    let root: &Path = runtime.state.project_root();
    match crate::translation::export::write_translation_export(root, &id, format, output_path)
        .await
    {
        Ok(result) => (
            StatusCode::OK,
            Json(serde_json::to_value(&result).unwrap_or(Value::Null)),
        )
            .into_response(),
        Err(message) => api_error(StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR", message),
    }
}
