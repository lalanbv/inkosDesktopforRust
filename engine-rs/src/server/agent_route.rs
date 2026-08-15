//! POST /api/v1/agent（server.ts L4805）——交互 agent 端点。
//!
//! 65 号：完整校验链 + 会话装配（404/409/kind 更新/书存在性）。
//! 66 号：agent 循环主路径（多轮 tool-use + SSE 增量 + abort 截断）。
//! 67 号：参数面补齐（actionSource/requestedIntent/actionPayload/sourceRequestId/
//! requestedSkills）+ **确认式生产任务分支**（write_next / create_book 意图执行器，
//! 见 [`crate::server::agent_production`]）+ 聊天分支背景任务上下文注入。

use std::sync::{Arc, Mutex, OnceLock};

use axum::body::Bytes;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde_json::{json, Map, Value};

use crate::interaction::agent_loop::{run_agent_loop, LoopChat, LoopEvents, LoopToolExecution};
use crate::interaction::book_session_store::{
    create_and_persist_book_session, load_book_session,
};
use crate::interaction::session::{utc_now_ms, SessionKind};
use crate::interaction::session_transcript::{append_transcript_events, TranscriptEvent};
use crate::llm::provider::LLMMessage;
use crate::server::agent_production::{
    self, normalize_action_source, normalize_requested_intent, resolve_confirmed_intent,
    ProductionRequest,
};
use crate::server::books_routes::BooksRuntime;
use crate::server::session_routes::{normalize_api_book_id, normalize_studio_session_kind};

/// 会话聊天轮的运行标记（abort 端点置位；与 agent loop 轮询的是**同一个**
/// Arc<Mutex<bool>>——68 号连通，置位即在下一轮检查点截断）。
pub struct AgentSessionHandle {
    pub abort_flag: crate::interaction::agent_loop::AbortHandle,
}

/// abort 注册表（sessionId → handle；64 号 abort 端点的消费面）。
pub fn running_agent_sessions() -> &'static Mutex<std::collections::HashMap<String, Arc<Mutex<AgentSessionHandle>>>> {
    static REGISTRY: OnceLock<Mutex<std::collections::HashMap<String, Arc<Mutex<AgentSessionHandle>>>>> =
        OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

fn api_error(status: StatusCode, code: &str, message: impl Into<String>) -> (StatusCode, Json<Value>) {
    (
        status,
        Json(json!({ "error": { "code": code, "message": message.into() } })),
    )
}

/// `normalizeSkillIdList`：单值/数组 → trim + lower + `^[a-z][a-z0-9-]*$` +
/// 去重保序；非字符串/非法格式 → Err。
fn normalize_skill_id_list(value: Option<&Value>) -> Result<Vec<String>, String> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let items: Vec<&Value> = match value {
        Value::Array(items) => items.iter().collect(),
        single => vec![single],
    };
    let mut out: Vec<String> = Vec::new();
    for item in items {
        let Some(text) = item.as_str() else {
            return Err("Skill id must use letters, numbers, and hyphens.".to_string());
        };
        let trimmed = text.trim().to_lowercase();
        if trimmed.is_empty() {
            return Err("Skill id must use letters, numbers, and hyphens.".to_string());
        }
        let valid = trimmed.starts_with(|c: char| c.is_ascii_lowercase())
            && trimmed
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if !valid {
            return Err(format!(
                "Skill id must use letters, numbers, and hyphens.: {text}"
            ));
        }
        if !out.contains(&trimmed) {
            out.push(trimmed);
        }
    }
    Ok(out)
}

/// manualToolAssistantMessage 的 provider/model 标签：请求级 service/model
/// 回退项目 LLM 配置（configuredEntry 面随 service 域深化接线，67 号取原始值）。
async fn production_provider_model_labels(
    root: &std::path::Path,
    payload: &Value,
) -> (String, String) {
    let config = crate::server::project_config_routes::load_raw_config(root).await;
    let llm = config.as_ref().and_then(|c| c.get("llm"));
    let provider = payload
        .get("service")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .or_else(|| {
            llm.and_then(|llm| {
                llm.get("service")
                    .or_else(|| llm.get("provider"))
                    .and_then(Value::as_str)
                    .map(String::from)
            })
        })
        .unwrap_or_else(|| "inkos".to_string());
    let model = payload
        .get("model")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .or_else(|| {
            llm.and_then(|llm| llm.get("model").and_then(Value::as_str).map(String::from))
        })
        .unwrap_or_else(|| "studio-agent".to_string());
    (provider, model)
}

/// appendManualSessionMessages 等价：request_started → user → assistant →
/// request_committed 四事件（65 号消费面：直通聊天轮持久化）。
async fn append_chat_turn(
    project_root: &std::path::Path,
    session_id: &str,
    instruction: &str,
    response_text: &str,
    session_kind: SessionKind,
) {
    let request_id = uuid::Uuid::new_v4().to_string();
    let now = utc_now_ms();
    append_transcript_events(project_root, session_id, |_events, next_seq| {
        let assistant_uuid = uuid::Uuid::new_v4().to_string();
        let user_uuid = uuid::Uuid::new_v4().to_string();
        vec![
            TranscriptEvent::RequestStarted {
                version: 1,
                session_id: session_id.to_string(),
                seq: next_seq,
                timestamp: now,
                request_id: request_id.clone(),
                session_kind: Some(session_kind),
                input: instruction.to_string(),
            },
            TranscriptEvent::Message {
                version: 1,
                session_id: session_id.to_string(),
                request_id: request_id.clone(),
                uuid: user_uuid.clone(),
                parent_uuid: None,
                seq: next_seq + 1,
                timestamp: now,
                role: "user".into(),
                pi_turn_index: None,
                tool_call_id: None,
                source_tool_assistant_uuid: None,
                legacy_display: None,
                message: json!({ "role": "user", "content": instruction, "timestamp": now }),
            },
            TranscriptEvent::Message {
                version: 1,
                session_id: session_id.to_string(),
                request_id: request_id.clone(),
                uuid: assistant_uuid,
                parent_uuid: Some(user_uuid),
                seq: next_seq + 2,
                timestamp: now + 1,
                role: "assistant".into(),
                pi_turn_index: None,
                tool_call_id: None,
                source_tool_assistant_uuid: None,
                legacy_display: None,
                message: json!({
                    "role": "assistant",
                    "content": [{ "type": "text", "text": response_text }],
                    "api": "openai-completions",
                    "provider": "inkos",
                    "model": "studio-agent",
                    "usage": { "input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0, "totalTokens": 0,
                               "cost": { "input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0, "total": 0 } },
                    "stopReason": "stop",
                    "timestamp": now + 1,
                }),
            },
            TranscriptEvent::RequestCommitted {
                version: 1,
                session_id: session_id.to_string(),
                seq: next_seq + 3,
                timestamp: now + 1,
                request_id,
            },
        ]
    })
    .await;
}

pub async fn post_agent(
    State(runtime): State<BooksRuntime>,
    body: Bytes,
) -> impl IntoResponse {
    let root = runtime.state.project_root();
    let Ok(payload) = serde_json::from_slice::<Value>(&body) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "No instruction provided" })),
        )
            .into_response();
    };
    let instruction = payload
        .get("instruction")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if instruction.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "No instruction provided" })),
        )
            .into_response();
    }
    let Some(session_id) = payload
        .get("sessionId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
    else {
        return api_error(StatusCode::BAD_REQUEST, "SESSION_ID_REQUIRED", "sessionId is required")
            .into_response();
    };

    // ── 参数归一（normalizeStudio* 包装；非法 → 400 INVALID_*） ──
    let action_source = match normalize_action_source(payload.get("actionSource")) {
        Ok(source) => source,
        Err(message) => return api_error(StatusCode::BAD_REQUEST, "INVALID_ACTION_SOURCE", message).into_response(),
    };
    let requested_intent = match normalize_requested_intent(payload.get("requestedIntent")) {
        Ok(intent) => intent,
        Err(message) => return api_error(StatusCode::BAD_REQUEST, "INVALID_REQUESTED_INTENT", message).into_response(),
    };
    // actionPayload：TS zod strict 全量校验（67 号结构守卫：必须为 object）。
    let action_payload = match payload.get("actionPayload") {
        None | Some(Value::Null) => None,
        Some(value) if value.is_object() => Some(value),
        Some(other) => {
            return api_error(
                StatusCode::BAD_REQUEST,
                "INVALID_ACTION_PAYLOAD",
                format!("Invalid actionPayload: {other}"),
            )
            .into_response()
        }
    };
    // requestedSkills / disabledSkills（normalizeSkillIdList：单值/数组 → trim/lower
    // + `^[a-z][a-z0-9-]*$` 校验 + 去重）。
    let requested_skills = match normalize_skill_id_list(payload.get("requestedSkills")) {
        Ok(skills) => skills,
        Err(message) => return api_error(StatusCode::BAD_REQUEST, "INVALID_SKILL_ID", message).into_response(),
    };
    let _disabled_skills = match normalize_skill_id_list(payload.get("disabledSkills")) {
        Ok(skills) => skills,
        Err(message) => return api_error(StatusCode::BAD_REQUEST, "INVALID_SKILL_ID", message).into_response(),
    };
    // sourceRequestId：clientRequestId trim + 128 码元截断。
    let source_request_id: Option<String> = payload
        .get("clientRequestId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.chars().take(128).collect());

    // ── 会话装配（404 / 409 / kind 更新 / 书存在性） ──────────────
    let Some(book_session) = load_book_session(root, session_id).await else {
        return api_error(
            StatusCode::NOT_FOUND,
            "SESSION_NOT_FOUND",
            format!("Session not found: {session_id}"),
        )
        .into_response();
    };
    let requested_active_book_id =
        match normalize_api_book_id(payload.get("activeBookId"), "activeBookId") {
            Ok(book_id) => book_id,
            Err(response) => return response.into_response(),
        };
    let persisted_book_id = book_session.book_id.clone();
    if let (Some(requested), Some(persisted)) = (&requested_active_book_id, &persisted_book_id) {
        if persisted != requested {
            return api_error(
                StatusCode::CONFLICT,
                "SESSION_BOOK_MISMATCH",
                format!("Session {session_id} is bound to {persisted}, not {requested}"),
            )
            .into_response();
        }
    }
    let agent_book_id = requested_active_book_id.or(persisted_book_id);
    let fallback_kind = agent_book_id
        .as_ref()
        .map(|_| SessionKind::Book)
        .unwrap_or(SessionKind::Chat);
    let stored_kind = book_session.session_kind;
    let session_kind = match normalize_studio_session_kind(payload.get("sessionKind"), stored_kind.unwrap_or(fallback_kind))
    {
        Ok(kind) => kind,
        Err(response) => return response.into_response(),
    };
    let play_mode = payload
        .get("playMode")
        .and_then(Value::as_str)
        .and_then(crate::interaction::session::PlayMode::parse);
    if book_session.session_kind != Some(session_kind)
        || play_mode.is_some() && book_session.play_mode != play_mode
    {
        create_and_persist_book_session(
            root,
            book_session.book_id.as_deref(),
            Some(&book_session.session_id),
            Some(session_kind),
            play_mode,
        )
        .await;
    }
    // 书存在性（loadBookConfig 失败 → 404 BOOK_NOT_FOUND）
    if let Some(book_id) = &agent_book_id {
        let book_path = root.join("books").join(book_id).join("book.json");
        if tokio::fs::metadata(&book_path).await.map(|m| !m.is_file()).unwrap_or(true) {
            return api_error(
                StatusCode::NOT_FOUND,
                "BOOK_NOT_FOUND",
                format!("Book not found: {book_id}"),
            )
            .into_response();
        }
    }

    // ── 事件面：agent:start ──────────────────────────────────────
    runtime.hub.broadcast(
        "agent:start",
        &json!({
            "instruction": instruction,
            "activeBookId": agent_book_id,
            "sessionId": session_id,
            "actionSource": action_source.as_str(),
            "requestedIntent": requested_intent.map(|intent| Value::from(intent.as_str())).unwrap_or(Value::Null),
            "requestedSkills": requested_skills,
            "attachments": 0,
        }),
    );

    // ── 确认式生产任务分支（67 号：write_next / create_book） ──────
    let confirmed_intent = resolve_confirmed_intent(
        instruction,
        agent_book_id.as_deref(),
        session_kind,
        action_source,
        requested_intent,
    );
    if let Some(intent) = confirmed_intent {
        let language = agent_production::current_project_language(root).await;
        let (provider_label, model_label) = production_provider_model_labels(root, &payload).await;
        let request = ProductionRequest {
            instruction,
            session_id,
            book_id: agent_book_id.as_deref(),
            session_kind,
            play_mode: play_mode.map(|mode| mode.as_str()),
            intent,
            action_payload,
            language,
            source_request_id: source_request_id.as_deref(),
            provider_label,
            model_label,
        };
        return match agent_production::run_confirmed_production(&runtime, request).await {
            Ok(outcome) => {
                runtime.hub.broadcast(
                    "agent:complete",
                    &json!({
                        "instruction": instruction,
                        "activeBookId": outcome.active_book_id,
                        "sessionId": session_id,
                        "sessionKind": session_kind.as_str(),
                    }),
                );
                let mut session_obj = Map::new();
                session_obj.insert("sessionId".into(), json!(session_id));
                session_obj.insert("sessionKind".into(), json!(session_kind.as_str()));
                if let Some(book_id) = &outcome.active_book_id {
                    session_obj.insert("activeBookId".into(), json!(book_id));
                }
                (
                    StatusCode::OK,
                    Json(json!({
                        "response": outcome.response_text,
                        "details": { "toolExecutions": [outcome.exec.to_json()] },
                        "session": Value::Object(session_obj),
                    })),
                )
                    .into_response()
            }
            Err(error) => {
                runtime.hub.broadcast(
                    "agent:error",
                    &json!({
                        "instruction": instruction,
                        "activeBookId": agent_book_id,
                        "sessionId": session_id,
                        "sessionKind": session_kind.as_str(),
                        "error": error.message,
                    }),
                );
                (
                    error.status,
                    Json(json!({
                        "error": { "code": error.code, "message": error.message },
                        "response": error.message,
                    })),
                )
                    .into_response()
            }
        };
    }

    // ── agent 循环主路径（66 号：多轮 tool-use + SSE 增量 + abort 截断） ──
    let abort_flag: crate::interaction::agent_loop::AbortHandle = Arc::new(Mutex::new(false));
    let handle = Arc::new(Mutex::new(AgentSessionHandle { abort_flag: abort_flag.clone() }));
    running_agent_sessions()
        .lock()
        .unwrap()
        .insert(session_id.to_string(), handle.clone());

    let mut system_prompt = format!(
        "你是 InkOS Studio 的创作助手。可以调用提供的工具查阅项目文件后回答。用与用户提问一致的语言简洁、具体地回答。{}",
        agent_book_id
            .as_ref()
            .map(|book_id| format!("当前活动书籍：{book_id}。回答时结合该书的创作上下文。"))
            .unwrap_or_default(),
    );
    // 后台生产任务与聊天并行时注入任务状态（suppressProductionTools 的硬剔除
    // 面——read/ls/grep 聊天工具集本就不含生产工具，天然满足）。
    if let Some(background_task) =
        agent_production::find_active_running_task(root, session_id).await
    {
        let language = agent_production::current_project_language(root).await;
        system_prompt.push('\n');
        system_prompt.push_str(&agent_production::build_running_task_context_block(
            &background_task,
            language,
        ));
    }

    struct RouterLoopChat<'a> {
        router: &'a crate::llm::agent_router::AgentRouter,
    }

    #[async_trait::async_trait]
    impl LoopChat for RouterLoopChat<'_> {
        async fn chat(
            &self,
            messages: &[LLMMessage],
            tools: Option<&Value>,
        ) -> Result<(String, Vec<(String, String, String)>), String> {
            let endpoint = self.router.resolve("studio-agent");
            let client = self.router.client_for_public(&endpoint).await;
            let completion = client
                .stream_chat(&crate::llm::streaming_client::ChatCompletionParams {
                    model: &endpoint.model,
                    messages,
                    temperature: 0.7,
                    max_tokens: endpoint.max_tokens,
                    stream: true,
                    extra: None,
                    tools,
                })
                .await
                .map_err(|e| e.to_string())?;
            let tool_calls = completion
                .tool_calls
                .into_iter()
                .map(|tc| (tc.id, tc.name, tc.arguments))
                .collect();
            Ok((completion.content, tool_calls))
        }
    }

    struct SseBridge<'a> {
        hub: &'a crate::server::sse::BroadcastHub,
        session_id: String,
    }

    impl LoopEvents for SseBridge<'_> {
        fn on_delta(&self, text: &str) {
            self.hub.broadcast("draft:delta", &json!({ "sessionId": self.session_id, "text": text }));
        }
        fn on_tool_start(&self, id: &str, tool: &str, args: &Value) {
            self.hub.broadcast("tool:start", &json!({
                "sessionId": self.session_id, "id": id, "tool": tool, "args": args,
                "stages": [],
            }));
        }
        fn on_tool_end(&self, id: &str, tool: &str, result_text: &str, is_error: bool) {
            self.hub.broadcast("tool:end", &json!({
                "sessionId": self.session_id, "id": id, "tool": tool,
                "result": { "content": result_text }, "isError": is_error,
            }));
        }
    }

    // ── 历史回放（68 号）：summary + 对话 + boundary 插在 system 与本轮指令间 ──
    let restored = crate::interaction::session_restore::restore_agent_messages_from_transcript(
        root,
        session_id,
        Some(session_kind.as_str()),
    )
    .await;
    let restored = crate::interaction::session_restore::append_restored_history_boundary(
        restored,
        match agent_production::current_project_language(root).await {
            agent_production::StudioLang::En => "en",
            agent_production::StudioLang::Zh => "zh",
        },
    );

    let loop_chat = RouterLoopChat { router: &runtime.router };
    let bridge = SseBridge { hub: &runtime.hub, session_id: session_id.to_string() };
    let tools = crate::interaction::project_tools::tools_payload();
    let loop_result = run_agent_loop(
        &loop_chat,
        root,
        &system_prompt,
        restored,
        instruction,
        Some(&tools),
        Some(&abort_flag),
        &bridge,
    )
    .await;

    running_agent_sessions().lock().unwrap().remove(session_id);

    let chat_result = loop_result.map(|outcome| (outcome.response_text, outcome.tool_executions));

    match chat_result {
        Ok((raw_text, tool_executions)) => {
            let response_text = if raw_text.is_empty() {
                "（无回复内容）".to_string()
            } else {
                raw_text
            };
            append_chat_turn(root, session_id, instruction, &response_text, session_kind).await;
            runtime.hub.broadcast(
                "agent:complete",
                &json!({
                    "instruction": instruction,
                    "activeBookId": agent_book_id,
                    "sessionId": session_id,
                    "sessionKind": session_kind.as_str(),
                }),
            );
            let mut session_obj = Map::new();
            session_obj.insert("sessionId".into(), json!(session_id));
            session_obj.insert("sessionKind".into(), json!(session_kind.as_str()));
            if let Some(book_id) = &agent_book_id {
                session_obj.insert("activeBookId".into(), json!(book_id));
            }
            (
                StatusCode::OK,
                Json(json!({
                    "response": response_text,
                    "details": { "toolExecutions": tool_execution_cards(&tool_executions) },
                    "session": Value::Object(session_obj),
                })),
            )
                .into_response()
        }
        Err(error) => {
            runtime.hub.broadcast(
                "agent:error",
                &json!({
                    "instruction": instruction,
                    "activeBookId": agent_book_id,
                    "sessionId": session_id,
                    "sessionKind": session_kind.as_str(),
                    "error": error,
                }),
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": { "code": "AGENT_SESSION_FAILED", "message": error },
                    "response": "Agent 会话执行失败。",
                })),
            )
                .into_response()
        }
    }
}

/// 工具执行卡 → 响应形态。
fn tool_execution_cards(executions: &[LoopToolExecution]) -> Vec<Value> {
    executions
        .iter()
        .map(|execution| {
            let mut card = json!({
                "id": execution.id,
                "tool": execution.tool,
                "label": execution.tool,
                "status": execution.status,
                "args": execution.args,
                "startedAt": execution.started_at,
            });
            let obj = card.as_object_mut().unwrap();
            if let Some(completed_at) = execution.completed_at {
                obj.insert("completedAt".into(), json!(completed_at));
            }
            if let Some(result) = &execution.result {
                obj.insert("result".into(), json!(result));
            }
            if let Some(error) = &execution.error {
                obj.insert("error".into(), json!(error));
            }
            card
        })
        .collect()
}
