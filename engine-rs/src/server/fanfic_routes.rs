//! 同人/番外/仿写创建域端点（59 号）。
//!
//! 契约来源 `packages/studio/src/api/server.ts`：
//! - `POST /fanfic/init`（L6259）：title/sourceText 必填；端点自带 bookConfig
//!   （status=outlining、fanficMode、默认 100 章/3000 字）→ **同步**执行
//!   initFanficBook（TS await）→ SSE `fanfic:start/complete/error`；`{ok, bookId}`
//! - `POST /books/:id/fanfic/refresh`（L6313）：sourceText 必填 → 按书的
//!   fanficMode 重导 fanfic_canon.md → SSE `fanfic:refresh:*`；`{ok}`
//! - `POST /spinoff/init`（L6333）：title/parentBookId 必填、parent 缺失 404 →
//!   buildStudioBookConfig（字段回退 parent）→ 409 → 后台 initSpinoffBook →
//!   SSE `spinoff:start/complete/error` + `book:created/book:error` + 状态机；
//!   响应 `{status:"creating", bookId}`
//! - `POST /imitation/init`（L6387）：title/referenceText/storyIdea 必填 →
//!   initBook（storyIdea 为外部指令）+ **强制**风格向导（失败上抛）→ 后台 +
//!   SSE `imitation:*`；响应 `{status:"creating", bookId}`
//!
//! initFanficBook（runner.ts L971-L1028）：saveBookConfig（直接目标目录）→
//! fanfic_canon 导入 → 审核环（fanfic 模式 + sourceCanon）→ 落盘 → 控制文档
//! → 风格向导（≥500）→ chapters/ + 空索引 + 快照 0。
//! initSpinoffBook（L1037-L1073）：saveBookConfig → importCanon（parent_canon.md）
//! → spinoffContext → 审核环（original 模式）→ 同尾。

use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde_json::{json, Value};

use crate::agents::architect::{
    generate_fanfic_foundation, generate_foundation, write_foundation_files, ArchitectCtx,
    FoundationWriteMode,
};
use crate::agents::fanfic_canon_importer::import_from_text;
use crate::agents::foundation_reviewer::{
    build_foundation_review_feedback, review_foundation, FoundationReviewMode, ReviewParams,
};
use crate::llm::agent_router::RoutedAgent;
use crate::models::book::{BookConfig, FanficMode};
use crate::server::book_create_routes::{complete_book_exists, BookCreateStatus};
use crate::server::books_routes::BooksRuntime;
use crate::server::style_routes::{
    generate_style_guide_for_book, import_canon as run_import_canon,
};
use crate::utils::language::WritingLanguage;
use crate::utils::utc_time::utc_now_iso;

type ApiError = (StatusCode, Json<Value>);

fn flat_internal(message: impl std::fmt::Display) -> ApiError {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "error": message.to_string() })),
    )
}

/// fanficMode 解析（"canon"|"au"|"ooc"|"cp"，缺省/未知回 canon——TS 端点强转）。
pub(crate) fn parse_fanfic_mode(value: Option<&str>) -> FanficMode {
    match value {
        Some("au") => FanficMode::Au,
        Some("ooc") => FanficMode::Ooc,
        Some("cp") => FanficMode::Cp,
        _ => FanficMode::Canon,
    }
}

/// 书级语言（book.language ?? genre.language）。
async fn resolved_language(runtime: &BooksRuntime, book: &BookConfig) -> Result<WritingLanguage, String> {
    if book.language.is_some() {
        return Ok(if book.language.as_deref() == Some("en") {
            WritingLanguage::En
        } else {
            WritingLanguage::Zh
        });
    }
    let parsed = crate::agents::rules_reader::read_genre_profile(
        runtime.state.project_root(),
        &book.genre,
        &runtime.builtin_genres_dir,
    )
    .await
    .map_err(|e| e.to_string())?;
    Ok(if parsed.profile.language == "en" { WritingLanguage::En } else { WritingLanguage::Zh })
}

/// 审核环通用底座：reviewer 装配 + 反馈重生成驱动（generate 为同步重入函数）。
async fn review_loop(
    runtime: &BooksRuntime,
    book: &BookConfig,
    language: WritingLanguage,
    mode: FoundationReviewMode,
    source_canon: Option<&str>,
    mut regenerate: impl FnMut(Option<String>) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<crate::agents::architect::ArchitectOutput, String>> + Send>>,
) -> Result<crate::agents::architect::ArchitectOutput, String> {
    let reviewer_chat = RoutedAgent {
        router: runtime.effective_router().await,
        agent: "foundation-reviewer",
    };
    let mut feedback: Option<String> = None;
    let mut foundation = regenerate(feedback.clone()).await?;
    for _ in 0..2 {
        let params = ReviewParams {
            foundation: &foundation,
            mode,
            source_canon,
            style_guide: None,
            language,
            target_chapters: Some(book.target_chapters),
        };
        let review = review_foundation(&reviewer_chat, &params).await?;
        if review.passed {
            return Ok(foundation);
        }
        feedback = Some(build_foundation_review_feedback(&review, language));
        foundation = regenerate(feedback.clone()).await?;
    }
    Ok(foundation)
}

// ── initFanficBook / initSpinoffBook 链 ──────────────────────────

/// 同人书创建链。对齐 runner.ts `initFanficBook`（直接目标目录，非 staging）。
pub(crate) async fn init_fanfic_book(
    runtime: &BooksRuntime,
    book: &BookConfig,
    source_text: &str,
    source_name: &str,
    fanfic_mode: FanficMode,
) -> Result<(), String> {
    let state = &runtime.state;
    let book_dir = state.book_dir(&book.id);
    let language = resolved_language(runtime, book).await?;

    state.save_book_config(&book.id, book).await.map_err(|e| e.to_string())?;

    // Step 1：同人正典导入。
    let importer_chat = RoutedAgent {
        router: runtime.effective_router().await,
        agent: "fanfic-canon-importer",
    };
    let canon = import_from_text(&importer_chat, source_text, source_name, fanfic_mode).await?;
    let story_dir = book_dir.join("story");
    tokio::fs::create_dir_all(&story_dir).await.map_err(|e| e.to_string())?;
    tokio::fs::write(story_dir.join("fanfic_canon.md"), &canon.full_document)
        .await
        .map_err(|e| e.to_string())?;

    // Step 2：审核环（fanfic 模式 + sourceCanon）。
    // 204 号：闭包 future 需 'static（`dyn Future + Send` 默认约束）——捕获
    // Arc 句柄、块内构造局部端口（每次 review 重生成一个栈值，零泄漏）。
    let architect_router = runtime.effective_router().await;
    let foundation = review_loop(
        runtime,
        book,
        language,
        FoundationReviewMode::Fanfic,
        Some(&canon.full_document),
        |feedback| {
            let book = book.clone();
            let canon_text = canon.full_document.clone();
            let builtin = runtime.builtin_genres_dir.clone();
            let root = state.project_root().to_path_buf();
            let router = architect_router.clone();
            Box::pin(async move {
                let ctx = ArchitectCtx { project_root: &root, builtin_genres_dir: &builtin };
                let chat = RoutedAgent { router, agent: "architect" };
                generate_fanfic_foundation(&ctx, &chat, &book, &canon_text, fanfic_mode, feedback.as_deref())
                    .await
                    .map_err(|e| e.to_string())
            })
        },
    )
    .await?;
    write_foundation_files(&book_dir, &foundation, language, FoundationWriteMode::Init).await?;
    state
        .ensure_control_documents(&book.id, None)
        .await
        .map_err(|e| e.to_string())?;

    // Step 3：风格向导（≥500 UTF-16；吞错）。
    if source_text.encode_utf16().count() >= 500 {
        let _ = generate_style_guide_for_book(
            state,
            &*runtime.effective_router().await,
            &runtime.builtin_genres_dir,
            &book.id,
            source_text,
            Some(source_name),
        )
        .await;
    }

    // Step 4：chapters/ + 空索引 + 快照 0。
    tokio::fs::create_dir_all(book_dir.join("chapters")).await.map_err(|e| e.to_string())?;
    state
        .save_chapter_index_at(&book_dir, &[], true)
        .await
        .map_err(|e| e.to_string())?;
    state.snapshot_state(&book.id, 0).await.map_err(|e| e.to_string())?;
    Ok(())
}

/// 番外书创建链。对齐 runner.ts `initSpinoffBook`。
pub(crate) async fn init_spinoff_book(
    runtime: &BooksRuntime,
    book: &BookConfig,
    parent_book_id: &str,
    direction: Option<&str>,
) -> Result<(), String> {
    let state = &runtime.state;
    let book_dir = state.book_dir(&book.id);
    let language = resolved_language(runtime, book).await?;

    state.save_book_config(&book.id, book).await.map_err(|e| e.to_string())?;

    // 正传正典导入（复用 55 号 importCanon）。
    let parent_canon = run_import_canon(
        state,
        &*runtime.effective_router().await,
        &runtime.builtin_genres_dir,
        &book.id,
        parent_book_id,
    )
    .await?;

    // spinoff 上下文 + 审核环（original 模式）。
    let spinoff_context = build_spinoff_foundation_context(&parent_canon, direction, language);
    let architect_router = runtime.effective_router().await;
    let foundation = review_loop(
        runtime,
        book,
        language,
        FoundationReviewMode::Original,
        None,
        |feedback| {
            let book = book.clone();
            let context = spinoff_context.clone();
            let builtin = runtime.builtin_genres_dir.clone();
            let root = state.project_root().to_path_buf();
            let router = architect_router.clone();
            Box::pin(async move {
                let ctx = ArchitectCtx { project_root: &root, builtin_genres_dir: &builtin };
                let chat = RoutedAgent { router, agent: "architect" };
                generate_foundation(&ctx, &chat, &book, Some(&context), feedback.as_deref())
                    .await
                    .map_err(|e| e.to_string())
            })
        },
    )
    .await?;
    write_foundation_files(&book_dir, &foundation, language, FoundationWriteMode::Init).await?;
    state
        .ensure_control_documents(&book.id, direction.map(str::trim).filter(|d| !d.is_empty()))
        .await
        .map_err(|e| e.to_string())?;

    tokio::fs::create_dir_all(book_dir.join("chapters")).await.map_err(|e| e.to_string())?;
    state
        .save_chapter_index_at(&book_dir, &[], true)
        .await
        .map_err(|e| e.to_string())?;
    state.snapshot_state(&book.id, 0).await.map_err(|e| e.to_string())?;
    Ok(())
}

/// spinoff 上下文拼装。对齐 runner.ts `buildSpinoffFoundationContext`（双语逐字）。
fn build_spinoff_foundation_context(parent_canon: &str, direction: Option<&str>, language: WritingLanguage) -> String {
    let dir = direction.map(str::trim).filter(|d| !d.is_empty());
    let mut parts: Vec<String> = Vec::new();
    if language == WritingLanguage::En {
        parts.push("## This is a SIDE-STORY (番外)".to_string());
        parts.push(
            "Reuse the established characters, world, and rules from the parent canon below. Tell an INDEPENDENT side plot — a bonus arc, a character backstory, or a what-if — that does NOT advance or contradict the parent work's main storyline."
                .to_string(),
        );
        if let Some(dir) = dir {
            parts.push(format!("\n## Side-story direction\n{dir}"));
        }
        parts.push(format!("\n## Parent canon (reuse these characters and settings)\n{parent_canon}"));
    } else {
        parts.push("## 这是一部番外".to_string());
        parts.push(
            "复用下方正传正典里已确立的角色、世界观与规则。讲一个独立的侧篇故事——支线、角色前传或 what-if——不要推进或违背正传的主线剧情。"
                .to_string(),
        );
        if let Some(dir) = dir {
            parts.push(format!("\n## 番外方向\n{dir}"));
        }
        parts.push(format!("\n## 正传正典（复用以下角色与设定）\n{parent_canon}"));
    }
    parts.join("\n")
}

// ── POST /api/v1/fanfic/init ─────────────────────────────────────

pub async fn fanfic_init(State(runtime): State<BooksRuntime>, req: axum::extract::Request) -> impl IntoResponse {
    // 273 号：sourceText 可为整本原著——`Bytes` 提取器 2MB 默认上限截断，
    // 改手工读（TS 无上限；此处 64MB 安全上界）。
    let body = match crate::server::read_body_capped(req, crate::server::BODY_CAP_LARGE_TEXT).await {
        Ok(body) => body,
        Err(status) => return (status, Json(json!({ "error": "request body too large" }))),
    };
    let Ok(parsed) = serde_json::from_slice::<Value>(&body) else {
        return flat_internal("Unexpected token");
    };
    let title = parsed.get("title").and_then(Value::as_str).unwrap_or("");
    let source_text = parsed.get("sourceText").and_then(Value::as_str).unwrap_or("");
    if title.is_empty() || source_text.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "title and sourceText are required" })),
        );
    }
    let fanfic_mode = parse_fanfic_mode(parsed.get("mode").and_then(Value::as_str));
    let now = utc_now_iso();
    // TS 端点自带构造：默认 other 题材/100 章/3000 字（非 buildStudioBookConfig）。
    let language = match parsed.get("language").and_then(Value::as_str) {
        Some("en") => Some("en".to_string()),
        Some("zh") => Some("zh".to_string()),
        _ => None,
    };
    let book = BookConfig {
        id: crate::server::book_create_routes::derive_book_id_from_title_pub(title),
        title: title.to_string(),
        platform: parsed
            .get("platform")
            .and_then(Value::as_str)
            .map(crate::models::book::normalize_platform_or_other)
            .unwrap_or(crate::models::book::Platform::Other),
        genre: parsed.get("genre").and_then(Value::as_str).unwrap_or("other").to_string(),
        status: crate::models::book::BookStatus::Outlining,
        target_chapters: parsed.get("targetChapters").and_then(Value::as_f64).map(|v| v as u32).unwrap_or(100),
        chapter_word_count: parsed.get("chapterWordCount").and_then(Value::as_f64).map(|v| v as u32).unwrap_or(3000),
        language,
        created_at: now.clone(),
        updated_at: now,
        parent_book_id: None,
        fanfic_mode: Some(fanfic_mode),
        series: None,
        writing: None,
    };
    let book_id = book.id.clone();
    let source_name = parsed.get("sourceName").and_then(Value::as_str).unwrap_or("source").to_string();

    runtime
        .hub
        .broadcast("fanfic:start", &json!({ "bookId": book_id, "title": title }));
    match init_fanfic_book(&runtime, &book, source_text, &source_name, fanfic_mode).await {
        Ok(()) => {
            runtime.hub.broadcast("fanfic:complete", &json!({ "bookId": book_id }));
            (StatusCode::OK, Json(json!({ "ok": true, "bookId": book_id })))
        }
        Err(error) => {
            runtime
                .hub
                .broadcast("fanfic:error", &json!({ "bookId": book_id, "error": error }));
            flat_internal(error)
        }
    }
}

// ── POST /api/v1/books/:id/fanfic/refresh ────────────────────────

pub async fn fanfic_refresh(
    State(runtime): State<BooksRuntime>,
    AxumPath(book_id): AxumPath<String>,
    req: axum::extract::Request,
) -> impl IntoResponse {
    // 273 号：同 fanfic_init——sourceText 整本，手工读放宽。
    let body = match crate::server::read_body_capped(req, crate::server::BODY_CAP_LARGE_TEXT).await {
        Ok(body) => body,
        Err(status) => return (status, Json(json!({ "error": "request body too large" }))),
    };
    let Ok(parsed) = serde_json::from_slice::<Value>(&body) else {
        return flat_internal("Unexpected token");
    };
    let Some(source_text) = parsed
        .get("sourceText")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|t| !t.is_empty())
    else {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "sourceText is required" })));
    };
    runtime.hub.broadcast("fanfic:refresh:start", &json!({ "bookId": book_id }));
    let result = async {
        let book = runtime.state.load_book_config(&book_id).await.map_err(|e| e.to_string())?;
        let fanfic_mode = book.fanfic_mode.unwrap_or(FanficMode::Canon);
        let importer_chat = RoutedAgent {
            router: runtime.effective_router().await,
            agent: "fanfic-canon-importer",
        };
        let source_name = parsed.get("sourceName").and_then(Value::as_str).unwrap_or("source");
        let canon = import_from_text(&importer_chat, source_text, source_name, fanfic_mode).await?;
        let story_dir = runtime.state.book_dir(&book_id).join("story");
        tokio::fs::create_dir_all(&story_dir).await.map_err(|e| e.to_string())?;
        tokio::fs::write(story_dir.join("fanfic_canon.md"), &canon.full_document)
            .await
            .map_err(|e| e.to_string())?;
        Ok::<(), String>(())
    }
    .await;
    match result {
        Ok(()) => {
            runtime
                .hub
                .broadcast("fanfic:refresh:complete", &json!({ "bookId": book_id }));
            (StatusCode::OK, Json(json!({ "ok": true })))
        }
        Err(error) => {
            runtime
                .hub
                .broadcast("fanfic:refresh:error", &json!({ "bookId": book_id, "error": error }));
            flat_internal(error)
        }
    }
}

// ── POST /api/v1/spinoff/init ────────────────────────────────────

pub async fn spinoff_init(State(runtime): State<BooksRuntime>, req: axum::extract::Request) -> impl IntoResponse {
    // 273 号：统一大载荷读取（payload 小，但与 TS 无上限语义对齐，无害）。
    let body = match crate::server::read_body_capped(req, crate::server::BODY_CAP_LARGE_TEXT).await {
        Ok(body) => body,
        Err(status) => return (status, Json(json!({ "error": "request body too large" }))),
    };
    let Ok(parsed) = serde_json::from_slice::<Value>(&body) else {
        return flat_internal("Unexpected token");
    };
    let title = parsed.get("title").and_then(Value::as_str).map(str::trim).unwrap_or("");
    let parent_book_id = parsed
        .get("parentBookId")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    if title.is_empty() || parent_book_id.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "title and parentBookId are required" })),
        );
    }
    let Ok(parent) = runtime.state.load_book_config(parent_book_id).await else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": format!("Parent book \"{parent_book_id}\" not found") })),
        );
    };
    let language = match parsed.get("language").and_then(Value::as_str) {
        Some(l) => Some(l.to_string()),
        None => parent.language.clone(),
    };
    let now = utc_now_iso();
    let book = crate::server::book_create_routes::build_studio_book_config_pub(
        title,
        parsed.get("genre").and_then(Value::as_str).unwrap_or(&parent.genre),
        language.as_deref(),
        parsed.get("platform").and_then(Value::as_str).or(Some(parent.platform.as_str())),
        parsed
            .get("targetChapters")
            .and_then(Value::as_f64)
            .map(|v| v as u32)
            .or(Some(parent.target_chapters)),
        parsed
            .get("chapterWordCount")
            .and_then(Value::as_f64)
            .map(|v| v as u32)
            .or(Some(parent.chapter_word_count)),
        &now,
    );
    let book_id = book.id.clone();
    if book_id.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Could not derive a valid book id from title" })),
        );
    }
    if complete_book_exists(&runtime.state.book_dir(&book_id)).await {
        return (
            StatusCode::CONFLICT,
            Json(json!({ "error": format!("Book \"{book_id}\" already exists") })),
        );
    }
    let direction = parsed.get("direction").and_then(Value::as_str).map(str::to_string);

    runtime.hub.broadcast(
        "spinoff:start",
        &json!({ "bookId": book_id, "title": title, "parentBookId": parent_book_id }),
    );
    crate::server::book_create_routes::set_create_status(
        &book_id,
        BookCreateStatus { status: "creating".to_string(), error: None },
    )
    .await;
    let background = {
        let runtime = runtime.clone();
        let book = book.clone();
        let parent_book_id = parent_book_id.to_string();
        let book_id = book_id.clone();
        async move {
            let result = init_spinoff_book(&runtime, &book, &parent_book_id, direction.as_deref()).await;
            match result {
                Ok(()) => {
                    crate::server::book_create_routes::set_create_status_removed(&book_id).await;
                    runtime.hub.broadcast("spinoff:complete", &json!({ "bookId": book_id }));
                    runtime
                        .hub
                        .broadcast("book:created", &json!({ "bookId": book_id }));
                }
                Err(error) => {
                    crate::server::book_create_routes::set_create_status(
                        &book_id,
                        BookCreateStatus { status: "error".to_string(), error: Some(error.clone()) },
                    )
                    .await;
                    runtime
                        .hub
                        .broadcast("spinoff:error", &json!({ "bookId": book_id, "error": error }));
                }
            }
        }
    };
    tokio::spawn(background);
    (StatusCode::OK, Json(json!({ "status": "creating", "bookId": book_id })))
}

// ── POST /api/v1/imitation/init ──────────────────────────────────

pub async fn imitation_init(State(runtime): State<BooksRuntime>, req: axum::extract::Request) -> impl IntoResponse {
    // 273 号：referenceText 可为整本仿写对象文本，手工读放宽。
    let body = match crate::server::read_body_capped(req, crate::server::BODY_CAP_LARGE_TEXT).await {
        Ok(body) => body,
        Err(status) => return (status, Json(json!({ "error": "request body too large" }))),
    };
    let Ok(parsed) = serde_json::from_slice::<Value>(&body) else {
        return flat_internal("Unexpected token");
    };
    let title = parsed.get("title").and_then(Value::as_str).map(str::trim).unwrap_or("");
    let reference_text = parsed
        .get("referenceText")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    let story_idea = parsed.get("storyIdea").and_then(Value::as_str).map(str::trim).unwrap_or("");
    if title.is_empty() || reference_text.is_empty() || story_idea.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "title, referenceText and storyIdea are required" })),
        );
    }
    let now = utc_now_iso();
    let book = crate::server::book_create_routes::build_studio_book_config_pub(
        title,
        parsed.get("genre").and_then(Value::as_str).unwrap_or("other"),
        parsed.get("language").and_then(Value::as_str),
        parsed.get("platform").and_then(Value::as_str),
        parsed.get("targetChapters").and_then(Value::as_f64).map(|v| v as u32),
        parsed.get("chapterWordCount").and_then(Value::as_f64).map(|v| v as u32),
        &now,
    );
    let book_id = book.id.clone();
    if book_id.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Could not derive a valid book id from title" })),
        );
    }
    if complete_book_exists(&runtime.state.book_dir(&book_id)).await {
        return (
            StatusCode::CONFLICT,
            Json(json!({ "error": format!("Book \"{book_id}\" already exists") })),
        );
    }
    let source_name = parsed
        .get("sourceName")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("reference")
        .to_string();

    runtime.hub.broadcast("imitation:start", &json!({ "bookId": book_id, "title": title }));
    crate::server::book_create_routes::set_create_status(
        &book_id,
        BookCreateStatus { status: "creating".to_string(), error: None },
    )
    .await;
    let background = {
        let runtime = runtime.clone();
        let book = book.clone();
        let book_id = book_id.clone();
        let reference_text = reference_text.to_string();
        let story_idea = story_idea.to_string();
        let source_name = source_name.clone();
        async move {
            // initBook（storyIdea 为外部指令）+ 强制风格向导（失败上抛）。
            let result = async {
                crate::server::book_create_routes::init_book(&runtime, &book, Some(&story_idea), None, None).await?;
                // 强制风格向导（短样本走确定性指南——对齐 initImitationBook
                // 直接 await generateStyleGuide 而非 tryGenerate 吞错）。
                generate_style_guide_for_book(
                    &runtime.state,
                    &*runtime.effective_router().await,
                    &runtime.builtin_genres_dir,
                    &book_id,
                    reference_text.trim(),
                    Some(&source_name),
                )
                .await
                .map(|_| ())
            }
            .await;
            match result {
                Ok(()) => {
                    crate::server::book_create_routes::set_create_status_removed(&book_id).await;
                    runtime.hub.broadcast("imitation:complete", &json!({ "bookId": book_id }));
                    runtime.hub.broadcast("book:created", &json!({ "bookId": book_id }));
                }
                Err(error) => {
                    crate::server::book_create_routes::set_create_status(
                        &book_id,
                        BookCreateStatus { status: "error".to_string(), error: Some(error.clone()) },
                    )
                    .await;
                    runtime
                        .hub
                        .broadcast("imitation:error", &json!({ "bookId": book_id, "error": error }));
                }
            }
        }
    };
    tokio::spawn(background);
    (StatusCode::OK, Json(json!({ "status": "creating", "bookId": book_id })))
}

