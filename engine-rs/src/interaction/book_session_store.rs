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
    active_chain_events, append_transcript_events, legacy_book_session_path,
    read_transcript_events, sessions_dir, transcript_head, transcript_path, TranscriptEvent,
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
            parent_seq: None,
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
            parent_seq: None,
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

/// `migrateBookSession`（67 号建书迁移）：未绑定会话 → 绑定新书并升级为 book
/// 会话。已绑定（bookId 非 null）→ Err（上层按 TS 语义静默忽略）。
pub async fn migrate_book_session(
    project_root: &Path,
    session_id: &str,
    new_book_id: &str,
) -> Result<Option<BookSession>, String> {
    let Some(session) = load_book_session(project_root, session_id).await else {
        return Ok(None);
    };
    if session.book_id.is_some() {
        return Err(format!(
            "Session {session_id} already migrated to book {}",
            session.book_id.unwrap_or_default()
        ));
    }
    append_session_metadata_updated_event(
        project_root,
        session_id,
        SessionMetadataUpdate {
            book_id: Some(Some(new_book_id.to_string())),
            session_kind: Some(SessionKind::Book),
            play_mode: None,
            title: None,
            updated_at: utc_now_ms(),
        },
    )
    .await;
    Ok(load_book_session(project_root, session_id).await)
}

/// `deleteBookSession`：transcript + legacy 双删（不存在静默）。
pub async fn delete_book_session(project_root: &Path, session_id: &str) {
    let _ = tokio::fs::remove_file(transcript_path(project_root, session_id)).await;
    let _ = tokio::fs::remove_file(legacy_book_session_path(project_root, session_id)).await;
}

/// R36 branch 结果（636 号）：移动后的 head 与累计分支次数。
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct BranchBookSessionResult {
    #[serde(rename = "head")]
    pub head: Option<u64>,
    #[serde(rename = "branchCount")]
    pub branch_count: u64,
}

/// branch 失败态（服务端映射：SessionMissing → 404；InvalidTarget → 400）。
#[derive(Debug, Clone, PartialEq)]
pub enum BranchBookSessionError {
    /// 会话不存在（无 transcript）。
    SessionMissing,
    /// 目标 seq 不在事件流中。
    InvalidTarget(u64),
}

/// R36 会话树化（636 号）：分支 = 追加一条 branch_moved 事件（append-only 不
/// 破坏）+ head 指针移动（replay 重建）。`to_seq` = 新 head 的事件 seq；None =
/// resetLeaf（head 置空，后续写入开新链根）。弃用路径零删改、可再 branch 回
/// 去。调用方负责忙判定（轮进行中不 branch，保证单请求事件整体在同一链上）。
pub async fn branch_book_session(
    project_root: &Path,
    session_id: &str,
    to_seq: Option<u64>,
) -> Result<BranchBookSessionResult, BranchBookSessionError> {
    let mut missing = false;
    let mut invalid_target: Option<u64> = None;
    append_transcript_events(project_root, session_id, |events, next_seq| {
        if events.is_empty() {
            missing = true;
            return Vec::new();
        }
        if let Some(target) = to_seq {
            if !events.iter().any(|event| event.seq() == target) {
                invalid_target = Some(target);
                return Vec::new();
            }
        }
        let from_seq = transcript_head(events.iter());
        vec![TranscriptEvent::BranchMoved {
            version: 1,
            session_id: session_id.to_string(),
            seq: next_seq,
            timestamp: utc_now_ms(),
            parent_seq: None,
            from_seq,
            to_seq,
        }]
    })
    .await;
    if let Some(target) = invalid_target {
        return Err(BranchBookSessionError::InvalidTarget(target));
    }
    if missing {
        return Err(BranchBookSessionError::SessionMissing);
    }

    let events = read_transcript_events(project_root, session_id).await;
    Ok(BranchBookSessionResult {
        head: transcript_head(events.iter()),
        branch_count: events
            .iter()
            .filter(|event| matches!(event, TranscriptEvent::BranchMoved { .. }))
            .count() as u64,
    })
}

/// R36 分支点读面（638 号）：一个可回退分支点 = 一条已提交请求的
/// request_committed 事件（该轮收尾点——消息先于 commit 落盘，branch 到它 =
/// 「保留到该轮结束」）。preview 取同 requestId 的 request_started.input；
/// on_active_chain 标记该点是否在当前 head 链上（false = 已弃用分支，可切回）。
/// 对齐 TS `SessionBranchPoint` / `SessionBranchPointsResult`。
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct SessionBranchPoint {
    pub seq: u64,
    #[serde(rename = "requestId")]
    pub request_id: String,
    pub timestamp: u64,
    pub preview: String,
    #[serde(rename = "onActiveChain")]
    pub on_active_chain: bool,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct SessionBranchPointsResult {
    #[serde(rename = "sessionId")]
    pub session_id: String,
    pub head: Option<u64>,
    #[serde(rename = "branchCount")]
    pub branch_count: u64,
    pub points: Vec<SessionBranchPoint>,
}

/// 分支点 derive：committed 轮全集 + started.input 预览 + active 链标记。
/// 会话不存在（无 transcript）返回 None。
pub async fn derive_branch_points(
    project_root: &Path,
    session_id: &str,
) -> Option<SessionBranchPointsResult> {
    let events = read_transcript_events(project_root, session_id).await;
    if events.is_empty() {
        return None;
    }
    let chain: Vec<&TranscriptEvent> = active_chain_events(&events);
    let chain_seqs: std::collections::HashSet<u64> = chain.iter().map(|event| event.seq()).collect();
    let started_inputs: std::collections::HashMap<&str, &str> = events
        .iter()
        .filter_map(|event| match event {
            TranscriptEvent::RequestStarted { request_id, input, .. } => {
                Some((request_id.as_str(), input.as_str()))
            }
            _ => None,
        })
        .collect();
    let mut points = Vec::new();
    for event in &events {
        if let TranscriptEvent::RequestCommitted { seq, timestamp, request_id, .. } = event {
            points.push(SessionBranchPoint {
                seq: *seq,
                request_id: request_id.clone(),
                timestamp: *timestamp,
                preview: started_inputs.get(request_id.as_str()).map(|s| (*s).to_string()).unwrap_or_default(),
                on_active_chain: chain_seqs.contains(seq),
            });
        }
    }
    Some(SessionBranchPointsResult {
        session_id: session_id.to_string(),
        head: transcript_head(events.iter()),
        branch_count: events
            .iter()
            .filter(|event| matches!(event, TranscriptEvent::BranchMoved { .. }))
            .count() as u64,
        points,
    })
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
    /// R36 树化（636 号）：active 链 head / 分支计数（derive 缺省 null/0）。
    pub head: Option<u64>,
    pub branch_count: u64,
    pub created_at: u64,
    pub updated_at: u64,
}

impl BookSessionSummary {
    pub fn to_json(&self) -> Value {
        // TS listBookSessions 摘要：playMode 未设时省略键（121 号对跑勘误）。
        let mut map = json!({
            "sessionId": self.session_id,
            "bookId": self.book_id,
            "sessionKind": self.session_kind,
            "title": self.title,
            "messageCount": self.message_count,
            "head": self.head,
            "branchCount": self.branch_count,
            "createdAt": self.created_at,
            "updatedAt": self.updated_at,
        });
        if let Some(play_mode) = self.play_mode {
            map["playMode"] = json!(play_mode);
        }
        map
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
            head: session.head,
            branch_count: session.branch_count,
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

#[cfg(test)]
mod tests {
    //! R36 会话树化（636 号）：分支/head replay/恢复读面链式回溯。
    //! 与 TS `session-branch.test.ts` 四态镜像（线性旧文件/单分支/多分支/
    //! 重启 replay）+ 压缩边界互不污染 + JSON 形态。

    use super::*;
    use crate::interaction::session_restore::restore_agent_messages_from_transcript_sync;
    use crate::interaction::session_transcript::active_chain_events;
    use std::path::PathBuf;

    fn root_of(dir: &tempfile::TempDir) -> PathBuf {
        dir.path().to_path_buf()
    }

    /// 一轮已提交对话：request_started + user + assistant + request_committed。
    /// parent_seq 构造恒 None——由 append 助手统一戳记（生产同路径）。
    async fn append_committed_round(
        project_root: &Path,
        session_id: &str,
        request_id: &str,
        user_text: &str,
        assistant_text: &str,
    ) -> u64 {
        let now = utc_now_ms();
        let user_uuid = format!("u-{request_id}-user");
        let appended = append_transcript_events(project_root, session_id, |_events, next_seq| {
            let mut seq = next_seq;
            vec![
                TranscriptEvent::RequestStarted {
                    parent_seq: None,
                    version: 1,
                    session_id: session_id.to_string(),
                    seq: { seq += 1; seq - 1 },
                    timestamp: now,
                    request_id: request_id.to_string(),
                    session_kind: None,
                    input: user_text.to_string(),
                },
                TranscriptEvent::Message {
                    parent_seq: None,
                    version: 1,
                    session_id: session_id.to_string(),
                    seq: { seq += 1; seq - 1 },
                    timestamp: now,
                    request_id: request_id.to_string(),
                    uuid: user_uuid.clone(),
                    parent_uuid: None,
                    role: "user".into(),
                    pi_turn_index: None,
                    tool_call_id: None,
                    source_tool_assistant_uuid: None,
                    legacy_display: None,
                    message: serde_json::json!({
                        "role": "user", "content": user_text, "timestamp": now
                    }),
                },
                TranscriptEvent::Message {
                    parent_seq: None,
                    version: 1,
                    session_id: session_id.to_string(),
                    seq: { seq += 1; seq - 1 },
                    timestamp: now,
                    request_id: request_id.to_string(),
                    uuid: format!("u-{request_id}-assistant"),
                    parent_uuid: Some(user_uuid.clone()),
                    role: "assistant".into(),
                    pi_turn_index: None,
                    tool_call_id: None,
                    source_tool_assistant_uuid: None,
                    legacy_display: None,
                    message: serde_json::json!({
                        "role": "assistant",
                        "content": [{ "type": "text", "text": assistant_text }],
                        "timestamp": now
                    }),
                },
                TranscriptEvent::RequestCommitted {
                    parent_seq: None,
                    version: 1,
                    session_id: session_id.to_string(),
                    seq,
                    timestamp: now,
                    request_id: request_id.to_string(),
                },
            ]
        })
        .await;
        assert!(!appended.is_empty());
        appended.last().unwrap().seq()
    }

    fn dialogue_texts(messages: &[crate::llm::provider::LLMMessage]) -> Vec<String> {
        messages
            .iter()
            .filter(|message| message.role != crate::llm::provider::LLMRole::System)
            .map(|message| message.content.clone())
            .collect()
    }

    async fn restored_texts(project_root: &Path, session_id: &str) -> Vec<String> {
        let events = read_transcript_events(project_root, session_id).await;
        dialogue_texts(&restore_agent_messages_from_transcript_sync(&events, None))
    }

    #[tokio::test]
    async fn append_helper_stamps_parent_seq_linear_chain() {
        let dir = tempfile::tempdir().unwrap();
        let root = root_of(&dir);
        append_committed_round(&root, "s1", "r1", "第一问", "第一答").await;
        let events = read_transcript_events(&root, "s1").await;
        let seqs: Vec<u64> = events.iter().map(|event| event.seq()).collect();
        assert_eq!(seqs, vec![1, 2, 3, 4]);
        assert_eq!(events[0].parent_seq(), Some(None));
        assert_eq!(events[1].parent_seq(), Some(Some(1)));
        assert_eq!(events[2].parent_seq(), Some(Some(2)));
        assert_eq!(events[3].parent_seq(), Some(Some(3)));
        assert_eq!(transcript_head(events.iter()), Some(4));
        assert_eq!(active_chain_events(&events).len(), 4);
    }

    #[tokio::test]
    async fn legacy_file_without_parent_seq_parses_and_restores_linear() {
        let dir = tempfile::tempdir().unwrap();
        let root = root_of(&dir);
        let path = transcript_path(&root, "s1");
        tokio::fs::create_dir_all(path.parent().unwrap()).await.unwrap();
        // 直接写 raw JSONL（绕过 append 助手）——模拟 R36 之前的旧文件形态。
        let legacy = [
            r#"{"type":"request_started","version":1,"sessionId":"s1","requestId":"r1","seq":1,"timestamp":1,"input":"旧一问"}"#,
            r#"{"type":"message","version":1,"sessionId":"s1","requestId":"r1","uuid":"lu1","parentUuid":null,"seq":2,"role":"user","timestamp":1,"message":{"role":"user","content":"旧一问","timestamp":1}}"#,
            r#"{"type":"message","version":1,"sessionId":"s1","requestId":"r1","uuid":"lu2","parentUuid":"lu1","seq":3,"role":"assistant","timestamp":1,"message":{"role":"assistant","content":[{"type":"text","text":"旧一答"}],"timestamp":1}}"#,
            r#"{"type":"request_committed","version":1,"sessionId":"s1","requestId":"r1","seq":4,"timestamp":1}"#,
        ]
        .join("\n");
        tokio::fs::write(&path, format!("{legacy}\n")).await.unwrap();

        let events = read_transcript_events(&root, "s1").await;
        assert_eq!(events.len(), 4);
        assert!(events.iter().all(|event| event.parent_seq().is_none()));
        assert_eq!(restored_texts(&root, "s1").await, vec!["旧一问", "旧一答"]);

        // 旧文件上继续追加（新写入恒带值）：链从 legacy 前缀自然延展。
        append_committed_round(&root, "s1", "r2", "新二问", "新二答").await;
        let after = read_transcript_events(&root, "s1").await;
        assert_eq!(after[4].parent_seq(), Some(Some(4)));
        assert_eq!(
            restored_texts(&root, "s1").await,
            vec!["旧一问", "旧一答", "新二问", "新二答"]
        );
    }

    #[tokio::test]
    async fn single_branch_prunes_abandoned_path() {
        let dir = tempfile::tempdir().unwrap();
        let root = root_of(&dir);
        let commit1_seq = append_committed_round(&root, "s1", "r1", "第一问", "第一答").await;
        append_committed_round(&root, "s1", "r2", "第二问（弃）", "第二答（弃）").await;

        let result = branch_book_session(&root, "s1", Some(commit1_seq)).await.unwrap();
        assert_eq!(result.head, Some(commit1_seq));
        assert_eq!(result.branch_count, 1);

        append_committed_round(&root, "s1", "r2b", "第二问", "第二答").await;
        let after = read_transcript_events(&root, "s1").await;
        let chain = active_chain_events(&after);
        assert!(chain
            .iter()
            .all(|event| !matches!(event, TranscriptEvent::BranchMoved { .. })));
        let texts = restored_texts(&root, "s1").await;
        assert_eq!(texts, vec!["第一问", "第一答", "第二问", "第二答"]);
        assert!(!texts.join("").contains("弃"));
        // 重启 replay：head 从落盘事件流重建
        assert_eq!(transcript_head(after.iter()), after.last().unwrap().seq().into());
    }

    #[tokio::test]
    async fn multi_branch_rewrites_from_branch_point() {
        let dir = tempfile::tempdir().unwrap();
        let root = root_of(&dir);
        let commit1_seq = append_committed_round(&root, "s1", "r1", "第一问", "第一答").await;

        branch_book_session(&root, "s1", Some(commit1_seq)).await.unwrap();
        let r1a_commit =
            append_committed_round(&root, "s1", "r1a", "（占位）", "（占位答）").await;
        // 事件流里 r1a 的 started seq = r1a_commit - 3
        let r1a_started = r1a_commit - 3;
        let second = branch_book_session(&root, "s1", Some(r1a_started)).await.unwrap();
        assert_eq!(second.head, Some(r1a_started));
        assert_eq!(second.branch_count, 2);
        append_committed_round(&root, "s1", "r1b", "改写一问", "改写一答").await;

        assert_eq!(
            restored_texts(&root, "s1").await,
            vec!["第一问", "第一答", "改写一问", "改写一答"]
        );
    }

    #[tokio::test]
    async fn reset_leaf_starts_new_root_chain() {
        let dir = tempfile::tempdir().unwrap();
        let root = root_of(&dir);
        append_committed_round(&root, "s1", "r1", "旧问", "旧答").await;
        let result = branch_book_session(&root, "s1", None).await.unwrap();
        assert_eq!(result.head, None);
        assert_eq!(result.branch_count, 1);
        assert!(restored_texts(&root, "s1").await.is_empty());

        append_committed_round(&root, "s1", "r2", "新链问", "新链答").await;
        let events = read_transcript_events(&root, "s1").await;
        let started2 = events
            .iter()
            .find(|event| matches!(event, TranscriptEvent::RequestStarted { request_id, .. } if request_id == "r2"))
            .unwrap();
        assert_eq!(started2.parent_seq(), Some(None));
        assert_eq!(restored_texts(&root, "s1").await, vec!["新链问", "新链答"]);
    }

    #[tokio::test]
    async fn invalid_target_and_missing_session() {
        let dir = tempfile::tempdir().unwrap();
        let root = root_of(&dir);
        assert_eq!(
            branch_book_session(&root, "ghost", Some(1)).await,
            Err(BranchBookSessionError::SessionMissing)
        );
        append_committed_round(&root, "s1", "r1", "第一问", "第一答").await;
        assert_eq!(
            branch_book_session(&root, "s1", Some(99)).await,
            Err(BranchBookSessionError::InvalidTarget(99))
        );
    }

    #[tokio::test]
    async fn compaction_on_abandoned_path_does_not_pollute_new_branch() {
        let dir = tempfile::tempdir().unwrap();
        let root = root_of(&dir);
        let commit1_seq = append_committed_round(&root, "s1", "r1", "第一问", "第一答").await;
        append_committed_round(&root, "s1", "r2", "第二问", "第二答").await;
        // 主路径上压缩：窗口从 r2 的 user 消息起
        append_transcript_events(&root, "s1", |_events, next_seq| {
            vec![TranscriptEvent::Compaction {
                parent_seq: None,
                version: 1,
                session_id: "s1".to_string(),
                seq: next_seq,
                timestamp: utc_now_ms(),
                request_id: "compact-1".to_string(),
                summary: "此前对话摘要".to_string(),
                first_kept_uuid: Some("u-r2-user".to_string()),
                tokens_before: 100,
                trigger: "threshold".to_string(),
            }]
        })
        .await;
        // 压缩生效：恢复窗口只有 r2
        assert_eq!(restored_texts(&root, "s1").await, vec!["第二问", "第二答"]);

        // 分支回 r1 之后：compaction 在弃用路径上，新分支恢复全量
        branch_book_session(&root, "s1", Some(commit1_seq)).await.unwrap();
        append_committed_round(&root, "s1", "r1b", "分支新问", "分支新答").await;
        assert_eq!(
            restored_texts(&root, "s1").await,
            vec!["第一问", "第一答", "分支新问", "分支新答"]
        );
    }

    #[tokio::test]
    async fn derive_exposes_head_and_branch_count() {
        let dir = tempfile::tempdir().unwrap();
        let root = root_of(&dir);
        let commit1_seq = append_committed_round(&root, "s1", "r1", "第一问", "第一答").await;
        append_committed_round(&root, "s1", "r2", "第二问", "第二答").await;
        branch_book_session(&root, "s1", Some(commit1_seq)).await.unwrap();
        append_committed_round(&root, "s1", "r2b", "第二问", "第二答").await;

        let session = load_book_session(&root, "s1").await.unwrap();
        assert_eq!(session.branch_count, 1);
        let last_seq = read_transcript_events(&root, "s1").await.last().unwrap().seq();
        assert_eq!(session.head, Some(last_seq));
        // active 路径：r1 + r2b（弃用 r2 不在 derive 消息里）
        let user_texts: Vec<String> = session
            .messages
            .iter()
            .filter(|message| message.get("role").and_then(Value::as_str) == Some("user"))
            .filter_map(|message| message.get("content").and_then(Value::as_str))
            .map(str::to_string)
            .collect();
        assert_eq!(user_texts, vec!["第一问", "第二问"]);

        // 列表摘要透出同字段
        let summaries = list_book_sessions(&root, None).await;
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].branch_count, 1);
        assert_eq!(summaries[0].head, Some(last_seq));
        assert_eq!(summaries[0].to_json()["branchCount"], serde_json::json!(1));
    }

    #[tokio::test]
    async fn branch_moved_line_serializes_camel_case_with_parent_seq() {
        let dir = tempfile::tempdir().unwrap();
        let root = root_of(&dir);
        let commit1_seq = append_committed_round(&root, "s1", "r1", "第一问", "第一答").await;
        branch_book_session(&root, "s1", Some(commit1_seq)).await.unwrap();

        let raw = tokio::fs::read_to_string(transcript_path(&root, "s1")).await.unwrap();
        let branch_line = raw
            .lines()
            .find(|line| line.contains("branch_moved"))
            .expect("branch_moved 行必须在场");
        let parsed: Value = serde_json::from_str(branch_line).unwrap();
        assert_eq!(parsed["type"], "branch_moved");
        assert_eq!(parsed["fromSeq"], serde_json::json!(commit1_seq));
        assert_eq!(parsed["toSeq"], serde_json::json!(commit1_seq));
        assert!(parsed.get("parentSeq").is_some(), "新写入恒带 parentSeq 键");
        // 全部新写入行恒带 parentSeq
        for line in raw.lines().filter(|line| !line.trim().is_empty()) {
            assert!(line.contains("\"parentSeq\""), "行缺 parentSeq：{line}");
        }
    }

    // ── R36 分支点读面（638 号）：TS session-branch-points.test.ts 五态镜像 ──

    #[tokio::test]
    async fn branch_points_linear_two_rounds_all_on_chain() {
        let dir = tempfile::tempdir().unwrap();
        let root = root_of(&dir);
        append_committed_round(&root, "s1", "r1", "第一个问题", "第一答").await;
        append_committed_round(&root, "s1", "r2", "第二个问题", "第二答").await;

        let result = derive_branch_points(&root, "s1").await.unwrap();
        assert_eq!(result.session_id, "s1");
        assert_eq!(result.branch_count, 0);
        assert_eq!(result.points.len(), 2);
        assert_eq!(result.points[0].preview, "第一个问题");
        assert_eq!(result.points[1].preview, "第二个问题");
        assert!(result.points.iter().all(|point| point.on_active_chain));
        let events = read_transcript_events(&root, "s1").await;
        let last_seq = events.last().unwrap().seq();
        assert_eq!(result.points[1].seq, last_seq);
        assert_eq!(result.head, Some(last_seq));
    }

    #[tokio::test]
    async fn branch_points_after_branch_marks_abandoned_off_chain() {
        let dir = tempfile::tempdir().unwrap();
        let root = root_of(&dir);
        append_committed_round(&root, "s1", "r1", "第一轮", "一答").await;
        append_committed_round(&root, "s1", "r2", "第二轮（将弃用）", "二答").await;
        let before = derive_branch_points(&root, "s1").await.unwrap();
        let first_commit_seq = before.points[0].seq;

        branch_book_session(&root, "s1", Some(first_commit_seq)).await.unwrap();
        append_committed_round(&root, "s1", "r2b", "第二轮重写", "重写答").await;

        let after = derive_branch_points(&root, "s1").await.unwrap();
        assert_eq!(after.branch_count, 1);
        assert_eq!(after.points.len(), 3);
        let abandoned: Vec<_> = after.points.iter().filter(|p| !p.on_active_chain).collect();
        assert_eq!(abandoned.len(), 1);
        assert_eq!(abandoned[0].preview, "第二轮（将弃用）");
        assert!(after
            .points
            .iter()
            .any(|p| p.preview == "第二轮重写" && p.on_active_chain));
        assert!(after
            .points
            .iter()
            .any(|p| p.seq == first_commit_seq && p.on_active_chain));
    }

    #[tokio::test]
    async fn branch_points_skip_failed_rounds() {
        let dir = tempfile::tempdir().unwrap();
        let root = root_of(&dir);
        append_committed_round(&root, "s1", "r1", "成功轮", "答").await;
        append_transcript_events(&root, "s1", |_events, next_seq| {
            vec![TranscriptEvent::RequestFailed {
                parent_seq: None,
                version: 1,
                session_id: "s1".to_string(),
                seq: next_seq,
                timestamp: utc_now_ms(),
                request_id: "r-failed".to_string(),
                error: "boom".to_string(),
            }]
        })
        .await;
        let result = derive_branch_points(&root, "s1").await.unwrap();
        assert_eq!(result.points.len(), 1);
        assert_eq!(result.points[0].preview, "成功轮");
    }

    #[tokio::test]
    async fn branch_points_orphan_committed_preview_empty_and_missing_session_none() {
        let dir = tempfile::tempdir().unwrap();
        let root = root_of(&dir);
        let path = transcript_path(&root, "s1");
        tokio::fs::create_dir_all(path.parent().unwrap()).await.unwrap();
        tokio::fs::write(
            &path,
            format!(
                "{}\n",
                serde_json::json!({
                    "type": "request_committed", "version": 1, "sessionId": "s1",
                    "requestId": "orphan", "seq": 0, "timestamp": 1, "parentSeq": null
                })
            ),
        )
        .await
        .unwrap();
        let result = derive_branch_points(&root, "s1").await.unwrap();
        assert_eq!(result.points.len(), 1);
        assert_eq!(result.points[0].preview, "");
        assert!(result.points[0].on_active_chain);

        assert!(derive_branch_points(&root, "ghost").await.is_none());
    }

    #[tokio::test]
    async fn branch_points_after_reset_leaf_all_off_chain_head_none() {
        let dir = tempfile::tempdir().unwrap();
        let root = root_of(&dir);
        append_committed_round(&root, "s1", "r1", "第一轮", "答").await;
        branch_book_session(&root, "s1", None).await.unwrap();
        let result = derive_branch_points(&root, "s1").await.unwrap();
        assert_eq!(result.head, None);
        assert_eq!(result.branch_count, 1);
        assert_eq!(result.points.len(), 1);
        assert!(!result.points[0].on_active_chain);
    }

    #[tokio::test]
    async fn branch_points_json_shape_camel_case() {
        let dir = tempfile::tempdir().unwrap();
        let root = root_of(&dir);
        append_committed_round(&root, "s1", "r1", "预览问", "答").await;
        let result = derive_branch_points(&root, "s1").await.unwrap();
        let value = serde_json::to_value(&result).unwrap();
        assert!(value.get("sessionId").is_some());
        assert!(value.get("branchCount").is_some());
        assert!(value.get("head").is_some());
        let point = &value["points"][0];
        assert!(point.get("requestId").is_some());
        assert!(point.get("onActiveChain").is_some());
        assert!(point.get("seq").is_some());
        assert!(point.get("preview").is_some());
    }

}
