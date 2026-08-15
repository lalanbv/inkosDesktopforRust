//! transcript → BookSession 重建（derive 子集）。
//!
//! 移植自 `packages/core/src/interaction/session-transcript-restore.ts`（886 行）
//! 的**会话端点消费面**：committedMessageEvents / messageEventsToInteractionMessages
//! （含工具执行卡还原与中文标签表）/ deriveBookSessionFromTranscript / legacy 迁移。
//! agent 消息重放面（68 号）：restoreAgentMessagesFromTranscript 本体（历史
//! 工具摘要 + text-only 对话 + boundary）。cleanRestored / adaptRestored 为
//! pi-agent 块模型的适配层——Rust agent loop 为 OpenAI 文本形态，历史本就
//! text-only，无需该层（偏差备案见 68 号记录）。

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde_json::{json, Value};

use crate::interaction::session::BookSession;
use crate::interaction::session_transcript::{
    append_transcript_events, legacy_book_session_path, read_transcript_events, TranscriptEvent,
};

fn is_object(value: &Value) -> bool {
    value.is_object()
}

fn content_blocks(message: &Value) -> Vec<&Value> {
    message
        .get("content")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().collect())
        .unwrap_or_default()
}

fn text_from_content(content: &Value) -> String {
    if let Some(text) = content.as_str() {
        return text.to_string();
    }
    let Some(blocks) = content.as_array() else {
        return String::new();
    };
    blocks
        .iter()
        .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|block| block.get("text").and_then(Value::as_str))
        .collect()
}

fn thinking_from_content(content: &Value) -> Option<String> {
    let blocks = content.as_array()?;
    let value: String = blocks
        .iter()
        .filter(|block| block.get("type").and_then(Value::as_str) == Some("thinking"))
        .filter_map(|block| block.get("thinking").and_then(Value::as_str))
        .collect();
    (!value.is_empty()).then_some(value)
}

fn join_thinking(parts: Vec<Option<String>>) -> Option<String> {
    let values: Vec<String> = parts
        .into_iter()
        .filter_map(|part| {
            let trimmed = part?.trim().to_string();
            (!trimmed.is_empty()).then_some(trimmed)
        })
        .collect();
    (!values.is_empty()).then(|| values.join("\n\n---\n\n"))
}

fn has_tool_call_content(message: &Value) -> bool {
    content_blocks(message).iter().any(|block| {
        is_object(block)
            && block.get("type").and_then(Value::as_str) == Some("toolCall")
            && block.get("id").and_then(Value::as_str).is_some()
    })
}

/// message 事件（宽松引用形态——借用事件字段）。
struct MessageRef<'a> {
    request_id: &'a str,
    role: &'a str,
    timestamp: u64,
    tool_call_id: Option<&'a str>,
    legacy_thinking: Option<&'a str>,
    legacy_tool_executions: Option<&'a Vec<Value>>,
    message: &'a Value,
}

fn as_message_ref<'a>(event: &'a TranscriptEvent) -> Option<MessageRef<'a>> {
    match event {
        TranscriptEvent::Message {
            request_id,
            role,
            timestamp,
            tool_call_id,
            legacy_display,
            message,
            ..
        } => Some(MessageRef {
            request_id,
            role,
            timestamp: *timestamp,
            tool_call_id: tool_call_id.as_deref(),
            legacy_thinking: legacy_display.as_ref().and_then(|d| d.thinking.as_deref()),
            legacy_tool_executions: legacy_display
                .as_ref()
                .map(|d| &d.tool_executions),
            message,
        }),
        _ => None,
    }
}

/// `committedMessageEvents`：仅取 request_committed（含 kind 过滤）的 message 事件。
pub fn committed_message_events<'e>(
    events: &'e [TranscriptEvent],
    session_kind: Option<&str>,
) -> Vec<&'e TranscriptEvent> {
    let request_kinds: HashMap<&'e str, Option<&'e str>> = events
        .iter()
        .filter_map(|event| match event {
            TranscriptEvent::RequestStarted {
                request_id,
                session_kind,
                ..
            } => Some((request_id.as_str(), session_kind.map(|k| k.as_str()))),
            _ => None,
        })
        .collect();
    let committed: HashSet<&'e str> = events
        .iter()
        .filter_map(|event| match event {
            TranscriptEvent::RequestCommitted { request_id, .. } => Some(request_id.as_str()),
            _ => None,
        })
        .filter(|request_id| {
            match session_kind.zip(request_kinds.get(request_id).copied().flatten()) {
                // 请求未声明 kind（legacy）→ 视为匹配任意过滤 kind
                None => true,
                Some((kind, event_kind)) => event_kind == kind,
            }
        })
        .collect();
    let mut messages: Vec<&TranscriptEvent> = events
        .iter()
        .filter(|event| {
            matches!(event, TranscriptEvent::Message { .. })
                && event
                    .message_request_id()
                    .map(|id| committed.contains(id))
                    .unwrap_or(false)
        })
        .collect();
    messages.sort_by_key(|event| event.seq());
    messages
}

trait MessageRequestId {
    fn message_request_id(&self) -> Option<&str>;
}

impl MessageRequestId for TranscriptEvent {
    fn message_request_id(&self) -> Option<&str> {
        match self {
            TranscriptEvent::Message { request_id, .. } => Some(request_id.as_str()),
            _ => None,
        }
    }
}

/// 工具/agent 标签表（restore.ts 中文标签逐字）。
fn agent_label(agent: &str) -> &str {
    match agent {
        "architect" => "建书",
        "writer" => "写作",
        "auditor" => "审计",
        "reviser" => "修订",
        "exporter" => "导出",
        other => other,
    }
}

fn tool_label(tool: &str) -> &str {
    match tool {
        "read" => "读取文件",
        "edit" => "编辑文件",
        "grep" => "搜索",
        "ls" => "列目录",
        "propose_action" => "确认动作",
        "short_fiction_run" => "短篇生产",
        "generate_cover" => "生成封面",
        "play_edit" => "编辑互动世界",
        "play_start" => "启动互动世界",
        "play_revise" => "重做互动回合",
        "play_step" => "推进互动世界",
        "create_narrative_forecast" => "剧情多线推演",
        "get_narrative_forecast" => "核验剧情推演",
        "select_narrative_branch" => "采用候选分支",
        other => other,
    }
}

const EXPIRED_SKILL_RESULT_TEXT: &str = "Skill instructions expired after their original turn.";

struct RestoredToolCall {
    #[allow(dead_code)]
    id: String,
    tool: String,
    args: Option<Value>,
    timestamp: u64,
}

fn object_args(value: Option<&Value>) -> Option<Value> {
    let value = value?;
    if value.is_object() {
        return Some(value.clone());
    }
    let text = value.as_str()?;
    if text.trim().is_empty() {
        return None;
    }
    let parsed: Value = serde_json::from_str(text).ok()?;
    parsed.is_object().then_some(parsed)
}

/// `messageEventsToInteractionMessages`：工具执行卡聚合 + thinking 合并 + play 卡独立。
pub fn message_events_to_interaction_messages(events: &[&TranscriptEvent]) -> Vec<Value> {
    let mut messages: Vec<Value> = Vec::new();
    let mut tool_calls: HashMap<(String, String), RestoredToolCall> = HashMap::new();
    // use_skill 调用轮的 thinking 不还原（restore.ts requestIdsUsingSkill）
    let skill_request_ids: HashSet<String> = events
        .iter()
        .filter_map(|event| {
            let reference = as_message_ref(event)?;
            if reference.role != "assistant" {
                return None;
            }
            let uses_skill = content_blocks(reference.message).iter().any(|block| {
                block.get("type").and_then(Value::as_str) == Some("toolCall")
                    && block.get("name").and_then(Value::as_str) == Some("use_skill")
            });
            uses_skill.then_some(reference.request_id.to_string())
        })
        .collect();

    let mut pending_tool_executions: Vec<Value> = Vec::new();
    let mut pending_thinking: Vec<Option<String>> = Vec::new();
    let mut active_request_id: Option<&str> = None;

    let has_completed_play_tool = |executions: &[Value]| {
        executions.iter().any(|execution| {
            execution["status"] == "completed"
                && ["play_start", "play_step", "play_revise"]
                    .iter()
                    .any(|tool| execution["tool"] == *tool)
        })
    };

    for event in events {
        let Some(reference) = as_message_ref(event) else {
            continue;
        };
        if active_request_id != Some(reference.request_id) {
            active_request_id = Some(reference.request_id);
            pending_tool_executions.clear();
            pending_thinking.clear();
        }

        if reference.role == "assistant" && is_object(reference.message) {
            // 记住 toolCall 块（供 toolResult 配对）
            for block in content_blocks(reference.message) {
                if block.get("type").and_then(Value::as_str) != Some("toolCall") {
                    continue;
                }
                let Some(id) = block.get("id").and_then(Value::as_str) else {
                    continue;
                };
                if id.is_empty() {
                    continue;
                }
                let tool = block
                    .get("name")
                    .and_then(Value::as_str)
                    .filter(|n| !n.is_empty())
                    .unwrap_or("tool");
                let args = object_args(block.get("arguments"));
                tool_calls.insert(
                    (reference.request_id.to_string(), id.to_string()),
                    RestoredToolCall {
                        id: id.to_string(),
                        tool: tool.to_string(),
                        args,
                        timestamp: reference.timestamp,
                    },
                );
            }

            let is_skill_request = skill_request_ids.contains(reference.request_id);
            let current_thinking = if is_skill_request {
                None
            } else {
                thinking_from_content(
                    reference.message.get("content").unwrap_or(&Value::Null),
                )
            };
            let current_text = text_from_content(
                reference.message.get("content").unwrap_or(&Value::Null),
            )
            .trim()
            .to_string();
            let legacy_tool_executions = reference.legacy_tool_executions;
            let has_legacy_display = current_thinking.is_some()
                || (!is_skill_request && reference.legacy_thinking.is_some())
                    || legacy_tool_executions
                        .map(|execs| !execs.is_empty())
                        .unwrap_or(false);
            if !pending_tool_executions.is_empty()
                && !has_completed_play_tool(&pending_tool_executions)
                && current_text.is_empty()
                && current_thinking.is_none()
                && !has_tool_call_content(reference.message)
                && !has_legacy_display
            {
                // 空收尾 assistant 消息只闭合模型轮；非 play 工具结果保持 pending 挂到前文
                continue;
            }

            let raw_text = text_from_content(
                reference.message.get("content").unwrap_or(&Value::Null),
            );
            let suppress_text = has_completed_play_tool(&pending_tool_executions);
            let content = if suppress_text { String::new() } else { raw_text.clone() };
            let thinking = if is_skill_request {
                None
            } else {
                let mut parts = pending_thinking.clone();
                parts.push(current_thinking.clone());
                parts.push(reference.legacy_thinking.map(str::to_string));
                if suppress_text {
                    parts.push(Some(raw_text.clone()));
                }
                join_thinking(parts)
            };
            let tool_executions: Option<Vec<Value>> = if !pending_tool_executions.is_empty() {
                Some(pending_tool_executions.clone())
            } else {
                legacy_tool_executions.cloned()
            };
            let has_tools = tool_executions.as_ref().map(|t| !t.is_empty()).unwrap_or(false);
            if content.is_empty() && thinking.is_none() && !has_tools {
                if has_tool_call_content(reference.message) && current_thinking.is_some() {
                    pending_thinking.push(current_thinking);
                }
                continue;
            }
            if content.is_empty() && !has_tools {
                continue;
            }
            let mut message = json!({ "role": "assistant", "content": content, "timestamp": reference.timestamp });
            let obj = message.as_object_mut().unwrap();
            if let Some(thinking) = thinking {
                obj.insert("thinking".into(), json!(thinking));
            }
            if has_tools {
                obj.insert(
                    "toolExecutions".into(),
                    json!(tool_executions.unwrap_or_default()),
                );
            }
            messages.push(message);
            pending_tool_executions.clear();
            pending_thinking.clear();
            continue;
        }

        if reference.role == "toolResult" {
            let raw = reference.message;
            if !is_object(raw) {
                continue;
            }
            let tool_call_id = raw
                .get("toolCallId")
                .and_then(Value::as_str)
                .or(reference.tool_call_id)
                .unwrap_or_default();
            if tool_call_id.is_empty() {
                continue;
            }
            let call = tool_calls.get(&(reference.request_id.to_string(), tool_call_id.to_string()));
            let tool = raw
                .get("toolName")
                .and_then(Value::as_str)
                .filter(|t| !t.is_empty())
                .map(str::to_string)
                .or_else(|| call.map(|c| c.tool.clone()))
                .unwrap_or_else(|| "tool".to_string());
            let args = call.and_then(|c| c.args.clone());
            let agent = if tool == "sub_agent" {
                args.as_ref()
                    .and_then(|a| a.get("agent"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
            } else {
                None
            };
            let text = text_from_content(raw.get("content").unwrap_or(&Value::Null))
                .trim()
                .to_string();
            let is_error = raw.get("isError") == Some(&Value::Bool(true));
            let details = raw.get("details").cloned();
            let expired_skill = tool == "use_skill";
            let label = if tool == "sub_agent" {
                agent.as_deref().map(agent_label).unwrap_or("sub_agent").to_string()
            } else {
                tool_label(&tool).to_string()
            };
            let mut execution = json!({
                "id": tool_call_id,
                "tool": tool,
                "label": label,
                "status": if is_error { "error" } else { "completed" },
                "startedAt": call.map(|c| c.timestamp).unwrap_or(reference.timestamp),
                "completedAt": reference.timestamp,
            });
            let obj = execution.as_object_mut().unwrap();
            if let Some(agent) = agent {
                obj.insert("agent".into(), json!(agent));
            }
            if let Some(args) = args {
                obj.insert("args".into(), args);
            }
            if is_error {
                let error = if text.is_empty() { "Tool execution failed".to_string() } else { text.chars().take(500).collect() };
                obj.insert("error".into(), json!(error));
            } else if expired_skill {
                obj.insert("result".into(), json!(EXPIRED_SKILL_RESULT_TEXT));
                obj.insert("details".into(), json!({ "kind": "skill_expired" }));
            } else if !text.is_empty() {
                obj.insert("result".into(), json!(text.chars().take(200).collect::<String>()));
            }
            if let Some(details) = details {
                if !expired_skill {
                    obj.insert("details".into(), details);
                }
            }
            pending_tool_executions.push(execution);
            continue;
        }

        // user / system
        pending_tool_executions.clear();
        pending_thinking.clear();
        let content = text_from_content(reference.message.get("content").unwrap_or(&Value::Null));
        if reference.role == "system" {
            if !content.is_empty() {
                messages.push(json!({ "role": "system", "content": content, "timestamp": reference.timestamp }));
            }
            continue;
        }
        if reference.role == "user" && !content.is_empty() {
            messages.push(json!({ "role": "user", "content": content, "timestamp": reference.timestamp }));
        }
    }

    // 尾部 pending 工具卡挂到最后一条 assistant 消息
    if !pending_tool_executions.is_empty() {
        let thinking = join_thinking(pending_thinking);
        let timestamp = pending_tool_executions
            .iter()
            .map(|execution| {
                execution["completedAt"].as_u64().unwrap_or(0).max(
                    execution["startedAt"].as_u64().unwrap_or(0),
                )
            })
            .max()
            .unwrap_or(0);
        if let Some(last) = messages.last_mut() {
            if last["role"] == "assistant" {
                let obj = last.as_object_mut().unwrap();
                if let Some(thinking) = thinking {
                    let merged = join_thinking(vec![
                        obj.get("thinking").and_then(Value::as_str).map(str::to_string),
                        Some(thinking),
                    ]);
                    obj.insert("thinking".into(), json!(merged));
                }
                let mut tools = obj
                    .get("toolExecutions")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                tools.extend(pending_tool_executions.clone());
                obj.insert("toolExecutions".into(), json!(tools));
            } else {
                let mut message = json!({ "role": "assistant", "content": "" });
                let obj = message.as_object_mut().unwrap();
                if let Some(thinking) = thinking {
                    obj.insert("thinking".into(), json!(thinking));
                }
                obj.insert("toolExecutions".into(), json!(pending_tool_executions));
                obj.insert("timestamp".into(), json!(timestamp));
                messages.push(message);
            }
        } else {
            let mut message = json!({ "role": "assistant", "content": "" });
            let obj = message.as_object_mut().unwrap();
            if let Some(thinking) = thinking {
                obj.insert("thinking".into(), json!(thinking));
            }
            obj.insert("toolExecutions".into(), json!(pending_tool_executions));
            obj.insert("timestamp".into(), json!(timestamp));
            messages.push(message);
        }
    }

    messages
}

/// `firstUserMessageTitle`（restore 版）：首条 user 消息裁 ≤20 字单行。
fn first_user_message_title(messages: &[Value]) -> Option<String> {
    for message in messages {
        if message.get("role").and_then(Value::as_str) != Some("user") {
            continue;
        }
        let content = message.get("content").and_then(Value::as_str).unwrap_or("");
        let one_line = collapse_whitespace(content.trim());
        if one_line.is_empty() {
            return None;
        }
        return Some(truncate_with_ellipsis(&one_line, 20));
    }
    None
}

fn collapse_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `slice(0, 20) + "…"`（UTF-16 码元计数）。
fn truncate_with_ellipsis(value: &str, max_units: usize) -> String {
    if value.encode_utf16().count() <= max_units {
        return value.to_string();
    }
    let mut units = 0usize;
    let mut out = String::new();
    for ch in value.chars() {
        let ch_units = ch.len_utf16();
        if units + ch_units > max_units {
            break;
        }
        out.push(ch);
        units += ch_units;
    }
    format!("{out}…")
}

/// `deriveBookSessionFromTranscript`：事件流 → BookSession（元数据折叠 + 消息重建）。
pub async fn derive_book_session_from_transcript(
    project_root: &Path,
    session_id: &str,
) -> Option<BookSession> {
    let events = read_transcript_events(project_root, session_id).await;
    if events.is_empty() {
        return None;
    }

    let created = events.iter().find(|event| {
        matches!(event, TranscriptEvent::SessionCreated { .. })
    });
    let (mut book_id, mut session_kind, mut play_mode, mut title, created_at, mut updated_at) =
        match created {
            Some(TranscriptEvent::SessionCreated {
                book_id,
                session_kind,
                play_mode,
                title,
                created_at,
                updated_at,
                ..
            }) => (
                book_id.clone(),
                *session_kind,
                *play_mode,
                title.clone(),
                *created_at,
                *updated_at,
            ),
            _ => {
                // TS：createdAt = events[0]?.timestamp ?? Date.now()；
                // updatedAt 初始 = events[last].timestamp ?? createdAt（后续与活动时间取最大）
                let now = crate::interaction::session::utc_now_ms();
                let created_at = events.first().map(other_timestamp).unwrap_or(now);
                let updated_at = events.last().map(other_timestamp).unwrap_or(created_at);
                (None, None, None, None, created_at, updated_at)
            }
        };

    // latestActivityTimestamp：session_created/metadata_updated 的 updatedAt 与全部 timestamp 取最大
    let mut latest = updated_at;
    for event in &events {
        let activity = match event {
            TranscriptEvent::SessionCreated { updated_at, .. }
            | TranscriptEvent::SessionMetadataUpdated { updated_at, .. } => *updated_at,
            other => other_timestamp(other),
        };
        latest = latest.max(activity);
    }
    updated_at = updated_at.max(latest);

    for event in &events {
        if let TranscriptEvent::SessionMetadataUpdated {
            book_id: event_book_id,
            session_kind: event_kind,
            play_mode: event_play_mode,
            title: event_title,
            updated_at: event_updated_at,
            ..
        } = event
        {
            if let Some(book) = event_book_id {
                book_id = Some(book.clone());
            }
            if let Some(kind) = event_kind {
                session_kind = Some(*kind);
            }
            if let Some(mode) = event_play_mode {
                play_mode = Some(*mode);
            }
            if let Some(event_title) = event_title {
                title = Some(event_title.clone());
            }
            updated_at = updated_at.max(*event_updated_at);
        }
    }

    let committed = committed_message_events(&events, None);
    let messages = message_events_to_interaction_messages(&committed);
    if title.is_none() {
        title = first_user_message_title(&messages);
    }

    Some(BookSession {
        session_id: session_id.to_string(),
        book_id,
        session_kind,
        play_mode,
        title,
        messages,
        created_at,
        updated_at,
    })
}

fn other_timestamp(event: &TranscriptEvent) -> u64 {
    match event {
        TranscriptEvent::SessionCreated { timestamp, .. }
        | TranscriptEvent::SessionMetadataUpdated { timestamp, .. }
        | TranscriptEvent::RequestStarted { timestamp, .. }
        | TranscriptEvent::RequestCommitted { timestamp, .. }
        | TranscriptEvent::RequestFailed { timestamp, .. }
        | TranscriptEvent::Message { timestamp, .. } => *timestamp,
    }
}

/// 旧版 `.json` 会话读取（zod parse 宽松等价：结构不符 → None）。
pub async fn read_legacy_book_session(
    project_root: &Path,
    session_id: &str,
) -> Option<BookSession> {
    let raw = tokio::fs::read_to_string(legacy_book_session_path(project_root, session_id))
        .await
        .ok()?;
    serde_json::from_str::<BookSession>(&raw).ok()
}

/// `migrateLegacyBookSessionToTranscript`：旧消息 → session_created + request 事件。
pub async fn migrate_legacy_book_session_to_transcript(
    project_root: &Path,
    session: &BookSession,
) {
    append_transcript_events(project_root, &session.session_id, |events, next_seq| {
        if !events.is_empty() {
            return Vec::new();
        }
        let mut seq = next_seq + 2; // session_created + request_started
        let request_id = format!("legacy-{}", uuid::Uuid::new_v4());
        let mut transcript_events = Vec::new();
        let mut parent_uuid: Option<String> = None;
        for legacy_message in &session.messages {
            let uuid = uuid::Uuid::new_v4().to_string();
            let role = legacy_message.get("role").and_then(Value::as_str).unwrap_or("user");
            let timestamp = legacy_message
                .get("timestamp")
                .and_then(Value::as_u64)
                .unwrap_or(crate::interaction::session::utc_now_ms());
            let message = if role == "assistant" {
                json!({
                    "role": "assistant",
                    "content": [{ "type": "text", "text": legacy_message.get("content").and_then(Value::as_str).unwrap_or("") }],
                    "api": "anthropic-messages",
                    "provider": "legacy",
                    "model": "unknown",
                    "usage": { "input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0, "totalTokens": 0,
                               "cost": { "input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0, "total": 0 } },
                    "stopReason": "stop",
                    "timestamp": timestamp,
                })
            } else {
                json!({
                    "role": role,
                    "content": legacy_message.get("content").and_then(Value::as_str).unwrap_or(""),
                    "timestamp": timestamp,
                })
            };
            transcript_events.push(TranscriptEvent::Message {
                version: 1,
                session_id: session.session_id.clone(),
                request_id: request_id.clone(),
                uuid: uuid.clone(),
                parent_uuid: parent_uuid.clone(),
                seq,
                timestamp,
                role: role.to_string(),
                pi_turn_index: None,
                tool_call_id: None,
                source_tool_assistant_uuid: None,
                legacy_display: None,
                message,
            });
            parent_uuid = Some(uuid);
            seq += 1;
        }
        let mut out = vec![
            TranscriptEvent::SessionCreated {
                version: 1,
                session_id: session.session_id.clone(),
                seq: next_seq,
                timestamp: session.created_at,
                book_id: session.book_id.clone(),
                session_kind: session.session_kind,
                play_mode: session.play_mode,
                title: session.title.clone(),
                created_at: session.created_at,
                updated_at: session.updated_at,
            },
            TranscriptEvent::RequestStarted {
                version: 1,
                session_id: session.session_id.clone(),
                seq: next_seq + 1,
                timestamp: session.updated_at,
                request_id: request_id.clone(),
                session_kind: session.session_kind,
                input: String::new(),
            },
        ];
        out.extend(transcript_events);
        out.push(TranscriptEvent::RequestCommitted {
            version: 1,
            session_id: session.session_id.clone(),
            seq,
            timestamp: crate::interaction::session::utc_now_ms(),
            request_id,
        });
        out
    })
    .await;
}

// ── agent 消息重放（68 号：restore.ts 后半） ─────────────────────

use crate::llm::provider::{LLMMessage, LLMRole};

const MAX_RESTORED_DIALOGUE_MESSAGES: usize = 12;
const MAX_RESTORED_TOOL_SUMMARY_ITEMS: usize = 8;

pub const RESTORED_HISTORY_BOUNDARY_ZH: &str = "[已完成的历史上下文]\n以上是已经完成并提交的历史上下文，只能用于回忆，不代表当前轮已经执行了动作。\n当前轮必须优先遵循用户接下来输入的最新指令；如果最新输入是提问、讨论或确认，直接回答，不要因为历史工具结果跳过判断或继续执行旧动作。";
pub const RESTORED_HISTORY_BOUNDARY_EN: &str = "[Completed history context]\nThe messages above are completed and committed history. Use them only as memory; they do not mean any action has already run in the current turn.\nFor the current turn, prioritize the next user instruction. If it is a question, discussion, or confirmation, answer directly instead of continuing an old tool action.";

fn system_llm_message(content: String, timestamp: u64) -> LLMMessage {
    let _ = timestamp;
    LLMMessage { role: LLMRole::System, content, tool_calls: None, tool_call_id: None }
}

/// event.message 为 object 判定 + 取值。
fn event_message(event: &TranscriptEvent) -> Option<&Value> {
    match event {
        TranscriptEvent::Message { message, .. } if message.is_object() => Some(message),
        _ => None,
    }
}

/// `buildHistoricalToolSummary`：全部已提交轮的工具动作摘要（system 消息，
/// 最近 8 条；行格式 `- {tool}:{agent} {status} — {180 截断文本}`）。
fn build_historical_tool_summary(
    events: &[TranscriptEvent],
    session_kind: Option<&str>,
) -> Option<LLMMessage> {
    let request_kinds: HashMap<&str, Option<&str>> = events
        .iter()
        .filter_map(|event| match event {
            TranscriptEvent::RequestStarted { request_id, session_kind, .. } => {
                Some((request_id.as_str(), session_kind.map(|k| k.as_str())))
            }
            _ => None,
        })
        .collect();
    let committed = committed_message_events(events, session_kind);

    // assistant toolCall 块 → "{requestId}\0{id}" 索引（tool 名 + agent 参数）。
    let mut calls: HashMap<String, (String, Option<String>)> = HashMap::new();
    let mut summaries: Vec<(u64, String)> = Vec::new();
    let mut latest_timestamp = 0u64;

    for event in &committed {
        let (request_id, role, timestamp) = match event {
            TranscriptEvent::Message { request_id, role, timestamp, .. } => {
                (request_id.as_str(), role.as_str(), *timestamp)
            }
            _ => continue,
        };
        // TS 摘要循环对 kind 不匹配（含 legacy 无 kind）一律跳过——与
        // committedMessageEvents 的 legacy 放行相反，逐字对齐。
        let request_kind = request_kinds.get(request_id).copied().flatten();
        if session_kind.is_some() && request_kind != session_kind {
            continue;
        }
        let Some(raw) = event_message(event) else { continue };

        if role == "assistant" {
            for block in content_blocks(raw) {
                if block.get("type").and_then(Value::as_str) != Some("toolCall") {
                    continue;
                }
                let Some(id) = block.get("id").and_then(Value::as_str).filter(|s| !s.is_empty())
                else {
                    continue;
                };
                let tool = block
                    .get("name")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .unwrap_or("tool");
                let agent = block
                    .get("arguments")
                    .and_then(|args| args.get("agent"))
                    .and_then(Value::as_str)
                    .map(String::from);
                calls.insert(format!("{request_id}\0{id}"), (tool.to_string(), agent));
            }
            continue;
        }
        if role != "toolResult" {
            continue;
        }
        let tool_call_id = raw
            .get("toolCallId")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| match event {
                TranscriptEvent::Message { tool_call_id, .. } => tool_call_id.clone(),
                _ => None,
            })
            .unwrap_or_default();
        let call = calls.get(&format!("{request_id}\0{tool_call_id}"));
        let tool = raw
            .get("toolName")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .or_else(|| call.map(|(tool, _)| tool.clone()))
            .unwrap_or_else(|| "tool".to_string());
        let agent = call.and_then(|(_, agent)| agent.clone());
        let status = if raw.get("isError") == Some(&Value::Bool(true)) { "failed" } else { "completed" };
        let text = if tool == "use_skill" {
            "expired; instructions are not active for later turns".to_string()
        } else {
            // `\s+` → " " 折叠 + trim（JS 正则 \s 含全角空格；Rust 按空白折叠）。
            let folded: String = text_from_content(raw.get("content").unwrap_or(&Value::Null))
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            truncate_utf16_ellipsis(&folded, 180)
        };
        let line = format!(
            "- {tool}{agent_part} {status}{trimmed_part}",
            agent_part = agent.as_deref().map(|a| format!(":{a}")).unwrap_or_default(),
            trimmed_part = if text.is_empty() { String::new() } else { format!(" — {text}") },
        );
        summaries.push((timestamp, line));
        latest_timestamp = latest_timestamp.max(timestamp);
    }

    if summaries.is_empty() {
        return None;
    }
    let latest: Vec<String> = summaries
        .iter()
        .rev()
        .take(MAX_RESTORED_TOOL_SUMMARY_ITEMS)
        .rev()
        .map(|(_, line)| line.clone())
        .collect();
    let mut content = String::from("[历史状态摘要]\n以下是已经完成的历史工具动作摘要，只说明当前状态；不是当前用户的新指令，也不表示本轮已经执行。");
    for line in latest {
        content.push('\n');
        content.push_str(&line);
    }
    Some(system_llm_message(content, latest_timestamp))
}

/// `text.slice(0, 180) + "..."`（UTF-16 码元计数，超长补省略号）。
fn truncate_utf16_ellipsis(text: &str, limit: usize) -> String {
    if text.encode_utf16().count() <= limit {
        return text.to_string();
    }
    let mut out = String::new();
    let mut units = 0usize;
    for ch in text.chars() {
        let ch_units = ch.len_utf16();
        if units + ch_units > limit {
            break;
        }
        out.push(ch);
        units += ch_units;
    }
    out.push_str("...");
    out
}

/// `textOnlyAgentMessage`：user/system 取纯文本；assistant 取非空文本块拼接。
fn text_only_agent_message(event: &TranscriptEvent) -> Option<LLMMessage> {
    let (role, timestamp, raw) = match event {
        TranscriptEvent::Message { role, timestamp, message, .. } => (role.as_str(), *timestamp, message),
        _ => return None,
    };
    if role == "toolResult" {
        return None;
    }
    if role == "user" || role == "system" {
        let content = text_from_content(raw.get("content").unwrap_or(&Value::Null));
        return (!content.trim().is_empty()).then(|| LLMMessage {
            role: if role == "user" { LLMRole::User } else { LLMRole::System },
            content,
            tool_calls: None,
            tool_call_id: None,
        });
    }
    if role == "assistant" {
        let texts: Vec<&str> = content_blocks(raw)
            .into_iter()
            .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|block| {
                block
                    .get("text")
                    .and_then(Value::as_str)
                    .filter(|text| !text.trim().is_empty())
            })
            .collect();
        if texts.is_empty() {
            return None;
        }
        return Some(LLMMessage {
            role: LLMRole::Assistant,
            content: texts.join("\n"),
            tool_calls: None,
            tool_call_id: None,
        });
    }
    let _ = timestamp;
    None
}

/// 带 toolCall/toolResult 活动的请求 id 集（`requestIdsWithToolActivity`）。
fn request_ids_with_tool_activity<'e>(events: &[&'e TranscriptEvent]) -> HashSet<&'e str> {
    let mut ids = HashSet::new();
    for event in events {
        let (request_id, role) = match event {
            TranscriptEvent::Message { request_id, role, .. } => (request_id.as_str(), role.as_str()),
            _ => continue,
        };
        if role == "toolResult" {
            ids.insert(request_id);
            continue;
        }
        if let Some(raw) = event_message(event) {
            if content_blocks(raw)
                .iter()
                .any(|block| block.get("type").and_then(Value::as_str) == Some("toolCall"))
            {
                ids.insert(request_id);
            }
        }
    }
    ids
}

/// `restoreAgentMessagesFromTranscript`：`[工具摘要?] + 最近 12 条 text-only
/// 对话`。带工具活动的轮次不回放（历史工具语义已折叠进摘要；legacy 无 kind
/// 轮的 user 原话保留为对话记忆）。
pub async fn restore_agent_messages_from_transcript(
    project_root: &Path,
    session_id: &str,
    session_kind: Option<&str>,
) -> Vec<LLMMessage> {
    let events = read_transcript_events(project_root, session_id).await;
    let summary = build_historical_tool_summary(&events, session_kind);
    let committed = committed_message_events(&events, session_kind);
    let tool_request_ids = request_ids_with_tool_activity(&committed);
    let request_kinds: HashMap<&str, Option<&str>> = events
        .iter()
        .filter_map(|event| match event {
            TranscriptEvent::RequestStarted { request_id, session_kind, .. } => {
                Some((request_id.as_str(), session_kind.map(|k| k.as_str())))
            }
            _ => None,
        })
        .collect();

    let mut dialogue: Vec<LLMMessage> = committed
        .iter()
        .filter_map(|event| {
            let request_id = event.message_request_id().unwrap_or("");
            if !tool_request_ids.contains(request_id) {
                return Some((*event).clone());
            }
            // legacy（请求无 kind）轮的 user 原话保留。
            let is_legacy_user = session_kind.is_some()
                && request_kinds.get(request_id).copied().flatten().is_none()
                && matches!(event, TranscriptEvent::Message { role, .. } if role == "user");
            if is_legacy_user {
                Some((*event).clone())
            } else {
                None
            }
        })
        .filter_map(|event| text_only_agent_message(&event))
        .collect();
    if dialogue.len() > MAX_RESTORED_DIALOGUE_MESSAGES {
        dialogue.drain(..dialogue.len() - MAX_RESTORED_DIALOGUE_MESSAGES);
    }

    let mut out = Vec::new();
    if let Some(summary) = summary {
        out.push(summary);
    }
    out.extend(dialogue);
    out
}

/// `appendRestoredHistoryBoundary`：历史尾部追加边界 system 消息（语言区分）。
pub fn append_restored_history_boundary(messages: Vec<LLMMessage>, language: &str) -> Vec<LLMMessage> {
    if messages.is_empty() {
        return messages;
    }
    let mut out = messages;
    out.push(system_llm_message(
        if language == "en" {
            RESTORED_HISTORY_BOUNDARY_EN.to_string()
        } else {
            RESTORED_HISTORY_BOUNDARY_ZH.to_string()
        },
        0,
    ));
    out
}

#[cfg(test)]
mod restore_agent_tests {
    use super::*;
    use crate::interaction::session_transcript::TranscriptEvent;

    fn msg_event(seq: u64, request_id: &str, role: &str, message: Value) -> TranscriptEvent {
        TranscriptEvent::Message {
            version: 1,
            session_id: "s".into(),
            request_id: request_id.into(),
            uuid: format!("u{seq}"),
            parent_uuid: None,
            seq,
            timestamp: 1000 + seq,
            role: role.into(),
            pi_turn_index: None,
            tool_call_id: None,
            source_tool_assistant_uuid: None,
            legacy_display: None,
            message,
        }
    }

    fn started(seq: u64, request_id: &str, kind: Option<&str>) -> TranscriptEvent {
        TranscriptEvent::RequestStarted {
            version: 1,
            session_id: "s".into(),
            seq,
            timestamp: 1000 + seq,
            request_id: request_id.into(),
            session_kind: kind.and_then(crate::interaction::session::SessionKind::parse),
            input: String::new(),
        }
    }

    fn committed(seq: u64, request_id: &str) -> TranscriptEvent {
        TranscriptEvent::RequestCommitted {
            version: 1,
            session_id: "s".into(),
            seq,
            timestamp: 1000 + seq,
            request_id: request_id.into(),
        }
    }

    /// 工具轮（user + assistant toolCall + toolResult）+ 对话轮（user + assistant）。
    fn fixture_events() -> Vec<TranscriptEvent> {
        vec![
            started(1, "r1", Some("chat")),
            msg_event(2, "r1", "user", json!({ "role": "user", "content": "看看设定文件" })),
            msg_event(3, "r1", "assistant", json!({
                "role": "assistant",
                "content": [
                    { "type": "toolCall", "id": "tc1", "name": "read", "arguments": { "path": "note.md" } },
                ],
            })),
            msg_event(4, "r1", "toolResult", json!({
                "role": "toolResult",
                "toolCallId": "tc1",
                "toolName": "read",
                "content": [{ "type": "text", "text": "主角名叫林动。" }],
            })),
            committed(5, "r1"),
            started(6, "r2", Some("chat")),
            msg_event(7, "r2", "user", json!({ "role": "user", "content": "主角叫什么？" })),
            msg_event(8, "r2", "assistant", json!({
                "role": "assistant",
                "content": [{ "type": "text", "text": "主角名叫林动。" }],
            })),
            committed(9, "r2"),
        ]
    }

    #[test]
    fn tool_summary_folds_tool_activity_into_system_message() {
        let events = fixture_events();
        let summary = build_historical_tool_summary(&events, Some("chat")).unwrap();
        assert_eq!(summary.role, LLMRole::System);
        assert!(summary.content.starts_with("[历史状态摘要]"), "content: {}", summary.content);
        assert!(
            summary.content.contains("- read completed — 主角名叫林动。"),
            "content: {}",
            summary.content
        );
    }

    #[test]
    fn dialogue_replays_text_only_turns_and_drops_tool_turns() {
        let events = fixture_events();
        let restored =
            restore_agent_messages_from_transcript_sync(&events, Some("chat"));
        // summary（system）+ r2 user + r2 assistant；r1 工具轮不回放（user 也
        // 不保留——非 legacy，request_started 带 kind）。
        assert_eq!(restored.len(), 3);
        assert_eq!(restored[0].role, LLMRole::System);
        assert_eq!(restored[1].role, LLMRole::User);
        assert_eq!(restored[1].content, "主角叫什么？");
        assert_eq!(restored[2].role, LLMRole::Assistant);
        assert_eq!(restored[2].content, "主角名叫林动。");
    }

    #[test]
    fn legacy_user_words_survive_from_tool_turns() {
        let events = fixture_events();
        let mut events = events;
        // r1 改为 legacy（request_started 无 kind）：其 user 原话保留。
        events[0] = started(1, "r1", None);
        let restored = restore_agent_messages_from_transcript_sync(&events, Some("chat"));
        let user_texts: Vec<&str> = restored
            .iter()
            .filter(|m| m.role == LLMRole::User)
            .map(|m| m.content.as_str())
            .collect();
        assert!(user_texts.contains(&"看看设定文件"), "users: {user_texts:?}");
    }

    #[test]
    fn kind_mismatched_turns_are_excluded() {
        let events = fixture_events();
        let mut events = events;
        // r2 标为 book kind：按 chat 过滤时被排除（committed 过滤语义）。
        events[5] = started(6, "r2", Some("book"));
        let restored = restore_agent_messages_from_transcript_sync(&events, Some("chat"));
        // 仅剩 summary（r1 的 toolResult 在摘要循环被 kind 过滤跳过？——
        // r1 是 chat：summary 有 read 行）。
        assert!(!restored.is_empty());
        assert!(!restored.iter().any(|m| m.content == "主角叫什么？"));
    }

    #[test]
    fn dialogue_caps_at_twelve_and_summary_at_eight() {
        let mut events = Vec::new();
        let mut seq = 1u64;
        // 20 轮纯对话 → dialogue 截最近 12 条。
        for i in 0..20 {
            let request_id = format!("d{i}");
            let s = seq;
            events.push(started(s, &request_id, Some("chat")));
            events.push(msg_event(s + 1, &request_id, "user", json!({ "role": "user", "content": format!("问{i}") })));
            events.push(msg_event(s + 2, &request_id, "assistant", json!({ "role": "assistant", "content": [{ "type": "text", "text": format!("答{i}") }] })));
            events.push(committed(s + 3, &request_id));
            seq += 4;
        }
        let restored = restore_agent_messages_from_transcript_sync(&events, None);
        // 12 条上限是消息级（非轮级）：40 条消息截最近 12 → 从第 14 轮 user 起。
        assert_eq!(restored.len(), 12);
        assert_eq!(restored[0].content, "问14");
        assert_eq!(restored[11].content, "答19");

        // 10 个工具结果 → 摘要截最近 8 条。
        let mut tool_events = vec![started(1, "rt", Some("chat"))];
        let mut tseq = 2u64;
        for i in 0..10 {
            tool_events.push(msg_event(tseq, "rt", "assistant", json!({
                "role": "assistant",
                "content": [{ "type": "toolCall", "id": format!("c{i}"), "name": "grep", "arguments": {} }],
            })));
            tool_events.push(msg_event(tseq + 1, "rt", "toolResult", json!({
                "role": "toolResult",
                "toolCallId": format!("c{i}"),
                "content": [{ "type": "text", "text": format!("结果{i}") }],
            })));
            tseq += 2;
        }
        tool_events.push(committed(tseq, "rt"));
        let summary = build_historical_tool_summary(&tool_events, None).unwrap();
        let lines: Vec<&str> = summary.content.lines().skip(2).collect();
        assert_eq!(lines.len(), 8);
        assert!(lines.last().unwrap().contains("结果9"), "lines: {lines:?}");
    }

    #[test]
    fn tool_summary_text_folds_whitespace_and_truncates_utf16() {
        let events = vec![
            started(1, "r1", Some("chat")),
            msg_event(2, "r1", "assistant", json!({
                "role": "assistant",
                "content": [{ "type": "toolCall", "id": "t1", "name": "grep", "arguments": {} }],
            })),
            msg_event(3, "r1", "toolResult", json!({
                "role": "toolResult",
                "toolCallId": "t1",
                "toolName": "grep",
                "content": [{ "type": "text", "text": format!("{}长文本", "词 ".repeat(200)) }],
            })),
            committed(4, "r1"),
        ];
        let summary = build_historical_tool_summary(&events, None).unwrap();
        let line = summary.content.lines().nth(2).unwrap();
        assert!(line.contains("..."), "line: {line}");
        assert!(line.encode_utf16().count() < 260, "line 应被截断: {line}");
    }

    #[test]
    fn use_skill_result_uses_expired_placeholder() {
        let events = vec![
            started(1, "r1", Some("chat")),
            msg_event(2, "r1", "assistant", json!({
                "role": "assistant",
                "content": [{ "type": "toolCall", "id": "t1", "name": "use_skill", "arguments": {} }],
            })),
            msg_event(3, "r1", "toolResult", json!({
                "role": "toolResult",
                "toolCallId": "t1",
                "toolName": "use_skill",
                "content": [{ "type": "text", "text": "# 技能全文指令" }],
            })),
            committed(4, "r1"),
        ];
        let summary = build_historical_tool_summary(&events, None).unwrap();
        assert!(
            summary.content.contains("expired; instructions are not active for later turns"),
            "content: {}",
            summary.content
        );
        assert!(!summary.content.contains("技能全文指令"));
    }

    #[test]
    fn boundary_appends_only_when_non_empty() {
        assert!(append_restored_history_boundary(Vec::new(), "zh").is_empty());
        let messages = vec![LLMMessage {
            role: LLMRole::User,
            content: "早".into(),
            tool_calls: None,
            tool_call_id: None,
        }];
        let zh = append_restored_history_boundary(messages.clone(), "zh");
        assert_eq!(zh.len(), 2);
        assert!(zh[1].content.starts_with("[已完成的历史上下文]"));
        let en = append_restored_history_boundary(messages, "en");
        assert!(en[1].content.starts_with("[Completed history context]"));
    }

    /// 同步驱动（绕开文件系统；read_transcript_events 的等价输入）。
    fn restore_agent_messages_from_transcript_sync(
        events: &[TranscriptEvent],
        session_kind: Option<&str>,
    ) -> Vec<LLMMessage> {
        let summary = build_historical_tool_summary(events, session_kind);
        let committed = committed_message_events(events, session_kind);
        let tool_request_ids = request_ids_with_tool_activity(&committed);
        let request_kinds: HashMap<&str, Option<&str>> = events
            .iter()
            .filter_map(|event| match event {
                TranscriptEvent::RequestStarted { request_id, session_kind, .. } => {
                    Some((request_id.as_str(), session_kind.map(|k| k.as_str())))
                }
                _ => None,
            })
            .collect();
        let mut dialogue: Vec<LLMMessage> = committed
            .iter()
            .filter_map(|event| {
                let request_id = event.message_request_id().unwrap_or("");
                if !tool_request_ids.contains(request_id) {
                    return Some((*event).clone());
                }
                let is_legacy_user = session_kind.is_some()
                    && request_kinds.get(request_id).copied().flatten().is_none()
                    && matches!(event, TranscriptEvent::Message { role, .. } if role == "user");
                if is_legacy_user {
                    Some((*event).clone())
                } else {
                    None
                }
            })
            .filter_map(|event| text_only_agent_message(&event))
            .collect();
        if dialogue.len() > MAX_RESTORED_DIALOGUE_MESSAGES {
            dialogue.drain(..dialogue.len() - MAX_RESTORED_DIALOGUE_MESSAGES);
        }
        let mut out = Vec::new();
        if let Some(summary) = summary {
            out.push(summary);
        }
        out.extend(dialogue);
        out
    }
}
