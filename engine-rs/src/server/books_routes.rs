//! books 域同步端点批量挂载（/plan、/settle、/draft、/revise/:chapter）。
//!
//! 契约来源 `packages/studio/src/api/server.ts`：
//! - `POST /books/:id/plan`（L3579）：**同步**，body `{context?}` →
//!   `{bookId, chapterNumber, intentPath, goal, conflicts: []}`（TS
//!   planChapter 的 PlanChapterResult 形状）
//! - `POST /books/:id/settle`：Node 侧无此端点（Node 走 /repair-state）；
//!   Rust 侧补齐——body `{chapter, title, content, allowReapply?}` →
//!   同步结算，返回落盘摘要
//! - `POST /books/:id/draft`（L3532）：**fire-and-forget**，body
//!   `{wordCount?, context?}` → `{status:"drafting", bookId}` + SSE
//!   draft:start/complete/error；复用 write-next manual 模式（写完即停 =
//!   draft 语义，变更记录备案）
//! - `POST /books/:id/revise/:chapter`（L5604）：**同步**，body
//!   `{mode?, brief?}`（默认 spot-fix）；章节缺失 404；SSE
//!   revise:start/complete/error；返回 ReviseOutput JSON

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::agents::reviser::{revise_chapter, ReviseMode, ReviseOptions};
use crate::llm::agent_router::{AgentRouter, RoutedAgent};
use crate::pipeline::persisted_governed_plan::relative_to_book_dir;
use crate::pipeline::write_next::{
    write_next_chapter, ChapterReviewMode, WriteNextAgents, WriteNextConfig, WriteNextCtx,
};
use crate::server::sse::BroadcastHub;
use crate::state::manager::StateManager;
use crate::state::store::FsStateStore;

/// 端点运行时（与 AuditRuntime 同款装配面）。
#[derive(Clone)]
pub struct BooksRuntime {
    pub hub: Arc<BroadcastHub>,
    pub state: Arc<StateManager>,
    pub router: Arc<AgentRouter>,
    pub builtin_genres_dir: std::path::PathBuf,
}

// ── POST /api/v1/books/:id/plan ─────────────────────────────────

#[derive(Debug, Default, Deserialize)]
pub struct PlanBody {
    #[serde(default)]
    pub context: Option<String>,
}

/// TS `PlanChapterResult` 形状。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanChapterResult {
    pub book_id: String,
    pub chapter_number: u32,
    pub intent_path: String,
    pub goal: String,
    pub conflicts: Vec<String>,
}

pub async fn plan(
    State(runtime): State<BooksRuntime>,
    Path(book_id): Path<String>,
    body: Option<Json<PlanBody>>,
) -> impl IntoResponse {
    let Json(body) = body.unwrap_or_default();
    match run_plan(&runtime, &book_id, body.context.as_deref()).await {
        Ok(result) => (StatusCode::OK, Json(serde_json::to_value(&result).unwrap_or_default())),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error.to_string() })),
        ),
    }
}

async fn run_plan(
    runtime: &BooksRuntime,
    book_id: &str,
    context: Option<&str>,
) -> Result<PlanChapterResult, crate::pipeline::write_next::WriteNextError> {
    runtime.state.ensure_control_documents(book_id, None).await?;
    let book = runtime.state.load_book_config(book_id).await?;
    let book_dir = runtime.state.book_dir(book_id);
    let chapter_number = runtime.state.get_next_chapter_number(book_id).await?;

    let planner: &'static RoutedAgent =
        Box::leak(Box::new(RoutedAgent { router: (*runtime.router).clone(), agent: "planner" }));
    let plan = crate::agents::planner::plan_chapter(
        planner,
        &crate::agents::planner::PlanChapterInput {
            book_language: book.language.as_deref().unwrap_or("zh"),
            book_dir: &book_dir,
            chapter_number,
            external_context: context,
        },
    )
    .await?;

    // 持久化（对齐 runner resolveGovernedPlan：plan.md 供后续 compose/write 复用）。
    let _ = crate::pipeline::persisted_governed_plan::save_persisted_plan(&book_dir, &plan).await;

    Ok(PlanChapterResult {
        book_id: book_id.to_string(),
        chapter_number,
        intent_path: relative_to_book_dir(&book_dir, &plan.runtime_path),
        goal: plan.intent.goal.clone(),
        conflicts: Vec::new(),
    })
}

// ── POST /api/v1/books/:id/settle（Rust 侧补齐端点）──────────────

#[derive(Debug, Deserialize)]
pub struct SettleBody {
    pub chapter: u32,
    pub title: String,
    pub content: String,
    #[serde(default)]
    pub allow_reapply: Option<bool>,
}

/// settle 落盘摘要（标题 + 字数 + 真相文件确认）。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettleResult {
    pub chapter_number: u32,
    pub title: String,
    pub word_count: u32,
    pub updated_state: String,
    pub updated_hooks: String,
}

pub async fn settle(
    State(runtime): State<BooksRuntime>,
    Path(book_id): Path<String>,
    Json(body): Json<SettleBody>,
) -> impl IntoResponse {
    match run_settle(&runtime, &book_id, &body).await {
        Ok(result) => (StatusCode::OK, Json(serde_json::to_value(&result).unwrap_or_default())),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error })),
        ),
    }
}

async fn run_settle(runtime: &BooksRuntime, book_id: &str, body: &SettleBody) -> Result<SettleResult, String> {
    let book = runtime
        .state
        .load_book_config(book_id)
        .await
        .map_err(|e| e.to_string())?;
    let book_dir = runtime.state.book_dir(book_id);

    let writer: &'static RoutedAgent =
        Box::leak(Box::new(RoutedAgent { router: (*runtime.router).clone(), agent: "writer" }));
    let ctx: &'static crate::agents::writer::WriterCtx =
        Box::leak(Box::new(crate::agents::writer::WriterCtx {
            project_root: Box::leak(runtime.state.project_root().to_path_buf().into_boxed_path()),
            builtin_genres_dir: Box::leak(runtime.builtin_genres_dir.clone().into_boxed_path()),
            prompt_store: Box::leak(Box::new(FsStateStore)),
            state_store: Box::leak(Box::new(FsStateStore)),
        }));
    let output = crate::agents::writer::settle_chapter_state(
        ctx,
        writer,
        &crate::agents::writer::SettleChapterStateInput {
            book: &book,
            book_dir: &book_dir,
            chapter_number: body.chapter,
            title: &body.title,
            content: &body.content,
            allow_reapply: body.allow_reapply,
            chapter_intent: None,
            context_package: None,
            rule_stack: None,
            validation_feedback: None,
        },
    )
    .await
    .map_err(|e| e.to_string())?;

    Ok(SettleResult {
        chapter_number: body.chapter,
        title: body.title.clone(),
        word_count: output.word_count,
        updated_state: output.updated_state,
        updated_hooks: output.updated_hooks,
    })
}

// ── POST /api/v1/books/:id/draft（fire-and-forget）──────────────

#[derive(Debug, Default, Deserialize)]
pub struct DraftBody {
    #[serde(rename = "wordCount", default)]
    pub word_count: Option<u32>,
    #[serde(default)]
    pub context: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DraftResponse {
    pub status: &'static str,
    #[serde(rename = "bookId")]
    pub book_id: String,
}

pub async fn draft(
    State(runtime): State<BooksRuntime>,
    Path(book_id): Path<String>,
    body: Option<Json<DraftBody>>,
) -> impl IntoResponse {
    let Json(body) = body.unwrap_or_default();
    runtime.hub.broadcast("draft:start", &serde_json::json!({ "bookId": book_id }));

    let task_runtime = runtime.clone();
    let task_book_id = book_id.clone();
    tokio::spawn(async move {
        // draft 语义 = 写完即停（manual 审核模式），结算照走。
        let result = run_draft(&task_runtime, &task_book_id, &body).await;
        match result {
            Ok(outcome) => {
                task_runtime.hub.broadcast(
                    "draft:complete",
                    &serde_json::json!({
                        "bookId": task_book_id,
                        "chapterNumber": outcome.chapter_number,
                        "title": outcome.title,
                        "wordCount": outcome.word_count,
                    }),
                );
            }
            Err(error) => {
                let message = error.to_string();
                task_runtime.hub.broadcast(
                    "draft:error",
                    &serde_json::json!({ "bookId": task_book_id, "error": message }),
                );
            }
        }
    });

    (
        StatusCode::OK,
        Json(DraftResponse { status: "drafting", book_id }),
    )
}

async fn run_draft(
    runtime: &BooksRuntime,
    book_id: &str,
    body: &DraftBody,
) -> Result<crate::pipeline::write_next::ChapterPipelineResult, crate::pipeline::write_next::WriteNextError> {
    let agents = build_write_next_agents(runtime);
    let ctx = build_write_next_ctx(runtime);
    write_next_chapter(
        &runtime.state,
        &agents,
        &ctx,
        &WriteNextConfig {
            chapter_review_mode: ChapterReviewMode::Manual,
            ..Default::default()
        },
        book_id,
        body.word_count,
        None,
        body.context.as_deref(),
    )
    .await
}

// ── POST /api/v1/books/:id/revise/:chapter（同步）────────────────

#[derive(Debug, Default, Deserialize)]
pub struct ReviseBody {
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub brief: Option<String>,
}

pub async fn revise(
    State(runtime): State<BooksRuntime>,
    Path((book_id, chapter)): Path<(String, String)>,
    body: Option<Json<ReviseBody>>,
) -> impl IntoResponse {
    let Json(body) = body.unwrap_or_default();
    let Ok(chapter_number) = chapter.parse::<u32>() else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": format!("invalid chapter number: {chapter}") })),
        );
    };

    runtime.hub.broadcast(
        "revise:start",
        &serde_json::json!({ "bookId": book_id, "chapter": chapter_number }),
    );

    match run_revise_chain(&runtime, &book_id, chapter_number, &body).await {
        Ok(result) => {
            runtime.hub.broadcast(
                "revise:complete",
                &serde_json::json!({ "bookId": book_id, "chapter": chapter_number }),
            );
            (StatusCode::OK, Json(serde_json::to_value(&result).unwrap_or_default()))
        }
        Err(error) => {
            let message = error.to_string();
            runtime.hub.broadcast(
                "revise:error",
                &serde_json::json!({ "bookId": book_id, "error": message }),
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": message })),
            )
        }
    }
}

// ---- 46 号：/revise 审核环（pre-audit → 修稿 → post-audit → 门控 → 落盘） ----

use crate::agents::consolidator::consolidate as run_consolidate;
use crate::agents::state_validator::validate as validate_state;
use crate::llm::agent_router::FullCycleAuditor;
use crate::pipeline::chapter_review_cycle::CycleAuditor as _;
use crate::pipeline::chapter_state_recovery::{
    retry_settlement_after_validation_failure, SettlePort, SettlementRetryParams, SettleRequest,
    ValidatePort,
};

/// 修订链结果（对齐 TS ReviseResult 的核心面）。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviseChainResult {
    pub chapter_number: u32,
    pub word_count: u32,
    pub fixed_issues: Vec<String>,
    pub applied: bool,
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skipped_reason: Option<String>,
    /// 修订后正文（applied 时存在）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revised_content: Option<String>,
}

/// /revise 审核环主链：pre-audit → 修稿 → post-audit → 门控 → 落盘。
async fn run_revise_chain(
    runtime: &BooksRuntime,
    book_id: &str,
    chapter_number: u32,
    body: &ReviseBody,
) -> Result<ReviseChainResult, String> {
    let book = runtime
        .state
        .load_book_config(book_id)
        .await
        .map_err(|e| e.to_string())?;
    let book_dir = runtime.state.book_dir(book_id);

    // 章节正文。
    let chapters_dir = book_dir.join("chapters");
    let padded = format!("{chapter_number:04}");
    let mut entries = tokio::fs::read_dir(&chapters_dir)
        .await
        .map_err(|_| "Chapter not found".to_string())?;
    let mut matched: Option<String> = None;
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with(&padded) && name.ends_with(".md") {
            matched = Some(name);
            break;
        }
    }
    let file_name = matched.ok_or_else(|| "Chapter not found".to_string())?;
    let raw = tokio::fs::read_to_string(chapters_dir.join(&file_name))
        .await
        .map_err(|_| "Chapter not found".to_string())?;
    let content = strip_title_line(&raw);

    let mode = match body.mode.as_deref() {
        Some("polish") => ReviseMode::Polish,
        Some("rewrite") => ReviseMode::Rewrite,
        Some("rework") => ReviseMode::Rework,
        Some("anti-detect") => ReviseMode::AntiDetect,
        _ => ReviseMode::SpotFix,
    };
    let explicit_revision = body.brief.as_deref().map(str::trim).is_some_and(|b| !b.is_empty())
        || mode == ReviseMode::Rewrite
        || mode == ReviseMode::Rework;

    // pre-audit（完整编排）。
    let auditor = FullCycleAuditor {
        router: (*runtime.router).clone(),
        project_root: runtime.state.project_root().to_path_buf(),
        builtin_genres_dir: runtime.builtin_genres_dir.clone(),
        book_dir: book_dir.clone(),
        chapter_number,
        genre: book.genre.clone(),
    };
    let pre = auditor.audit_chapter(&content, None, None).await?;
    let pre_blocking = pre
        .issues
        .iter()
        .filter(|i| i.severity != crate::agents::continuity::AuditSeverity::Info)
        .count();

    // 无问题且无显式修订请求 → unchanged（Node 语义）。
    if pre_blocking == 0 && !explicit_revision {
        let language = match book.language.as_deref() {
            Some("en") => crate::utils::language::WritingLanguage::En,
            _ => crate::utils::language::WritingLanguage::Zh,
        };
        return Ok(ReviseChainResult {
            chapter_number,
            word_count: crate::utils::length_metrics::count_chapter_length(
                &content,
                crate::utils::length_metrics::resolve_length_counting_mode(language),
            ),
            fixed_issues: Vec::new(),
            applied: false,
            status: "unchanged",
            skipped_reason: Some("No warning, critical, or AI-tell issues to fix.".to_string()),
            revised_content: None,
        });
    }

    // 修稿（以 pre 审计问题驱动）。
    let reviser: &'static RoutedAgent =
        Box::leak(Box::new(RoutedAgent { router: (*runtime.router).clone(), agent: "reviser" }));
    let reviser_ctx: &'static crate::agents::reviser::ReviserCtx =
        Box::leak(Box::new(crate::agents::reviser::ReviserCtx {
            project_root: Box::leak(runtime.state.project_root().to_path_buf().into_boxed_path()),
            builtin_genres_dir: Box::leak(runtime.builtin_genres_dir.clone().into_boxed_path()),
            prompt_store: Box::leak(Box::new(FsStateStore)),
        }));
    let revise_output = revise_chapter(
        reviser,
        reviser_ctx,
        &book_dir,
        &content,
        chapter_number,
        &pre.issues,
        mode,
        Some(&book.genre),
        &ReviseOptions {
            chapter_intent: None,
            chapter_memo: None,
            chapter_intent_data: None,
            context_package: None,
            rule_stack: None,
            length_spec: None,
        },
    )
    .await
    .map_err(|e| e.to_string())?;
    if revise_output.revised_content.is_empty() {
        return Err("Reviser returned empty content".to_string());
    }

    // post-audit（temp 0）——修订稿评估。
    let post = auditor
        .audit_chapter(&revise_output.revised_content, None, Some(0.0))
        .await?;
    let post_blocking = post
        .issues
        .iter()
        .filter(|i| i.severity != crate::agents::continuity::AuditSeverity::Info)
        .count();

    // strict 门控：不变差 && （blocking 或问题数改善）。
    let improved = post_blocking < pre_blocking || post.issues.len() < pre.issues.len();
    let did_not_worsen = post_blocking <= pre_blocking;
    if !(did_not_worsen && improved) {
        return Ok(ReviseChainResult {
            chapter_number,
            word_count: crate::utils::length_metrics::count_chapter_length(
                &content,
                crate::utils::length_metrics::resolve_length_counting_mode(
                    crate::utils::language::WritingLanguage::Zh,
                ),
            ),
            fixed_issues: Vec::new(),
            applied: false,
            status: "unchanged",
            skipped_reason: Some(format!(
                "Manual revision kept original chapter: before blocking={pre_blocking}; after blocking={post_blocking}."
            )),
            revised_content: None,
        });
    }

    // 落盘：章节文件（标题保留）+ 最新章真相回写。
    let title_line = raw.lines().next().unwrap_or("").to_string();
    let revised_full = format!("{title_line}\n\n{}", revise_output.revised_content);
    tokio::fs::write(chapters_dir.join(&file_name), &revised_full)
        .await
        .map_err(|e| e.to_string())?;

    // 仅最新章拥有当前真相（Node 语义）。
    let index = runtime
        .state
        .load_chapter_index(book_id)
        .await
        .map_err(|e| e.to_string())?;
    let latest = index.iter().map(|m| m.number).max().unwrap_or(chapter_number);
    if chapter_number == latest {
        let story_dir = book_dir.join("story");
        if revise_output.updated_state != "(状态卡未更新)" {
            let _ = tokio::fs::write(story_dir.join("current_state.md"), &revise_output.updated_state).await;
        }
        if revise_output.updated_hooks != "(伏笔池未更新)" {
            let _ = tokio::fs::write(story_dir.join("pending_hooks.md"), &revise_output.updated_hooks).await;
        }
    }

    let language = match book.language.as_deref() {
        Some("en") => crate::utils::language::WritingLanguage::En,
        _ => crate::utils::language::WritingLanguage::Zh,
    };
    Ok(ReviseChainResult {
        chapter_number,
        word_count: crate::utils::length_metrics::count_chapter_length(
            &revise_output.revised_content,
            crate::utils::length_metrics::resolve_length_counting_mode(language),
        ),
        fixed_issues: revise_output.fixed_issues,
        applied: true,
        status: "revised",
        skipped_reason: None,
        revised_content: Some(revise_output.revised_content),
    })
}

fn strip_title_line(raw: &str) -> String {
    // TS readChapterContent：跳过标题行取首个非空行起。
    let lines: Vec<&str> = raw.split('\n').collect();
    if lines.len() < 2 {
        return raw.trim().to_string();
    }
    let content_start = lines[1..]
        .iter()
        .position(|l| !l.trim().is_empty())
        .map(|i| i + 1)
        .unwrap_or(1);
    lines[content_start..].join("\n").trim().to_string()
}

// ── POST /api/v1/books/:id/compose ──────────────────────────────

#[derive(Debug, Default, Deserialize)]
pub struct ComposeBody {
    #[serde(default)]
    pub context: Option<String>,
}

/// TS `ComposeChapterResult` 形状。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComposeChapterResult {
    pub book_id: String,
    pub chapter_number: u32,
    pub intent_path: String,
    pub goal: String,
    pub conflicts: Vec<String>,
    pub context_path: String,
    pub rule_stack_path: String,
    pub trace_path: String,
}

pub async fn compose(
    State(runtime): State<BooksRuntime>,
    Path(book_id): Path<String>,
    body: Option<Json<ComposeBody>>,
) -> impl IntoResponse {
    let Json(body) = body.unwrap_or_default();
    match run_compose(&runtime, &book_id, body.context.as_deref()).await {
        Ok(result) => (StatusCode::OK, Json(serde_json::to_value(&result).unwrap_or_default())),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error })),
        ),
    }
}

async fn run_compose(
    runtime: &BooksRuntime,
    book_id: &str,
    context: Option<&str>,
) -> Result<ComposeChapterResult, String> {
    runtime
        .state
        .ensure_control_documents(book_id, None)
        .await
        .map_err(|e| e.to_string())?;
    let book = runtime
        .state
        .load_book_config(book_id)
        .await
        .map_err(|e| e.to_string())?;
    let book_dir = runtime.state.book_dir(book_id);
    let chapter_number = runtime
        .state
        .get_next_chapter_number(book_id)
        .await
        .map_err(|e| e.to_string())?;

    // plan（持久化复用：无新上下文跳过 planner LLM）。
    let plan = match crate::pipeline::persisted_governed_plan::load_persisted_plan(
        &book_dir,
        chapter_number,
    )
    .await
    {
        Some(plan) if context.map(str::trim).unwrap_or("").is_empty() => plan,
        _ => {
            let planner: &'static RoutedAgent = Box::leak(Box::new(RoutedAgent {
                router: (*runtime.router).clone(),
                agent: "planner",
            }));
            let plan = crate::agents::planner::plan_chapter(
                planner,
                &crate::agents::planner::PlanChapterInput {
                    book_language: book.language.as_deref().unwrap_or("zh"),
                    book_dir: &book_dir,
                    chapter_number,
                    external_context: context,
                },
            )
            .await
            .map_err(|e| e.to_string())?;
            crate::pipeline::persisted_governed_plan::save_persisted_plan(&book_dir, &plan)
                .await
                .map_err(|e| e.to_string())?;
            plan
        }
    };

    // compose（35 号编排；outline 选段走 LLM 端口）。
    let composer: &'static RoutedAgent = Box::leak(Box::new(RoutedAgent {
        router: (*runtime.router).clone(),
        agent: "composer",
    }));
    let selector = crate::agents::composer::LlmOutlineSelector { chat: composer };
    let compiler = crate::agents::composer::LlmContextCompiler { chat: composer };
    let composed = crate::agents::composer::compose_governed_chapter(
        &crate::agents::composer::ComposeChapterInput {
            book_language: book.language.as_deref(),
            book_dir: &book_dir,
            chapter_number,
            plan: &plan,
            context_budget: None,
            compiler: Some(&compiler),
            outline_section_selector: Some(&selector),
            on_context_compression: None,
        },
    )
    .await
    .map_err(|e| e.to_string())?;

    Ok(ComposeChapterResult {
        book_id: book_id.to_string(),
        chapter_number,
        intent_path: relative_to_book_dir(&book_dir, &plan.runtime_path),
        goal: plan.intent.goal.clone(),
        conflicts: Vec::new(),
        context_path: relative_to_book_dir(&book_dir, &composed.context_path),
        rule_stack_path: relative_to_book_dir(&book_dir, &composed.rule_stack_path),
        trace_path: relative_to_book_dir(&book_dir, &composed.trace_path),
    })
}

// ── POST /api/v1/books/:id/consolidate ──────────────────────────

pub async fn consolidate_endpoint(
    State(runtime): State<BooksRuntime>,
    Path(book_id): Path<String>,
) -> impl IntoResponse {
    let book_dir = runtime.state.book_dir(&book_id.clone());
    let consolidator: &'static RoutedAgent = Box::leak(Box::new(RoutedAgent {
        router: (*runtime.router).clone(),
        agent: "consolidator",
    }));
    match run_consolidate(consolidator, &book_dir).await {
        Ok(result) => {
            runtime.hub.broadcast(
                "consolidate:complete",
                &serde_json::json!({ "bookId": book_id, "archivedVolumes": result.archived_volumes }),
            );
            (StatusCode::OK, Json(serde_json::to_value(&result).unwrap_or_default()))
        }
        Err(error) => {
            let message = error.to_string();
            runtime.hub.broadcast(
                "consolidate:error",
                &serde_json::json!({ "bookId": book_id, "error": message }),
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": message })),
            )
        }
    }
}

// ── POST /api/v1/books/:id/repair-state/:chapter ────────────────

/// validator 端口适配（RoutedAgent → ValidatePort）。
struct RoutedValidator {
    router: Arc<AgentRouter>,
}

#[async_trait]
impl ValidatePort for RoutedValidator {
    async fn validate(
        &self,
        params: crate::pipeline::chapter_state_recovery::ValidateRequest<'_>,
    ) -> Result<crate::agents::state_validator::ValidationResult, String> {
        let chat = RoutedAgent {
            router: (*self.router).clone(),
            agent: "state-validator",
        };
        validate_state(
            &chat,
            &crate::agents::state_validator::ValidateParams {
                chapter_content: params.content,
                chapter_number: params.chapter_number,
                old_state: params.old_state,
                new_state: params.new_state,
                old_hooks: params.old_hooks,
                new_hooks: params.new_hooks,
                language: params.language,
                authority_context: params.authority_context,
            },
        )
        .await
        .map_err(|e| e.to_string())
    }
}

/// settle 端口（真实 writer.settleChapterState）。
struct RepairSettle {
    router: Arc<AgentRouter>,
    project_root: std::path::PathBuf,
    builtin_genres_dir: std::path::PathBuf,
    chapter_number: u32,
}

#[async_trait]
impl crate::pipeline::chapter_state_recovery::SettlePort for RepairSettle {
    async fn settle(&self, params: SettleRequest<'_>) -> Result<crate::agents::writer::WriteChapterOutput, String> {
        let writer = RoutedAgent {
            router: (*self.router).clone(),
            agent: "writer",
        };
        let ctx: &'static crate::agents::writer::WriterCtx =
            Box::leak(Box::new(crate::agents::writer::WriterCtx {
                project_root: Box::leak(self.project_root.clone().into_boxed_path()),
                builtin_genres_dir: Box::leak(self.builtin_genres_dir.clone().into_boxed_path()),
                prompt_store: Box::leak(Box::new(FsStateStore)),
                state_store: Box::leak(Box::new(FsStateStore)),
            }));
        crate::agents::writer::settle_chapter_state(
            ctx,
            &writer,
            &crate::agents::writer::SettleChapterStateInput {
                book: params.book,
                book_dir: params.book_dir,
                chapter_number: self.chapter_number,
                title: params.title,
                content: params.content,
                allow_reapply: Some(params.allow_reapply),
                chapter_intent: params.chapter_intent,
                context_package: params.context_package,
                rule_stack: params.rule_stack,
                validation_feedback: params.validation_feedback,
            },
        )
        .await
        .map_err(|e| e.to_string())
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepairStateResult {
    pub chapter_number: u32,
    pub title: String,
    pub status: &'static str,
    pub passed: bool,
    pub summary: String,
}

pub async fn repair_state(
    State(runtime): State<BooksRuntime>,
    Path((book_id, chapter)): Path<(String, String)>,
) -> impl IntoResponse {
    let Ok(chapter_number) = chapter.parse::<u32>() else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": format!("invalid chapter number: {chapter}") })),
        );
    };
    match run_repair_state(&runtime, &book_id, chapter_number).await {
        Ok(result) => {
            runtime.hub.broadcast(
                "repair-state:complete",
                &serde_json::json!({ "bookId": book_id, "chapter": chapter_number }),
            );
            (StatusCode::OK, Json(serde_json::to_value(&result).unwrap_or_default()))
        }
        Err(error) => {
            let message = error.to_string();
            runtime.hub.broadcast(
                "repair-state:error",
                &serde_json::json!({ "bookId": book_id, "chapter": chapter_number, "error": message }),
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": message })),
            )
        }
    }
}

async fn run_repair_state(
    runtime: &BooksRuntime,
    book_id: &str,
    chapter_number: u32,
) -> Result<RepairStateResult, String> {
    let book = runtime
        .state
        .load_book_config(book_id)
        .await
        .map_err(|e| e.to_string())?;
    let book_dir = runtime.state.book_dir(book_id);
    let index = runtime
        .state
        .load_chapter_index(book_id)
        .await
        .map_err(|e| e.to_string())?;
    if index.is_empty() {
        return Err(format!("Book \"{book_id}\" has no persisted chapters to repair."));
    }
    let target = chapter_number;
    let Some(target_meta) = index.iter().find(|m| m.number == target) else {
        return Err(format!("Chapter {target} not found in \"{book_id}\"."));
    };
    let latest = index.iter().map(|m| m.number).max().unwrap_or(target);
    if target_meta.status != crate::models::chapter::ChapterStatus::StateDegraded {
        return Err(format!("Chapter {target} is not state-degraded."));
    }
    if target != latest {
        return Err(format!(
            "Only the latest state-degraded chapter can be repaired safely (latest is {latest})."
        ));
    }

    // 章节正文。
    let chapters_dir = book_dir.join("chapters");
    let padded = format!("{target:04}");
    let mut entries = tokio::fs::read_dir(&chapters_dir)
        .await
        .map_err(|e| e.to_string())?;
    let mut matched: Option<String> = None;
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with(&padded) && name.ends_with(".md") {
            matched = Some(name);
            break;
        }
    }
    let file_name = matched.ok_or_else(|| "Chapter not found".to_string())?;
    let raw = tokio::fs::read_to_string(chapters_dir.join(file_name))
        .await
        .map_err(|e| e.to_string())?;
    let content = strip_title_line(&raw);

    let story_dir = book_dir.join("story");
    let old_state = tokio::fs::read_to_string(story_dir.join("current_state.md"))
        .await
        .unwrap_or_default();
    let old_hooks = tokio::fs::read_to_string(story_dir.join("pending_hooks.md"))
        .await
        .unwrap_or_default();

    let language = match book.language.as_deref() {
        Some("en") => crate::utils::language::WritingLanguage::En,
        _ => crate::utils::language::WritingLanguage::Zh,
    };

    let settler = RepairSettle {
        router: runtime.router.clone(),
        project_root: runtime.state.project_root().to_path_buf(),
        builtin_genres_dir: runtime.builtin_genres_dir.clone(),
        chapter_number: target,
    };
    let validator = RoutedValidator { router: runtime.router.clone() };

    // settle → validate → 失败重试链。
    let repaired = settler
        .settle(SettleRequest {
            book: &book,
            book_dir: &book_dir,
            title: &target_meta.title,
            content: &content,
            allow_reapply: true,
            chapter_intent: None,
            context_package: None,
            rule_stack: None,
            validation_feedback: None,
        })
        .await
        .map_err(|e| e.to_string())?;
    let validation = validator
        .validate(crate::pipeline::chapter_state_recovery::ValidateRequest {
            content: &content,
            chapter_number: target,
            old_state: &old_state,
            new_state: &repaired.updated_state,
            old_hooks: &old_hooks,
            new_hooks: &repaired.updated_hooks,
            language,
            authority_context: None,
        })
        .await
        .map_err(|e| e.to_string())?;

    let (repaired, validation) = if !validation.passed {
        let recovery = retry_settlement_after_validation_failure(SettlementRetryParams {
            writer: &settler,
            validator: &validator,
            book: &book,
            book_dir: &book_dir,
            chapter_number: target,
            title: &target_meta.title,
            content: &content,
            control: None,
            old_state: &old_state,
            old_hooks: &old_hooks,
            original_validation: &validation,
            language,
            log_warn: &|zh, en| tracing::warn!(target: "repair-state", "{zh} / {en}"),
        })
        .await
        .map_err(|e| e.to_string())?;
        match recovery {
            crate::pipeline::chapter_state_recovery::SettlementRetryResult::Recovered { output, validation } => (*output, validation),
            crate::pipeline::chapter_state_recovery::SettlementRetryResult::Degraded { issues } => {
                return Err(issues
                    .first()
                    .map(|i| i.description.clone())
                    .unwrap_or_else(|| format!("State repair still failed for chapter {target}.")));
            }
        }
    } else {
        (repaired, validation)
    };
    if !validation.passed {
        return Err(format!("State repair still failed for chapter {target}."));
    }

    // 真相回写 + 快照。
    if repaired.updated_state != "(状态卡未更新)" {
        let _ = tokio::fs::write(story_dir.join("current_state.md"), &repaired.updated_state).await;
    }
    if repaired.updated_hooks != "(伏笔池未更新)" {
        let _ = tokio::fs::write(story_dir.join("pending_hooks.md"), &repaired.updated_hooks).await;
    }
    let _ = runtime.state.snapshot_state(book_id, target).await;

    // 索引状态回翻（降级 → 基础状态，注入问题清除）。
    let base_status = crate::pipeline::chapter_state_recovery::resolve_state_degraded_base_status(target_meta);
    let injected: std::collections::HashSet<String> =
        crate::pipeline::chapter_state_recovery::parse_state_degraded_review_note(
            target_meta.review_note.as_deref(),
        )
        .map(|note| note.injected_issues.into_iter().collect())
        .unwrap_or_default();
    let mut updated_index = index.clone();
    if let Some(slot) = updated_index.iter_mut().find(|m| m.number == target) {
        slot.status = match base_status {
            "audit-failed" => crate::models::chapter::ChapterStatus::AuditFailed,
            _ => crate::models::chapter::ChapterStatus::ReadyForReview,
        };
        slot.updated_at = crate::utils::utc_time::utc_now_iso();
        slot.audit_issues = target_meta
            .audit_issues
            .iter()
            .filter(|issue| !injected.contains(*issue))
            .cloned()
            .collect();
        slot.review_note = None;
    }
    runtime
        .state
        .save_chapter_index(book_id, &updated_index)
        .await
        .map_err(|e| e.to_string())?;

    let status: &'static str = if base_status == "audit-failed" { "audit-failed" } else { "ready-for-review" };
    Ok(RepairStateResult {
        chapter_number: target,
        title: target_meta.title.clone(),
        status,
        passed: status != "audit-failed",
        summary: if status != "audit-failed" {
            "state repaired".to_string()
        } else {
            "state repaired but chapter still needs review".to_string()
        },
    })
}

// ── 共享装配 ─────────────────────────────────────────────────────

fn build_write_next_agents(runtime: &BooksRuntime) -> WriteNextAgents<'static> {
    let leak = |agent: &'static str| -> &'static RoutedAgent {
        Box::leak(Box::new(RoutedAgent { router: (*runtime.router).clone(), agent }))
    };
    let settler: &'static crate::llm::agent_router::RoutedSettler =
        Box::leak(Box::new(crate::llm::agent_router::RoutedSettler {
            router: (*runtime.router).clone(),
            ctx: crate::agents::writer::WriterCtx {
                project_root: Box::leak(runtime.state.project_root().to_path_buf().into_boxed_path()),
                builtin_genres_dir: Box::leak(runtime.builtin_genres_dir.clone().into_boxed_path()),
                prompt_store: Box::leak(Box::new(FsStateStore)),
                state_store: Box::leak(Box::new(FsStateStore)),
            },
            chapter_number: 0,
        }));
    WriteNextAgents {
        writer: leak("writer"),
        planner: leak("planner"),
        composer: leak("composer"),
        reviser: leak("reviser"),
        auditor: leak("auditor"),
        full_auditor: None,
        normalizer: leak("length-normalizer"),
        analyzer: leak("chapter-analyzer"),
        state_validator: leak("state-validator"),
        settler,
    }
}

fn build_write_next_ctx(runtime: &BooksRuntime) -> WriteNextCtx<'static> {
    let prompt_store: &'static FsStateStore = Box::leak(Box::new(FsStateStore));
    WriteNextCtx {
        project_root: Box::leak(runtime.state.project_root().to_path_buf().into_boxed_path()),
        builtin_genres_dir: Box::leak(runtime.builtin_genres_dir.clone().into_boxed_path()),
        prompt_store,
        state_store: prompt_store,
        context_budget: None,
        notify: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::agent_router::LlmEndpointConfig;
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
        }
    }

    fn fixture(root: &std::path::Path) {
        let book = root.join("books").join("b1");
        std::fs::create_dir_all(book.join("chapters")).unwrap();
        std::fs::write(
            book.join("book.json"),
            r#"{"id":"b1","title":"t","platform":"other","genre":"other","status":"active","targetChapters":10,"chapterWordCount":3000,"language":"zh","createdAt":"","updatedAt":""}"#,
        )
        .unwrap();
        std::fs::write(
            book.join("chapters").join("0001_风起.md"),
            "# 第1章 风起\n\n林动睁开双眼，灵气顺着经脉游走。",
        )
        .unwrap();
    }

    fn app(runtime: BooksRuntime) -> axum::Router {
        axum::Router::new()
            .route("/api/v1/books/:id/plan", axum::routing::post(plan))
            .route("/api/v1/books/:id/settle", axum::routing::post(settle))
            .route("/api/v1/books/:id/draft", axum::routing::post(draft))
            .route("/api/v1/books/:id/revise/:chapter", axum::routing::post(revise))
            .route("/api/v1/books/:id/repair-state/:chapter", axum::routing::post(repair_state))
            .with_state(runtime)
    }

    fn post(uri: &str, body: &str) -> axum::http::Request<axum::body::Body> {
        axum::http::Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json")
            .body(axum::body::Body::from(body.to_string()))
            .unwrap()
    }

    #[tokio::test]
    async fn plan_unreachable_llm_returns_500() {
        let dir = tempfile::tempdir().unwrap();
        fixture(dir.path());
        let runtime = runtime_for(dir.path());
        let response = app(runtime)
            .oneshot(post("/api/v1/books/b1/plan", "{}"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = axum::body::to_bytes(response.into_body(), 4096).await.unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(parsed["error"].as_str().unwrap().contains("chat"));
    }

    #[tokio::test]
    async fn draft_returns_drafting_immediately_and_errors_via_sse() {
        let dir = tempfile::tempdir().unwrap();
        fixture(dir.path());
        let runtime = runtime_for(dir.path());
        let hub = runtime.hub.clone();
        let mut subscriber = hub.subscribe();
        let response = app(runtime)
            .oneshot(post("/api/v1/books/b1/draft", "{}"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 4096).await.unwrap().to_vec();
        #[derive(Deserialize)]
        struct TestDto {
            status: String,
            #[serde(rename = "bookId")]
            book_id: String,
        }
        let parsed: TestDto = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed.status, "drafting");
        assert_eq!(parsed.book_id, "b1");

        assert_eq!(subscriber.recv().await.unwrap().event, "draft:start");
        let second = tokio::time::timeout(std::time::Duration::from_secs(10), subscriber.recv())
            .await
            .expect("draft:error 应到达")
            .unwrap();
        assert_eq!(second.event, "draft:error");
    }

    #[tokio::test]
    async fn settle_missing_book_returns_500() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = runtime_for(dir.path());
        let response = app(runtime)
            .oneshot(post(
                "/api/v1/books/missing/settle",
                r#"{"chapter":1,"title":"t","content":"c"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn revise_missing_chapter_returns_500_with_node_message() {
        let dir = tempfile::tempdir().unwrap();
        fixture(dir.path());
        let runtime = runtime_for(dir.path());
        let response = app(runtime)
            .oneshot(post("/api/v1/books/b1/revise/9", "{}"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = axum::body::to_bytes(response.into_body(), 4096).await.unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed["error"], "Chapter not found");
    }

    #[tokio::test]
    async fn revise_invalid_chapter_returns_400() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = runtime_for(dir.path());
        let response = app(runtime)
            .oneshot(post("/api/v1/books/b1/revise/abc", "{}"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn repair_state_requires_degraded_chapter() {
        let dir = tempfile::tempdir().unwrap();
        fixture(dir.path());
        let runtime = runtime_for(dir.path());
        let response = app(runtime)
            .oneshot(post("/api/v1/books/b1/repair-state/1", "{}"))
            .await
            .unwrap();
        // 索引无该章（fixture 不建索引）→ not found 类 500。
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
