//! 书籍会话存储 + 项目级会话。
//!
//! 移植自 `packages/core/src/interaction/book-session-store.ts`（251 行）与
//! `project-session-store.ts`（81 行）：load/list/rename/delete/createAndPersist
//! （transcript 事件流为真源 + legacy .json 迁移）+ loadProjectSession /
//! resolveSessionActiveBook。

use std::path::Path;

use serde_json::{json, Value};

use crate::interaction::session::{
    create_book_session, utc_now_ms, BookSession, PlayMode, SessionKind,
};
use crate::interaction::session_restore::{
    derive_book_session_from_transcript, migrate_legacy_book_session_to_transcript,
    read_legacy_book_session,
};
use crate::interaction::session_transcript::{
    append_transcript_events, legacy_book_session_path, read_transcript_events,
    sessions_dir, transcript_path, TranscriptEvent,
};

/// `loadBookSession`：transcript derive → legacy 读取 + 就地迁移。
pub async fn load_book_session(project_root: &Path, session_id: &str) -> Option<BookSession> {
    if let Some(transcript_session) = derive_book_session_from_transcript(project_root, session_id).await {
        return Some(transcript_session);
    }
    let legacy_session = read_legacy_book_session(project_root, session_id).await?;
    migrate_legacy_book_session_to_transcript(project_root, &legacy_session).await;
    derive_book_session_from_transcript(project_root, session_id)
        .await
        .or(Some(legacy_session))
}

async fn append_session_created_event(project_root: &Path, session: &BookSession) {
    append_transcript_events(project_root, &session.session_id, |events, next_seq| {
        if events
            .iter()
            .any(|event| matches!(event, TranscriptEvent::SessionCreated { .. }))
        {
            return Vec::new();
        }
        vec![TranscriptEvent::SessionCreated {
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
        }]
    })
    .await;
}

async fn append_session_metadata_updated_event(
    project_root: &Path,
    session_id: &str,
    metadata: SessionMetadataUpdate,
) {
    append_transcript_events(project_root, session_id, |_events, next_seq| {
        vec![TranscriptEvent::SessionMetadataUpdated {
            version: 1,
            session_id: session_id.to_string(),
            seq: next_seq,
            timestamp: metadata.updated_at,
            updated_at: metadata.updated_at,
            book_id: metadata.book_id.flatten(),
            session_kind: metadata.session_kind,
            play_mode: metadata.play_mode,
            title: metadata.title.flatten(),
        }]
    })
    .await;
}

/// metadata 更新字段（undefined = 不携带该键）。
pub struct SessionMetadataUpdate {
    pub book_id: Option<Option<String>>,
    pub session_kind: Option<SessionKind>,
    pub play_mode: Option<PlayMode>,
    pub title: Option<Option<String>>,
    pub updated_at: u64,
}

/// `renameBookSession`：追加 title 元数据事件 → 重新 derive。
pub async fn rename_book_session(
    project_root: &Path,
    session_id: &str,
    title: &str,
) -> Option<BookSession> {
    load_book_session(project_root, session_id).await?;
    append_session_metadata_updated_event(
        project_root,
        session_id,
        SessionMetadataUpdate {
            book_id: None,
            session_kind: None,
            play_mode: None,
            title: Some(Some(title.to_string())),
            updated_at: utc_now_ms(),
        },
    )
    .await;
    load_book_session(project_root, session_id).await
}

/// `deleteBookSession`：transcript + legacy 双删（不存在静默）。
pub async fn delete_book_session(project_root: &Path, session_id: &str) {
    let _ = tokio::fs::remove_file(transcript_path(project_root, session_id)).await;
    let _ = tokio::fs::remove_file(legacy_book_session_path(project_root, session_id)).await;
}

/// 会话摘要。对齐 TS `BookSessionSummary`。
#[derive(Debug, Clone)]
pub struct BookSessionSummary {
    pub session_id: String,
    pub book_id: Option<String>,
    pub session_kind: Option<SessionKind>,
    pub play_mode: Option<PlayMode>,
    pub title: Option<String>,
    pub message_count: usize,
    pub created_at: u64,
    pub updated_at: u64,
}

impl BookSessionSummary {
    pub fn to_json(&self) -> Value {
        json!({
            "sessionId": self.session_id,
            "bookId": self.book_id,
            "sessionKind": self.session_kind,
            "playMode": self.play_mode,
            "title": self.title,
            "messageCount": self.message_count,
            "createdAt": self.created_at,
            "updatedAt": self.updated_at,
        })
    }
}

/// `listBookSessions`：目录扫描（.jsonl/.json 双形态）→ 逐会话 derive →
/// bookId 过滤 → updatedAt 降序。
pub async fn list_book_sessions(
    project_root: &Path,
    book_id: Option<&str>,
) -> Vec<BookSessionSummary> {
    let Ok(files) = tokio::fs::read_dir(sessions_dir(project_root)).await else {
        return Vec::new();
    };
    let mut session_ids: Vec<String> = Vec::new();
    let mut entries = files;
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name().to_string_lossy().to_string();
        if let Some(id) = name.strip_suffix(".jsonl") {
            session_ids.push(id.to_string());
        } else if let Some(id) = name.strip_suffix(".json") {
            session_ids.push(id.to_string());
        }
    }
    session_ids.sort();
    session_ids.dedup();

    let mut out: Vec<BookSessionSummary> = Vec::new();
    for session_id in &session_ids {
        let Some(session) = load_book_session(project_root, session_id).await else {
            continue;
        };
        // TS `session.bookId !== bookId`：null 只匹配 null
        let matches = match (session.book_id.as_deref(), book_id) {
            (None, None) => true,
            (Some(a), Some(b)) => a == b,
            _ => false,
        };
        if !matches {
            continue;
        }
        out.push(BookSessionSummary {
            session_id: session.session_id.clone(),
            book_id: session.book_id.clone(),
            session_kind: session.session_kind,
            play_mode: session.play_mode,
            title: session.title.clone(),
            message_count: session.messages.len(),
            created_at: session.created_at,
            updated_at: session.updated_at,
        });
    }
    out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    out
}

/// `createAndPersistBookSession`：幂等创建（同 id 已存在直接返回；
/// kind/playMode 有变则追加 metadata 事件）。
pub async fn create_and_persist_book_session(
    project_root: &Path,
    book_id: Option<&str>,
    session_id: Option<&str>,
    session_kind: Option<SessionKind>,
    play_mode: Option<PlayMode>,
) -> BookSession {
    if let Some(session_id) = session_id.filter(|s| !s.is_empty()) {
        if let Some(existing) = load_book_session(project_root, session_id).await {
            let kind_changed = session_kind.is_some() && existing.session_kind != session_kind;
            let play_changed = play_mode.is_some() && existing.play_mode != play_mode;
            if kind_changed || play_changed {
                append_session_metadata_updated_event(
                    project_root,
                    session_id,
                    SessionMetadataUpdate {
                        book_id: None,
                        session_kind,
                        play_mode,
                        title: None,
                        updated_at: utc_now_ms(),
                    },
                )
                .await;
                return load_book_session(project_root, session_id)
                    .await
                    .unwrap_or(existing);
            }
            return existing;
        }
    }
    let session = create_book_session(book_id, session_id, session_kind, play_mode);
    append_session_created_event(project_root, &session).await;
    session
}

/// `persistBookSession`：元数据追加（messages 为空 → created；否则 metadata）。
pub async fn persist_book_session(project_root: &Path, session: &BookSession) {
    let events = read_transcript_events(project_root, &session.session_id).await;
    if events.is_empty() {
        if session.messages.is_empty() {
            append_session_created_event(project_root, session).await;
            return;
        }
        migrate_legacy_book_session_to_transcript(project_root, session).await;
        return;
    }
    append_session_metadata_updated_event(
        project_root,
        &session.session_id,
        SessionMetadataUpdate {
            book_id: Some(session.book_id.clone()),
            session_kind: session.session_kind,
            play_mode: session.play_mode,
            title: Some(session.title.clone()),
            updated_at: session.updated_at,
        },
    )
    .await;
}

// ── 项目级会话（project-session-store.ts） ──────────────────────

/// `loadProjectSession`：`.inkos/session.json` → InteractionSession（宽松 +
/// zod default 填充）；读取失败 → 新会话。
pub async fn load_project_session(project_root: &Path) -> Value {
    let fallback = || {
        json!({
            "sessionId": format!("{}", utc_now_ms()),
            "projectRoot": project_root.display().to_string(),
            "automationMode": "semi",
            "messages": [],
            "draftRounds": [],
            "events": [],
        })
    };
    let Ok(raw) = tokio::fs::read_to_string(project_root.join(".inkos").join("session.json"))
        .await
        .map_err(|_| ())
    else {
        return fallback();
    };
    let Ok(mut parsed) = serde_json::from_str::<Value>(&raw) else {
        return fallback();
    };
    if !parsed.is_object() {
        return fallback();
    }
    let obj = parsed.as_object_mut().unwrap();
    obj.entry("automationMode".to_string())
        .or_insert_with(|| json!("semi"));
    obj.entry("messages".to_string()).or_insert_with(|| json!([]));
    obj.entry("draftRounds".to_string()).or_insert_with(|| json!([]));
    obj.entry("events".to_string()).or_insert_with(|| json!([]));
    if !obj.get("sessionId").and_then(Value::as_str).map(|s| !s.is_empty()).unwrap_or(false) {
        obj.insert("sessionId".to_string(), json!(format!("{}", utc_now_ms())));
    }
    if !obj
        .get("projectRoot")
        .and_then(Value::as_str)
        .map(|s| !s.is_empty())
        .unwrap_or(false)
    {
        obj.insert(
            "projectRoot".to_string(),
            json!(project_root.display().to_string()),
        );
    }
    parsed
}

/// `resolveSessionActiveBook`：activeBookId 在册 → 返回；唯一书 → 返回；否则 None。
pub async fn resolve_session_active_book(
    project_root: &Path,
    session: &Value,
) -> Option<String> {
    let mut book_ids: Vec<String> = Vec::new();
    let Ok(mut entries) = tokio::fs::read_dir(project_root.join("books")).await else {
        return None;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        if entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false) {
            book_ids.push(entry.file_name().to_string_lossy().to_string());
        }
    }
    book_ids.sort();

    if let Some(active) = session
        .get("activeBookId")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
    {
        if book_ids.iter().any(|id| id == active) {
            return Some(active.to_string());
        }
    }
    if book_ids.len() == 1 {
        return book_ids.into_iter().next();
    }
    None
}
