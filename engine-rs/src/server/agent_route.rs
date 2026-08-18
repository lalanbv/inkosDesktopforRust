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
    // actionPayload：zod strict 全量校验（75 号：顶层+子域 unknown 键拒绝 +
    // 字段类型/枚举/区间——ActionPayloadSchema 逐字）。
    let action_payload = match payload.get("actionPayload") {
        None | Some(Value::Null) => None,
        Some(value) => match validate_action_payload_strict(value) {
            Ok(()) => Some(value),
            Err(message) => {
                return api_error(
                    StatusCode::BAD_REQUEST,
                    "INVALID_ACTION_PAYLOAD",
                    format!("Invalid actionPayload: {message}"),
                )
                .into_response()
            }
        },
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

    // ── play 会话聊天面（80 号）：世界存在 → play 工具 + play 系统提示词 ──
    let surface_language = match agent_production::current_project_language(root).await {
        agent_production::StudioLang::En => "en",
        agent_production::StudioLang::Zh => "zh",
    };
    let play_world_exists = session_kind == SessionKind::Play
        && crate::interaction::play_tools::session_world_exists(root, session_id).await;

    let mut system_prompt = if play_world_exists {
        crate::interaction::play_tools::play_chat_system_prompt(surface_language == "en")
    } else {
        format!(
            "你是 InkOS Studio 的创作助手。可以调用提供的工具查阅项目文件后回答。用与用户提问一致的语言简洁、具体地回答。{}",
            agent_book_id
                .as_ref()
                .map(|book_id| format!("当前活动书籍：{book_id}。回答时结合该书的创作上下文。"))
                .unwrap_or_default(),
        )
    };
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
    // 89 号注册矩阵对齐 TS agent-session 真值表：book/book-create（有书）
    // = bookTools；edit = 确定性五件（TS edit 过滤器去 sub_agent/
    // generate_cover/research/import）；chat/short/script/storyboard/film/
    // play 走各自分支（chat 带 book 亦然——TS 按 sessionKind 先返回，
    // bookId 不影响这些分支的工具面）。
    let has_book = agent_book_id.is_some();
    let book_session =
        has_book && matches!(session_kind, SessionKind::Book | SessionKind::BookCreate);
    let edit_session = has_book && session_kind == SessionKind::Edit;
    let book_edit_session = book_session || edit_session;
    let propose_registered =
        !play_world_exists && (session_kind == SessionKind::Chat || !has_book);
    let research_registered =
        matches!(session_kind, SessionKind::Chat | SessionKind::BookCreate | SessionKind::Book);
    let import_registered = session_kind == SessionKind::Chat || book_session;
    // 工具面：文件工具 + material 双工具（全部会话）+ propose_action（无书
    // 会话；play 有世界时除外）+ research/import（分支矩阵）+ sub_agent
    // （book/book-create）+ 编辑工具族（book/edit）+ play 三工具。
    let mut tools = crate::interaction::project_tools::tools_payload();
    if let Some(entries) = tools.as_array_mut() {
        entries.extend(crate::interaction::material_tools::material_tool_schemas());
        if propose_registered {
            entries.push(crate::interaction::propose_action_tool::propose_action_schema());
        }
        if research_registered {
            entries.push(crate::interaction::research_tool::research_tool_schema());
        }
        if import_registered {
            entries.push(crate::interaction::import_chapters_tool::import_chapters_schema());
        }
        if book_session {
            entries.push(crate::interaction::sub_agent_tool::sub_agent_schema());
        }
        if book_edit_session {
            entries.extend(crate::interaction::book_edit_tools::deterministic_tool_schemas());
        }
        if book_session {
            entries.push(crate::interaction::book_edit_tools::generate_cover_schema());
        }
        if play_world_exists {
            entries.extend(crate::interaction::play_tools::play_tool_schemas());
        }
    }
    let play_deps = play_world_exists.then(|| crate::interaction::play_tools::PlayToolDeps {
        project_root: root,
        session_id,
        router: &runtime.router,
        language: surface_language,
    });
    // propose_action：无书会话注册（play 有世界时除外；sameSession =
    // sessionKind !== "chat"）。
    let propose_deps = propose_registered.then(|| crate::interaction::propose_action_tool::ProposeDeps {
        language: surface_language,
        same_session: session_kind != SessionKind::Chat,
        requested_skills: &requested_skills,
    });
    // research：chat/book-create/book 分支；import：chat 与 book 分支。
    let research_enabled = research_registered;
    let import_deps = import_registered.then_some(ImportDeps {
        runtime: &runtime,
        active_book_id: agent_book_id.as_deref(),
    });
    // sub_agent：book/book-create 会话（architect 建书走确认面）。
    let sub_agent_deps = book_session.then_some(crate::interaction::sub_agent_tool::SubAgentDeps {
        runtime: &runtime,
        active_book_id: agent_book_id.as_deref(),
        language: surface_language,
    });
    // 编辑工具族（book/edit 会话；active_book_id 恒有）。
    let book_edit_deps = book_edit_session
        .then(|| agent_book_id.as_deref().map(|active| crate::interaction::book_edit_tools::BookEditDeps {
            runtime: &runtime,
            active_book_id: active,
        }))
        .flatten();
    let tool_executor = ChatToolRouter {
        root,
        play_deps,
        propose_deps,
        research_enabled,
        import_deps,
        sub_agent_deps,
        book_edit_deps,
    };
    let loop_result = run_agent_loop(
        &loop_chat,
        &tool_executor,
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
            if let Some(details) = &execution.details {
                obj.insert("details".into(), details.clone());
            }
            card
        })
        .collect()
}

/// import_chapters 依赖（85 号）：runtime + 活动书。
struct ImportDeps<'a> {
    runtime: &'a BooksRuntime,
    active_book_id: Option<&'a str>,
}

/// 聊天回环组合执行器（84/85/87/89 号）：propose_action → research/import
/// → sub_agent → 书会话编辑工具族 → play 工具 → 项目文件工具（含 material
/// 双件）。
struct ChatToolRouter<'a> {
    root: &'a std::path::Path,
    play_deps: Option<crate::interaction::play_tools::PlayToolDeps<'a>>,
    propose_deps: Option<crate::interaction::propose_action_tool::ProposeDeps<'a>>,
    research_enabled: bool,
    import_deps: Option<ImportDeps<'a>>,
    sub_agent_deps: Option<crate::interaction::sub_agent_tool::SubAgentDeps<'a>>,
    book_edit_deps: Option<crate::interaction::book_edit_tools::BookEditDeps<'a>>,
}

#[async_trait::async_trait]
impl crate::interaction::agent_loop::LoopToolExecutor for ChatToolRouter<'_> {
    #[allow(clippy::too_many_lines)]
    async fn execute(&self, name: &str, args: &Value) -> crate::interaction::project_tools::ToolResult {
        if name == "propose_action" {
            if let Some(deps) = &self.propose_deps {
                return crate::interaction::propose_action_tool::tool_propose_action(deps, args).await;
            }
        }
        if name == "research_web" && self.research_enabled {
            let config = crate::interaction::research_tool::read_research_search_config(self.root).await;
            let transport = crate::interaction::research_tool::TavilyTransport::from_config(&config);
            return crate::interaction::research_tool::tool_research_web(self.root, &transport, args).await;
        }
        if name == "import_chapters" {
            if let Some(deps) = &self.import_deps {
                return crate::interaction::import_chapters_tool::tool_import_chapters(
                    deps.runtime,
                    self.root,
                    deps.active_book_id,
                    args,
                )
                .await;
            }
        }
        if name == "sub_agent" {
            if let Some(deps) = &self.sub_agent_deps {
                return crate::interaction::sub_agent_tool::tool_sub_agent(deps, args).await;
            }
        }
        if let Some(deps) = &self.book_edit_deps {
            if let Some(result) =
                crate::interaction::book_edit_tools::execute_book_edit_tool(deps, name, args).await
            {
                return result;
            }
        }
        if let Some(deps) = &self.play_deps {
            if let Some(result) = crate::interaction::play_tools::execute_play_tool(deps, name, args).await {
                return result;
            }
        }
        crate::interaction::project_tools::execute_tool(self.root, name, args).await
    }
}

// ── actionPayload strict 校验（ActionPayloadSchema 逐字，75 号） ──

enum PayloadField<'a> {
    /// z.string().min(1).optional()
    StrNonEmpty,
    /// z.number().int().min(min).max(max)
    IntRange { min: Option<f64>, max: Option<f64> },
    /// z.enum([...]).optional()
    Enum(&'a [&'a str]),
    /// z.boolean().optional()
    Bool,
    /// z.array(z.string().min(1)).min(min).max(max)
    StrArray { min: usize, max: usize },
    /// z.object(...)（如 connectChoice.node: StoryNodeSchema——结构校验由执行器承担）
    Object,
}

const PAYLOAD_TOP_LEVEL_KEYS: &[&str] = &[
    "createBook", "writeNext", "shortRun", "playStart", "generateCover",
    "scriptCreate", "storyboardCreate", "interactiveFilmCreate", "translationCreate",
    "draftStructure", "connectChoice", "removeNode",
];

type PayloadSchema = (&'static str, bool, Vec<(&'static str, PayloadField<'static>)>);

fn payload_schemas() -> Vec<PayloadSchema> {
    use PayloadField::*;
    vec![
        ("createBook", true, vec![
            ("title", StrNonEmpty), ("genre", StrNonEmpty),
            ("platform", Enum(&["tomato", "qidian", "feilu", "other"])),
            ("language", Enum(&["zh", "en"])),
            ("targetChapters", IntRange { min: Some(1.0), max: None }),
            ("chapterWordCount", IntRange { min: Some(1.0), max: None }),
        ]),
        ("writeNext", true, vec![
            ("chapterCount", IntRange { min: Some(1.0), max: Some(20.0) }),
        ]),
        ("shortRun", true, vec![
            ("direction", StrNonEmpty), ("reference", StrNonEmpty), ("storyId", StrNonEmpty),
            ("language", Enum(&["zh", "en"])),
            ("chapters", IntRange { min: Some(12.0), max: Some(18.0) }),
            ("charsPerChapter", IntRange { min: Some(600.0), max: Some(1200.0) }),
            ("cover", Bool),
        ]),
        ("playStart", true, vec![
            ("title", StrNonEmpty), ("premise", StrNonEmpty),
            ("worldContract", StrNonEmpty), ("visualContract", StrNonEmpty),
            ("mode", Enum(&["open", "guided"])),
            ("initialScene", StrNonEmpty),
            ("suggestedActions", StrArray { min: 1, max: 4 }),
        ]),
        ("generateCover", true, vec![
            ("title", StrNonEmpty), ("intro", StrNonEmpty),
            ("sellingPoints", StrNonEmpty), ("coverPrompt", StrNonEmpty),
            ("outputDir", StrNonEmpty),
        ]),
        ("scriptCreate", true, vec![
            ("title", StrNonEmpty), ("sourceKind", StrNonEmpty),
            ("targetFormat", Enum(&[
                "vertical_short_drama", "screenplay", "audio_drama",
                "interactive_script", "general_script",
            ])),
            ("sourceText", StrNonEmpty), ("sourcePath", StrNonEmpty),
            ("requirements", StrNonEmpty),
            ("episodeCount", IntRange { min: Some(1.0), max: None }),
            ("episodeDuration", StrNonEmpty), ("projectId", StrNonEmpty),
            ("outDir", StrNonEmpty),
        ]),
        ("storyboardCreate", true, vec![
            ("title", StrNonEmpty), ("sourceKind", StrNonEmpty),
            ("sourceText", StrNonEmpty), ("sourcePath", StrNonEmpty),
            ("requirements", StrNonEmpty), ("visualStyle", StrNonEmpty),
            ("aspectRatio", StrNonEmpty), ("granularity", StrNonEmpty),
            ("maxShots", IntRange { min: Some(1.0), max: None }),
            ("projectId", StrNonEmpty), ("outDir", StrNonEmpty),
        ]),
        ("interactiveFilmCreate", true, vec![
            ("title", StrNonEmpty), ("sourceKind", StrNonEmpty),
            ("sourceText", StrNonEmpty), ("sourcePath", StrNonEmpty),
            ("requirements", StrNonEmpty), ("targetAudience", StrNonEmpty),
            ("episodeCount", IntRange { min: Some(1.0), max: None }),
            ("episodeDuration", StrNonEmpty), ("budget", StrNonEmpty),
            ("referenceMode", StrNonEmpty), ("projectId", StrNonEmpty),
            ("outDir", StrNonEmpty),
        ]),
        ("translationCreate", true, vec![
            ("filePath", StrNonEmpty), ("sourceLanguage", StrNonEmpty),
            ("targetLanguage", StrNonEmpty), ("title", StrNonEmpty),
            ("segmentMaxChars", IntRange { min: Some(1.0), max: None }),
        ]),
        // 以下三个子域 TS 非 strict（无 .strict()）：只做字段形态校验。
        ("draftStructure", false, vec![
            ("projectId", StrNonEmpty), ("instruction", StrNonEmpty),
        ]),
        ("connectChoice", false, vec![
            ("projectId", StrNonEmpty), ("node", Object),
        ]),
        ("removeNode", false, vec![
            ("projectId", StrNonEmpty), ("nodeId", StrNonEmpty),
        ]),
    ]
}

/// 顶层 object + strict（unknown 键拒绝）+ 子域 strict/字段校验；shortRun 的
/// language+charsPerChapter 联动分段（superRefine）一并校验。
pub(crate) fn validate_action_payload_strict(value: &Value) -> Result<(), String> {
    let Some(object) = value.as_object() else {
        return Err(format!("expected object, got {value}"));
    };
    for key in object.keys() {
        if !PAYLOAD_TOP_LEVEL_KEYS.contains(&key.as_str()) {
            return Err(format!("Unrecognized key: {key} — ActionPayloadSchema is strict"));
        }
    }
    let schemas = payload_schemas();
    for (domain, strict, fields) in &schemas {
        let Some(domain_value) = object.get(*domain) else { continue };
        let Some(domain_object) = domain_value.as_object() else {
            return Err(format!("{domain}: expected object"));
        };
        if *strict {
            for key in domain_object.keys() {
                if !fields.iter().any(|(name, _)| name == key) {
                    return Err(format!("{domain}: Unrecognized key: {key}"));
                }
            }
        }
        for (field, spec) in fields {
            let Some(field_value) = domain_object.get(*field) else { continue };
            let valid = match spec {
                PayloadField::StrNonEmpty => field_value.as_str().is_some_and(|text| !text.is_empty()),
                PayloadField::IntRange { min, max } => field_value.as_f64().is_some_and(|number| {
                    number.fract() == 0.0
                        && min.is_none_or(|min| number >= min)
                        && max.is_none_or(|max| number <= max)
                }),
                PayloadField::Enum(values) => field_value
                    .as_str()
                    .is_some_and(|text| values.contains(&text)),
                PayloadField::Bool => field_value.is_boolean(),
                PayloadField::StrArray { min, max } => field_value
                    .as_array()
                    .is_some_and(|items| {
                        items.len() >= *min
                            && items.len() <= *max
                            && items
                                .iter()
                                .all(|item| item.as_str().is_some_and(|s| !s.is_empty()))
                    }),
                PayloadField::Object => field_value.is_object(),
            };
            if !valid {
                return Err(format!("{domain}.{field}: invalid value {field_value}"));
            }
        }
    }
    // shortRun superRefine：zh 900-1200 汉字 / en 600-800 英文词。
    if let Some(short_run) = object.get("shortRun").and_then(Value::as_object) {
        let language = short_run.get("language").and_then(Value::as_str);
        let chars = short_run
            .get("charsPerChapter")
            .and_then(Value::as_f64)
            .filter(|v| v.fract() == 0.0);
        if let (Some(language), Some(chars)) = (language, chars) {
            let (min, max) = match language {
                "en" => (600.0, 800.0),
                _ => (900.0, 1200.0),
            };
            if !(chars >= min && chars <= max) {
                return Err(format!(
                    "shortRun.charsPerChapter: charsPerChapter={chars} 超出范围（{min}-{max}）"
                ));
            }
        }
    }
    Ok(())
}


#[cfg(test)]
mod payload_strict_tests {
    use super::validate_action_payload_strict;
    use serde_json::json;

    #[test]
    fn top_level_strict_and_domain_strict() {
        assert!(validate_action_payload_strict(&json!({})).is_ok());
        // 顶层 unknown 键拒绝。
        let err = validate_action_payload_strict(&json!({ "bogus": {} })).unwrap_err();
        assert!(err.contains("Unrecognized key: bogus"), "{err}");
        // 子域 unknown 键拒绝（createBook strict）。
        let err = validate_action_payload_strict(&json!({
            "createBook": { "title": "X", "extra": 1 }
        }))
        .unwrap_err();
        assert!(err.contains("createBook: Unrecognized key: extra"), "{err}");
        // 非 strict 子域（draftStructure）unknown 键放行。
        assert!(validate_action_payload_strict(&json!({
            "draftStructure": { "instruction": "i", "extra": 1 }
        }))
        .is_ok());
    }

    #[test]
    fn field_type_and_range_rules() {
        // platform 枚举。
        assert!(validate_action_payload_strict(&json!({ "createBook": { "platform": "tomato" } })).is_ok());
        assert!(validate_action_payload_strict(&json!({ "createBook": { "platform": "nope" } })).is_err());
        // writeNext chapterCount 1-20。
        assert!(validate_action_payload_strict(&json!({ "writeNext": { "chapterCount": 20 } })).is_ok());
        assert!(validate_action_payload_strict(&json!({ "writeNext": { "chapterCount": 21 } })).is_err());
        // min(1) 字符串：空串非法（空白串长度≥1 合法——zod min(1) 不 trim）。
        assert!(validate_action_payload_strict(&json!({ "translationCreate": { "filePath": "" } })).is_err());
        assert!(validate_action_payload_strict(&json!({ "translationCreate": { "filePath": "  " } })).is_ok());
        // playStart suggestedActions 数组 1-4 非空串。
        assert!(validate_action_payload_strict(&json!({ "playStart": { "suggestedActions": ["a", "b"] } })).is_ok());
        assert!(validate_action_payload_strict(&json!({ "playStart": { "suggestedActions": [] } })).is_err());
        assert!(validate_action_payload_strict(&json!({ "playStart": { "suggestedActions": ["a", ""] } })).is_err());
    }

    #[test]
    fn short_run_language_chars_cross_validation() {
        // zh 900-1200；en 600-800（superRefine 联动）。
        assert!(validate_action_payload_strict(&json!({
            "shortRun": { "language": "zh", "charsPerChapter": 1000 }
        }))
        .is_ok());
        assert!(validate_action_payload_strict(&json!({
            "shortRun": { "language": "zh", "charsPerChapter": 700 }
        }))
        .is_err());
        assert!(validate_action_payload_strict(&json!({
            "shortRun": { "language": "en", "charsPerChapter": 700 }
        }))
        .is_ok());
        // 无 language 时维持 600-1200 并集（基础 IntRange）。
        assert!(validate_action_payload_strict(&json!({
            "shortRun": { "charsPerChapter": 700 }
        }))
        .is_ok());
    }
}
