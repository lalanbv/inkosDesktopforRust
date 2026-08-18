//! 书籍创建与章节导入端点（58 号）。
//!
//! 契约来源 `packages/studio/src/api/server.ts`：
//! - `POST /books/create`（L3054）：`buildStudioBookConfig`（id 派生 +
//!   默认 200 章 / defaultChapterLength）→ completeBookExists 409 → SSE
//!   `book:creating` + 内存状态机 → 后台 `init_book`（staging 原子落盘）→
//!   成功删状态 + SSE `book:created`；失败置 error + SSE `book:error`；
//!   响应 `{status: "creating", bookId}`
//! - `POST /books/:id/import/chapters`（L6218）：`splitChapters` → SSE
//!   `import:start` → `generate_foundation_from_import` + 落盘 +
//!   `resetImportReplayTruthFiles` + 空索引 + 快照 0 + 风格向导（≥500 字）→
//!   逐章 ChapterAnalyzer 回放（saveChapter + saveNewTruthFiles + 索引 +
//!   快照）→ SSE `import:complete`；`{bookId, importedCount, totalWords, nextChapter}`
//!
//! `init_book` 对齐 runner.ts `initBook`（L734-L809）：审核环生成基础设定 →
//! staging 目录全量落盘（book.json / 基础设定 / 控制文档 / brief.md /
//! current_focus / 空索引 / 快照 0）→ 目标已存在校验 → 原子 rename。
//!
//! bookCreateStatus 为进程级内存状态机（对齐 TS server 内存 Map——
//! create-status 端点的内存分支由此接通）。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use axum::body::Bytes;
use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde_json::{json, Value};

use crate::agents::architect::{
    generate_foundation, generate_foundation_from_import, write_foundation_files, ArchitectCtx,
    FoundationWriteMode, ImportMode,
};
use crate::agents::foundation_reviewer::{
    build_foundation_review_feedback, review_foundation, FoundationReviewMode, ReviewParams,
};
use crate::llm::agent_router::RoutedAgent;
use crate::models::book::{BookConfig, Platform};
use crate::server::books_routes::BooksRuntime;
use crate::utils::chapter_splitter::split_chapters;
use crate::utils::language::WritingLanguage;
use crate::utils::length_metrics::{count_chapter_length, default_chapter_length, resolve_length_counting_mode};
use crate::utils::utc_time::utc_now_iso;

type ApiError = (StatusCode, Json<Value>);

fn flat_internal(message: impl std::fmt::Display) -> ApiError {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "error": message.to_string() })),
    )
}

// ── bookCreateStatus 内存状态机（TS server 内存 Map 的进程级等价物） ──

#[derive(Debug, Clone)]
pub struct BookCreateStatus {
    pub status: String,
    pub error: Option<String>,
}

fn create_status_map() -> &'static Arc<tokio::sync::Mutex<HashMap<String, BookCreateStatus>>> {
    static MAP: OnceLock<Arc<tokio::sync::Mutex<HashMap<String, BookCreateStatus>>>> = OnceLock::new();
    MAP.get_or_init(|| Arc::new(tokio::sync::Mutex::new(HashMap::new())))
}

/// 状态机查询（53 号 create-status 端点的内存分支）。
pub async fn peek_create_status(book_id: &str) -> Option<BookCreateStatus> {
    create_status_map().lock().await.get(book_id).cloned()
}

// ── studio 配置构造（book-create.ts） ─────────────────────────────

/// 标题派生书籍 id：lower → 非 [a-z0-9 汉字] 折叠为 `-` → 压缩 → 截 30。
fn derive_book_id_from_title(title: &str) -> String {
    let lower: String = title.trim().to_lowercase();
    let mut out = String::new();
    let mut last_dash = false;
    for c in lower.chars() {
        let keep = c.is_ascii_lowercase() || c.is_ascii_digit() || ('\u{4e00}'..='\u{9fff}').contains(&c);
        if keep {
            out.push(c);
            last_dash = false;
        } else if !last_dash && !out.is_empty() {
            out.push('-');
            last_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    while out.starts_with('-') {
        out.remove(0);
    }
    let units: Vec<(usize, char)> = out.char_indices().collect();
    let mut cut = out.len();
    for (idx, c) in units {
        if idx + c.len_utf8() > 30 {
            cut = idx;
            break;
        }
    }
    out[..cut].to_string()
}

/// 对齐 TS `buildStudioBookConfig`（默认 200 章 / 语言感知默认章长）。
#[allow(clippy::too_many_arguments)]
fn build_studio_book_config(
    title: &str,
    genre: &str,
    language: Option<&str>,
    platform: Option<&str>,
    target_chapters: Option<u32>,
    chapter_word_count: Option<u32>,
    now: &str,
) -> BookConfig {
    let language = match language {
        Some("en") => Some("en".to_string()),
        Some("zh") => Some("zh".to_string()),
        _ => None,
    };
    let default_length = default_chapter_length(
        if language.as_deref() == Some("en") { WritingLanguage::En } else { WritingLanguage::Zh },
    );
    BookConfig {
        id: derive_book_id_from_title(title),
        title: title.to_string(),
        platform: platform
            .map(crate::models::book::normalize_platform_or_other)
            .unwrap_or(Platform::Other),
        genre: genre.to_string(),
        status: crate::models::book::BookStatus::Outlining,
        target_chapters: target_chapters.unwrap_or(200),
        chapter_word_count: chapter_word_count.unwrap_or(default_length),
        language,
        created_at: now.to_string(),
        updated_at: now.to_string(),
        parent_book_id: None,
        fanfic_mode: None,
        writing: None,
    }
}

/// 状态机写入（59 号 fanfic/spinoff/imitation 端点用）。
pub async fn set_create_status(book_id: &str, status: BookCreateStatus) {
    create_status_map().lock().await.insert(book_id.to_string(), status);
}

/// 状态机清除（创建成功）。
pub async fn set_create_status_removed(book_id: &str) {
    create_status_map().lock().await.remove(book_id);
}

/// id 派生（59 号 fanfic 端点复用）。
pub fn derive_book_id_from_title_pub(title: &str) -> String {
    derive_book_id_from_title(title)
}

/// studio 配置构造（59 号 spinoff/imitation 端点复用）。
#[allow(clippy::too_many_arguments)]
pub fn build_studio_book_config_pub(
    title: &str,
    genre: &str,
    language: Option<&str>,
    platform: Option<&str>,
    target_chapters: Option<u32>,
    chapter_word_count: Option<u32>,
    now: &str,
) -> crate::models::book::BookConfig {
    build_studio_book_config(title, genre, language, platform, target_chapters, chapter_word_count, now)
}

/// 对齐 TS `completeBookExists`：book.json + story/story_bible.md 双存在。
pub async fn complete_book_exists(book_dir: &Path) -> bool {
    tokio::fs::try_exists(book_dir.join("book.json")).await.unwrap_or(false)
        && tokio::fs::try_exists(book_dir.join("story").join("story_bible.md")).await.unwrap_or(false)
}

// ── init_book（runner.ts initBook 的 Rust 装配） ──────────────────

/// staging 后缀：`.tmp-book-create-{id}-{ts36}-{rand}`。
fn staging_dir(books_dir: &Path, book_id: &str) -> PathBuf {
    let now_millis = crate::utils::utc_time::utc_now_millis();
    let rand: u32 = std::process::id().wrapping_mul(0x9E37_79B9) ^ (now_millis as u32);
    books_dir.join(format!(
        ".tmp-book-create-{book_id}-{:x}-{:x}",
        now_millis,
        rand & 0xFFFF
    ))
}

/// 创建链全量（审核环 + staging 原子落盘）。对齐 `initBook`。
pub async fn init_book(
    runtime: &BooksRuntime,
    book: &BookConfig,
    external_context: Option<&str>,
    author_intent: Option<&str>,
    current_focus: Option<&str>,
) -> Result<(), String> {
    let architect_chat: &'static RoutedAgent =
        Box::leak(Box::new(RoutedAgent { router: (*runtime.effective_router().await).clone(), agent: "architect" }));
    let reviewer_chat: &'static RoutedAgent = Box::leak(Box::new(RoutedAgent {
        router: (*runtime.effective_router().await).clone(),
        agent: "foundation-reviewer",
    }));
    let architect_ctx = ArchitectCtx {
        project_root: runtime.state.project_root(),
        builtin_genres_dir: &runtime.builtin_genres_dir,
    };
    let state = &runtime.state;
    let book_dir = state.book_dir(&book.id);
    let staging = staging_dir(&state.books_dir(), &book.id);

    let parsed_genre = crate::agents::rules_reader::read_genre_profile(
        state.project_root(),
        &book.genre,
        &runtime.builtin_genres_dir,
    )
    .await
    .map_err(|e| e.to_string())?;
    let gp = &parsed_genre.profile;
    let language = match book.language.as_deref() {
        Some("en") => WritingLanguage::En,
        Some(_) => WritingLanguage::Zh,
        None if gp.language == "en" => WritingLanguage::En,
        None => WritingLanguage::Zh,
    };
    // 审核环生成基础设定（maxRetries 默认 2）。
    let foundation = generate_and_review_foundation_multi(
        &architect_ctx,
        architect_chat,
        reviewer_chat,
        book.clone(),
        external_context.map(str::to_string),
        language,
        book.target_chapters,
        foundation_review_retries(state.project_root()).await,
    )
    .await?;

    let result = async {
        state.save_book_config_at(&staging, book).await.map_err(|e| e.to_string())?;
        write_foundation_files(&staging, &foundation, language, FoundationWriteMode::Init)
            .await?;

        if let Some(context) = external_context.filter(|c| !c.trim().is_empty()) {
            let story_dir = staging.join("story");
            tokio::fs::create_dir_all(&story_dir).await.map_err(|e| e.to_string())?;
            tokio::fs::write(story_dir.join("brief.md"), context).await.map_err(|e| e.to_string())?;
        }

        state
            .ensure_control_documents_at(
                &staging,
                language,
                author_intent.or(external_context),
            )
            .await
            .map_err(|e| e.to_string())?;
        if let Some(focus) = current_focus.map(str::trim).filter(|f| !f.is_empty()) {
            tokio::fs::write(
                staging.join("story").join("current_focus.md"),
                format!("{}\n", focus.trim_end()),
            )
            .await
            .map_err(|e| e.to_string())?;
        }

        state
            .save_chapter_index_at(&staging, &[], true)
            .await
            .map_err(|e| e.to_string())?;
        state.snapshot_state_at(&staging, 0).await.map_err(|e| e.to_string())?;

        if tokio::fs::try_exists(&book_dir).await.unwrap_or(false) {
            if complete_book_exists(&book_dir).await {
                return Err(format!(
                    "Book \"{}\" already exists at books/{}/. Use a different title or delete the existing book first.",
                    book.id, book.id
                ));
            }
            tokio::fs::remove_dir_all(&book_dir).await.map_err(|e| e.to_string())?;
        }
        tokio::fs::rename(&staging, &book_dir).await.map_err(|e| e.to_string())?;
        Ok(())
    }
    .await;

    if result.is_err() {
        let _ = tokio::fs::remove_dir_all(&staging).await;
    }
    result
}

/// 审核环装配（生成闭包走 architect；reviewFoundation 参数绑定）。
#[allow(clippy::too_many_arguments)]
async fn generate_and_review_foundation_multi(
    architect_ctx: &ArchitectCtx<'_>,
    architect_chat: &'static RoutedAgent,
    reviewer_chat: &'static RoutedAgent,
    book: BookConfig,
    external_context: Option<String>,
    language: WritingLanguage,
    target_chapters: u32,
    max_retries: usize,
) -> Result<crate::agents::architect::ArchitectOutput, String> {
    let mut feedback: Option<String> = None;
    let mut foundation = generate_foundation(architect_ctx, architect_chat, &book, external_context.as_deref(), feedback.as_deref())
        .await
        .map_err(|e| e.to_string())?;

    for _ in 0..max_retries {
        let params = ReviewParams {
            foundation: &foundation,
            mode: FoundationReviewMode::Original,
            source_canon: None,
            style_guide: None,
            language,
            target_chapters: Some(target_chapters),
        };
        let review = review_foundation(reviewer_chat, &params).await?;
        if review.passed {
            return Ok(foundation);
        }
        feedback = Some(build_foundation_review_feedback(&review, language));
        foundation = generate_foundation(architect_ctx, architect_chat, &book, external_context.as_deref(), feedback.as_deref())
            .await
            .map_err(|e| e.to_string())?;
    }

    // 终审（兜底接受——不再重生成）。
    let params = ReviewParams {
        foundation: &foundation,
        mode: FoundationReviewMode::Original,
        source_canon: None,
        style_guide: None,
        language,
        target_chapters: Some(target_chapters),
    };
    let _ = review_foundation(reviewer_chat, &params).await;
    Ok(foundation)
}

/// series 导入评审环（92 号）：生成 → foundation-reviewer 评审（series 五维）
/// → 不过则带反馈重生成（max 2 轮，TS `foundationReviewRetries ?? 2`）→
/// 终审兜底接受。对齐 TS `generateAndReviewFoundation`（mode "series"，
/// 无 sourceCanon/styleGuide；review 失败硬传播，终审容错——与确认面
/// multi 环同 reductions）。
async fn generate_and_review_foundation_import(
    architect_ctx: &ArchitectCtx<'_>,
    architect_chat: &'static RoutedAgent,
    reviewer_chat: &'static RoutedAgent,
    book: &BookConfig,
    foundation_source: &str,
    language: WritingLanguage,
    max_retries: usize,
) -> Result<crate::agents::architect::ArchitectOutput, String> {
    let target_chapters = book.target_chapters;
    let mut feedback: Option<String> = None;
    let mut foundation = generate_foundation_from_import(
        architect_ctx,
        architect_chat,
        book,
        foundation_source,
        None,
        feedback.as_deref(),
        ImportMode::Series,
    )
    .await
    .map_err(|e| e.to_string())?;

    for _ in 0..max_retries {
        let params = ReviewParams {
            foundation: &foundation,
            mode: FoundationReviewMode::Series,
            source_canon: None,
            style_guide: None,
            language,
            target_chapters: Some(target_chapters),
        };
        let review = review_foundation(reviewer_chat, &params).await?;
        if review.passed {
            return Ok(foundation);
        }
        feedback = Some(build_foundation_review_feedback(&review, language));
        foundation = generate_foundation_from_import(
            architect_ctx,
            architect_chat,
            book,
            foundation_source,
            None,
            feedback.as_deref(),
            ImportMode::Series,
        )
        .await
        .map_err(|e| e.to_string())?;
    }

    // 终审（兜底接受——不再重生成）。
    let params = ReviewParams {
        foundation: &foundation,
        mode: FoundationReviewMode::Series,
        source_canon: None,
        style_guide: None,
        language,
        target_chapters: Some(target_chapters),
    };
    let _ = review_foundation(reviewer_chat, &params).await;
    Ok(foundation)
}

// ── POST /api/v1/books/create ────────────────────────────────────

pub async fn create_book(
    State(runtime): State<BooksRuntime>,
    body: Bytes,
) -> impl IntoResponse {
    let Ok(parsed) = serde_json::from_slice::<Value>(&body) else {
        return flat_internal("Unexpected token");
    };
    let title = parsed.get("title").and_then(Value::as_str).unwrap_or("");
    let genre = parsed.get("genre").and_then(Value::as_str).unwrap_or("");
    if title.is_empty() || genre.is_empty() {
        // TS buildStudioBookConfig 不校验；缺 title → id 空 → 400 逐字。
        if title.is_empty() {
            return (StatusCode::BAD_REQUEST, Json(json!({ "error": "Could not derive a valid book id from title" })));
        }
    }
    let now = utc_now_iso();
    let book = build_studio_book_config(
        title,
        genre,
        parsed.get("language").and_then(Value::as_str),
        parsed.get("platform").and_then(Value::as_str),
        parsed.get("targetChapters").and_then(Value::as_f64).map(|v| v as u32),
        parsed.get("chapterWordCount").and_then(Value::as_f64).map(|v| v as u32),
        &now,
    );
    let book_id = book.id.clone();
    if book_id.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "Could not derive a valid book id from title" })));
    }
    if complete_book_exists(&runtime.state.book_dir(&book_id)).await {
        return (StatusCode::CONFLICT, Json(json!({ "error": format!("Book \"{book_id}\" already exists") })));
    }

    runtime.hub.broadcast("book:creating", &json!({ "bookId": book_id, "title": title }));
    create_status_map()
        .lock()
        .await
        .insert(book_id.clone(), BookCreateStatus { status: "creating".to_string(), error: None });

    // 后台异步创建（TS processProjectInteractionRequest().then 链）。
    let background = {
        let runtime = runtime.clone();
        let book = book.clone();
        let external_context = build_creation_external_context(&parsed);
        let book_id = book_id.clone();
        async move {
            let result = init_book(&runtime, &book, external_context.as_deref(), None, None).await;
            match result {
                Ok(()) => {
                    // 磁盘完成性复核（staging rename 后 bookDir 齐备）。
                    if !complete_book_exists(&runtime.state.book_dir(&book_id)).await {
                        let error = "Book creation artifact is incomplete on disk.".to_string();
                        create_status_map().lock().await.insert(
                            book_id.clone(),
                            BookCreateStatus { status: "error".to_string(), error: Some(error.clone()) },
                        );
                        runtime.hub.broadcast("book:error", &json!({ "bookId": book_id, "error": error }));
                        return;
                    }
                    create_status_map().lock().await.remove(&book_id);
                    let book_summary = runtime
                        .state
                        .load_book_config(&book_id)
                        .await
                        .ok()
                        .map(|config| json!({ "id": config.id, "title": config.title }));
                    runtime
                        .hub
                        .broadcast("book:created", &json!({ "bookId": book_id, "book": book_summary }));
                }
                Err(error) => {
                    create_status_map().lock().await.insert(
                        book_id.clone(),
                        BookCreateStatus { status: "error".to_string(), error: Some(error.clone()) },
                    );
                    runtime.hub.broadcast("book:error", &json!({ "bookId": book_id, "error": error }));
                }
            }
        }
    };
    tokio::spawn(background);

    (StatusCode::OK, Json(json!({ "status": "creating", "bookId": book_id })))
}

/// 创建外部指令拼接（blurb/worldPremise/... 八段）。对齐 `buildCreationExternalContext`。
fn build_creation_external_context(parsed: &Value) -> Option<String> {
    let field = |key: &str| {
        parsed
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
    };
    let sections: Vec<String> = [
        ("worldPremise", "## 世界观与核心设定"),
        ("settingNotes", "## 补充设定"),
        ("protagonist", "## 主角设定"),
        ("supportingCast", "## 关键角色与势力"),
        ("conflictCore", "## 核心冲突"),
        ("volumeOutline", "## 卷纲方向"),
        ("blurb", "## 简介卖点"),
        ("constraints", "## 创作约束"),
    ]
    .iter()
    .filter_map(|(key, heading)| field(key).map(|value| format!("{heading}\n{value}")))
    .collect();
    (!sections.is_empty()).then(|| sections.join("\n\n"))
}

// ── POST /api/v1/books/:id/import/chapters ───────────────────────

/// 回放种子：当前状态空表。对齐 `buildImportReplayStateSeed`。
fn import_replay_state_seed(language: WritingLanguage) -> String {
    if language == WritingLanguage::En {
        "# Current State\n\n| Field | Value |\n| --- | --- |\n| Current Chapter | 0 |\n| Current Location | (not set) |\n| Protagonist State | (not set) |\n| Current Goal | (not set) |\n| Current Constraint | (not set) |\n| Current Alliances | (not set) |\n| Current Conflict | (not set) |\n".to_string()
    } else {
        "# 当前状态\n\n| 字段 | 值 |\n| --- | --- |\n| 当前章节 | 0 |\n| 当前位置 | （未设定） |\n| 主角状态 | （未设定） |\n| 当前目标 | （未设定） |\n| 当前限制 | （未设定） |\n| 当前敌我 | （未设定） |\n| 当前冲突 | （未设定） |\n".to_string()
    }
}

/// 回放种子：伏笔池空表头。对齐 `buildImportReplayHooksSeed`。
fn import_replay_hooks_seed(language: WritingLanguage) -> String {
    if language == WritingLanguage::En {
        "# Pending Hooks\n\n| hook_id | start_chapter | type | status | last_advanced_chapter | expected_payoff | notes |\n| --- | --- | --- | --- | --- | --- | --- |\n".to_string()
    } else {
        "# 伏笔池\n\n| hook_id | 起始章节 | 类型 | 状态 | 最近推进 | 预期回收 | 备注 |\n| --- | --- | --- | --- | --- | --- | --- |\n".to_string()
    }
}

/// 回放前清空运行时真相（对齐 `resetImportReplayTruthFiles`）。
async fn reset_import_replay_truth_files(book_dir: &Path, language: WritingLanguage) {
    let story = book_dir.join("story");
    let _ = tokio::fs::write(story.join("current_state.md"), import_replay_state_seed(language)).await;
    let _ = tokio::fs::write(story.join("pending_hooks.md"), import_replay_hooks_seed(language)).await;
    for file in [
        "chapter_summaries.md",
        "subplot_board.md",
        "emotional_arcs.md",
        "character_matrix.md",
        "volume_summaries.md",
        "particle_ledger.md",
        "memory.db",
        "memory.db-shm",
        "memory.db-wal",
    ] {
        let _ = tokio::fs::remove_file(story.join(file)).await;
    }
    let _ = tokio::fs::remove_dir_all(story.join("state")).await;
    let _ = tokio::fs::remove_dir_all(story.join("snapshots")).await;
}

/// 章节导入全链。对齐 `importChapters`（Step1 地基 + Step2 逐章回放）。
pub(crate) async fn import_chapters_chain(
    runtime: &BooksRuntime,
    book_id: &str,
    chapters: &[crate::utils::chapter_splitter::SplitChapter],
) -> Result<Value, String> {
    import_chapters_chain_with_resume(runtime, book_id, chapters, 1, ImportMode::Continuation, None)
        .await
}

/// 导入链中止错误（英文锚文本；确认/聊天执行器据此本地化——与 write_next
/// 链 101 号 `WriteNextError::Aborted` 同一文案源）。
pub(crate) const IMPORT_ABORTED_MESSAGE: &str =
    "Operation aborted: the user requested to stop this task.";

fn import_check_aborted(abort: Option<&crate::interaction::agent_loop::AbortHandle>) -> Result<(), String> {
    if abort.is_some_and(|flag| *flag.lock().unwrap()) {
        return Err(IMPORT_ABORTED_MESSAGE.to_string());
    }
    Ok(())
}

/// `importChapters` 链（91 号补 resumeFrom/importMode；102 号补链内中止
/// 检查点——TS importChapters 三检查点逐位对齐：入口 / 每章回放头 /
/// 分析后落盘前，章粒度安全点）：
/// start_from == 1 → Step 1 全量重建（地基 + 真相重置 + 空索引 + 快照 0 +
/// 风格指纹）；start_from > 1 → 跳过 Step 1，保留既有地基与早前章节，
/// 逐章回放同号替换索引（resume 语义）。import_mode 直通架构师地基生成
/// （series 评审环见 91 号偏差备案）。
pub(crate) async fn import_chapters_chain_with_resume(
    runtime: &BooksRuntime,
    book_id: &str,
    chapters: &[crate::utils::chapter_splitter::SplitChapter],
    start_from: u32,
    import_mode: ImportMode,
    abort: Option<&crate::interaction::agent_loop::AbortHandle>,
) -> Result<Value, String> {
    // 检查点①：入口（TS 2826——锁前即查）。
    import_check_aborted(abort)?;
    let state = &runtime.state;
    let book = state.load_book_config(book_id).await.map_err(|e| e.to_string())?;
    let book_dir = state.book_dir(book_id);
    let parsed_genre = crate::agents::rules_reader::read_genre_profile(
        state.project_root(),
        &book.genre,
        &runtime.builtin_genres_dir,
    )
    .await
    .map_err(|e| e.to_string())?;
    let gp = &parsed_genre.profile;
    let language = match book.language.as_deref() {
        Some("en") => WritingLanguage::En,
        Some(_) => WritingLanguage::Zh,
        None if gp.language == "en" => WritingLanguage::En,
        None => WritingLanguage::Zh,
    };
    let counting_mode = resolve_length_counting_mode(language);

    // Step 1：首次导入（start_from == 1）时全量重建；续放跳过（保留既有地基）。
    if start_from == 1 {
        let foundation_source = build_import_foundation_source(chapters);
        let architect_chat: &'static RoutedAgent =
            Box::leak(Box::new(RoutedAgent { router: (*runtime.effective_router().await).clone(), agent: "architect" }));
        let architect_ctx = ArchitectCtx {
            project_root: state.project_root(),
            builtin_genres_dir: &runtime.builtin_genres_dir,
        };
        let foundation = if matches!(import_mode, ImportMode::Series) {
            // series：生成 → 评审 → 带反馈重生成（92 号评审环）。
            let reviewer_chat: &'static RoutedAgent = Box::leak(Box::new(RoutedAgent {
                router: (*runtime.effective_router().await).clone(),
                agent: "foundation-reviewer",
            }));
            generate_and_review_foundation_import(
                &architect_ctx,
                architect_chat,
                reviewer_chat,
                &book,
                &foundation_source,
                language,
                foundation_review_retries(state.project_root()).await,
            )
            .await?
        } else {
            generate_foundation_from_import(
                &architect_ctx,
                architect_chat,
                &book,
                &foundation_source,
                None,
                None,
                import_mode,
            )
            .await
            .map_err(|e| e.to_string())?
        };
        write_foundation_files(&book_dir, &foundation, language, FoundationWriteMode::Init).await?;
        reset_import_replay_truth_files(&book_dir, language).await;
        state
            .save_chapter_index_at(&book_dir, &[], true)
            .await
            .map_err(|e| e.to_string())?;
        state.snapshot_state_at(&book_dir, 0).await.map_err(|e| e.to_string())?;

        // 风格向导（资料包 ≥500 UTF-16 码元；吞错）。
        if foundation_source.encode_utf16().count() >= 500 {
            let _ = crate::server::style_routes::generate_style_guide_for_book(
                state, &*runtime.effective_router().await, &runtime.builtin_genres_dir, book_id, &foundation_source, Some(&book.title),
            )
            .await;
        }
    }

    // Step 2：逐章回放。
    let analyzer_chat: &'static RoutedAgent = Box::leak(Box::new(RoutedAgent {
        router: (*runtime.effective_router().await).clone(),
        agent: "chapter-analyzer",
    }));
    let analyzer_ctx: &'static crate::agents::chapter_analyzer::ChapterAnalyzerCtx =
        Box::leak(Box::new(crate::agents::chapter_analyzer::ChapterAnalyzerCtx {
            project_root: Box::leak(state.project_root().to_path_buf().into_boxed_path()),
            builtin_genres_dir: Box::leak(runtime.builtin_genres_dir.clone().into_boxed_path()),
        }));
    let writer_ctx: &'static crate::agents::writer::WriterCtx =
        Box::leak(Box::new(crate::agents::writer::WriterCtx {
            project_root: Box::leak(state.project_root().to_path_buf().into_boxed_path()),
            builtin_genres_dir: Box::leak(runtime.builtin_genres_dir.clone().into_boxed_path()),
            prompt_store: Box::leak(Box::new(crate::state::store::FsStateStore)),
            state_store: Box::leak(Box::new(crate::state::store::FsStateStore)),
        }));

    let mut total_words: u64 = 0;
    let mut imported_count = 0u32;
    // 回放治理输入端口（112 号：TS importChapters 的 prepareWriteInput——
    // v2 治理三件逐章构造，plan 持久化复用同链）。
    let write_agents = crate::server::books_routes::build_write_next_agents(runtime).await;
    let write_ctx = crate::server::books_routes::build_write_next_ctx(runtime);
    let governance_config = crate::pipeline::write_next::WriteNextConfig::default();
    for (index, chapter) in chapters.iter().enumerate().skip(start_from.saturating_sub(1) as usize) {
        // 检查点②：每章回放头（TS 2858）。
        import_check_aborted(abort)?;
        let chapter_number = (index + 1) as u32;
        let governed = crate::pipeline::write_next::prepare_write_input(
            state,
            &write_agents,
            &write_ctx,
            &governance_config,
            &book,
            &book_dir,
            chapter_number,
            None,
        )
        .await
        .map_err(|e| e.to_string())?;
        let output = crate::agents::chapter_analyzer::analyze_chapter(
            analyzer_chat,
            analyzer_ctx,
            &crate::agents::chapter_analyzer::AnalyzeChapterInput {
                book: &book,
                book_dir: &book_dir,
                chapter_number,
                chapter_content: &chapter.content,
                chapter_title: Some(&chapter.title),
                chapter_intent: governed.chapter_intent.as_deref(),
                context_package: governed.context_package.as_ref(),
                rule_stack: governed.rule_stack.as_ref(),
            },
        )
        .await
        .map_err(|e| e.to_string())?;
        // 检查点③：分析后、本章落盘前（TS 2896）——章粒度安全点：此前各章
        // 已完整落盘（含索引与快照），本章分析与产物全部丢弃、零残留。
        import_check_aborted(abort)?;

        let chapter_word_count =
            count_chapter_length(&chapter.content, counting_mode) as u32;
        // TS `{...output, content, wordCount, postWriteErrors: [], postWriteWarnings: []}`
        // ——分析面补全为落盘面（runtime 工件字段缺省）。
        let persisted = crate::agents::writer::WriteChapterOutput {
            chapter_number: output.chapter_number,
            title: output.title,
            content: chapter.content.clone(),
            word_count: chapter_word_count,
            pre_write_check: output.pre_write_check,
            post_settlement: output.post_settlement,
            runtime_state_delta: None,
            runtime_state_snapshot: None,
            updated_state: output.updated_state,
            updated_ledger: output.updated_ledger,
            updated_hooks: output.updated_hooks,
            chapter_summary: output.chapter_summary,
            updated_chapter_summaries: None,
            updated_subplots: output.updated_subplots,
            updated_emotional_arcs: output.updated_emotional_arcs,
            updated_character_matrix: output.updated_character_matrix,
            post_write_errors: Vec::new(),
            post_write_warnings: Vec::new(),
            hook_health_issues: Vec::new(),
            token_usage: Default::default(),
        };

        crate::agents::writer::save_chapter(writer_ctx, &book_dir, &persisted, gp.numerical_system, language)
            .await
            .map_err(|e| e.to_string())?;
        crate::agents::writer::save_new_truth_files(&book_dir, &persisted, language)
            .await
            .map_err(|e| e.to_string())?;
        // 114 号：TS syncLegacyStructuredStateFromMarkdown——analyzer 输出无 runtime
        // delta 时以 markdown 真相面强制回写 state/*.json 四件套（磁盘格式对齐）。
        crate::state::state_bootstrap::rewrite_structured_state_from_markdown(
            &crate::state::store::FsStateStore,
            &book_dir.display().to_string(),
            Some(chapter_number),
        )
        .await
        .map_err(|e| e.to_string())?;

        // 索引：同号替换（resume），否则追加；status "imported"。
        let existing = state.load_chapter_index(book_id).await.map_err(|e| e.to_string())?;
        let now = utc_now_iso();
        let new_entry = crate::models::chapter::ChapterMeta {
            number: chapter_number,
            title: persisted.title.clone(),
            status: crate::models::chapter::ChapterStatus::Imported,
            word_count: chapter_word_count,
            created_at: now.clone(),
            updated_at: now,
            audit_issues: Vec::new(),
            length_warnings: Vec::new(),
            review_note: None,
            detection_score: None,
            detection_provider: None,
            detected_at: None,
            length_telemetry: None,
            token_usage: None,
        };
        let mut updated = existing;
        match updated.iter().position(|m| m.number == chapter_number) {
            Some(idx) => updated[idx] = new_entry,
            None => updated.push(new_entry),
        }
        state.save_chapter_index(book_id, &updated).await.map_err(|e| e.to_string())?;
        state.snapshot_state(book_id, chapter_number).await.map_err(|e| e.to_string())?;

        imported_count += 1;
        total_words += chapter_word_count as u64;
    }

    Ok(json!({
        "bookId": book_id,
        "importedCount": imported_count,
        "totalWords": total_words,
        "nextChapter": chapters.len() as u32 + 1,
    }))
}

/// `foundation.reviewRetries`（TS FoundationConfigSchema：0-10，缺省 2）。
async fn foundation_review_retries(root: &Path) -> usize {
    crate::server::project_config_routes::load_raw_config(root)
        .await
        .and_then(|config| config.get("foundation").cloned())
        .and_then(|foundation| {
            foundation
                .get("reviewRetries")
                .and_then(serde_json::Value::as_u64)
                .map(|value| (value as usize).min(10))
        })
        .unwrap_or(2)
}

/// 导入地基资料包（章节目录 + 正文）。对齐 `buildImportFoundationSource` 的
/// 目录形态（TS 实现按章拼接标题与正文）。
fn build_import_foundation_source(chapters: &[crate::utils::chapter_splitter::SplitChapter]) -> String {
    chapters
        .iter()
        .map(|chapter| format!("# {}\n\n{}", chapter.title, chapter.content))
        .collect::<Vec<_>>()
        .join("\n\n")
}

pub async fn import_chapters_endpoint(
    State(runtime): State<BooksRuntime>,
    AxumPath(book_id): AxumPath<String>,
    body: Bytes,
) -> impl IntoResponse {
    let Ok(parsed) = serde_json::from_slice::<Value>(&body) else {
        return flat_internal("Unexpected token");
    };
    let Some(text) = parsed.get("text").and_then(Value::as_str).map(str::trim).filter(|t| !t.is_empty()) else {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "text is required" })));
    };
    let split_regex = parsed.get("splitRegex").and_then(Value::as_str);

    runtime
        .hub
        .broadcast("import:start", &json!({ "bookId": book_id, "type": "chapters" }));
    let chapters = split_chapters(text, split_regex);
    match import_chapters_chain(&runtime, &book_id, &chapters).await {
        Ok(result) => {
            runtime.hub.broadcast(
                "import:complete",
                &json!({ "bookId": book_id, "type": "chapters", "count": result["importedCount"] }),
            );
            (StatusCode::OK, Json(result))
        }
        Err(error) => {
            runtime
                .hub
                .broadcast("import:error", &json!({ "bookId": book_id, "error": error }));
            flat_internal(error)
        }
    }
}

// ── POST /books/:id/foundation/revise（72 号：reviseFoundation 四文件装配） ──

/// 架构稿修订：备份 → 旧四文读取（phase5 走 outline 新布局，phase4 走 legacy）
/// → architect 修订提示重写 → 审核（容错）→ Revise 模式落盘。
pub async fn revise_foundation(
    axum::extract::State(runtime): axum::extract::State<BooksRuntime>,
    axum::extract::Path(book_id): axum::extract::Path<String>,
    body: axum::body::Bytes,
) -> impl axum::response::IntoResponse {
    let payload: Value = serde_json::from_slice(&body).unwrap_or(json!({}));
    let feedback = payload
        .get("feedback")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let Some(feedback) = feedback else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "feedback is required" })),
        );
    };
    let result = revise_foundation_chain(&runtime, &book_id, feedback).await;
    match result {
        Ok(()) => {
            runtime
                .hub
                .broadcast("foundation:revised", &json!({ "bookId": book_id }));
            (StatusCode::OK, Json(json!({ "ok": true })))
        }
        Err(message) => {
            runtime.hub.broadcast(
                "foundation:error",
                &json!({ "bookId": book_id, "error": message }),
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": message })),
            )
        }
    }
}

/// revise_foundation 链（无 HTTP 面，88 号提取）：phase 探测 → 备份 →
/// architect 修订 → 容错审核 → Revise 模式落盘。sub_agent architect.revise
/// 聊天面复用（对齐 TS pipeline.reviseFoundation）。
pub(crate) async fn revise_foundation_chain(
    runtime: &BooksRuntime,
    book_id: &str,
    feedback: &str,
) -> Result<(), String> {
    let state = &runtime.state;
    let book_dir = state.book_dir(book_id);
    let story_dir = book_dir.join("story");
    let is_phase5 = story_dir.join("outline").join("story_frame.md").is_file();

    // 备份：legacy 四文件 + phase5 的 outline（浅）与 roles（深）。
    let timestamp = utc_now_iso().replace([':', '.'], "-");
    let backup_tag = if is_phase5 { "phase5" } else { "phase4" };
    let backup_dir = story_dir.join(format!(".backup-{backup_tag}-{timestamp}"));
    let _ = tokio::fs::create_dir_all(&backup_dir).await;
    for file_name in ["story_bible.md", "volume_outline.md", "book_rules.md", "character_matrix.md"] {
        if let Ok(content) = tokio::fs::read_to_string(story_dir.join(file_name)).await {
            let _ = tokio::fs::write(backup_dir.join(file_name), content).await;
        }
    }
    if is_phase5 {
        copy_dir_shallow(&story_dir.join("outline"), &backup_dir.join("outline")).await;
        copy_dir_deep(&story_dir.join("roles"), &backup_dir.join("roles")).await;
    }

    revise_foundation_inner(runtime, book_id, feedback, is_phase5).await
}

async fn revise_foundation_inner(
    runtime: &BooksRuntime,
    book_id: &str,
    feedback: &str,
    is_phase5: bool,
) -> Result<(), String> {
    use crate::agents::architect::{
        generate_foundation_inner, write_foundation_files, ArchitectCtx,
        FoundationWriteMode,
    };
    use crate::utils::outline_paths::{read_character_context, read_story_frame, read_volume_map};

    let state = &runtime.state;
    let book_dir = state.book_dir(book_id);
    let story_dir = book_dir.join("story");
    let read_or_empty = |path: std::path::PathBuf| async move {
        tokio::fs::read_to_string(path).await.unwrap_or_default()
    };

    // 旧四文：phase5 从新布局读（book_rules 仍为 legacy 文件）；phase4 全 legacy。
    let (old_story_bible, old_volume_outline, old_character_matrix) = if is_phase5 {
        let (a, b, c) = tokio::join!(
            read_story_frame(&book_dir, ""),
            read_volume_map(&book_dir, ""),
            read_character_context(&book_dir, "")
        );
        (a, b, c)
    } else {
        let (a, b, c) = tokio::join!(
            read_or_empty(story_dir.join("story_bible.md")),
            read_or_empty(story_dir.join("volume_outline.md")),
            read_or_empty(story_dir.join("character_matrix.md")),
        );
        (a, b, c)
    };
    let old_book_rules = read_or_empty(story_dir.join("book_rules.md")).await;

    let book = state.load_book_config(book_id).await.map_err(|e| e.to_string())?;
    let architect_chat: &'static crate::llm::agent_router::RoutedAgent =
        Box::leak(Box::new(crate::llm::agent_router::RoutedAgent {
            router: (*runtime.effective_router().await).clone(),
            agent: "architect",
        }));
    let architect_ctx = ArchitectCtx {
        project_root: state.project_root(),
        builtin_genres_dir: &runtime.builtin_genres_dir,
    };
    let revise_prompt = crate::agents::architect::build_revise_prompt(
        &old_story_bible,
        &old_volume_outline,
        &old_book_rules,
        &old_character_matrix,
        feedback,
    );
    let foundation = generate_foundation_inner(
        &architect_ctx,
        architect_chat,
        &book,
        None,
        None,
        Some(&revise_prompt),
    )
    .await
    .map_err(|e| e.to_string())?;

    // 审核环：失败/未通过仅记录（TS accept rewrite 语义——审核不阻断）。
    {
        let reviewer_chat: &'static crate::llm::agent_router::RoutedAgent =
            Box::leak(Box::new(crate::llm::agent_router::RoutedAgent {
                router: (*runtime.effective_router().await).clone(),
                agent: "foundation-reviewer",
            }));
        let language = if book.language.as_deref() == Some("en") {
            crate::utils::language::WritingLanguage::En
        } else {
            crate::utils::language::WritingLanguage::Zh
        };
        let review = crate::agents::foundation_reviewer::review_foundation(
            reviewer_chat,
            &crate::agents::foundation_reviewer::ReviewParams {
                foundation: &foundation,
                language,
                mode: crate::agents::foundation_reviewer::FoundationReviewMode::Original,
                source_canon: None,
                style_guide: None,
                target_chapters: Some(book.target_chapters),
            },
        )
        .await;
        if let Ok(review) = review {
            if !review.passed {
                tracing::warn!(
                    "[reviseFoundation] Foundation review did not pass; accepting rewrite."
                );
            }
        }
    }

    let _ = tokio::fs::create_dir_all(story_dir.join("outline")).await;
    let _ = tokio::fs::create_dir_all(story_dir.join("roles").join("主要角色")).await;
    let _ = tokio::fs::create_dir_all(story_dir.join("roles").join("次要角色")).await;
    let language = if book.language.as_deref() == Some("en") {
        crate::utils::language::WritingLanguage::En
    } else {
        crate::utils::language::WritingLanguage::Zh
    };
    write_foundation_files(&book_dir, &foundation, language, FoundationWriteMode::Revise).await
}

async fn copy_dir_shallow(src: &std::path::Path, dest: &std::path::Path) {
    let Ok(mut entries) = tokio::fs::read_dir(src).await else {
        return;
    };
    let _ = tokio::fs::create_dir_all(dest).await;
    while let Ok(Some(entry)) = entries.next_entry().await {
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if let Ok(content) = tokio::fs::read_to_string(src.join(&name)).await {
            let _ = tokio::fs::write(dest.join(name), content).await;
        }
    }
}

async fn copy_dir_deep(src: &std::path::Path, dest: &std::path::Path) {
    let Ok(mut entries) = tokio::fs::read_dir(src).await else {
        return;
    };
    let _ = tokio::fs::create_dir_all(dest).await;
    while let Ok(Some(entry)) = entries.next_entry().await {
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let src_path = src.join(&name);
        let dest_path = dest.join(name);
        if tokio::fs::metadata(&src_path).await.map(|m| m.is_dir()).unwrap_or(false) {
            Box::pin(copy_dir_deep(&src_path, &dest_path)).await;
        } else if let Ok(content) = tokio::fs::read_to_string(&src_path).await {
            let _ = tokio::fs::write(&dest_path, content).await;
        }
    }
}
