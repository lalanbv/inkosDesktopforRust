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

use crate::agents::continuity::{AuditChapterOptions, TruthFileOverrides};
use crate::agents::reviser::{revise_chapter, ReviseMode, ReviseOptions};
use crate::llm::agent_router::{AgentRouter, RoutedAgent};
use crate::pipeline::merged_audit::{
    evaluate_merged_audit, restore_actionable_audit_if_lost, MergedAuditEvaluation,
    RevisionGate,
};
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
    /// 修订门控（TS config.revisionGate，默认 strict；bin 经 INKOS_REVISION_GATE 注入）。
    pub revision_gate: RevisionGate,
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

    match run_revise_chain(&runtime, &book_id, chapter_number, &body, runtime.revision_gate).await {
        Ok(result) => {
            runtime.hub.broadcast(
                "revise:complete",
                &serde_json::json!({ "bookId": book_id, "chapter": chapter_number }),
            );
            (StatusCode::OK, Json(serde_json::to_value(&result).unwrap_or_default()))
        }
        Err(error) => {
            let message = error.message().to_string();
            let status = if error.is_not_found() { StatusCode::NOT_FOUND } else { StatusCode::INTERNAL_SERVER_ERROR };
            runtime.hub.broadcast(
                "revise:error",
                &serde_json::json!({ "bookId": book_id, "error": message }),
            );
            (status, Json(serde_json::json!({ "error": message })))
        }
    }
}

// ── POST /api/v1/books/:id/rewrite/:chapter（51 号） ──────────────

/// 定点重写：reviseDraft 的 rework 变体——`revisionGate=always`（修稿结果
/// 恒采纳，不因变差拒绝），不回滚下游章（下游仅标 needs-revision 提示重审，
/// 与 revise 链一致）。brief 有键则先落盘 user-brief（server.ts L5994）。
pub async fn rewrite(
    State(runtime): State<BooksRuntime>,
    Path((book_id, chapter)): Path<(String, String)>,
    body: Option<Json<serde_json::Value>>,
) -> impl IntoResponse {
    let parsed = body.map(|Json(value)| value).unwrap_or_else(|| serde_json::json!({}));
    let broadcast_error = |runtime: &BooksRuntime, book_id: &str, message: String| {
        runtime
            .hub
            .broadcast("rewrite:error", &serde_json::json!({ "bookId": book_id, "error": message }));
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": message })),
        )
    };
    // TS parseInt：NaN → 链内 find 无命中；负数/0 → reviseDraft 前置守卫。
    // rewrite 端点统一 500（server.ts L6019 catch 无 404 分支）。
    let chapter_number = match chapter.parse::<i64>() {
        Ok(number) if number >= 1 => number as u32,
        Ok(_) => {
            let message = format!("No chapters to revise for \"{book_id}\"");
            return broadcast_error(&runtime, &book_id, message);
        }
        Err(_) => {
            let message = "Chapter NaN not found in index".to_string();
            return broadcast_error(&runtime, &book_id, message);
        }
    };
    runtime.hub.broadcast(
        "rewrite:start",
        &serde_json::json!({ "bookId": book_id, "chapter": chapter_number }),
    );

    // hasOwnProperty("brief") → saveChapterUserBrief(brief ?? "")：
    // 字符串落盘（trim；空串删文件）、null 同空串、其他类型按 TS TypeError → 500。
    if let Some(brief) = parsed.get("brief") {
        let value = match brief {
            serde_json::Value::String(s) => s.as_str(),
            serde_json::Value::Null => "",
            _ => {
                let message = "brief must be a string".to_string();
                return broadcast_error(&runtime, &book_id, message);
            }
        };
        let book_dir = runtime.state.book_dir(&book_id).to_string_lossy().into_owned();
        if let Err(error) =
            crate::state::chapter_workspace::save_chapter_user_brief(&FsStateStore, &book_dir, chapter_number, value)
                .await
        {
            return broadcast_error(&runtime, &book_id, error.to_string());
        }
    }

    let chain_body = ReviseBody {
        mode: Some("rework".to_string()),
        brief: parsed.get("brief").and_then(serde_json::Value::as_str).map(str::to_string),
    };
    match run_revise_chain(&runtime, &book_id, chapter_number, &chain_body, RevisionGate::Always).await {
        Ok(result) => {
            runtime.hub.broadcast(
                "rewrite:complete",
                &serde_json::json!({
                    "bookId": book_id,
                    "chapterNumber": result.chapter_number,
                    "wordCount": result.word_count,
                    "status": result.status,
                }),
            );
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "status": "complete",
                    "bookId": book_id,
                    "chapter": chapter_number,
                    "result": result,
                })),
            )
        }
        Err(error) => {
            let message = error.message().to_string();
            broadcast_error(&runtime, &book_id, message)
        }
    }
}

// ---- 47 号：/revise 审核环全量（merged audit + revisionGate 三档 + 索引回写） ----

use crate::agents::consolidator::consolidate as run_consolidate;
use crate::agents::state_validator::validate as validate_state;
use crate::llm::agent_router::FullCycleAuditor;
use crate::pipeline::chapter_state_recovery::{
    retry_settlement_after_validation_failure, SettlePort, SettlementRetryParams, SettleRequest,
    ValidatePort,
};

/// revise 链错误：NotFound 对齐 Node 404（缺章），Internal 对齐 500。
/// 88 号提 pub(crate)：sub_agent reviser 聊天面复用链错误文本。
pub(crate) enum ReviseChainError {
    NotFound(String),
    Internal(String),
}

impl ReviseChainError {
    pub(crate) fn is_not_found(&self) -> bool {
        matches!(self, ReviseChainError::NotFound(_))
    }

    pub(crate) fn message(&self) -> &str {
        match self {
            ReviseChainError::NotFound(m) | ReviseChainError::Internal(m) => m,
        }
    }
}

impl From<String> for ReviseChainError {
    fn from(message: String) -> Self {
        ReviseChainError::Internal(message)
    }
}

/// 门控计数（revisionDiagnostics.before/after）。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GateCounts {
    pub blocking_count: usize,
    pub critical_count: usize,
    pub ai_tell_count: usize,
}

fn gate_counts(eval: &MergedAuditEvaluation) -> GateCounts {
    GateCounts {
        blocking_count: eval.blocking_count,
        critical_count: eval.critical_count,
        ai_tell_count: eval.ai_tell_count,
    }
}

/// 修订拒绝时的剩余问题（前 6 条 warning/critical）。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemainingIssue {
    pub severity: String,
    pub category: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
}

fn severity_string(severity: crate::agents::continuity::AuditSeverity) -> String {
    match severity {
        crate::agents::continuity::AuditSeverity::Critical => "critical".to_string(),
        crate::agents::continuity::AuditSeverity::Warning => "warning".to_string(),
        crate::agents::continuity::AuditSeverity::Info => "info".to_string(),
    }
}

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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision_diagnostics: Option<RevisionDiagnostics>,
    /// 修订后正文（applied 时存在）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revised_content: Option<String>,
}

/// 门控诊断（TS reviseDraft 拒绝分支的 revisionDiagnostics）。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RevisionDiagnostics {
    pub standard: &'static str,
    pub before: GateCounts,
    pub after: GateCounts,
    pub remaining_issues: Vec<RemainingIssue>,
}

/// /revise 审核环主链：pre merged-audit → 修稿 → post merged-audit（temp 0 +
/// 修稿真相覆盖）→ restore → revisionGate 三档门控 → 落盘 + 索引回写。
/// `gate` 由端点注入（revise 用 runtime 配置；rewrite 强制 Always）。
/// 88 号提 pub(crate)：sub_agent reviser 聊天面复用（gate 用 runtime 配置，
/// 对齐 TS sub_agent → pipeline.reviseDraft 的 config.revisionGate ?? strict）。
pub(crate) async fn run_revise_chain(
    runtime: &BooksRuntime,
    book_id: &str,
    chapter_number: u32,
    body: &ReviseBody,
    gate: RevisionGate,
) -> Result<ReviseChainResult, ReviseChainError> {
    let internal = |e: String| ReviseChainError::Internal(e);
    let book = runtime
        .state
        .load_book_config(book_id)
        .await
        .map_err(|e| internal(e.to_string()))?;
    let book_dir = runtime.state.book_dir(book_id);
    let language = match book.language.as_deref() {
        Some("en") => crate::utils::language::WritingLanguage::En,
        _ => crate::utils::language::WritingLanguage::Zh,
    };

    // 章节正文（缺文件 → Node 404 "Chapter not found"）。
    let chapters_dir = book_dir.join("chapters");
    let padded = format!("{chapter_number:04}");
    let mut entries = tokio::fs::read_dir(&chapters_dir)
        .await
        .map_err(|_| ReviseChainError::NotFound("Chapter not found".to_string()))?;
    let mut matched: Option<String> = None;
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with(&padded) && name.ends_with(".md") {
            matched = Some(name);
            break;
        }
    }
    let file_name = matched.ok_or_else(|| ReviseChainError::NotFound("Chapter not found".to_string()))?;
    let raw = tokio::fs::read_to_string(chapters_dir.join(&file_name))
        .await
        .map_err(|_| ReviseChainError::NotFound("Chapter not found".to_string()))?;
    let content = strip_title_line(&raw);

    // 索引（标题/最新章判定/回写）。
    let index = runtime
        .state
        .load_chapter_index(book_id)
        .await
        .map_err(|e| internal(e.to_string()))?;
    let chapter_meta = index
        .iter()
        .find(|m| m.number == chapter_number)
        .ok_or_else(|| internal(format!("Chapter {chapter_number} not found in index")))?;
    let chapter_title = chapter_meta.title.clone();
    let latest = index.iter().map(|m| m.number).max().unwrap_or(chapter_number);
    let is_latest = chapter_number == latest;

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

    let counting_mode = crate::utils::length_metrics::resolve_length_counting_mode(language);
    let unchanged_result = |word_count: u32,
                            skipped_reason: String,
                            diagnostics: Option<RevisionDiagnostics>|
     -> ReviseChainResult {
        ReviseChainResult {
            chapter_number,
            word_count,
            fixed_issues: Vec::new(),
            applied: false,
            status: "unchanged",
            skipped_reason: Some(skipped_reason),
            revision_diagnostics: diagnostics,
            revised_content: None,
        }
    };

    // pre merged-audit（四源合并）。
    let auditor = FullCycleAuditor {
        router: (*runtime.router).clone(),
        project_root: runtime.state.project_root().to_path_buf(),
        builtin_genres_dir: runtime.builtin_genres_dir.clone(),
        book_dir: book_dir.clone(),
        chapter_number,
        genre: book.genre.clone(),
    };
    let pre = evaluate_merged_audit(
        &auditor,
        &book_dir,
        &content,
        chapter_number,
        language,
        &AuditChapterOptions::default(),
    )
    .await
    .map_err(internal)?;

    // 无阻塞问题且无显式修订请求 → unchanged（Node 逐字文案）。
    if pre.blocking_count == 0 && pre.ai_tell_count == 0 && !explicit_revision {
        return Ok(unchanged_result(
            crate::utils::length_metrics::count_chapter_length(&content, counting_mode),
            "No warning, critical, or AI-tell issues to fix.".to_string(),
            None,
        ));
    }

    // 修稿（以 pre 合并审计问题驱动）。
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
        &pre.audit_result.issues,
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
    .map_err(|e| internal(e.to_string()))?;
    if revise_output.revised_content.is_empty() {
        return Err(internal("Reviser returned empty content".to_string()));
    }

    // post merged-audit（temp 0 + 修稿器产出的临时真相覆盖）。
    let placeholder_state = "(状态卡未更新)";
    let placeholder_ledger = "(账本未更新)";
    let placeholder_hooks = "(伏笔池未更新)";
    let post_options = AuditChapterOptions {
        temperature: Some(0.0),
        truth_file_overrides: Some(TruthFileOverrides {
            current_state: (revise_output.updated_state != placeholder_state)
                .then(|| revise_output.updated_state.clone()),
            ledger: (revise_output.updated_ledger != placeholder_ledger)
                .then(|| revise_output.updated_ledger.clone()),
            hooks: (revise_output.updated_hooks != placeholder_hooks)
                .then(|| revise_output.updated_hooks.clone()),
        }),
        ..Default::default()
    };
    let post = evaluate_merged_audit(
        &auditor,
        &book_dir,
        &revise_output.revised_content,
        chapter_number,
        language,
        &post_options,
    )
    .await
    .map_err(internal)?;
    let effective_post = restore_actionable_audit_if_lost(&pre, &post);

    // 三档门控（strict/lenient/always；rewrite 端点注入 Always）。
    if !gate.should_apply(&pre, &effective_post) {
        let remaining_issues: Vec<RemainingIssue> = effective_post
            .revision_blocking_issues
            .iter()
            .filter(|issue| {
                matches!(
                    issue.severity,
                    crate::agents::continuity::AuditSeverity::Warning
                        | crate::agents::continuity::AuditSeverity::Critical
                )
            })
            .take(6)
            .map(|issue| RemainingIssue {
                severity: severity_string(issue.severity),
                category: issue.category.clone(),
                description: issue.description.clone(),
                suggestion: (!issue.suggestion.is_empty()).then(|| issue.suggestion.clone()),
            })
            .collect();
        return Ok(unchanged_result(
            crate::utils::length_metrics::count_chapter_length(&content, counting_mode),
            format!(
                "Manual revision kept original chapter: before blocking={}, critical={}, aiTell={}; after blocking={}, critical={}, aiTell={}.",
                pre.blocking_count,
                pre.critical_count,
                pre.ai_tell_count,
                effective_post.blocking_count,
                effective_post.critical_count,
                effective_post.ai_tell_count,
            ),
            Some(RevisionDiagnostics {
                standard: gate.standard(),
                before: gate_counts(&pre),
                after: gate_counts(&effective_post),
                remaining_issues,
            }),
        ));
    }

    // 落盘：章节文件（标准标题重构，对齐 TS reviseHeading）。
    let revised_word_count =
        crate::utils::length_metrics::count_chapter_length(&revise_output.revised_content, counting_mode);
    let heading = if language == crate::utils::language::WritingLanguage::En {
        format!("# Chapter {chapter_number}: {chapter_title}")
    } else {
        format!("# 第{chapter_number}章 {chapter_title}")
    };
    let revised_full = format!("{heading}\n\n{}", revise_output.revised_content);
    tokio::fs::write(chapters_dir.join(&file_name), &revised_full)
        .await
        .map_err(|e| internal(e.to_string()))?;

    // 仅最新章拥有当前真相（Node 语义；ledger 回写 47 号补齐）。
    if is_latest {
        let story_dir = book_dir.join("story");
        if revise_output.updated_state != placeholder_state {
            let _ = tokio::fs::write(story_dir.join("current_state.md"), &revise_output.updated_state).await;
        }
        if revise_output.updated_ledger != placeholder_ledger {
            let _ = tokio::fs::write(story_dir.join("particle_ledger.md"), &revise_output.updated_ledger).await;
        }
        if revise_output.updated_hooks != placeholder_hooks {
            let _ = tokio::fs::write(story_dir.join("pending_hooks.md"), &revise_output.updated_hooks).await;
        }
    }

    // 索引回写：目标章状态/字数/审计问题；下游章 needs-revision + 重审提示。
    let passed = effective_post.audit_result.passed;
    let downstream_notice = if language == crate::utils::language::WritingLanguage::En {
        format!("[warning] Chapter {chapter_number} changed; re-review this downstream chapter for continuity.")
    } else {
        format!("[warning] 第{chapter_number}章已重写，请重新检查本章与前文的连续性。")
    };
    let now = crate::utils::utc_time::utc_now_iso();
    let mut updated_index = index.clone();
    for slot in updated_index.iter_mut() {
        if slot.number == chapter_number {
            slot.status = if passed {
                crate::models::chapter::ChapterStatus::ReadyForReview
            } else {
                crate::models::chapter::ChapterStatus::AuditFailed
            };
            slot.word_count = revised_word_count;
            slot.updated_at = now.clone();
            slot.audit_issues = effective_post
                .audit_result
                .issues
                .iter()
                .map(|issue| format!("[{}] {}", severity_string(issue.severity), issue.description))
                .collect();
        } else if slot.number > chapter_number {
            slot.status = crate::models::chapter::ChapterStatus::NeedsRevision;
            slot.updated_at = now.clone();
            slot.audit_issues.retain(|issue| {
                !issue.contains("re-review this downstream chapter")
                    && !issue.contains("请重新检查本章与前文")
            });
            slot.audit_issues.push(downstream_notice.clone());
        }
    }
    runtime
        .state
        .save_chapter_index(book_id, &updated_index)
        .await
        .map_err(|e| internal(e.to_string()))?;

    Ok(ReviseChainResult {
        chapter_number,
        word_count: revised_word_count,
        fixed_issues: revise_output.fixed_issues,
        applied: true,
        status: if passed { "ready-for-review" } else { "audit-failed" },
        skipped_reason: None,
        revision_diagnostics: None,
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

// ── POST /api/v1/books/:id/resync/:chapter（51 号）───────────────

/// resync 审计结果（TS ChapterPipelineResult.auditResult 的 resync 形状：
/// issues 恒空，summary 描述工件同步结果）。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResyncAuditResult {
    pub passed: bool,
    pub issues: Vec<String>,
    pub summary: String,
}

/// resync 结果（对齐 TS `ChapterPipelineResult` 的 resync 形状）。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResyncResult {
    pub chapter_number: u32,
    pub title: String,
    pub word_count: u32,
    pub audit_result: ResyncAuditResult,
    pub revised: bool,
    pub status: &'static str,
    pub length_warnings: Vec<String>,
    pub length_telemetry: Option<crate::models::length_governance::LengthTelemetry>,
    pub token_usage: Option<crate::models::chapter::TokenUsage>,
}

pub async fn resync(
    State(runtime): State<BooksRuntime>,
    Path((book_id, chapter)): Path<(String, String)>,
    body: Option<Json<ReviseBody>>,
) -> impl IntoResponse {
    // TS parseInt NaN → findIndex 无命中 → 500 逐字文案；body（externalContext）
    // 仅进 governed 输入（暂缓件），此处容忍解析。
    let _ = body;
    let chapter_number = match chapter.parse::<i64>() {
        Ok(number) if number >= 1 => number as u32,
        _ => {
            let display = if chapter.parse::<i64>().is_ok() { chapter.clone() } else { "NaN".to_string() };
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("Chapter {display} not found in \"{book_id}\".") })),
            );
        }
    };
    match run_resync_chain(&runtime, &book_id, chapter_number).await {
        Ok(result) => (StatusCode::OK, Json(serde_json::to_value(&result).unwrap_or_default())),
        Err(message) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": message })),
        ),
    }
}

/// 章节工件重建（TS `_resyncChapterArtifactsLocked`）：从已编辑正文重跑
/// settle → validate →（失败重试）→ 章节与全量真相落盘 → 快照 → 索引回写。
/// 与 repair-state 的差异：不限 state-degraded（任意状态可同步）、非降级
/// 章直接置 ready-for-review、落盘走 saveChapter + saveNewTruthFiles 全量面。
async fn run_resync_chain(
    runtime: &BooksRuntime,
    book_id: &str,
    chapter_number: u32,
) -> Result<ResyncResult, String> {
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
        return Err(format!("Book \"{book_id}\" has no persisted chapters to sync."));
    }
    let Some(target_meta) = index.iter().find(|m| m.number == chapter_number) else {
        return Err(format!("Chapter {chapter_number} not found in \"{book_id}\"."));
    };
    let latest = index.iter().map(|m| m.number).max().unwrap_or(chapter_number);
    if chapter_number != latest {
        return Err(format!(
            "Only the latest persisted chapter can be synced safely (latest is {latest})."
        ));
    }

    // 章节正文（去标题行）+ 旧真相。
    let chapters_dir = book_dir.join("chapters");
    let padded = format!("{chapter_number:04}");
    let mut matched: Option<String> = None;
    if let Ok(mut entries) = tokio::fs::read_dir(&chapters_dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with(&padded) && name.ends_with(".md") {
                matched = Some(name);
                break;
            }
        }
    }
    let file_name = matched.ok_or_else(|| "Chapter not found".to_string())?;
    let raw = tokio::fs::read_to_string(chapters_dir.join(&file_name))
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

    // 语言：book.language ?? genre.language。
    let parsed_genre = crate::agents::rules_reader::read_genre_profile(
        runtime.state.project_root(),
        &book.genre,
        &runtime.builtin_genres_dir,
    )
    .await
    .map_err(|e| e.to_string())?;
    let language = match book.language.as_deref() {
        Some("en") => crate::utils::language::WritingLanguage::En,
        Some(_) => crate::utils::language::WritingLanguage::Zh,
        None if parsed_genre.profile.language == "en" => crate::utils::language::WritingLanguage::En,
        None => crate::utils::language::WritingLanguage::Zh,
    };

    // settle → validate → 失败重试链（复用 repair-state 端口装配）。
    let settler = RepairSettle {
        router: runtime.router.clone(),
        project_root: runtime.state.project_root().to_path_buf(),
        builtin_genres_dir: runtime.builtin_genres_dir.clone(),
        chapter_number,
    };
    let validator = RoutedValidator { router: runtime.router.clone() };
    let mut synced_output = settler
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
    let mut validation = validator
        .validate(crate::pipeline::chapter_state_recovery::ValidateRequest {
            content: &content,
            chapter_number,
            old_state: &old_state,
            new_state: &synced_output.updated_state,
            old_hooks: &old_hooks,
            new_hooks: &synced_output.updated_hooks,
            language,
            authority_context: None,
        })
        .await
        .map_err(|e| e.to_string())?;
    if !validation.passed {
        let recovery = retry_settlement_after_validation_failure(SettlementRetryParams {
            writer: &settler,
            validator: &validator,
            book: &book,
            book_dir: &book_dir,
            chapter_number,
            title: &target_meta.title,
            content: &content,
            control: None,
            old_state: &old_state,
            old_hooks: &old_hooks,
            original_validation: &validation,
            language,
            log_warn: &|zh, en| tracing::warn!(target: "resync", "{zh} / {en}"),
        })
        .await
        .map_err(|e| e.to_string())?;
        match recovery {
            crate::pipeline::chapter_state_recovery::SettlementRetryResult::Recovered { output, validation: recovered } => {
                synced_output = *output;
                validation = recovered;
            }
            crate::pipeline::chapter_state_recovery::SettlementRetryResult::Degraded { issues } => {
                return Err(issues
                    .first()
                    .map(|i| i.description.clone())
                    .unwrap_or_else(|| format!("Chapter sync still failed for chapter {chapter_number}.")));
            }
        }
    }
    if !validation.passed {
        return Err(format!("Chapter sync still failed for chapter {chapter_number}."));
    }

    // 章节与全量真相落盘（saveChapter + saveNewTruthFiles，对齐 TS 调用序）。
    let writer_ctx: &'static crate::agents::writer::WriterCtx =
        Box::leak(Box::new(crate::agents::writer::WriterCtx {
            project_root: Box::leak(runtime.state.project_root().to_path_buf().into_boxed_path()),
            builtin_genres_dir: Box::leak(runtime.builtin_genres_dir.clone().into_boxed_path()),
            prompt_store: Box::leak(Box::new(FsStateStore)),
            state_store: Box::leak(Box::new(FsStateStore)),
        }));
    crate::agents::writer::save_chapter(writer_ctx, &book_dir, &synced_output, parsed_genre.profile.numerical_system, language)
        .await
        .map_err(|e| e.to_string())?;
    crate::agents::writer::save_new_truth_files(&book_dir, &synced_output, language)
        .await
        .map_err(|e| e.to_string())?;
    let _ = runtime.state.snapshot_state(book_id, chapter_number).await;

    // 索引回写：state-degraded → 基础状态 + 剥注入问题；否则直接 ready-for-review。
    let mut updated_index = index.clone();
    let final_status: &'static str;
    if let Some(slot) = updated_index.iter_mut().find(|m| m.number == chapter_number) {
        if target_meta.status == crate::models::chapter::ChapterStatus::StateDegraded {
            let base = crate::pipeline::chapter_state_recovery::resolve_state_degraded_base_status(target_meta);
            let injected: std::collections::HashSet<String> =
                crate::pipeline::chapter_state_recovery::parse_state_degraded_review_note(
                    target_meta.review_note.as_deref(),
                )
                .map(|note| note.injected_issues.into_iter().collect())
                .unwrap_or_default();
            slot.status = if base == "audit-failed" {
                crate::models::chapter::ChapterStatus::AuditFailed
            } else {
                crate::models::chapter::ChapterStatus::ReadyForReview
            };
            slot.audit_issues = target_meta
                .audit_issues
                .iter()
                .filter(|issue| !injected.contains(*issue))
                .cloned()
                .collect();
            slot.review_note = None;
            final_status = if base == "audit-failed" { "audit-failed" } else { "ready-for-review" };
        } else {
            slot.status = crate::models::chapter::ChapterStatus::ReadyForReview;
            final_status = "ready-for-review";
        }
        slot.updated_at = crate::utils::utc_time::utc_now_iso();
    } else {
        final_status = "ready-for-review";
    }
    runtime
        .state
        .save_chapter_index(book_id, &updated_index)
        .await
        .map_err(|e| e.to_string())?;

    Ok(ResyncResult {
        chapter_number,
        title: target_meta.title.clone(),
        word_count: target_meta.word_count,
        audit_result: ResyncAuditResult {
            passed: final_status != "audit-failed",
            issues: Vec::new(),
            summary: if final_status == "audit-failed" {
                "chapter truth/state resynced from edited body, but chapter still needs audit fixes".to_string()
            } else {
                "chapter truth/state resynced from edited body".to_string()
            },
        },
        revised: false,
        status: final_status,
        length_warnings: target_meta.length_warnings.clone(),
        length_telemetry: target_meta.length_telemetry.clone(),
        token_usage: target_meta.token_usage.clone(),
    })
}

// ── GET /api/v1/books/:id/analytics（47 号）──────────────────────

pub async fn analytics(
    State(runtime): State<BooksRuntime>,
    Path(book_id): Path<String>,
) -> impl IntoResponse {
    // TS 契约：loadChapterIndex 缺书不抛错（返回 []）→ 200 空统计；
    // 意外错误才 404（`Book "id" not found`）。
    match runtime.state.load_chapter_index(&book_id).await {
        Ok(index) => {
            let chapters: Vec<crate::utils::analytics::AnalyticsChapter> =
                index.iter().map(crate::utils::analytics::AnalyticsChapter::from_meta).collect();
            let data = crate::utils::analytics::compute_analytics(&book_id, &chapters);
            (
                StatusCode::OK,
                Json(serde_json::to_value(&data).unwrap_or_default()),
            )
        }
        Err(_) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("Book \"{book_id}\" not found") })),
        ),
    }
}

// ── GET /api/v1/books/:id/eval（47 号）────────────────────────────

#[derive(Debug, Default, Deserialize)]
pub struct EvalQuery {
    #[serde(default)]
    pub chapters: Option<String>,
}

pub async fn eval(
    State(runtime): State<BooksRuntime>,
    Path(book_id): Path<String>,
    axum::extract::Query(query): axum::extract::Query<EvalQuery>,
) -> impl IntoResponse {
    match crate::utils::book_eval::evaluate_book_quality(&runtime.state, &book_id, query.chapters.as_deref())
        .await
    {
        Ok(report) => (
            StatusCode::OK,
            Json(serde_json::to_value(&report).unwrap_or_default()),
        ),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error })),
        ),
    }
}

// ── GET /api/v1/books/:id/export（47 号）──────────────────────────

#[derive(Debug, Default, Deserialize)]
pub struct ExportQuery {
    #[serde(default)]
    pub format: Option<String>,
    #[serde(rename = "approvedOnly", default)]
    pub approved_only: Option<String>,
}

pub async fn export(
    State(runtime): State<BooksRuntime>,
    Path(book_id): Path<String>,
    axum::extract::Query(query): axum::extract::Query<ExportQuery>,
) -> axum::response::Response {
    let format = crate::interaction::export_artifact::ExportFormat::parse(query.format.as_deref());
    let approved_only = query.approved_only.as_deref() == Some("true");
    match crate::interaction::export_artifact::build_export_artifact(
        &runtime.state,
        &book_id,
        format,
        approved_only,
        None,
    )
    .await
    {
        Ok(artifact) => {
            let mut response = (StatusCode::OK, artifact.payload).into_response();
            let headers = response.headers_mut();
            if let Ok(value) = axum::http::HeaderValue::from_str(&artifact.content_type) {
                headers.insert(axum::http::header::CONTENT_TYPE, value);
            }
            if let Ok(value) =
                axum::http::HeaderValue::from_str(&format!("attachment; filename=\"{}\"", artifact.file_name))
            {
                headers.insert(axum::http::header::CONTENT_DISPOSITION, value);
            }
            response
        }
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "Export failed" })),
        )
            .into_response(),
    }
}

// ── 共享装配 ─────────────────────────────────────────────────────

pub(crate) fn build_write_next_agents(runtime: &BooksRuntime) -> WriteNextAgents<'static> {
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

pub(crate) fn build_write_next_ctx(runtime: &BooksRuntime) -> WriteNextCtx<'static> {
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
            revision_gate: RevisionGate::default(),
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
            .route("/api/v1/books/:id/analytics", axum::routing::get(analytics))
            .route("/api/v1/books/:id/eval", axum::routing::get(eval))
            .route("/api/v1/books/:id/export", axum::routing::get(export))
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
    async fn revise_missing_chapter_returns_404_with_node_message() {
        let dir = tempfile::tempdir().unwrap();
        fixture(dir.path());
        let runtime = runtime_for(dir.path());
        let response = app(runtime)
            .oneshot(post("/api/v1/books/b1/revise/9", "{}"))
            .await
            .unwrap();
        // Node 契约：缺章 404 {"error": "Chapter not found"}。
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = axum::body::to_bytes(response.into_body(), 4096).await.unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed["error"], "Chapter not found");
    }

    #[tokio::test]
    async fn analytics_missing_book_returns_empty_stats_not_404() {
        // TS 怪癖：loadChapterIndex 缺书返回 [] → 200 空统计（404 分支不可达）。
        let dir = tempfile::tempdir().unwrap();
        let runtime = runtime_for(dir.path());
        let response = app(runtime)
            .oneshot(
                axum::http::Request::builder()
                    .uri("/api/v1/books/ghost/analytics")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 8192).await.unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed["totalChapters"], 0);
        assert_eq!(parsed["auditPassRate"], 100);
    }

    #[tokio::test]
    async fn export_without_chapters_returns_500_export_failed() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = runtime_for(dir.path());
        let response = app(runtime)
            .oneshot(
                axum::http::Request::builder()
                    .uri("/api/v1/books/ghost/export?format=txt")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = axum::body::to_bytes(response.into_body(), 4096).await.unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed["error"], "Export failed");
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
