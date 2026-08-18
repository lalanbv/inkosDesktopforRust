//! sub_agent 聊天工具（87 号；88 号补 reviser + architect.revise）——书会话五代理委托。
//!
//! 移植自 `packages/core/src/agent/agent-tools.ts` 的 `createSubAgentTool`：
//! architect（建书，复用 67 号确认执行器——文本/details 已逐字；revise 模式
//! 复用 72 号 reviseFoundation 链）/ writer（单章 + 多章连写，复用 write_next 链）/
//! auditor（复用 audit 链）/ reviser（复用 47 号 /revise 审核环——五模式 +
//! revision gate 诊断文本）/ exporter（复用 export 工件链 + 落盘）。

use serde_json::{json, Value};

use crate::interaction::import_chapters_tool::resolve_tool_book_id;
use crate::interaction::project_tools::{error_result, ToolResult};
use crate::server::books_routes::{BooksRuntime, ReviseChainResult, RevisionDiagnostics};

pub const SUB_AGENTS: &[&str] = &["architect", "writer", "auditor", "reviser", "exporter"];

/// 工具依赖：runtime + 活动书 + 会话语言（双语守卫文案）+ 聊天轮中止句柄
/// （101 号：writer 委托在链内安全点响应中止——TS runPipelineWithAbortSignal
/// 把聊天轮 signal 注入 pipeline 的等价物）。
pub struct SubAgentDeps<'a> {
    pub runtime: &'a BooksRuntime,
    pub active_book_id: Option<&'a str>,
    pub language: &'a str,
    pub abort: Option<crate::interaction::agent_loop::AbortHandle>,
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

/// `sub_agent` 执行器。
pub async fn tool_sub_agent(deps: &SubAgentDeps<'_>, args: &Value) -> ToolResult {
    let agent = args.get("agent").and_then(Value::as_str).unwrap_or_default();
    if !SUB_AGENTS.contains(&agent) {
        return error_result(format!(
            "Invalid sub_agent.agent: {agent}（expected one of {}）",
            SUB_AGENTS.join(" | ")
        ));
    }
    let instruction = field_str(args, "instruction").unwrap_or_default();
    if instruction.is_empty() {
        return error_result("sub_agent requires an instruction argument".to_string());
    }
    // 无活动书：只有 architect 可用。
    if deps.active_book_id.is_none() && agent != "architect" {
        return text_result(
            "No active book. Only the architect agent can create a book from this session.",
            None,
        );
    }
    // 有活动书：architect 建书分支拒绝（revise 模式放行，走架构稿重写链）。
    let architect_revise = args.get("revise").and_then(Value::as_bool) == Some(true);
    if deps.active_book_id.is_some() && agent == "architect" && !architect_revise {
        let is_en = deps.language == "en";
        let message = if is_en {
            "This session already has a book, so no new book is needed. To create a new book, go back to the home page first."
        } else {
            "当前已有书籍，不需要建书。如果你想创建新书，请先回到首页。"
        };
        return text_result(message, None);
    }
    match agent {
        "architect" if architect_revise => architect_revise_foundation(deps, args, instruction).await,
        "architect" => architect_create(deps, args, instruction).await,
        "writer" => writer(deps, args).await,
        "auditor" => auditor(deps, args).await,
        "reviser" => reviser(deps, args, instruction).await,
        "exporter" => exporter(deps, args, instruction).await,
        _ => error_result(format!("Unknown agent: {agent}")),
    }
}

/// architect：建书（复用 67 号确认执行器，文本/details 已逐字）。
async fn architect_create(deps: &SubAgentDeps<'_>, args: &Value, instruction: &str) -> ToolResult {
    let Some(title) = field_str(args, "title") else {
        return text_result("Error: title is required for the architect agent.", None);
    };
    match crate::server::agent_production::execute_create_book(
        deps.runtime,
        instruction,
        title,
        None,
        &mut |_| {},
    )
    .await
    {
        Ok(outcome) => text_result(outcome.text, Some(outcome.details)),
        Err(message) => error_result(message),
    }
}

/// architect.revise：架构稿重写（复用 72 号 reviseFoundation 链：备份 →
/// architect 修订 → 容错审核 → Revise 模式落盘）。文案逐字对齐 TS。
async fn architect_revise_foundation(
    deps: &SubAgentDeps<'_>,
    args: &Value,
    instruction: &str,
) -> ToolResult {
    if deps.active_book_id.is_none() {
        return text_result("Open the book first before revising its foundation.", None);
    }
    let book_id = match resolve_tool_book_id("architect", field_str(args, "bookId"), deps.active_book_id) {
        Ok(book_id) => book_id,
        Err(message) => return error_result(message),
    };
    // TS `feedback ?? instruction`：feedback 缺省/空串回退 instruction。
    let feedback = field_str(args, "feedback").unwrap_or(instruction);
    match crate::server::book_create_routes::revise_foundation_chain(deps.runtime, &book_id, feedback).await {
        Ok(()) => {
            let message = if deps.language == "en" {
                format!(
                    "Book \"{book_id}\" foundation has been rewritten as requested. The previous itemized foundation was backed up to story/.backup-phase4-<timestamp>/."
                )
            } else {
                format!(
                    "Book \"{book_id}\" 架构稿已按要求重写。原书的条目式架构稿已备份到 story/.backup-phase4-<时间戳>/。"
                )
            };
            text_result(message, None)
        }
        Err(message) => error_result(message),
    }
}

/// writer：单章 / 多章连写（复用 write_next 链 + Manual 审核模式装配）。
async fn writer(deps: &SubAgentDeps<'_>, args: &Value) -> ToolResult {
    let book_id = match resolve_tool_book_id("writer", field_str(args, "bookId"), deps.active_book_id) {
        Ok(book_id) => book_id,
        Err(message) => return error_result(message),
    };
    let chapter_count = args
        .get("chapterCount")
        .and_then(Value::as_f64)
        .filter(|v| v.fract() == 0.0 && (1.0..=20.0).contains(v))
        .map(|v| v as u32)
        .unwrap_or(1);
    let word_count = args
        .get("chapterWordCount")
        .and_then(Value::as_f64)
        .filter(|v| v.fract() == 0.0 && *v > 0.0)
        .map(|v| v as u32);
    let runtime = deps.runtime;
    let agents = crate::server::books_routes::build_write_next_agents(runtime).await;
    let ctx = crate::server::books_routes::build_write_next_ctx(runtime);
    // 默认 Auto 审核模式（TS writeNextChapter 语义：审后 ready-for-review）。
    let config = crate::pipeline::write_next::WriteNextConfig {
        abort: deps.abort.clone(),
        ..crate::pipeline::write_next::WriteNextConfig::from_project(
            deps.runtime.state.project_root(),
        )
        .await
    };
    let aborted_message = || -> String {
        if deps.language == "en" {
            "Operation aborted: the user requested to stop this task.".to_string()
        } else {
            "操作已中止：用户请求停止该任务。".to_string()
        }
    };
    let mut chapters: Vec<Value> = Vec::new();
    let mut stopped_status: Option<&'static str> = None;
    let mut last_chapter_number = 0u32;
    for _ in 0..chapter_count {
        if deps
            .abort
            .as_ref()
            .is_some_and(|flag| *flag.lock().unwrap())
        {
            return error_result(aborted_message());
        }
        match crate::pipeline::write_next::write_next_chapter(
            &runtime.state,
            &agents,
            &ctx,
            &config,
            &book_id,
            word_count,
            None,
            None,
        )
        .await
        {
            Ok(result) => {
                last_chapter_number = result.chapter_number;
                chapters.push(json!({
                    "chapterNumber": result.chapter_number,
                    "title": result.title,
                    "wordCount": result.word_count,
                    "status": result.status,
                }));
                if result.status != "ready-for-review" {
                    stopped_status = Some(result.status);
                    break;
                }
            }
            Err(error) => {
                // 链内安全点中止 → 双语逐字文案；其余错误面保持 to_string。
                if matches!(error, crate::pipeline::write_next::WriteNextError::Aborted) {
                    return error_result(aborted_message());
                }
                return error_result(error.to_string());
            }
        }
    }
    if chapter_count > 1 {
        let completed = chapters.len();
        let text = if let Some(status) = stopped_status {
            format!(
                "Writer completed {completed} of {chapter_count} requested chapters for \"{book_id}\" and stopped because chapter {last_chapter_number} ended with status \"{status}\"."
            )
        } else {
            format!("Writer completed {completed} consecutive chapters for \"{book_id}\".")
        };
        let mut details = json!({
            "kind": "chapters_written",
            "bookId": book_id,
            "requestedCount": chapter_count,
            "completedCount": completed,
            "chapters": chapters,
        });
        if let Some(status) = stopped_status {
            details
                .as_object_mut()
                .unwrap()
                .insert("stoppedStatus".into(), json!(status));
        }
        return text_result(text, Some(details));
    }
    // 单章：status 非 ready-for-review 且非 active → 需复查提示。
    let last = chapters.first().cloned().unwrap_or(Value::Null);
    let status = last.get("status").and_then(Value::as_str).unwrap_or_default();
    let word_count_value = last.get("wordCount").cloned().unwrap_or(json!("unknown"));
    let message = if !status.is_empty() && status != "ready-for-review" && status != "active" {
        format!(
            "Chapter output for \"{book_id}\" ended with status \"{status}\" and needs review before it is treated as complete. Word count: {word_count_value}."
        )
    } else {
        format!("Chapter written for \"{book_id}\". Word count: {word_count_value}.")
    };
    text_result(
        message,
        Some(json!({
            "kind": "chapter_written",
            "bookId": book_id,
            "chapterNumber": last.get("chapterNumber"),
            "title": last.get("title"),
            "wordCount": word_count_value,
            "status": status,
        })),
    )
}

/// auditor：审一章（缺省最新章）。
async fn auditor(deps: &SubAgentDeps<'_>, args: &Value) -> ToolResult {
    let book_id = match resolve_tool_book_id("auditor", field_str(args, "bookId"), deps.active_book_id) {
        Ok(book_id) => book_id,
        Err(message) => return error_result(message),
    };
    let chapter_number = match args
        .get("chapterNumber")
        .and_then(Value::as_f64)
        .filter(|v| v.fract() == 0.0 && *v > 0.0)
    {
        Some(value) => value as u32,
        None => match deps.runtime.state.get_next_chapter_number(&book_id).await {
            Ok(next) if next > 1 => next - 1,
            _ => return error_result(format!("No chapters found in book \"{book_id}\".")),
        },
    };
    let audit_runtime = crate::server::audit_route::AuditRuntime {
        hub: deps.runtime.hub.clone(),
        state: deps.runtime.state.clone(),
        router: deps.runtime.effective_router().await.clone(),
        builtin_genres_dir: deps.runtime.builtin_genres_dir.clone(),
    };
    match crate::server::audit_route::run_audit_flow(&audit_runtime, &book_id, chapter_number, None).await {
        Ok(audit) => {
            let issue_lines: Vec<String> = audit
                .issues
                .iter()
                .map(|issue| {
                    format!("[{:?}] {}", issue.severity, issue.description)
                })
                .collect();
            let passed = if audit.passed { "PASSED" } else { "FAILED" };
            let mut text = format!(
                "Audit chapter {chapter_number}: {passed}, {} issue(s).",
                audit.issues.len()
            );
            if !issue_lines.is_empty() {
                text.push('\n');
                text.push_str(&issue_lines.join("\n"));
            }
            text_result(text, None)
        }
        Err(error) => error_result(error.to_string()),
    }
}

/// reviser：修一章（缺省最新章；五模式 spot-fix/polish/rewrite/rework/anti-detect，
/// 缺省 spot-fix）。复用 47 号 /revise 审核环，gate 用 runtime 配置（TS sub_agent
/// → pipeline.reviseDraft 的 `config.revisionGate ?? "strict"`）。文本/details 逐字。
async fn reviser(deps: &SubAgentDeps<'_>, args: &Value, instruction: &str) -> ToolResult {
    let book_id = match resolve_tool_book_id("reviser", field_str(args, "bookId"), deps.active_book_id) {
        Ok(book_id) => book_id,
        Err(message) => return error_result(message),
    };
    // TS reviseDraft：显式 0/负数与"最新章 - 1 < 1"同报"无章可修"。
    let no_chapters = error_result(format!("No chapters to revise for \"{book_id}\""));
    let chapter_number = match args
        .get("chapterNumber")
        .and_then(Value::as_f64)
        .filter(|v| v.fract() == 0.0)
    {
        Some(value) if value >= 1.0 => value as u32,
        Some(_) => return no_chapters,
        None => match deps.runtime.state.get_next_chapter_number(&book_id).await {
            Ok(next) if next > 1 => next - 1,
            Ok(_) => return no_chapters,
            Err(error) => return error_result(error.to_string()),
        },
    };
    let mode = field_str(args, "mode").unwrap_or("spot-fix").to_string();
    let body = crate::server::books_routes::ReviseBody {
        mode: Some(mode.clone()),
        brief: Some(instruction.to_string()),
    };
    match crate::server::books_routes::run_revise_chain(
        deps.runtime,
        &book_id,
        chapter_number,
        &body,
        deps.runtime.revision_gate,
    )
    .await
    {
        Ok(result) => {
            let details = revision_details_json(&book_id, &mode, &result);
            if result.applied {
                text_result(
                    format!("Revision ({mode}) complete for \"{book_id}\" chapter {chapter_number}."),
                    Some(details),
                )
            } else {
                // TS：skippedReason ?? status ?? 固定回退；诊断文本前置空行块。
                let diagnostic_text = result
                    .revision_diagnostics
                    .as_ref()
                    .map(revision_diagnostics_text)
                    .unwrap_or_default();
                let reason = result
                    .skipped_reason
                    .clone()
                    .unwrap_or_else(|| result.status.to_string());
                text_result(
                    format!(
                        "Revision not applied for \"{book_id}\" chapter {chapter_number}: {reason}.{diagnostic_text}"
                    ),
                    Some(details),
                )
            }
        }
        Err(error) => error_result(error.message().to_string()),
    }
}

/// chapter_revision details（TS 键序；skippedReason/revisionDiagnostics 无值
/// 不出现，对齐 JSON.stringify 省略 undefined）。
fn revision_details_json(book_id: &str, mode: &str, result: &ReviseChainResult) -> Value {
    let mut details = json!({
        "kind": "chapter_revision",
        "bookId": book_id,
        "chapterNumber": result.chapter_number,
        "mode": mode,
        "applied": result.applied,
        "status": result.status,
        "wordCount": result.word_count,
        "fixedIssues": result.fixed_issues,
    });
    let object = details.as_object_mut().unwrap();
    if let Some(reason) = &result.skipped_reason {
        object.insert("skippedReason".into(), json!(reason));
    }
    if let Some(diagnostics) = &result.revision_diagnostics {
        if let Ok(value) = serde_json::to_value(diagnostics) {
            object.insert("revisionDiagnostics".into(), value);
        }
    }
    details
}

/// 门控诊断文本（TS diagnosticText 逐字：空行 + Revision gate + 前后计数 +
/// 剩余问题缩进列表）。
fn revision_diagnostics_text(diagnostics: &RevisionDiagnostics) -> String {
    let mut lines = vec![
        String::new(),
        "Revision gate:".to_string(),
        format!("- Standard: {}", diagnostics.standard),
        format!(
            "- Before: blocking={}, critical={}, aiTell={}",
            diagnostics.before.blocking_count,
            diagnostics.before.critical_count,
            diagnostics.before.ai_tell_count
        ),
        format!(
            "- After: blocking={}, critical={}, aiTell={}",
            diagnostics.after.blocking_count,
            diagnostics.after.critical_count,
            diagnostics.after.ai_tell_count
        ),
    ];
    if !diagnostics.remaining_issues.is_empty() {
        lines.push("- Remaining issues:".to_string());
        for issue in &diagnostics.remaining_issues {
            let suggestion = issue
                .suggestion
                .as_deref()
                .filter(|s| !s.is_empty())
                .map(|s| format!(" ({s})"))
                .unwrap_or_default();
            lines.push(format!(
                "  - [{}] {}: {}{}",
                issue.severity, issue.category, issue.description, suggestion
            ));
        }
    }
    lines.join("\n")
}

/// exporter：导出工件（format/approvedOnly 可从 instruction 推断）+ 落盘。
async fn exporter(deps: &SubAgentDeps<'_>, args: &Value, instruction: &str) -> ToolResult {
    let book_id = match resolve_tool_book_id("exporter", field_str(args, "bookId"), deps.active_book_id) {
        Ok(book_id) => book_id,
        Err(message) => return error_result(message),
    };
    let format_arg = field_str(args, "format");
    let inferred_format = match format_arg {
        Some(format) => format.to_string(),
        None => {
            let lower = instruction.to_lowercase();
            if lower.contains("epub") {
                "epub".to_string()
            } else if lower.contains("markdown") || instruction.contains(" md ") || instruction.contains("MD") {
                "md".to_string()
            } else {
                "txt".to_string()
            }
        }
    };
    let approved_only = match args.get("approvedOnly").and_then(Value::as_bool) {
        Some(value) => value,
        None => instruction.contains("approved")
            || instruction.contains("已通过")
            || instruction.contains("通过章节"),
    };
    let format = crate::interaction::export_artifact::ExportFormat::parse(Some(&inferred_format));
    match crate::interaction::export_artifact::build_export_artifact(
        &deps.runtime.state,
        &book_id,
        format,
        approved_only,
        None,
    )
    .await
    {
        Ok(artifact) => {
            // writeExportArtifact：落盘到工件输出路径。
            if let Some(parent) = std::path::Path::new(&artifact.output_path).parent() {
                let _ = tokio::fs::create_dir_all(parent).await;
            }
            if let Err(e) = tokio::fs::write(&artifact.output_path, &artifact.payload).await {
                return error_result(format!("Export failed: {e}"));
            }
            text_result(
                format!(
                    "Exported \"{book_id}\": {} chapters, {} words → {}",
                    artifact.chapters_exported, artifact.total_words, artifact.output_path
                ),
                None,
            )
        }
        Err(_) => error_result("Export failed".to_string()),
    }
}

/// `sub_agent` schema（SubAgentParams 逐字）。
pub fn sub_agent_schema() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "sub_agent",
            "description": "Delegate a heavy operation to a specialised sub-agent. Use agent='architect' to initialise a new book, 'writer' to write the next chapter, 'auditor' to audit quality, 'reviser' to revise a chapter, 'exporter' to export.",
            "parameters": {
                "type": "object",
                "properties": {
                    "agent": { "type": "string", "enum": SUB_AGENTS },
                    "instruction": { "type": "string", "description": "Natural language instruction for the sub-agent. For reviser, this is passed as the one-off revision brief." },
                    "bookId": { "type": "string", "description": "Optional book ID. In active-book sessions, omit it to use the current active book; if provided, it must match the current active book. For architect creation, this optionally sets the new book ID." },
                    "chapterNumber": { "type": "number", "description": "auditor/reviser: target chapter number. Omit to use the latest chapter." },
                    "chapterCount": { "type": "integer", "minimum": 1, "maximum": 20, "description": "writer only: number of consecutive new chapters to write in this operation. Default: 1. InkOS writes them sequentially under one book lock." },
                    "title": { "type": "string", "description": "architect only: explicit book title. Required when creating a book." },
                    "genre": { "type": "string", "description": "architect only: genre (xuanhuan, urban, mystery, romance, scifi, fantasy, wuxia, general, etc.)" },
                    "platform": { "type": "string", "enum": ["tomato", "qidian", "feilu", "other"], "description": "architect only: target platform. Default: other" },
                    "language": { "type": "string", "enum": ["zh", "en"], "description": "architect only: writing language. Default: zh" },
                    "targetChapters": { "type": "number", "description": "architect only: total chapter count. Default: 200" },
                    "chapterWordCount": { "type": "number", "description": "architect/writer: per-chapter length in the book's native unit (zh characters / en words). Default: 3000 zh, 2000 en" },
                    "revise": { "type": "boolean", "description": "architect only: true 表示在当前 active book 上重新生成架构稿，而不是新建书籍。no-book creation sessions cannot revise an existing book." },
                    "feedback": { "type": "string", "description": "architect only: revise 模式下的调整要求。举例：把架构稿从条目式升级成段落式架构稿、某个角色设定需要重新设计、主线冲突表达太弱需要加强等。如果是架构稿评审未通过要求重写的场景，把评审意见的 overallFeedback 原样传入即可" },
                    "mode": { "type": "string", "enum": ["spot-fix", "polish", "rewrite", "rework", "anti-detect"], "description": "reviser only: revision mode. Default: spot-fix" },
                    "format": { "type": "string", "enum": ["txt", "md", "epub"], "description": "exporter only: export format. Default: txt" },
                    "approvedOnly": { "type": "boolean", "description": "exporter only: export only approved chapters. Default: false" },
                },
                "required": ["agent", "instruction"],
            },
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn schema_shape() {
        let schema = sub_agent_schema();
        assert_eq!(schema["function"]["name"], "sub_agent");
        assert_eq!(schema["function"]["parameters"]["required"], json!(["agent", "instruction"]));
        assert_eq!(
            schema["function"]["parameters"]["properties"]["agent"]["enum"].as_array().unwrap().len(),
            5
        );
    }

    #[test]
    fn sub_agent_enum_and_instruction_validation() {
        let tokio_rt = tokio::runtime::Runtime::new().unwrap();
        let root = std::env::temp_dir().join("sub87-nonexistent");
        let hub = Arc::new(crate::server::sse::BroadcastHub::new());
        let state = Arc::new(crate::state::manager::StateManager::new(root.clone()));
        let router = Arc::new(crate::llm::agent_router::AgentRouter::new(
            crate::llm::agent_router::LlmEndpointConfig {
                base_url: "http://127.0.0.1:9".into(),
                api_key: "k".into(),
                model: "m".into(),
                max_tokens: 16,
                extra_headers: std::collections::HashMap::new(),
            },
            std::collections::HashMap::new(),
        ));
        let books = crate::server::books_routes::BooksRuntime {
            hub,
            state,
            router,
            builtin_genres_dir: root.join("assets").join("genres"),
            revision_gate: Default::default(),
        };
        let deps = SubAgentDeps { runtime: &books, active_book_id: None, language: "zh", abort: None };
        // 非法 agent / 缺 instruction。
        let bad = tokio_rt.block_on(tool_sub_agent(&deps, &json!({ "agent": "bogus", "instruction": "x" })));
        assert!(bad.is_error && bad.text.contains("Invalid sub_agent.agent"));
        let missing = tokio_rt.block_on(tool_sub_agent(&deps, &json!({ "agent": "writer" })));
        assert!(missing.is_error && missing.text.contains("requires an instruction"));
        // 无书守卫（writer/auditor/exporter）。
        let no_book = tokio_rt.block_on(tool_sub_agent(&deps, &json!({ "agent": "writer", "instruction": "写一章" })));
        assert_eq!(
            no_book.text,
            "No active book. Only the architect agent can create a book from this session."
        );
        assert!(!no_book.is_error);
        // 有书时 architect 建书拒绝（双语；revise=true 放行不走此文案）。
        let with_book = SubAgentDeps { runtime: &books, active_book_id: Some("b1"), language: "zh", abort: None };
        let rejected = tokio_rt.block_on(tool_sub_agent(&with_book, &json!({ "agent": "architect", "instruction": "建书" })));
        assert_eq!(rejected.text, "当前已有书籍，不需要建书。如果你想创建新书，请先回到首页。");
        // reviser 无章可修（书不存在 → 最新章 - 1 = 0，TS 链内错误文本）。
        let reviser = tokio_rt.block_on(tool_sub_agent(&with_book, &json!({ "agent": "reviser", "instruction": "改一章" })));
        assert!(reviser.is_error);
        assert_eq!(reviser.text, "No chapters to revise for \"b1\"");
        // reviser 显式 0 章同报无章可修。
        let zero = tokio_rt.block_on(tool_sub_agent(&with_book, &json!({ "agent": "reviser", "instruction": "改一章", "chapterNumber": 0 })));
        assert!(zero.is_error);
        assert_eq!(zero.text, "No chapters to revise for \"b1\"");
        // architect.revise 无书守卫（TS 逐字）。
        let no_book_revise = tokio_rt.block_on(tool_sub_agent(
            &deps,
            &json!({ "agent": "architect", "instruction": "重写架构稿", "revise": true }),
        ));
        assert_eq!(no_book_revise.text, "Open the book first before revising its foundation.");
        assert!(!no_book_revise.is_error);
        // architect 缺 title。
        let no_title = tokio_rt.block_on(tool_sub_agent(&deps, &json!({ "agent": "architect", "instruction": "建书" })));
        assert_eq!(no_title.text, "Error: title is required for the architect agent.");
    }

    /// 拒绝分支的 details/诊断文本逐字（fabricated 门控拒绝结果）。
    #[test]
    fn revision_refusal_details_and_diagnostics_text() {
        use crate::server::books_routes::{GateCounts, RemainingIssue};
        let result = ReviseChainResult {
            chapter_number: 3,
            word_count: 2890,
            fixed_issues: Vec::new(),
            applied: false,
            status: "unchanged",
            skipped_reason: Some(
                "Manual revision kept original chapter: before blocking=2, critical=1, aiTell=1; after blocking=3, critical=1, aiTell=0.".to_string(),
            ),
            revision_diagnostics: Some(RevisionDiagnostics {
                standard: crate::pipeline::merged_audit::RevisionGate::Strict.standard(),
                before: GateCounts { blocking_count: 2, critical_count: 1, ai_tell_count: 1 },
                after: GateCounts { blocking_count: 3, critical_count: 1, ai_tell_count: 0 },
                remaining_issues: vec![RemainingIssue {
                    severity: "warning".to_string(),
                    category: "节奏".to_string(),
                    description: "中段推进略缓。".to_string(),
                    suggestion: Some("压缩。".to_string()),
                }],
            }),
            revised_content: None,
        };
        let details = revision_details_json("b1", "polish", &result);
        assert_eq!(details["kind"], "chapter_revision");
        assert_eq!(details["chapterNumber"], 3);
        assert_eq!(details["mode"], "polish");
        assert_eq!(details["applied"], false);
        assert_eq!(details["status"], "unchanged");
        assert!(details["skippedReason"].as_str().unwrap().starts_with("Manual revision kept"));
        let diagnostics = &details["revisionDiagnostics"];
        assert_eq!(diagnostics["before"]["blockingCount"], 2);
        assert_eq!(diagnostics["after"]["aiTellCount"], 0);
        assert_eq!(diagnostics["remainingIssues"].as_array().unwrap().len(), 1);
        assert_eq!(diagnostics["remainingIssues"][0]["suggestion"], "压缩。");

        let text = revision_diagnostics_text(result.revision_diagnostics.as_ref().unwrap());
        assert!(text.starts_with("\nRevision gate:\n- Standard: "), "{text}");
        assert!(text.contains("- Before: blocking=2, critical=1, aiTell=1"), "{text}");
        assert!(text.contains("- After: blocking=3, critical=1, aiTell=0"), "{text}");
        assert!(
            text.contains("- Remaining issues:\n  - [warning] 节奏: 中段推进略缓。 (压缩。)"),
            "{text}"
        );

        // applied 分支：details 无 skippedReason/revisionDiagnostics 键。
        let applied = ReviseChainResult {
            chapter_number: 3,
            word_count: 3010,
            fixed_issues: vec!["修正了措辞".to_string()],
            applied: true,
            status: "ready-for-review",
            skipped_reason: None,
            revision_diagnostics: None,
            revised_content: Some("正文".to_string()),
        };
        let details = revision_details_json("b1", "spot-fix", &applied);
        assert_eq!(details["applied"], true);
        assert!(details.get("skippedReason").is_none());
        assert!(details.get("revisionDiagnostics").is_none());
        assert_eq!(details["fixedIssues"].as_array().unwrap().len(), 1);
    }
}
