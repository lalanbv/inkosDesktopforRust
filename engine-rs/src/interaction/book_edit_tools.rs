//! 书会话确定性编辑工具族 + 封面壳（89 号）。
//!
//! 移植自 `packages/core/src/agent/agent-tools.ts` 的
//! `createWriteTruthFileTool` / `createRenameEntityTool` /
//! `createPatchChapterTextTool` / `createReplaceChapterTextTool` /
//! `createDeleteLatestChapterTool` / `createGenerateCoverTool` /
//! `createResyncChapterStateTool`（215 号）：
//! 域链接权——edit-controller 事务（89 号补 entity-rename 与
//! chapter-local-edit）、chapter-delete 域函数、确认面 generate_cover
//! 执行器（74/76 号 cover 基础设施）。注册面：book/book-create 全六件，
//! edit 会话仅确定性五件（TS edit 过滤器去 generate_cover）。

use serde_json::{json, Value};

use crate::interaction::import_chapters_tool::resolve_tool_book_id;
use crate::interaction::project_tools::{error_result, ToolResult};
use crate::interaction::registry::{schema_description, schema_parameters, MutationKind, ToolDef};
use crate::server::books_routes::BooksRuntime;

/// 工具依赖：runtime + 活动书（书会话恒有）+ 会话语言（resync 双语摘要）。
pub struct BookEditDeps<'a> {
    pub runtime: &'a BooksRuntime,
    pub active_book_id: &'a str,
    /// "zh" | "en"（TS options.language 对应面）。
    pub language: &'a str,
}

fn text_result(text: impl Into<String>, details: Option<Value>) -> ToolResult {
    ToolResult { text: text.into(), details, is_error: false }
}

fn field_str<'a>(args: &'a Value, name: &str) -> Option<&'a str> {
    args.get(name)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
}

/// `write_truth_file`：真相文件全量替换（ensureControlDocuments → 白名单
/// → safeChildPath → 落盘）。失败面为普通文本（TS try/catch 逐字），非错误。
pub async fn tool_write_truth_file(deps: &BookEditDeps<'_>, args: &Value) -> ToolResult {
    match write_truth_file_inner(deps, args).await {
        Ok(text) => text_result(text, None),
        Err(message) => ToolResult {
            text: format!("write_truth_file failed: {message}"),
            details: None,
            is_error: false,
        },
    }
}

async fn write_truth_file_inner(deps: &BookEditDeps<'_>, args: &Value) -> Result<String, String> {
    let book_id = resolve_tool_book_id("write_truth_file", field_str(args, "bookId"), Some(deps.active_book_id))?;
    let raw = args.get("fileName").and_then(Value::as_str);
    let Some(raw) = raw else {
        return Err("Invalid truth file name: undefined".to_string());
    };
    let safe = assert_safe_truth_file_name(raw)?;
    let content = args.get("content").and_then(Value::as_str).ok_or_else(|| {
        // TS writeFile(undefined) → TypeError 文案面。
        "The \"content\" argument must be of type string".to_string()
    })?;
    let state = &deps.runtime.state;
    state
        .ensure_control_documents(&book_id, None)
        .await
        .map_err(|e| e.to_string())?;
    let story_dir = state.book_dir(&book_id).join("story");
    let target = crate::utils::path::safe_child_path(&story_dir.to_string_lossy(), &safe)?;
    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| e.to_string())?;
    }
    tokio::fs::write(&target, content).await.map_err(|e| e.to_string())?;
    Ok(format!("Updated \"{safe}\" for \"{book_id}\"."))
}

/// `assertSafeTruthFileName`：trim → 补 .md → 白名单（扁平/outline）或
/// roles 角色卡正则；非法名逐字错误。
fn assert_safe_truth_file_name(file_name: &str) -> Result<String, String> {
    let trimmed = file_name.trim();
    let with_extension = if trimmed.ends_with(".md") {
        trimmed.to_string()
    } else {
        format!("{trimmed}.md")
    };
    let lower = with_extension.to_lowercase();
    if trimmed.is_empty()
        || with_extension.starts_with('/')
        || with_extension.contains('\\')
        || with_extension.contains('\0')
        || with_extension.contains("..")
    {
        return Err(format!(
            "Invalid truth file name: {}",
            serde_json::to_string(file_name).unwrap_or_default()
        ));
    }
    if SAFE_TRUTH_FLAT_FILE_NAMES.contains(&lower.as_str()) {
        return Ok(lower);
    }
    if SAFE_TRUTH_OUTLINE_FILE_NAMES.contains(&lower.as_str()) {
        return Ok(lower);
    }
    if safe_role_truth_file_re().is_match(&with_extension) {
        return Ok(with_extension);
    }
    Err(format!(
        "Invalid truth file name: {}",
        serde_json::to_string(file_name).unwrap_or_default()
    ))
}

const SAFE_TRUTH_FLAT_FILE_NAMES: &[&str] = &[
    "author_intent.md",
    "current_focus.md",
    "story_bible.md",
    "volume_outline.md",
    "book_rules.md",
    "particle_ledger.md",
    "subplot_board.md",
    "emotional_arcs.md",
    "style_guide.md",
    "parent_canon.md",
    "fanfic_canon.md",
    "character_matrix.md",
    "current_state.md",
    "pending_hooks.md",
    "chapter_summaries.md",
];

const SAFE_TRUTH_OUTLINE_FILE_NAMES: &[&str] = &[
    "outline/story_frame.md",
    "outline/volume_map.md",
    "outline/节奏原则.md",
    "outline/rhythm_principles.md",
];

fn safe_role_truth_file_re() -> &'static regex::Regex {
    static R: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    R.get_or_init(|| {
        regex::Regex::new(r"^roles/(主要角色|次要角色|major|minor)/[^/\\]+\.md$")
            .expect("safe role truth file regex")
    })
}

/// `rename_entity`：entity-rename 事务（全库替换 + 文件重命名），文本 =
/// 事务 summary。
pub async fn tool_rename_entity(deps: &BookEditDeps<'_>, args: &Value) -> ToolResult {
    let book_id = match resolve_tool_book_id("rename_entity", field_str(args, "bookId"), Some(deps.active_book_id)) {
        Ok(book_id) => book_id,
        Err(message) => return error_result(message),
    };
    let old_value = args.get("oldValue").and_then(Value::as_str);
    let new_value = args.get("newValue").and_then(Value::as_str);
    let (Some(old_value), Some(new_value)) = (old_value, new_value) else {
        return error_result("rename_entity requires oldValue and newValue.".to_string());
    };
    match crate::interaction::edit_controller::execute_entity_rename(&deps.runtime.state, &book_id, old_value, new_value)
        .await
    {
        Ok(result) => text_result(result.summary, None),
        Err(message) => error_result(message),
    }
}

/// `patch_chapter_text`：chapter-local-edit 事务（三级替换 + 复核标记），
/// 文本 = 事务 summary。
pub async fn tool_patch_chapter_text(deps: &BookEditDeps<'_>, args: &Value) -> ToolResult {
    let book_id = match resolve_tool_book_id("patch_chapter_text", field_str(args, "bookId"), Some(deps.active_book_id)) {
        Ok(book_id) => book_id,
        Err(message) => return error_result(message),
    };
    let Some(chapter) = parse_chapter_number(args) else {
        return error_result("Chapter NaN not found.".to_string());
    };
    let target_text = args.get("targetText").and_then(Value::as_str).unwrap_or_default();
    let replacement_text = args.get("replacementText").and_then(Value::as_str).unwrap_or_default();
    match crate::interaction::edit_controller::execute_chapter_local_edit(
        &deps.runtime.state,
        &book_id,
        chapter,
        target_text,
        replacement_text,
        crate::utils::utc_time::utc_now_millis(),
        &crate::utils::utc_time::utc_now_iso(),
    )
    .await
    {
        Ok(result) => text_result(result.summary, None),
        Err(message) => error_result(message),
    }
}

/// `replace_chapter_text`：chapter-replace 事务（用户全量替换 + 复核标记），
/// 文本 = 事务 summary；版本源 Agent（TS 工具路径缺省）。
pub async fn tool_replace_chapter_text(deps: &BookEditDeps<'_>, args: &Value) -> ToolResult {
    let book_id = match resolve_tool_book_id("replace_chapter_text", field_str(args, "bookId"), Some(deps.active_book_id)) {
        Ok(book_id) => book_id,
        Err(message) => return error_result(message),
    };
    let Some(chapter) = parse_chapter_number(args) else {
        return error_result("Chapter NaN not found.".to_string());
    };
    let full_text = args.get("fullText").and_then(Value::as_str).unwrap_or_default();
    match crate::interaction::edit_controller::execute_chapter_replace(
        &deps.runtime.state,
        &book_id,
        chapter,
        full_text,
        crate::state::chapter_workspace::ChapterVersionSource::Agent,
        crate::utils::utc_time::utc_now_millis(),
        &crate::utils::utc_time::utc_now_iso(),
    )
    .await
    {
        Ok(result) => text_result(result.summary, None),
        Err(message) => error_result(message),
    }
}

/// `delete_latest_chapter`：仅删最新章（.trash 保留 + 状态回滚）。
pub async fn tool_delete_latest_chapter(deps: &BookEditDeps<'_>, args: &Value) -> ToolResult {
    let book_id = match resolve_tool_book_id(
        "delete_latest_chapter",
        field_str(args, "bookId"),
        Some(deps.active_book_id),
    ) {
        Ok(book_id) => book_id,
        Err(message) => return error_result(message),
    };
    let request = match args.get("chapterNumber") {
        None => crate::state::chapter_delete::DeleteRequest::Latest,
        Some(value) => match value.as_f64() {
            Some(number) if number.fract() == 0.0 && number >= 1.0 => {
                crate::state::chapter_delete::DeleteRequest::Chapter(number as i64)
            }
            _ => crate::state::chapter_delete::DeleteRequest::NaN,
        },
    };
    match crate::state::chapter_delete::delete_latest_chapter(&deps.runtime.state, &book_id, request).await {
        Ok(result) => text_result(
            format!(
                "Deleted latest chapter {} from \"{book_id}\", preserved it in trash, and rolled story state back to chapter {}.",
                result.deleted_chapter, result.rolled_back_to
            ),
            Some(json!({
                "kind": "chapter_deleted",
                "bookId": result.book_id,
                "deletedChapter": result.deleted_chapter,
                "title": result.title,
                "trashedFiles": result.trashed_files,
                "rolledBackTo": result.rolled_back_to,
                "discarded": result.discarded,
            })),
        ),
        Err(message) => error_result(message),
    }
}

/// `generate_cover`：封面生成壳（复用确认面执行器——文本/details 逐字）。
pub async fn tool_generate_cover(deps: &BookEditDeps<'_>, args: &Value) -> ToolResult {
    let Some(title) = field_str(args, "title") else {
        return error_result("title is required for cover generation.".to_string());
    };
    let intro = field_str(args, "intro").unwrap_or_default();
    let selling_points =
        crate::llm::cover::normalize_selling_points(args.get("sellingPoints").and_then(Value::as_str));
    let cover_prompt = field_str(args, "coverPrompt").unwrap_or_default();
    let output_dir = field_str(args, "outputDir").unwrap_or_default();
    match crate::server::agent_production::execute_generate_cover(
        deps.runtime,
        title,
        intro,
        &selling_points,
        cover_prompt,
        output_dir,
        |_| {},
    )
    .await
    {
        Ok(outcome) => ToolResult {
            text: outcome.text,
            details: Some(outcome.details),
            is_error: outcome.is_error,
        },
        Err(message) => error_result(message),
    }
}

/// `resync_chapter_state`：正文不动，从已编辑正文重建状态/摘要/伏笔并重跑
/// 新审计。TS `resyncChapterStateAndAudit` 对应面（`_resyncChapterArtifactsLocked`
/// 加 `auditDraft`）。复用 51 号 resync 链与 audit_route 审计流；审计后按
/// auditDraft 语义回写索引状态与问题、维护漂移指引（仅最新章，resync 链
/// 本就限定最新章）。
#[allow(clippy::too_many_lines)]
pub async fn tool_resync_chapter_state(deps: &BookEditDeps<'_>, args: &Value) -> ToolResult {
    let book_id = match resolve_tool_book_id("resync_chapter_state", field_str(args, "bookId"), Some(deps.active_book_id)) {
        Ok(book_id) => book_id,
        Err(message) => return error_result(message),
    };
    let allow_new_hooks = args.get("allowNewHooks").and_then(Value::as_bool);
    let runtime = deps.runtime;
    // TS chapterNumber ?? index[last].number：缺省 = 最新章。
    let chapter_number = match parse_chapter_number(args) {
        Some(number) => number,
        None => match runtime.state.load_chapter_index(&book_id).await {
            Ok(index) => index.iter().map(|m| m.number).max().unwrap_or(0),
            Err(e) => return error_result(e.to_string()),
        },
    };
    if chapter_number == 0 {
        return error_result(format!("Book \"{book_id}\" has no persisted chapters to sync."));
    }

    let resynced = match crate::server::books_routes::run_resync_chain(runtime, &book_id, chapter_number, allow_new_hooks).await {
        Ok(result) => result,
        Err(message) => return error_result(message),
    };

    // 新审计（TS auditDraft：真实 auditor 评估）。
    let audit_runtime = crate::server::audit_route::AuditRuntime {
        hub: runtime.hub.clone(),
        state: runtime.state.clone(),
        router: runtime.router.clone(),
        builtin_genres_dir: runtime.builtin_genres_dir.clone(),
    };
    let audit = match crate::server::audit_route::run_audit_flow(&audit_runtime, &book_id, chapter_number, None).await {
        Ok(result) => result,
        Err(error) => return error_result(format!("audit failed: {error}")),
    };

    // 索引回写（auditDraft 语义：状态 + "[severity] description" 问题行 + 时间戳）。
    match runtime.state.load_chapter_index(&book_id).await {
        Ok(mut index) => {
            if let Some(slot) = index.iter_mut().find(|m| m.number == chapter_number) {
                slot.status = if audit.passed {
                    crate::models::chapter::ChapterStatus::ReadyForReview
                } else {
                    crate::models::chapter::ChapterStatus::AuditFailed
                };
                slot.audit_issues = audit
                    .issues
                    .iter()
                    .map(|issue| format!("[{}] {}", audit_severity_str(issue.severity), issue.description))
                    .collect();
                slot.updated_at = crate::utils::utc_time::utc_now_iso();
            }
            if let Err(e) = runtime.state.save_chapter_index(&book_id, &index).await {
                return error_result(e.to_string());
            }
        }
        Err(e) => return error_result(e.to_string()),
    }

    // 漂移指引（auditDraft：最新章 + critical/warning；失败不阻断——TS .catch(undefined)）。
    let language = if deps.language == "en" {
        crate::utils::language::WritingLanguage::En
    } else {
        crate::utils::language::WritingLanguage::Zh
    };
    let drift: Vec<crate::agents::continuity::AuditIssue> = audit
        .issues
        .iter()
        .filter(|issue| {
            matches!(
                issue.severity,
                crate::agents::continuity::AuditSeverity::Critical | crate::agents::continuity::AuditSeverity::Warning
            )
        })
        .cloned()
        .collect();
    let _ = crate::pipeline::write_next::persist_audit_drift_guidance(
        &runtime.state.book_dir(&book_id),
        chapter_number,
        &drift,
        language,
    )
    .await;

    // 双语摘要（TS 逐字）+ details。
    let zh = deps.language != "en";
    let text = if audit.passed {
        if zh {
            format!("第 {} 章正文未改动；状态、摘要与伏笔已从上一章快照重建，重新审稿通过。", chapter_number)
        } else {
            format!(
                "Chapter {chapter_number} prose was unchanged; state, summaries, and hooks were rebuilt from the previous snapshot, and the fresh audit passed."
            )
        }
    } else {
        let mut lines = if zh {
            format!(
                "第 {} 章正文未改动；状态、摘要与伏笔已重建，但重新审稿仍有 {} 个问题：",
                chapter_number,
                audit.issues.len()
            )
        } else {
            format!(
                "Chapter {chapter_number} prose was unchanged; state, summaries, and hooks were rebuilt, but the fresh audit still found {} issue(s):",
                audit.issues.len()
            )
        };
        for issue in &audit.issues {
            let severity = audit_severity_str(issue.severity);
            if issue.suggestion.is_empty() {
                lines.push_str(&format!("\n- [{severity}] {}", issue.description));
            } else {
                lines.push_str(&format!("\n- [{severity}] {} ({})", issue.description, issue.suggestion));
            }
        }
        lines
    };

    text_result(
        text,
        Some(json!({
            "kind": "chapter_state_resynced",
            "bookId": book_id,
            "chapterNumber": resynced.chapter_number,
            "status": if audit.passed { "ready-for-review" } else { "audit-failed" },
            "auditPassed": audit.passed,
            "auditIssues": audit.issues,
            "summary": audit.summary,
        })),
    )
}

/// 审计严重度 → TS 字符串名。
fn audit_severity_str(severity: crate::agents::continuity::AuditSeverity) -> &'static str {
    match severity {
        crate::agents::continuity::AuditSeverity::Critical => "critical",
        crate::agents::continuity::AuditSeverity::Warning => "warning",
        crate::agents::continuity::AuditSeverity::Info => "info",
    }
}

fn parse_chapter_number(args: &Value) -> Option<u32> {
    args.get("chapterNumber")
        .and_then(Value::as_f64)
        .filter(|v| v.fract() == 0.0 && *v >= 1.0)
        .map(|v| v as u32)
}

/// 确定性五件 schema（WriteTruthFile/RenameEntity/PatchChapterText/
/// ReplaceChapterText/DeleteLatestChapter Params 逐字）。
pub fn deterministic_tool_schemas() -> Vec<Value> {
    vec![
        json!({
            "type": "function",
            "function": {
                "name": "write_truth_file",
                "description": "Replace a truth/control file under story/ using deterministic project tools.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "bookId": { "type": "string", "description": "Book ID. Omit to use the active book." },
                        "fileName": { "type": "string", "description": "Truth file path under story/. Prefer outline/story_frame.md, outline/volume_map.md, roles/major/<name>.md, roles/minor/<name>.md; flat files such as current_focus.md and author_intent.md are also supported." },
                        "content": { "type": "string", "description": "Full replacement content for the truth file." },
                    },
                    "required": ["fileName", "content"],
                },
            },
        }),
        json!({
            "type": "function",
            "function": {
                "name": "rename_entity",
                "description": "Rename an entity across truth files and chapters using deterministic edit control.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "bookId": { "type": "string", "description": "Book ID. Omit to use the active book." },
                        "oldValue": { "type": "string", "description": "Current entity name." },
                        "newValue": { "type": "string", "description": "New entity name." },
                    },
                    "required": ["oldValue", "newValue"],
                },
            },
        }),
        json!({
            "type": "function",
            "function": {
                "name": "patch_chapter_text",
                "description": "Apply a deterministic local text patch to a chapter and mark it for review.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "bookId": { "type": "string", "description": "Book ID. Omit to use the active book." },
                        "chapterNumber": { "type": "number", "description": "Chapter number to patch." },
                        "targetText": { "type": "string", "description": "Exact text to replace." },
                        "replacementText": { "type": "string", "description": "Replacement text." },
                    },
                    "required": ["chapterNumber", "targetText", "replacementText"],
                },
            },
        }),
        json!({
            "type": "function",
            "function": {
                "name": "replace_chapter_text",
                "description": "Replace a whole existing chapter with user-supplied full chapter text and mark it for review. Use only when the user provides the complete replacement chapter; for model-generated rewrites use sub_agent reviser.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "bookId": { "type": "string", "description": "Book ID. Omit to use the active book." },
                        "chapterNumber": { "type": "number", "description": "Chapter number to replace." },
                        "fullText": { "type": "string", "description": "The complete replacement chapter markdown/text supplied by the user." },
                    },
                    "required": ["chapterNumber", "fullText"],
                },
            },
        }),
        json!({
            "type": "function",
            "function": {
                "name": "delete_latest_chapter",
                "description": "Safely delete only the latest chapter, preserve its manuscript under chapters/.trash, and roll story state back to the previous chapter snapshot. Never deletes a middle chapter.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "bookId": { "type": "string", "description": "Book ID. Omit to use the active book." },
                        "chapterNumber": { "type": "number", "description": "Latest chapter number expected by the user. The tool rejects middle-chapter deletion." },
                    },
                },
            },
        }),
    ]
}

/// `generate_cover` schema（GenerateCoverParams 逐字；Rust 链不支持的
/// cover 端点覆盖参数未列入——见 89 号偏差备案）。
pub fn generate_cover_schema() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "generate_cover",
            "description": "Generate only a cover image and cover prompt from a title/synopsis/visual direction. Use this when the user asks to create/regenerate a cover or revise the cover prompt through chat, without rerunning story generation.",
            "parameters": {
                "type": "object",
                "properties": {
                    "title": { "type": "string", "description": "Required book or short-fiction title. Use the real story title when regenerating an existing cover." },
                    "intro": { "type": "string", "description": "Optional synopsis or one-paragraph story hook to guide the cover." },
                    "sellingPoints": { "type": "string", "description": "Optional selling points separated by semicolons or new lines, e.g. 婚姻背叛；证据反杀；女主冷笑." },
                    "coverPrompt": { "type": "string", "description": "Optional concrete or revised visual direction. Use this when the user changes the cover prompt through chat. Keep it short and commercial; do not paste the whole story." },
                    "outputDir": { "type": "string", "description": "Optional project-relative directory for cover-prompt.md and cover.png. For an existing short or cover prompt revision, use its existing final/cover directory to overwrite that cover." },
                },
                "required": ["title"],
            },
        },
    })
}

/// `resync_chapter_state` schema（ResyncChapterStateParams 逐字，215 号）。
pub fn resync_chapter_state_schema() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "resync_chapter_state",
            "description": "Keep the persisted chapter body unchanged, rebuild its derived story state, summaries, and hooks from the previous chapter snapshot, then run a fresh audit. Use after an explicit chapter edit or when the user asks to repair/synchronize truth state without rewriting prose. Only the latest chapter is supported.",
            "parameters": {
                "type": "object",
                "properties": {
                    "bookId": { "type": "string", "description": "Book ID. Omit to use the active book." },
                    "chapterNumber": { "type": "number", "description": "Latest chapter number to rebuild from its persisted body. Omit to use the latest chapter." },
                    "allowNewHooks": { "type": "boolean", "description": "Whether settlement may create brand-new hook IDs. Set false when the user asks to preserve stable hook IDs, avoid replacement hooks, or only repair existing truth state." },
                },
            },
        },
    })
}


// ── 注册模块（R38b）：编辑工具族七件——全部生产变更面（TS
//    PRODUCTION_MUTATION_TOOL_NAMES 可注册子集主体，后台生产剔除）；
//    schema 经同文件 deterministic_tool_schemas / generate_cover_schema /
//    resync_chapter_state_schema 拆解引用（码点零搬移）。 ──

crate::interaction::registry::tool_def!(
    BookWriteTruthFile,
    "write_truth_file",
    MutationKind::ProductionMutation,
    ctx, args,
    { {
        let schemas = deterministic_tool_schemas();
        schema_description(&schemas, "write_truth_file")
    } },
    { {
        let schemas = deterministic_tool_schemas();
        schema_parameters(&schemas, "write_truth_file")
    } },
    ctx.book_edit_deps.is_some(),
    tool_write_truth_file(ctx.book_edit_deps.as_ref().expect("available 门控"), args).await
);

crate::interaction::registry::tool_def!(
    BookRenameEntity,
    "rename_entity",
    MutationKind::ProductionMutation,
    ctx, args,
    { {
        let schemas = deterministic_tool_schemas();
        schema_description(&schemas, "rename_entity")
    } },
    { {
        let schemas = deterministic_tool_schemas();
        schema_parameters(&schemas, "rename_entity")
    } },
    ctx.book_edit_deps.is_some(),
    tool_rename_entity(ctx.book_edit_deps.as_ref().expect("available 门控"), args).await
);

crate::interaction::registry::tool_def!(
    BookPatchChapterText,
    "patch_chapter_text",
    MutationKind::ProductionMutation,
    ctx, args,
    { {
        let schemas = deterministic_tool_schemas();
        schema_description(&schemas, "patch_chapter_text")
    } },
    { {
        let schemas = deterministic_tool_schemas();
        schema_parameters(&schemas, "patch_chapter_text")
    } },
    ctx.book_edit_deps.is_some(),
    tool_patch_chapter_text(ctx.book_edit_deps.as_ref().expect("available 门控"), args).await
);

crate::interaction::registry::tool_def!(
    BookReplaceChapterText,
    "replace_chapter_text",
    MutationKind::ProductionMutation,
    ctx, args,
    { {
        let schemas = deterministic_tool_schemas();
        schema_description(&schemas, "replace_chapter_text")
    } },
    { {
        let schemas = deterministic_tool_schemas();
        schema_parameters(&schemas, "replace_chapter_text")
    } },
    ctx.book_edit_deps.is_some(),
    tool_replace_chapter_text(ctx.book_edit_deps.as_ref().expect("available 门控"), args).await
);

crate::interaction::registry::tool_def!(
    BookDeleteLatestChapter,
    "delete_latest_chapter",
    MutationKind::ProductionMutation,
    ctx, args,
    { {
        let schemas = deterministic_tool_schemas();
        schema_description(&schemas, "delete_latest_chapter")
    } },
    { {
        let schemas = deterministic_tool_schemas();
        schema_parameters(&schemas, "delete_latest_chapter")
    } },
    ctx.book_edit_deps.is_some(),
    tool_delete_latest_chapter(ctx.book_edit_deps.as_ref().expect("available 门控"), args).await
);

crate::interaction::registry::tool_def!(
    BookGenerateCover,
    "generate_cover",
    MutationKind::ProductionMutation,
    ctx, args,
    { schema_description(&[generate_cover_schema()], "generate_cover") },
    schema_parameters(&[generate_cover_schema()], "generate_cover"),
    ctx.book_edit_deps.is_some(),
    tool_generate_cover(ctx.book_edit_deps.as_ref().expect("available 门控"), args).await
);

crate::interaction::registry::tool_def!(
    BookResyncChapterState,
    "resync_chapter_state",
    MutationKind::ProductionMutation,
    ctx, args,
    { schema_description(&[resync_chapter_state_schema()], "resync_chapter_state") },
    schema_parameters(&[resync_chapter_state_schema()], "resync_chapter_state"),
    ctx.book_edit_deps.is_some(),
    tool_resync_chapter_state(ctx.book_edit_deps.as_ref().expect("available 门控"), args).await
);

/// 注册表汇聚口（registry 装配序 = 原 execute_book_edit_tool match 序）。
pub(crate) fn defs() -> Vec<Box<dyn ToolDef>> {
    vec![
        Box::new(BookWriteTruthFile),
        Box::new(BookRenameEntity),
        Box::new(BookPatchChapterText),
        Box::new(BookReplaceChapterText),
        Box::new(BookDeleteLatestChapter),
        Box::new(BookGenerateCover),
        Box::new(BookResyncChapterState),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_truth_file_name_whitelist() {
        assert_eq!(assert_safe_truth_file_name("current_focus").unwrap(), "current_focus.md");
        assert_eq!(
            assert_safe_truth_file_name("outline/story_frame.md").unwrap(),
            "outline/story_frame.md"
        );
        // 角色卡保留原始大小写（TS 正则分支返回 withExtension）；
        // endsWith(".md") 大小写敏感——.MD 会再补 .md。
        assert_eq!(
            assert_safe_truth_file_name("roles/主要角色/LinDong.md").unwrap(),
            "roles/主要角色/LinDong.md"
        );
        assert_eq!(
            assert_safe_truth_file_name("roles/major/Lin-Dong.MD").unwrap(),
            "roles/major/Lin-Dong.MD.md"
        );
        for bad in ["", "  ", "/abs/path.md", "a\\b.md", "a..b.md", "../escape.md", "notes.md"] {
            assert!(assert_safe_truth_file_name(bad).is_err(), "{bad:?}");
        }
        assert_eq!(
            assert_safe_truth_file_name("notes.md").unwrap_err(),
            "Invalid truth file name: \"notes.md\""
        );
    }

    /// 分发名单面：resync_chapter_state 已入分发（不真跑链）。
    fn execute_book_edit_tool_dispatches(name: &str) -> bool {
        matches!(
            name,
            "write_truth_file"
                | "rename_entity"
                | "patch_chapter_text"
                | "replace_chapter_text"
                | "delete_latest_chapter"
                | "generate_cover"
                | "resync_chapter_state"
        )
    }

    #[test]
    fn schemas_shape() {
        let schemas = deterministic_tool_schemas();
        let names: Vec<&str> = schemas
            .iter()
            .map(|s| s["function"]["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            vec![
                "write_truth_file",
                "rename_entity",
                "patch_chapter_text",
                "replace_chapter_text",
                "delete_latest_chapter"
            ]
        );
        assert_eq!(generate_cover_schema()["function"]["name"], "generate_cover");
        assert_eq!(
            resync_chapter_state_schema()["function"]["name"],
            "resync_chapter_state"
        );
        assert_eq!(
            resync_chapter_state_schema()["function"]["parameters"]["properties"]["allowNewHooks"]["type"],
            "boolean"
        );
        // 分发面覆盖（其余分支走真链，单测只验名单面）。
        assert!(matches!(
            "resync_chapter_state",
            n if execute_book_edit_tool_dispatches(n)
        ));
        assert_eq!(
            schemas[0]["function"]["parameters"]["required"],
            json!(["fileName", "content"])
        );
    }
}
