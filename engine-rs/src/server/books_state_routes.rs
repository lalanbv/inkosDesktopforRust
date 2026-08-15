//! books 域状态端点批量挂载（48 号：列表/详情/更新/删除 + 章节读 +
//! approve/reject + truth 文件 + chapter-review-mode；49 号：编辑事务域——
//! 章节版本化 + PUT/DELETE 章节替换/删除 + workspace）。
//!
//! 契约来源 `packages/studio/src/api/server.ts`：
//! - `GET /books`（L3016）：`{books: [{...bookConfig, chaptersWritten}]}`
//! - `GET /books/:id`（L3022）：`{book, chapters, nextChapter}`；404
//! - `PUT /books/:id`（L5967）：四字段更新 + updatedAt；`{ok, book}`
//! - `DELETE /books/:id`（L5952）：rm -rf；`{ok, bookId}` + SSE book:deleted
//! - `GET /books/:id/chapters/:num`（L3146）：`{chapterNumber, filename, content}`；
//!   404（parseInt 失败同样 404——无匹配文件）
//! - `POST .../approve`（L3632）：索引状态翻转（目标缺失静默 200）
//! - `POST .../reject`（L3648）：rollbackToChapter(num-1)；目标缺失 404
//! - `GET /books/:id/truth`（L4392）：`{files: [{name, size, preview, legacy?,
//!   readonly?, readonlyReason?}]}`（size/preview 均 UTF-16 码元语义）
//! - `GET /books/:id/truth/:file{.+}`（L3451）：白名单校验（400）→ 200
//!   `{file, content, frontmatter?, body?, legacy?, readonly?}`（缺文件
//!   content:null 仍 200）
//! - `GET/PUT /books/:id/chapter-review-mode`（L5817/L5837）：book.json
//!   writing.reviewMode 读写（raw JSON 保未知字段；inkos.json 缺失按 TS
//!   怪癖 404）
//! - `GET .../chapters/:num/workspace`（L3164）：`{chapterNumber, brief,
//!   plan, versions, canDelete}`（非法章节号 400）
//! - `PUT .../chapters/:num/workspace/brief`（L3191）：brief 落盘（空串删
//!   文件）；400 文案逐字
//! - `GET .../chapters/:num/versions/:versionId`（L3276）：版本正文；任何
//!   错误 404（含非法 id）
//! - `POST .../versions/:versionId/restore`（L3291）：版本恢复经
//!   chapter-replace 事务 + SSE chapter:restored；错误 500
//! - `DELETE .../chapters/:num`（L3324）：deleteLatestChapter（trash 保留 +
//!   回滚链）；错误 400 + SSE chapter:deleted
//! - `PUT .../chapters/:num`（L3341）：chapter-replace（manual 来源归档）
//!   → 索引 audit-failed 待复核；错误 500

use std::sync::OnceLock;

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use regex::Regex;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::interaction::edit_controller::execute_chapter_replace;
use crate::models::book::{BookStatus, ChapterReviewModeVal};
use crate::server::books_routes::BooksRuntime;
use crate::state::chapter_delete::{delete_latest_chapter, DeleteRequest};
use crate::state::chapter_workspace::{
    list_chapter_versions, read_chapter_plan_document, read_chapter_user_brief,
    read_chapter_version, save_chapter_user_brief, ChapterVersionSource,
};
use crate::state::store::FsStateStore;
use crate::utils::book_id::is_safe_book_id;
use crate::utils::utc_time::{utc_now_iso, utc_now_millis};

// ── GET /api/v1/books ────────────────────────────────────────────

pub async fn list_books(State(runtime): State<BooksRuntime>) -> impl IntoResponse {
    let ids = runtime.state.list_books().await;
    let mut books: Vec<Value> = Vec::with_capacity(ids.len());
    for id in &ids {
        // TS Promise.all：任一书损坏 → 整体 500（loadStudioBookListSummary 抛）。
        let book = match runtime.state.load_book_config(id).await {
            Ok(book) => book,
            Err(error) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": error.to_string() })),
                )
            }
        };
        let next = match runtime.state.get_next_chapter_number(id).await {
            Ok(next) => next,
            Err(error) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": error.to_string() })),
                )
            }
        };
        let mut entry = serde_json::to_value(&book).unwrap_or_else(|_| json!({}));
        entry["chaptersWritten"] = json!(next.saturating_sub(1));
        books.push(entry);
    }
    (StatusCode::OK, Json(json!({ "books": books })))
}

// ── GET /api/v1/books/:id ────────────────────────────────────────

pub async fn book_detail(
    State(runtime): State<BooksRuntime>,
    Path(book_id): Path<String>,
) -> impl IntoResponse {
    let book = runtime.state.load_book_config(&book_id).await;
    let chapters = runtime.state.load_chapter_index(&book_id).await;
    let next = runtime.state.get_next_chapter_number(&book_id).await;
    match (book, chapters, next) {
        (Ok(book), Ok(chapters), Ok(next_chapter)) => (
            StatusCode::OK,
            Json(json!({
                "book": book,
                "chapters": chapters,
                "nextChapter": next_chapter,
            })),
        ),
        _ => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": format!("Book \"{book_id}\" not found") })),
        ),
    }
}

// ── PUT /api/v1/books/:id ────────────────────────────────────────

#[derive(Debug, Default, Deserialize)]
pub struct UpdateBookBody {
    #[serde(rename = "chapterWordCount", default)]
    pub chapter_word_count: Option<Value>,
    #[serde(rename = "targetChapters", default)]
    pub target_chapters: Option<Value>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub language: Option<String>,
}

/// 对齐 TS `Number(value)` 的宽容数值化：数字/数字字符串可转，其余 None
/// （TS 会写入 null——此处跳过字段，差异备案）。
fn coerce_number(value: &Value) -> Option<u32> {
    let number = value
        .as_f64()
        .or_else(|| value.as_str().and_then(|s| s.trim().parse::<f64>().ok()))?;
    if !number.is_finite() || number < 0.0 || number.fract() != 0.0 {
        return None;
    }
    Some(number as u32)
}

pub async fn update_book(
    State(runtime): State<BooksRuntime>,
    Path(book_id): Path<String>,
    Json(body): Json<UpdateBookBody>,
) -> impl IntoResponse {
    let mut book = match runtime.state.load_book_config(&book_id).await {
        Ok(book) => book,
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": error.to_string() })),
            )
        }
    };
    if let Some(words) = body.chapter_word_count.as_ref().and_then(coerce_number) {
        book.chapter_word_count = words;
    }
    if let Some(target) = body.target_chapters.as_ref().and_then(coerce_number) {
        book.target_chapters = target;
    }
    if let Some(status) = body.status.as_deref() {
        match serde_json::from_value::<BookStatus>(Value::String(status.to_string())) {
            Ok(parsed) => book.status = parsed,
            Err(error) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": error.to_string() })),
                )
            }
        }
    }
    if let Some(language) = body.language.as_deref() {
        book.language = Some(language.to_string());
    }
    book.updated_at = utc_now_iso();
    match runtime.state.save_book_config(&book_id, &book).await {
        Ok(()) => (StatusCode::OK, Json(json!({ "ok": true, "book": book }))),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": error.to_string() })),
        ),
    }
}

// ── DELETE /api/v1/books/:id ─────────────────────────────────────

pub async fn delete_book(
    State(runtime): State<BooksRuntime>,
    Path(book_id): Path<String>,
) -> impl IntoResponse {
    let book_dir = runtime.state.book_dir(&book_id);
    match tokio::fs::remove_dir_all(&book_dir).await {
        Ok(()) => {
            runtime.hub.broadcast("book:deleted", &json!({ "bookId": book_id }));
            (StatusCode::OK, Json(json!({ "ok": true, "bookId": book_id })))
        }
        // TS rm { force: true }：目标不存在时不报错（ENOENT 被吞）→ ok。
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            runtime.hub.broadcast("book:deleted", &json!({ "bookId": book_id }));
            (StatusCode::OK, Json(json!({ "ok": true, "bookId": book_id })))
        }
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": error.to_string() })),
        ),
    }
}

// ── GET /api/v1/books/:id/chapters/:num ──────────────────────────

pub async fn read_chapter(
    State(runtime): State<BooksRuntime>,
    Path((book_id, chapter)): Path<(String, String)>,
) -> impl IntoResponse {
    let not_found = || (StatusCode::NOT_FOUND, Json(json!({ "error": "Chapter not found" })));
    // TS parseInt 失败 → NaN → 无文件匹配 → 404（同样落 404，无 400 分支）。
    let Ok(number) = chapter.parse::<u32>() else {
        return not_found();
    };
    let chapters_dir = runtime.state.book_dir(&book_id).join("chapters");
    let Ok(mut entries) = tokio::fs::read_dir(&chapters_dir).await else {
        return not_found();
    };
    let padded = format!("{number:04}");
    let mut matched: Option<String> = None;
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with(&padded) && name.ends_with(".md") {
            matched = Some(name);
            break;
        }
    }
    let Some(file_name) = matched else {
        return not_found();
    };
    let Ok(content) = tokio::fs::read_to_string(chapters_dir.join(&file_name)).await else {
        return not_found();
    };
    (
        StatusCode::OK,
        Json(json!({ "chapterNumber": number, "filename": file_name, "content": content })),
    )
}

// ── POST /api/v1/books/:id/chapters/:num/approve ─────────────────

pub async fn approve_chapter(
    State(runtime): State<BooksRuntime>,
    Path((book_id, chapter)): Path<(String, String)>,
) -> impl IntoResponse {
    let Ok(number) = chapter.parse::<u32>() else {
        // TS parseInt NaN → map 无命中 → 200 ok（静默）。
        return (StatusCode::OK, Json(json!({ "ok": true, "chapterNumber": null, "status": "approved" })));
    };
    match runtime.state.load_chapter_index(&book_id).await {
        Ok(index) => {
            let mut updated = index;
            for slot in updated.iter_mut() {
                if slot.number == number {
                    slot.status = crate::models::chapter::ChapterStatus::Approved;
                }
            }
            match runtime.state.save_chapter_index(&book_id, &updated).await {
                Ok(()) => (
                    StatusCode::OK,
                    Json(json!({ "ok": true, "chapterNumber": number, "status": "approved" })),
                ),
                Err(error) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": error.to_string() })),
                ),
            }
        }
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": error.to_string() })),
        ),
    }
}

// ── POST /api/v1/books/:id/chapters/:num/reject ──────────────────

pub async fn reject_chapter(
    State(runtime): State<BooksRuntime>,
    Path((book_id, chapter)): Path<(String, String)>,
) -> impl IntoResponse {
    let Ok(number) = chapter.parse::<u32>() else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": format!("Chapter {chapter} not found") })),
        );
    };
    let index = match runtime.state.load_chapter_index(&book_id).await {
        Ok(index) => index,
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": error.to_string() })),
            )
        }
    };
    if !index.iter().any(|m| m.number == number) {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": format!("Chapter {number} not found") })),
        );
    }
    // rollbackTarget = num - 1；num=0 → -1 → restore 必败（TS 同款 500）。
    let rollback_target = number as i64 - 1;
    if rollback_target < 0 {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": format!("Cannot restore snapshot for chapter {rollback_target} in \"{book_id}\"")
            })),
        );
    }
    match runtime
        .state
        .rollback_to_chapter(&book_id, rollback_target as u32)
        .await
    {
        Ok(discarded) => (
            StatusCode::OK,
            Json(json!({
                "ok": true,
                "chapterNumber": number,
                "status": "rejected",
                "rolledBackTo": rollback_target,
                "discarded": discarded,
            })),
        ),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": error })),
        ),
    }
}

// ── 编辑事务域（49 号） ──────────────────────────────────────────

/// 对齐 JS `parseInt(raw, 10)`：去前导空白 → 可选符号 → ASCII 数字前缀；
/// 无数字 → None（NaN 语义）。
fn ts_parse_int(raw: &str) -> Option<i64> {
    let mut digits = String::new();
    for c in raw.trim_start().chars() {
        let is_sign = digits.is_empty() && (c == '+' || c == '-');
        if is_sign || c.is_ascii_digit() {
            digits.push(c);
        } else {
            break;
        }
    }
    if digits.is_empty() || digits == "+" || digits == "-" {
        return None;
    }
    digits.parse::<i64>().ok()
}

fn internal_error(message: impl std::fmt::Display) -> (StatusCode, Json<Value>) {
    (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": message.to_string() })))
}

/// 章节 JSON 正文解析：无效 JSON / 缺失或非字符串字段 → None。
/// （TS 分别抛异常/TypeError → 500；此处统一 500，文案偏差备案。）
fn json_string_field(bytes: &Bytes, field: &str) -> Option<String> {
    let parsed: Value = serde_json::from_slice(bytes).ok()?;
    parsed.get(field).and_then(Value::as_str).map(str::to_string)
}

// ── GET /api/v1/books/:id/chapters/:num/workspace ────────────────

pub async fn chapter_workspace(
    State(runtime): State<BooksRuntime>,
    Path((book_id, chapter)): Path<(String, String)>,
) -> impl IntoResponse {
    // TS parseInt + Number.isInteger + num >= 1 → 400。
    let Some(number) = ts_parse_int(&chapter).filter(|n| *n >= 1) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Invalid chapter number" })),
        );
    };
    let number = number as u32;
    let store = FsStateStore;
    let book_dir = runtime.state.book_dir(&book_id).to_string_lossy().into_owned();
    let Ok(brief) = read_chapter_user_brief(&store, &book_dir, number).await else {
        return internal_error("workspace brief read failed");
    };
    let Ok(plan) = read_chapter_plan_document(&store, &book_dir, number).await else {
        return internal_error("workspace plan read failed");
    };
    let Ok(versions) = list_chapter_versions(&store, &book_dir, number).await else {
        return internal_error("workspace versions read failed");
    };
    let index = match runtime.state.load_chapter_index(&book_id).await {
        Ok(index) => index,
        Err(error) => return internal_error(error),
    };
    let latest = index.iter().map(|m| m.number).max().unwrap_or(0);
    let versions: Vec<Value> = versions
        .iter()
        .map(|v| {
            json!({
                "id": v.id,
                "chapterNumber": v.chapter_number,
                "source": v.source.as_id_segment(),
                "createdAt": v.created_at,
                "characterCount": v.character_count,
            })
        })
        .collect();
    (
        StatusCode::OK,
        Json(json!({
            "chapterNumber": number,
            "brief": brief,
            "plan": plan,
            "versions": versions,
            "canDelete": number == latest,
        })),
    )
}

// ── PUT /api/v1/books/:id/chapters/:num/workspace/brief ──────────

pub async fn put_workspace_brief(
    State(runtime): State<BooksRuntime>,
    Path((book_id, chapter)): Path<(String, String)>,
    body: Bytes,
) -> impl IntoResponse {
    // TS c.req.json().catch(() => ({}))：无效 JSON 视同缺 brief → 400。
    let parsed: Value = serde_json::from_slice(&body).unwrap_or_else(|_| json!({}));
    let Some(number) = ts_parse_int(&chapter).filter(|n| *n >= 1) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "A valid chapter number and brief string are required" })),
        );
    };
    let Some(brief) = parsed.get("brief").and_then(Value::as_str) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "A valid chapter number and brief string are required" })),
        );
    };
    let number = number as u32;
    let book_dir = runtime.state.book_dir(&book_id).to_string_lossy().into_owned();
    match save_chapter_user_brief(&FsStateStore, &book_dir, number, brief).await {
        Ok(()) => (
            StatusCode::OK,
            Json(json!({ "ok": true, "chapterNumber": number, "brief": brief.trim() })),
        ),
        Err(error) => internal_error(error),
    }
}

// ── GET /api/v1/books/:id/chapters/:num/versions/:versionId ──────

pub async fn get_chapter_version(
    State(runtime): State<BooksRuntime>,
    Path((book_id, chapter, version_id)): Path<(String, String, String)>,
) -> impl IntoResponse {
    // TS 任何错误（非法章节号/非法版本 id/缺文件）→ 404。
    let Some(number) = ts_parse_int(&chapter).filter(|n| *n >= 1) else {
        let display = ts_parse_int(&chapter).map_or("NaN".to_string(), |n| n.to_string());
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": format!("Invalid chapter number: {display}") })),
        );
    };
    let number = number as u32;
    let book_dir = runtime.state.book_dir(&book_id).to_string_lossy().into_owned();
    match read_chapter_version(&FsStateStore, &book_dir, number, &version_id).await {
        Ok(content) => (
            StatusCode::OK,
            Json(json!({
                "chapterNumber": number,
                "versionId": version_id,
                "content": content,
            })),
        ),
        Err(error) => (StatusCode::NOT_FOUND, Json(json!({ "error": error.to_string() }))),
    }
}

// ── POST /api/v1/books/:id/chapters/:num/versions/:versionId/restore ──

pub async fn restore_chapter_version(
    State(runtime): State<BooksRuntime>,
    Path((book_id, chapter, version_id)): Path<(String, String, String)>,
) -> impl IntoResponse {
    // TS readChapterVersion 的 assert 错误落在外层 catch → 500。
    let Some(number) = ts_parse_int(&chapter).filter(|n| *n >= 1) else {
        let display = ts_parse_int(&chapter).map_or("NaN".to_string(), |n| n.to_string());
        return internal_error(format!("Invalid chapter number: {display}"));
    };
    let number = number as u32;
    let book_dir = runtime.state.book_dir(&book_id).to_string_lossy().into_owned();
    let full_text = match read_chapter_version(&FsStateStore, &book_dir, number, &version_id).await {
        Ok(content) => content,
        Err(error) => return internal_error(error),
    };
    match execute_chapter_replace(
        &runtime.state,
        &book_id,
        number,
        &full_text,
        ChapterVersionSource::Restore,
        utc_now_millis(),
        &utc_now_iso(),
    )
    .await
    {
        Ok(result) => {
            runtime
                .hub
                .broadcast("chapter:restored", &json!({ "bookId": book_id, "chapterNumber": number }));
            (
                StatusCode::OK,
                Json(json!({
                    "ok": true,
                    "chapterNumber": number,
                    "versionId": version_id,
                    "result": result,
                })),
            )
        }
        Err(error) => internal_error(error),
    }
}

// ── PUT /api/v1/books/:id/chapters/:num ──────────────────────────

pub async fn put_chapter(
    State(runtime): State<BooksRuntime>,
    Path((book_id, chapter)): Path<(String, String)>,
    body: Bytes,
) -> impl IntoResponse {
    // TS parseInt 失败/负数 → findChapterPath padStart 后必然无匹配 → 该文案。
    let Some(number) = ts_parse_int(&chapter).filter(|n| *n >= 1) else {
        let display = ts_parse_int(&chapter).map_or("NaN".to_string(), |n| n.to_string());
        return internal_error(format!("Chapter {display} not found."));
    };
    let Some(content) = json_string_field(&body, "content") else {
        return internal_error("content must be a string");
    };
    match execute_chapter_replace(
        &runtime.state,
        &book_id,
        number as u32,
        &content,
        ChapterVersionSource::Manual,
        utc_now_millis(),
        &utc_now_iso(),
    )
    .await
    {
        Ok(result) => (
            StatusCode::OK,
            Json(json!({ "ok": true, "chapterNumber": number, "result": result })),
        ),
        Err(error) => internal_error(error),
    }
}

// ── DELETE /api/v1/books/:id/chapters/:num ───────────────────────

pub async fn delete_chapter(
    State(runtime): State<BooksRuntime>,
    Path((book_id, chapter)): Path<(String, String)>,
) -> impl IntoResponse {
    let request = match ts_parse_int(&chapter) {
        Some(number) => DeleteRequest::Chapter(number),
        None => DeleteRequest::NaN,
    };
    match delete_latest_chapter(&runtime.state, &book_id, request).await {
        Ok(result) => {
            runtime.hub.broadcast(
                "chapter:deleted",
                &json!({ "bookId": book_id, "chapterNumber": result.deleted_chapter }),
            );
            // TS `{ ok: true, ...result }`。
            let mut body = serde_json::to_value(&result).unwrap_or_else(|_| json!({}));
            body["ok"] = json!(true);
            (StatusCode::OK, Json(body))
        }
        // TS DELETE 错误 → 400（与 PUT 的 500 不同）。
        Err(error) => (StatusCode::BAD_REQUEST, Json(json!({ "error": error }))),
    }
}

// ── truth 文件白名单 ─────────────────────────────────────────────

/// 平面真相文件白名单（server.ts TRUTH_FLAT_FILES，15 项）。
const TRUTH_FLAT_FILES: &[&str] = &[
    "author_intent.md",
    "current_focus.md",
    "story_bible.md",
    "book_rules.md",
    "volume_outline.md",
    "current_state.md",
    "particle_ledger.md",
    "pending_hooks.md",
    "chapter_summaries.md",
    "subplot_board.md",
    "emotional_arcs.md",
    "character_matrix.md",
    "style_guide.md",
    "parent_canon.md",
    "fanfic_canon.md",
];

/// Phase 5 大纲文件白名单（TRUTH_OUTLINE_FILES，4 项）。
const TRUTH_OUTLINE_FILES: &[&str] = &[
    "outline/story_frame.md",
    "outline/volume_map.md",
    "outline/节奏原则.md",
    "outline/rhythm_principles.md",
];

/// 兼容指针 shim（新布局书中不再权威；GET 标记 legacy）。
const LEGACY_SHIM_FILES: &[&str] = &["story_bible.md", "book_rules.md"];

/// runtime 章节诊断文件：`runtime/chapter-NNNN.(intent.md|plan.md|context.json|rule-stack.yaml|trace.json)`。
fn runtime_diagnostic_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"^runtime/chapter-\d{4}\.(?:intent\.md|plan\.md|context\.json|rule-stack\.yaml|trace\.json)$")
            .unwrap()
    })
}

/// 角色卡路径：`roles/(主要角色|次要角色|major|minor)/名称.md`。
fn roles_file_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^roles/(主要角色|次要角色|major|minor)/[^/]+\.md$").unwrap())
}

/// 校验真相文件相对路径（resolveTruthFilePath 逐条对齐）：
/// 拒空/\0/绝对/..；白名单命中；join 后不得逃出 story/。
fn resolve_truth_file_path(book_dir: &std::path::Path, file: &str) -> Option<std::path::PathBuf> {
    if file.is_empty()
        || file.contains('\0')
        || std::path::Path::new(file).is_absolute()
        || file.contains("..")
    {
        return None;
    }
    let allowed = TRUTH_FLAT_FILES.contains(&file)
        || TRUTH_OUTLINE_FILES.contains(&file)
        || runtime_diagnostic_re().is_match(file)
        || roles_file_re().is_match(file);
    if !allowed {
        return None;
    }
    let story_dir = book_dir.join("story");
    let resolved = story_dir.join(file);
    let relative = resolved.strip_prefix(&story_dir).ok()?;
    if relative.as_os_str().is_empty() {
        return None;
    }
    Some(resolved)
}

async fn is_new_layout_book(book_dir: &std::path::Path) -> bool {
    tokio::fs::try_exists(book_dir.join("story").join("outline").join("story_frame.md"))
        .await
        .unwrap_or(false)
}

/// UTF-16 码元切片（TS `content.slice(0, 200)`）。
fn utf16_preview(content: &str, limit: usize) -> String {
    if content.encode_utf16().count() <= limit {
        return content.to_string();
    }
    content
        .encode_utf16()
        .take(limit)
        .collect::<Vec<u16>>()
        .iter()
        .map(|&unit| char::from_u32(unit as u32).unwrap_or('\u{FFFD}'))
        .collect()
}

// ── GET /api/v1/books/:id/truth ──────────────────────────────────

async fn list_md_json_yaml(dir: &std::path::Path) -> Vec<String> {
    let mut names = Vec::new();
    let Ok(mut entries) = tokio::fs::read_dir(dir).await else {
        return names;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.ends_with(".md") || name.ends_with(".json") || name.ends_with(".yaml") {
            names.push(name);
        }
    }
    names
}

pub async fn truth_list(
    State(runtime): State<BooksRuntime>,
    Path(book_id): Path<String>,
) -> impl IntoResponse {
    let book_dir = runtime.state.book_dir(&book_id);
    let story_dir = book_dir.join("story");
    let new_layout = is_new_layout_book(&book_dir).await;

    let flat_files: Vec<String> = list_md_json_yaml(&story_dir)
        .await
        .into_iter()
        .filter(|f| !f.starts_with("outline") && !f.starts_with("roles"))
        .collect();
    let outline_files: Vec<String> =
        list_md_json_yaml(&story_dir.join("outline")).await.into_iter().map(|f| format!("outline/{f}")).collect();
    let mut all = flat_files;
    all.extend(outline_files);
    for role_dir in ["roles/主要角色", "roles/次要角色", "roles/major", "roles/minor"] {
        let entries: Vec<String> = list_md_json_yaml(&story_dir.join(role_dir))
            .await
            .into_iter()
            .map(|f| format!("{role_dir}/{f}"))
            .collect();
        all.extend(entries);
    }
    let runtime_files: Vec<String> = list_md_json_yaml(&story_dir.join("runtime"))
        .await
        .into_iter()
        .map(|f| format!("runtime/{f}"))
        .filter(|f| runtime_diagnostic_re().is_match(f))
        .collect();
    all.extend(runtime_files);

    let mut files: Vec<Value> = Vec::new();
    for rel in all {
        let Ok(content) = tokio::fs::read_to_string(story_dir.join(&rel)).await else {
            continue;
        };
        let is_shim = LEGACY_SHIM_FILES.contains(&rel.as_str()) && new_layout;
        let is_runtime_diagnostic = runtime_diagnostic_re().is_match(&rel);
        let base = json!({
            "name": rel,
            "size": content.encode_utf16().count(),
            "preview": utf16_preview(&content, 200),
        });
        let mut entry = base;
        if is_shim {
            entry["legacy"] = json!(true);
        }
        if is_runtime_diagnostic {
            entry["readonly"] = json!(true);
            entry["readonlyReason"] = json!("runtime-diagnostic");
        }
        files.push(entry);
    }
    (StatusCode::OK, Json(json!({ "files": files })))
}

// ── GET /api/v1/books/:id/truth/*file ────────────────────────────

pub async fn truth_file(
    State(runtime): State<BooksRuntime>,
    Path((book_id, file)): Path<(String, String)>,
) -> impl IntoResponse {
    let book_dir = runtime.state.book_dir(&book_id);
    let Some(resolved) = resolve_truth_file_path(&book_dir, &file) else {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "Invalid truth file" })));
    };
    let legacy = LEGACY_SHIM_FILES.contains(&file.as_str()) && is_new_layout_book(&book_dir).await;
    let runtime_diagnostic = runtime_diagnostic_re().is_match(&file);

    let mut payload = json!({ "file": file });
    if legacy {
        payload["legacy"] = json!(true);
    }
    if runtime_diagnostic {
        payload["readonly"] = json!(true);
        payload["readonlyReason"] = json!("runtime-diagnostic");
    }
    match tokio::fs::read_to_string(&resolved).await {
        Ok(content) => {
            payload["content"] = json!(content);
            // outline/* 携带 YAML frontmatter：结构化字段供 UI 卡片渲染，
            // content 保持原文（编辑器往返不变）。
            if let Ok(parsed) = crate::models::book_rules::try_parse_book_rules_frontmatter(&content) {
                payload["frontmatter"] = serde_json::to_value(&parsed.rules).unwrap_or(Value::Null);
                payload["body"] = json!(parsed.body);
            }
        }
        Err(_) => {
            // 缺文件 → 200 content:null（带 legacy/readonly 标记）。
            payload["content"] = Value::Null;
        }
    }
    (StatusCode::OK, Json(payload))
}

// ── GET/PUT /api/v1/books/:id/chapter-review-mode ────────────────

fn normalize_review_mode(mode: &str) -> ChapterReviewModeVal {
    if mode == "manual" {
        ChapterReviewModeVal::Manual
    } else {
        ChapterReviewModeVal::Auto
    }
}

fn review_mode_str(mode: ChapterReviewModeVal) -> &'static str {
    match mode {
        ChapterReviewModeVal::Manual => "manual",
        ChapterReviewModeVal::Auto => "auto",
    }
}

/// inkos.json writing.reviewMode（normalize：manual → manual，其余 auto）。
async fn project_review_mode(root: &std::path::Path) -> Option<ChapterReviewModeVal> {
    let raw = tokio::fs::read_to_string(root.join("inkos.json")).await.ok()?;
    let parsed: Value = serde_json::from_str(&raw).ok()?;
    let mode = parsed.get("writing")?.get("reviewMode")?.as_str()?;
    Some(normalize_review_mode(mode))
}

/// book.json writing.reviewMode（仅精确 manual/auto 值，其余 None）。
fn book_review_mode(raw_book: &Value) -> Option<ChapterReviewModeVal> {
    let mode = raw_book.get("writing")?.get("reviewMode")?.as_str()?;
    if mode == "manual" {
        Some(ChapterReviewModeVal::Manual)
    } else if mode == "auto" {
        Some(ChapterReviewModeVal::Auto)
    } else {
        None
    }
}

async fn load_raw_book_config(root: &std::path::Path, book_id: &str) -> std::io::Result<Value> {
    let raw = tokio::fs::read_to_string(root.join("books").join(book_id).join("book.json")).await?;
    Ok(serde_json::from_str(&raw)?)
}

pub async fn get_review_mode(
    State(runtime): State<BooksRuntime>,
    Path(book_id): Path<String>,
) -> impl IntoResponse {
    if !is_safe_book_id(&book_id) {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "Invalid book id" })));
    }
    let root = runtime.state.project_root();
    // TS 怪癖：inkos.json 缺失 → loadRawConfig 抛 → 404 Book not found。
    let (Ok(project_mode), Ok(raw_book)) = (
        project_review_mode(root).await.ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "config")),
        load_raw_book_config(root, &book_id).await,
    ) else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": format!("Book \"{book_id}\" not found") })),
        );
    };
    let book_mode = book_review_mode(&raw_book);
    let effective = book_mode.unwrap_or(project_mode);
    (
        StatusCode::OK,
        Json(json!({
            "mode": review_mode_str(effective),
            "bookMode": book_mode.map(review_mode_str),
            "projectMode": review_mode_str(project_mode),
        })),
    )
}

#[derive(Debug, Default, Deserialize)]
pub struct ReviewModeBody {
    #[serde(default)]
    pub mode: Option<String>,
}

pub async fn put_review_mode(
    State(runtime): State<BooksRuntime>,
    Path(book_id): Path<String>,
    Json(body): Json<ReviewModeBody>,
) -> impl IntoResponse {
    if !is_safe_book_id(&book_id) {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "Invalid book id" })));
    }
    let root = runtime.state.project_root();
    let (Ok(project_mode), Ok(mut raw_book)) = (
        project_review_mode(root).await.ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "config")),
        load_raw_book_config(root, &book_id).await,
    ) else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": format!("Book \"{book_id}\" not found") })),
        );
    };

    match body.mode.as_deref() {
        Some("inherit") => {
            // 删 writing.reviewMode；writing 空则整体移除（TS undefined 键不序列化）。
            if let Some(writing) = raw_book.get_mut("writing").and_then(Value::as_object_mut) {
                writing.remove("reviewMode");
                if writing.is_empty() {
                    raw_book.as_object_mut().map(|root| root.remove("writing"));
                }
            }
        }
        Some(mode) => {
            let normalized = normalize_review_mode(mode);
            let mut writing = raw_book
                .get("writing")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            writing.insert("reviewMode".to_string(), json!(review_mode_str(normalized)));
            raw_book
                .as_object_mut()
                .unwrap()
                .insert("writing".to_string(), Value::Object(writing));
        }
        None => {
            // TS normalizeChapterReviewMode(undefined) → "auto"。
            let mut writing = raw_book
                .get("writing")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            writing.insert("reviewMode".to_string(), json!("auto"));
            raw_book
                .as_object_mut()
                .unwrap()
                .insert("writing".to_string(), Value::Object(writing));
        }
    }

    let book_path = root.join("books").join(&book_id).join("book.json");
    // TS JSON.stringify(raw, null, 2)：2 空格缩进、无尾换行。
    let serialized = serde_json::to_string_pretty(&raw_book).unwrap_or_default();
    if tokio::fs::write(&book_path, serialized).await.is_err() {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("Book \"{book_id}\" not found") })),
        );
    }
    let book_mode = book_review_mode(&raw_book);
    let effective = book_mode.unwrap_or(project_mode);
    (
        StatusCode::OK,
        Json(json!({
            "ok": true,
            "mode": review_mode_str(effective),
            "bookMode": book_mode.map(review_mode_str),
            "projectMode": review_mode_str(project_mode),
        })),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use crate::pipeline::merged_audit::RevisionGate;
    use crate::server::sse::BroadcastHub;
    use crate::state::manager::StateManager;
    use std::sync::Arc;
    use tower::util::ServiceExt;

    fn runtime_for(root: &std::path::Path) -> BooksRuntime {
        BooksRuntime {
            hub: Arc::new(BroadcastHub::new()),
            state: Arc::new(StateManager::new(root)),
            router: Arc::new(AgentRouter::new(
                LlmEndpointConfig {
                    base_url: "http://127.0.0.1:9".into(),
                    api_key: "k".into(),
                    model: "m".into(),
                    max_tokens: 1024,
                    extra_headers: Default::default(),
                },
                Default::default(),
            )),
            builtin_genres_dir: root.to_path_buf(),
            revision_gate: RevisionGate::default(),
        }
    }

    fn fixture(root: &std::path::Path) {
        let book = root.join("books").join("b1");
        std::fs::create_dir_all(book.join("chapters")).unwrap();
        std::fs::create_dir_all(book.join("story").join("outline")).unwrap();
        std::fs::write(
            root.join("inkos.json"),
            r#"{ "writing": { "reviewMode": "manual" } }"#,
        )
        .unwrap();
        std::fs::write(
            book.join("book.json"),
            r#"{"id":"b1","title":"t","platform":"other","genre":"other","status":"active","targetChapters":10,"chapterWordCount":3000,"language":"zh","createdAt":"","updatedAt":""}"#,
        )
        .unwrap();
        std::fs::write(
            book.join("chapters").join("0001_风起.md"),
            "# 第1章 风起\n\n林动睁开双眼。",
        )
        .unwrap();
    }

    fn app(runtime: BooksRuntime) -> axum::Router {
        axum::Router::new()
            .route("/api/v1/books", axum::routing::get(list_books))
            .route("/api/v1/books/:id", axum::routing::get(book_detail).put(update_book).delete(delete_book))
            .route("/api/v1/books/:id/chapters/:num", axum::routing::get(read_chapter))
            .route("/api/v1/books/:id/chapters/:num/approve", axum::routing::post(approve_chapter))
            .route("/api/v1/books/:id/chapters/:num/reject", axum::routing::post(reject_chapter))
            .route("/api/v1/books/:id/truth", axum::routing::get(truth_list))
            .route("/api/v1/books/:id/truth/*file", axum::routing::get(truth_file))
            .route("/api/v1/books/:id/chapter-review-mode", axum::routing::get(get_review_mode).put(put_review_mode))
            .with_state(runtime)
    }

    fn request(method: &str, uri: &str, body: Option<&str>) -> axum::http::Request<axum::body::Body> {
        let mut builder = axum::http::Request::builder().method(method).uri(uri);
        if body.is_some() {
            builder = builder.header("content-type", "application/json");
        }
        builder.body(axum::body::Body::from(body.unwrap_or("").to_string())).unwrap()
    }

    async fn json_body(response: axum::http::Response<axum::body::Body>) -> Value {
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn list_books_returns_summary_with_chapters_written() {
        let dir = tempfile::tempdir().unwrap();
        fixture(dir.path());
        let response = app(runtime_for(dir.path()))
            .oneshot(request("GET", "/api/v1/books", None))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let parsed = json_body(response).await;
        let books = parsed["books"].as_array().unwrap();
        assert_eq!(books.len(), 1);
        assert_eq!(books[0]["id"], "b1");
        assert_eq!(books[0]["chaptersWritten"], 1);
    }

    #[tokio::test]
    async fn book_detail_returns_book_chapters_next() {
        let dir = tempfile::tempdir().unwrap();
        fixture(dir.path());
        let response = app(runtime_for(dir.path()))
            .oneshot(request("GET", "/api/v1/books/b1", None))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let parsed = json_body(response).await;
        assert_eq!(parsed["book"]["id"], "b1");
        assert_eq!(parsed["nextChapter"], 2);
        assert!(!parsed["chapters"].as_array().unwrap().is_empty());

        let missing = app(runtime_for(dir.path()))
            .oneshot(request("GET", "/api/v1/books/ghost", None))
            .await
            .unwrap();
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn update_book_applies_fields_and_persists() {
        let dir = tempfile::tempdir().unwrap();
        fixture(dir.path());
        let response = app(runtime_for(dir.path()))
            .oneshot(request(
                "PUT",
                "/api/v1/books/b1",
                Some(r#"{"chapterWordCount":5000,"status":"paused"}"#),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let parsed = json_body(response).await;
        assert_eq!(parsed["ok"], true);
        assert_eq!(parsed["book"]["chapterWordCount"], 5000);
        assert_eq!(parsed["book"]["status"], "paused");
        let raw = std::fs::read_to_string(dir.path().join("books").join("b1").join("book.json")).unwrap();
        assert!(raw.contains("\"chapterWordCount\": 5000"));
    }

    #[tokio::test]
    async fn read_chapter_returns_content_and_404s() {
        let dir = tempfile::tempdir().unwrap();
        fixture(dir.path());
        let runtime = runtime_for(dir.path());
        let response = app(runtime)
            .oneshot(request("GET", "/api/v1/books/b1/chapters/1", None))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let parsed = json_body(response).await;
        assert_eq!(parsed["chapterNumber"], 1);
        assert_eq!(parsed["filename"], "0001_风起.md");
        assert!(parsed["content"].as_str().unwrap().contains("林动"));

        let runtime = runtime_for(dir.path());
        let missing = app(runtime)
            .oneshot(request("GET", "/api/v1/books/b1/chapters/9", None))
            .await
            .unwrap();
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);
        let runtime = runtime_for(dir.path());
        let invalid = app(runtime)
            .oneshot(request("GET", "/api/v1/books/b1/chapters/abc", None))
            .await
            .unwrap();
        assert_eq!(invalid.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn approve_flips_index_status() {
        let dir = tempfile::tempdir().unwrap();
        fixture(dir.path());
        let state = StateManager::new(dir.path());
        state
            .save_chapter_index(
                "b1",
                &[crate::models::chapter::ChapterMeta {
                    number: 1,
                    title: "风起".into(),
                    status: crate::models::chapter::ChapterStatus::ReadyForReview,
                    word_count: 10,
                    created_at: "".into(),
                    updated_at: "".into(),
                    audit_issues: vec![],
                    length_warnings: vec![],
                    review_note: None,
                    detection_score: None,
                    detection_provider: None,
                    detected_at: None,
                    length_telemetry: None,
                    token_usage: None,
                }],
            )
            .await
            .unwrap();
        let response = app(runtime_for(dir.path()))
            .oneshot(request("POST", "/api/v1/books/b1/chapters/1/approve", None))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let parsed = json_body(response).await;
        assert_eq!(parsed["ok"], true);
        assert_eq!(parsed["status"], "approved");
        let index = state.load_chapter_index("b1").await.unwrap();
        assert_eq!(index[0].status, crate::models::chapter::ChapterStatus::Approved);
    }

    #[tokio::test]
    async fn reject_rolls_back_to_previous_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        fixture(dir.path());
        let state = StateManager::new(dir.path());
        let meta = |number: u32| crate::models::chapter::ChapterMeta {
            number,
            title: format!("第{number}章"),
            status: crate::models::chapter::ChapterStatus::ReadyForReview,
            word_count: 10,
            created_at: "".into(),
            updated_at: "".into(),
            audit_issues: vec![],
            length_warnings: vec![],
            review_note: None,
            detection_score: None,
            detection_provider: None,
            detected_at: None,
            length_telemetry: None,
            token_usage: None,
        };
        // 第 1 章快照 + 第 2 章索引/文件。
        let book_dir = dir.path().join("books").join("b1");
        std::fs::write(book_dir.join("story").join("current_state.md"), "状态v1").unwrap();
        std::fs::write(book_dir.join("story").join("pending_hooks.md"), "伏笔v1").unwrap();
        state.snapshot_state("b1", 1).await.unwrap();
        std::fs::write(
            book_dir.join("chapters").join("0002_云涌.md"),
            "# 第2章 云涌\n\n正文二",
        )
        .unwrap();
        std::fs::write(book_dir.join("story").join("current_state.md"), "状态v2").unwrap();
        state.save_chapter_index("b1", &[meta(1), meta(2)]).await.unwrap();

        let response = app(runtime_for(dir.path()))
            .oneshot(request("POST", "/api/v1/books/b1/chapters/2/reject", None))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let parsed = json_body(response).await;
        assert_eq!(parsed["ok"], true);
        assert_eq!(parsed["rolledBackTo"], 1);
        assert_eq!(parsed["discarded"], serde_json::json!([2]));
        // 第 2 章文件删除，状态回滚到快照。
        assert!(!book_dir.join("chapters").join("0002_云涌.md").exists());
        assert_eq!(
            std::fs::read_to_string(book_dir.join("story").join("current_state.md")).unwrap(),
            "状态v1"
        );
        let index = state.load_chapter_index("b1").await.unwrap();
        assert_eq!(index.len(), 1);

        // 目标缺失 → 404。
        let response = app(runtime_for(dir.path()))
            .oneshot(request("POST", "/api/v1/books/b1/chapters/9/reject", None))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn truth_endpoints_whitelist_and_legacy() {
        let dir = tempfile::tempdir().unwrap();
        fixture(dir.path());
        let book_dir = dir.path().join("books").join("b1");
        std::fs::write(book_dir.join("story").join("pending_hooks.md"), "| 伏笔 | 状态 |").unwrap();
        // 新布局：story_bible 变 shim → legacy 标记。
        std::fs::write(
            book_dir.join("story").join("outline").join("story_frame.md"),
            "---\nversion: \"1.0\"\nprotagonist:\n  name: 林动\n---\n正文框架",
        )
        .unwrap();
        std::fs::write(book_dir.join("story").join("story_bible.md"), "兼容指针").unwrap();

        let runtime = runtime_for(dir.path());
        let response = app(runtime)
            .oneshot(request("GET", "/api/v1/books/b1/truth", None))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let parsed = json_body(response).await;
        let files = parsed["files"].as_array().unwrap();
        let names: Vec<&str> = files.iter().map(|f| f["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"pending_hooks.md"));
        assert!(names.contains(&"story_bible.md"));
        assert!(names.contains(&"outline/story_frame.md"));
        let bible = files.iter().find(|f| f["name"] == "story_bible.md").unwrap();
        assert_eq!(bible["legacy"], true);

        // 单文件：frontmatter 解析 + legacy。
        let runtime = runtime_for(dir.path());
        let response = app(runtime)
            .oneshot(request("GET", "/api/v1/books/b1/truth/outline/story_frame.md", None))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let parsed = json_body(response).await;
        assert!(parsed["content"].as_str().unwrap().starts_with("---"));
        assert_eq!(parsed["frontmatter"]["protagonist"]["name"], "林动");
        assert_eq!(parsed["body"], "正文框架");

        // 白名单拒绝 → 400；缺文件 → 200 content null。
        let runtime = runtime_for(dir.path());
        let invalid = app(runtime)
            .oneshot(request("GET", "/api/v1/books/b1/truth/../../etc/passwd", None))
            .await
            .unwrap();
        assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
        let runtime = runtime_for(dir.path());
        let missing = app(runtime)
            .oneshot(request("GET", "/api/v1/books/b1/truth/pending_hooks2.md", None))
            .await
            .unwrap();
        assert_eq!(missing.status(), StatusCode::BAD_REQUEST);
        let runtime = runtime_for(dir.path());
        let absent = app(runtime)
            .oneshot(request("GET", "/api/v1/books/b1/truth/style_guide.md", None))
            .await
            .unwrap();
        assert_eq!(absent.status(), StatusCode::OK);
        let parsed = json_body(absent).await;
        assert!(parsed["content"].is_null());
    }

    #[tokio::test]
    async fn review_mode_get_and_put_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        fixture(dir.path());
        let runtime = runtime_for(dir.path());
        let response = app(runtime)
            .oneshot(request("GET", "/api/v1/books/b1/chapter-review-mode", None))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let parsed = json_body(response).await;
        // book 未设 → mode = projectMode = manual。
        assert_eq!(parsed["mode"], "manual");
        assert!(parsed["bookMode"].is_null());
        assert_eq!(parsed["projectMode"], "manual");

        // PUT manual → bookMode 覆盖。
        let runtime = runtime_for(dir.path());
        let response = app(runtime)
            .oneshot(request(
                "PUT",
                "/api/v1/books/b1/chapter-review-mode",
                Some(r#"{"mode":"manual"}"#),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let parsed = json_body(response).await;
        assert_eq!(parsed["ok"], true);
        assert_eq!(parsed["bookMode"], "manual");
        let raw = std::fs::read_to_string(dir.path().join("books").join("b1").join("book.json")).unwrap();
        assert!(raw.contains("\"reviewMode\": \"manual\""));

        // PUT inherit → 键删除。
        let runtime = runtime_for(dir.path());
        let response = app(runtime)
            .oneshot(request(
                "PUT",
                "/api/v1/books/b1/chapter-review-mode",
                Some(r#"{"mode":"inherit"}"#),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let raw = std::fs::read_to_string(dir.path().join("books").join("b1").join("book.json")).unwrap();
        assert!(!raw.contains("reviewMode"));
    }
}
