//! 会话 transcript 事件流（JSONL 持久层）。
//!
//! 移植自 `packages/core/src/interaction/session-transcript.ts`（208 行）+
//! `session-transcript-schema.ts`（6 事件类型）：
//! - 事件类型 [`TranscriptEvent`]（session_created / session_metadata_updated /
//!   request_started / request_committed / request_failed / message）
//! - [`read_transcript_events`]：逐行 JSON 解析（坏行跳过，seq 排序）
//! - [`append_transcript_events`]：per-session 串行队列（读-建-追加）

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::interaction::session::{PlayMode, SessionKind};

/// transcript 角色。
pub const TRANSCRIPT_ROLES: [&str; 4] = ["user", "assistant", "toolResult", "system"];

fn is_zero(n: &u64) -> bool {
    *n == 0
}

/// transcript 事件（tagged by `type`；字段名 camelCase 对齐 zod schema）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TranscriptEvent {
    SessionCreated {
        version: u32,
        #[serde(rename = "sessionId")]
        session_id: String,
        seq: u64,
        timestamp: u64,
        #[serde(rename = "bookId")]
        book_id: Option<String>,
        #[serde(rename = "sessionKind", skip_serializing_if = "Option::is_none")]
        session_kind: Option<SessionKind>,
        #[serde(rename = "playMode", skip_serializing_if = "Option::is_none")]
        play_mode: Option<PlayMode>,
        #[serde(rename = "title", default)]
        title: Option<String>,
        #[serde(rename = "createdAt")]
        created_at: u64,
        #[serde(rename = "updatedAt")]
        updated_at: u64,
    },
    SessionMetadataUpdated {
        version: u32,
        #[serde(rename = "sessionId")]
        session_id: String,
        seq: u64,
        timestamp: u64,
        #[serde(rename = "updatedAt")]
        updated_at: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[serde(rename = "bookId")]
        book_id: Option<String>,
        #[serde(rename = "sessionKind", skip_serializing_if = "Option::is_none")]
        session_kind: Option<SessionKind>,
        #[serde(rename = "playMode", skip_serializing_if = "Option::is_none")]
        play_mode: Option<PlayMode>,
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
    },
    RequestStarted {
        version: u32,
        #[serde(rename = "sessionId")]
        session_id: String,
        seq: u64,
        timestamp: u64,
        #[serde(rename = "requestId")]
        request_id: String,
        #[serde(rename = "sessionKind", skip_serializing_if = "Option::is_none")]
        session_kind: Option<SessionKind>,
        input: String,
    },
    RequestCommitted {
        version: u32,
        #[serde(rename = "sessionId")]
        session_id: String,
        seq: u64,
        timestamp: u64,
        #[serde(rename = "requestId")]
        request_id: String,
    },
    RequestFailed {
        version: u32,
        #[serde(rename = "sessionId")]
        session_id: String,
        seq: u64,
        timestamp: u64,
        #[serde(rename = "requestId")]
        request_id: String,
        error: String,
    },
    Message {
        version: u32,
        #[serde(rename = "sessionId")]
        session_id: String,
        seq: u64,
        timestamp: u64,
        #[serde(rename = "requestId")]
        request_id: String,
        uuid: String,
        #[serde(rename = "parentUuid")]
        parent_uuid: Option<String>,
        role: String,
        #[serde(rename = "piTurnIndex", skip_serializing_if = "Option::is_none")]
        pi_turn_index: Option<u64>,
        #[serde(rename = "toolCallId", skip_serializing_if = "Option::is_none")]
        tool_call_id: Option<String>,
        #[serde(rename = "sourceToolAssistantUuid", skip_serializing_if = "Option::is_none")]
        source_tool_assistant_uuid: Option<String>,
        #[serde(rename = "legacyDisplay", skip_serializing_if = "Option::is_none")]
        legacy_display: Option<LegacyDisplay>,
        /// 原始 AgentMessage（宽松：不做结构校验，重放/展示层自行解析）。
        message: Value,
    },
}

/// message 事件的 legacy 展示补充。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LegacyDisplay {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<String>,
    #[serde(rename = "toolExecutions", default, skip_serializing_if = "Vec::is_empty")]
    pub tool_executions: Vec<Value>,
}

impl TranscriptEvent {
    pub fn seq(&self) -> u64 {
        match self {
            TranscriptEvent::SessionCreated { seq, .. }
            | TranscriptEvent::SessionMetadataUpdated { seq, .. }
            | TranscriptEvent::RequestStarted { seq, .. }
            | TranscriptEvent::RequestCommitted { seq, .. }
            | TranscriptEvent::RequestFailed { seq, .. }
            | TranscriptEvent::Message { seq, .. } => *seq,
        }
    }
}

/// `.inkos/sessions`。
pub fn sessions_dir(project_root: &Path) -> PathBuf {
    project_root.join(".inkos").join("sessions")
}

/// `{root}/.inkos/sessions/{sessionId}.jsonl`。
pub fn transcript_path(project_root: &Path, session_id: &str) -> PathBuf {
    sessions_dir(project_root).join(format!("{session_id}.jsonl"))
}

/// 旧版单文件会话路径（`{sessionId}.json`）。
pub fn legacy_book_session_path(project_root: &Path, session_id: &str) -> PathBuf {
    sessions_dir(project_root).join(format!("{session_id}.json"))
}

/// 读事件流：坏行跳过（zod safeParse 语义），seq 升序。
pub async fn read_transcript_events(project_root: &Path, session_id: &str) -> Vec<TranscriptEvent> {
    let Ok(raw) = tokio::fs::read_to_string(transcript_path(project_root, session_id)).await
    else {
        return Vec::new();
    };
    let mut events = Vec::new();
    for line in raw.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(parsed) = serde_json::from_str::<TranscriptEvent>(line) {
            // zod 拒绝 version≠1 的行；serde 宽松接受（偏差备案：边缘差异）
            if let Ok(Value::Number(version)) =
                serde_json::from_str::<Value>(line).map(|v| v["version"].clone())
            {
                if version.as_u64() != Some(1) {
                    continue;
                }
            }
            events.push(parsed);
        }
    }
    events.sort_by_key(TranscriptEvent::seq);
    events
}

fn session_lock(project_root: &Path, session_id: &str) -> Arc<tokio::sync::Mutex<()>> {
    static LOCKS: OnceLock<std::sync::Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>> =
        OnceLock::new();
    let locks = LOCKS.get_or_init(|| std::sync::Mutex::new(HashMap::new()));
    let key = format!("{}:{}", project_root.display(), session_id);
    locks
        .lock()
        .unwrap()
        .entry(key)
        .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
        .clone()
}

/// 追加事件（per-session 串行：读现有 → 计算次 seq → 追加写）。
/// `build_events` 收到 `(events, next_seq)`；返回实际写入的事件。
pub async fn append_transcript_events<F>(
    project_root: &Path,
    session_id: &str,
    build_events: F,
) -> Vec<TranscriptEvent>
where
    F: FnOnce(&[TranscriptEvent], u64) -> Vec<TranscriptEvent>,
{
    let lock = session_lock(project_root, session_id);
    let _guard = lock.lock().await;
    let events = read_transcript_events(project_root, session_id).await;
    let next_seq = events.iter().map(TranscriptEvent::seq).max().unwrap_or(0) + 1;
    let built = build_events(&events, next_seq);
    if built.is_empty() {
        return Vec::new();
    }
    if tokio::fs::create_dir_all(sessions_dir(project_root)).await.is_err() {
        return Vec::new();
    }
    let mut payload = String::new();
    for event in &built {
        payload.push_str(&serde_json::to_string(event).unwrap_or_default());
        payload.push('\n');
    }
    use tokio::io::AsyncWriteExt;
    let Ok(mut file) = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(transcript_path(project_root, session_id))
        .await
    else {
        return Vec::new();
    };
    let _ = file.write_all(payload.as_bytes()).await;
    // tokio::fs::File 的写经内部缓冲：write_all 返回 ≠ 数据已到 OS（drop 时
    // 才异步刷出）——同进程内紧随的读取会读到旧内容（68 号在 E2E 并发下实测
    // 捕获：追加"成功"但紧随读取缺行）。显式 flush 把缓冲推到 OS 后返回。
    let _ = file.flush().await;
    built
}

/// 忽略未使用常量警告的占位（TRANSCRIPT_ROLES 供后续 agent 端点校验用）。
#[allow(dead_code)]
fn _roles_used() -> [&'static str; 4] {
    TRANSCRIPT_ROLES
}

#[allow(dead_code)]
fn _zero_helper(n: u64) -> bool {
    is_zero(&n)
}
