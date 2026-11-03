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

/// `Option<Option<T>>` 的 serde 语义修正：JSON `null` 默认塌缩为外层 `None`
/// （与键缺省不可分）。本仓 parentSeq 三态必须可分：缺省=legacy 线性链、
/// null=显式链根、数字=父 seq。键缺省走 `default`；在场时包一层 Some。
fn deserialize_double_option<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    Ok(Some(Option::<T>::deserialize(deserializer)?))
}

/// transcript 事件（tagged by `type`；字段名 camelCase 对齐 zod schema）。
///
/// R36 会话树化（636 号）：全部变体携带 `parentSeq`（链父 seq）。三态镜像
/// TS `number | null | undefined`：`Option<Option<u64>>`——`None`=键缺省
/// （legacy 线性语义），`Some(None)`=显式链根（null），`Some(Some(n))`=父
/// seq。新写入由 [`append_transcript_events`] 统一戳记（恒带键，链根为
/// null）；旧文件零改写解析。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TranscriptEvent {
    SessionCreated {
        version: u32,
        #[serde(rename = "sessionId")]
        session_id: String,
        seq: u64,
        timestamp: u64,
        #[serde(
            rename = "parentSeq",
            default,
            deserialize_with = "deserialize_double_option"
        )]
        parent_seq: Option<Option<u64>>,
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
        #[serde(
            rename = "parentSeq",
            default,
            deserialize_with = "deserialize_double_option"
        )]
        parent_seq: Option<Option<u64>>,
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
        #[serde(
            rename = "parentSeq",
            default,
            deserialize_with = "deserialize_double_option"
        )]
        parent_seq: Option<Option<u64>>,
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
        #[serde(
            rename = "parentSeq",
            default,
            deserialize_with = "deserialize_double_option"
        )]
        parent_seq: Option<Option<u64>>,
        #[serde(rename = "requestId")]
        request_id: String,
    },
    RequestFailed {
        version: u32,
        #[serde(rename = "sessionId")]
        session_id: String,
        seq: u64,
        timestamp: u64,
        #[serde(
            rename = "parentSeq",
            default,
            deserialize_with = "deserialize_double_option"
        )]
        parent_seq: Option<Option<u64>>,
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
        #[serde(
            rename = "parentSeq",
            default,
            deserialize_with = "deserialize_double_option"
        )]
        parent_seq: Option<Option<u64>>,
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
        /// 876 号：上游 2.0（b399a8d3，8/30）把 `legacyDisplay` 改名 `display`
        /// 且 TS 严格 schema 只认新名——此前 Rust 写旧名，Node 回退端读到即整份
        /// 会话拒读。写新名、读兼容旧名（alias：旧 Rust 文件 + 2.0 前 TS 文件）。
        /// 装箱：展示补充稀有出现，避免把每个事件撑到最大变体尺寸（large_enum_variant）。
        #[serde(rename = "display", alias = "legacyDisplay", skip_serializing_if = "Option::is_none")]
        display: Option<Box<MessageDisplay>>,
        /// 876 号：上游 2.0 消息可见性——`"model"`=仅模型可见，UI 恢复剔除
        /// （TS visibleMessageEvents 同语义）；Rust 自身不写。
        #[serde(skip_serializing_if = "Option::is_none")]
        visibility: Option<String>,
        /// 原始 AgentMessage（宽松：不做结构校验，重放/展示层自行解析）。
        message: Value,
    },
    /// R32a 会话压缩条目（553 号）：恢复窗口从 `firstKeptUuid` 起，其前的对话
    /// 以 `summary`（LLM 生成，迭代链式时已并入上一条摘要）替代。与 TS
    /// CompactionEventSchema 同形（camelCase 序列化对齐 zod schema）。
    /// R36 起压缩沿 active 链解释（636 号）：恢复取 root→head 路径上最近
    /// 一条 compaction，分支各自的压缩边界互不污染。
    Compaction {
        version: u32,
        #[serde(rename = "sessionId")]
        session_id: String,
        seq: u64,
        timestamp: u64,
        #[serde(
            rename = "parentSeq",
            default,
            deserialize_with = "deserialize_double_option"
        )]
        parent_seq: Option<Option<u64>>,
        #[serde(rename = "requestId")]
        request_id: String,
        summary: String,
        #[serde(rename = "firstKeptUuid")]
        first_kept_uuid: Option<String>,
        #[serde(rename = "tokensBefore")]
        tokens_before: u64,
        trigger: String,
    },
    /// R36 会话树化（636 号）：分支指针移动（对齐 Pi branch 的 leaf 语义）。
    /// `toSeq` = 新 head（null = resetLeaf）；`fromSeq` = 移动前 head。自身
    /// 不入任何对话链——replay 只改写 head 不延伸链。
    BranchMoved {
        version: u32,
        #[serde(rename = "sessionId")]
        session_id: String,
        seq: u64,
        timestamp: u64,
        #[serde(
            rename = "parentSeq",
            default,
            deserialize_with = "deserialize_double_option"
        )]
        parent_seq: Option<Option<u64>>,
        #[serde(rename = "fromSeq")]
        from_seq: Option<u64>,
        #[serde(rename = "toSeq")]
        to_seq: Option<u64>,
    },
    /// 876 号：上游 2.0 请求交付账本事件（TS agent-session 每轮末追加，
    /// `state` 为 RequestDeliveryState）。Rust 不消费交付语义（695 未迁域），
    /// 但必须能解析：此前无此变体，读取器把该行当坏行跳过，`next_seq` 与
    /// `transcript_head` 都漏算它——Node 写过的会话经 Rust 追加时复用其 seq
    /// 并从其前驱分叉父链（R36 树化会话损坏）。`state` 原样保真。
    RequestDelivery {
        version: u32,
        #[serde(rename = "sessionId")]
        session_id: String,
        seq: u64,
        timestamp: u64,
        #[serde(
            rename = "parentSeq",
            default,
            deserialize_with = "deserialize_double_option"
        )]
        parent_seq: Option<Option<u64>>,
        #[serde(rename = "requestId")]
        request_id: String,
        state: Value,
    },
}

/// message 事件的展示补充（876 号：对齐上游 2.0 MessageEventSchema.display）。
/// thinking/toolExecutions 为 Rust 回放所用；completion/userInput 是 2.0 TS
/// 写入的展示字段，Rust 恢复暂不消费（2.0 恢复语义属 695 未迁域），原样保真。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct MessageDisplay {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion: Option<Value>,
    #[serde(rename = "userInput", skip_serializing_if = "Option::is_none")]
    pub user_input: Option<Value>,
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
            | TranscriptEvent::Message { seq, .. }
            | TranscriptEvent::Compaction { seq, .. }
            | TranscriptEvent::BranchMoved { seq, .. }
            | TranscriptEvent::RequestDelivery { seq, .. } => *seq,
        }
    }

    /// message 事件的定位 uuid（R32a 压缩窗口锚点；其余事件 None）。
    pub fn message_uuid(&self) -> Option<&str> {
        match self {
            TranscriptEvent::Message { uuid, .. } => Some(uuid.as_str()),
            _ => None,
        }
    }

    /// parentSeq 三态读取：`None`=键缺省（legacy 线性），`Some(None)`=显式
    /// 链根，`Some(Some(n))`=父 seq（R36 链回溯消费）。
    pub fn parent_seq(&self) -> Option<Option<u64>> {
        match self {
            TranscriptEvent::SessionCreated { parent_seq, .. }
            | TranscriptEvent::SessionMetadataUpdated { parent_seq, .. }
            | TranscriptEvent::RequestStarted { parent_seq, .. }
            | TranscriptEvent::RequestCommitted { parent_seq, .. }
            | TranscriptEvent::RequestFailed { parent_seq, .. }
            | TranscriptEvent::Message { parent_seq, .. }
            | TranscriptEvent::Compaction { parent_seq, .. }
            | TranscriptEvent::BranchMoved { parent_seq, .. }
            | TranscriptEvent::RequestDelivery { parent_seq, .. } => *parent_seq,
        }
    }

    fn set_parent_seq(&mut self, value: Option<Option<u64>>) {
        match self {
            TranscriptEvent::SessionCreated { parent_seq, .. }
            | TranscriptEvent::SessionMetadataUpdated { parent_seq, .. }
            | TranscriptEvent::RequestStarted { parent_seq, .. }
            | TranscriptEvent::RequestCommitted { parent_seq, .. }
            | TranscriptEvent::RequestFailed { parent_seq, .. }
            | TranscriptEvent::Message { parent_seq, .. }
            | TranscriptEvent::Compaction { parent_seq, .. }
            | TranscriptEvent::BranchMoved { parent_seq, .. }
            | TranscriptEvent::RequestDelivery { parent_seq, .. } => *parent_seq = value,
        }
    }

    fn is_branch_moved(&self) -> bool {
        matches!(self, TranscriptEvent::BranchMoved { .. })
    }
}

/// R36 head replay（636 号）：按 seq 序重放 branch_moved 重建当前 head——
/// `toSeq` 即新 head（null = resetLeaf 置空），其余事件延伸链（head = seq）。
/// 与 Pi「leaf 不落盘」不同：head 从事件流可重建（O(n) 一次，装载路径已有
/// 全量读取），服务端跨进程重启/双引擎切换后分支状态不丢。
pub fn transcript_head<'a>(events: impl IntoIterator<Item = &'a TranscriptEvent>) -> Option<u64> {
    let mut head = None;
    for event in events {
        head = match event {
            TranscriptEvent::BranchMoved { to_seq, .. } => *to_seq,
            other => Some(other.seq()),
        };
    }
    head
}

/// R36 active 链过滤（636 号）：从 head 沿 parentSeq 反向回溯出 root→head
/// 路径（升序返回，等于 seq 序的路径子集——父 seq 恒小于子 seq，append-only
/// 不变量）。弃用分支上的事件被剪除；纯 legacy 文件回溯退化为全量（键缺省
/// 语义 = 线性链，父即 seq 序前一事件）——旧行为零改写。链断（parentSeq
/// 指向不存在事件）时保留已收集后缀，恢复安全网同 553 号 compaction 容错
/// 先例。branch_moved 自身不入链（元事件，replay 时改写 head 而非延伸链）。
pub fn active_chain_events(events: &[TranscriptEvent]) -> Vec<&TranscriptEvent> {
    if events.is_empty() {
        return Vec::new();
    }
    let mut sorted: Vec<&TranscriptEvent> = events.iter().collect();
    sorted.sort_by_key(|event| event.seq());
    let Some(head) = transcript_head(sorted.iter().copied()) else {
        return Vec::new();
    };

    let index_by_seq: HashMap<u64, usize> = sorted
        .iter()
        .enumerate()
        .map(|(index, event)| (event.seq(), index))
        .collect();

    let mut chain: Vec<&TranscriptEvent> = Vec::new();
    let mut cursor = index_by_seq.get(&head).copied();
    while let Some(index) = cursor {
        if chain.len() > sorted.len() {
            break;
        }
        let event = sorted[index];
        if event.is_branch_moved() {
            break;
        }
        chain.push(event);
        cursor = match event.parent_seq() {
            // 键缺省（legacy 线性语义）：父 = seq 序前一事件
            None => index.checked_sub(1).map(|prev| sorted[prev].seq()),
            Some(None) => None,
            Some(Some(parent)) => Some(parent),
        }
        .and_then(|seq| index_by_seq.get(&seq).copied());
    }
    chain.reverse();
    chain
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
    parse_transcript(&raw).0
}

/// 解析 transcript 原文：返回（可识别事件 seq 升序，全部行的原始 seq 最大值）。
/// 876 号：后者覆盖本端不识别的行——上游后续新增事件类型时，追加 seq 仍取
/// 全文件最大值 +1，不会复用被跳过行的 seq（request_delivery 缺变体时的教训）。
fn parse_transcript(raw: &str) -> (Vec<TranscriptEvent>, Option<u64>) {
    let mut events = Vec::new();
    let mut raw_max_seq: Option<u64> = None;
    for line in raw.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if let Some(seq) = value.get("seq").and_then(Value::as_u64) {
            raw_max_seq = Some(raw_max_seq.map_or(seq, |max| max.max(seq)));
        }
        // zod 拒绝 version≠1 的行；serde 宽松接受（偏差备案：边缘差异）
        if value.get("version").and_then(Value::as_u64).is_some_and(|version| version != 1) {
            continue;
        }
        if let Ok(parsed) = serde_json::from_value::<TranscriptEvent>(value) {
            events.push(parsed);
        }
    }
    events.sort_by_key(TranscriptEvent::seq);
    (events, raw_max_seq)
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
///
/// R36（636 号）：parentSeq 由本助手统一戳记——链语义单一事实源，写入方
/// 零感知。branch_moved 改写 head 不延伸链；其余事件以戳记时 head 为父并
/// 推进 head。per-session 串行锁保证戳记时 head 即追加序前驱。
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
    let raw = tokio::fs::read_to_string(transcript_path(project_root, session_id))
        .await
        .unwrap_or_default();
    let (events, raw_max_seq) = parse_transcript(&raw);
    // 876 号：取全文件原始 seq 最大值（含本端不识别而被跳过的行），防 seq 复用。
    let next_seq = raw_max_seq.unwrap_or(0) + 1;
    let mut built = build_events(&events, next_seq);
    if built.is_empty() {
        return Vec::new();
    }
    let mut head = transcript_head(&events);
    for event in built.iter_mut() {
        event.set_parent_seq(Some(head));
        head = if event.is_branch_moved() {
            match event {
                TranscriptEvent::BranchMoved { to_seq, .. } => *to_seq,
                _ => unreachable!("is_branch_moved 已判定"),
            }
        } else {
            Some(event.seq())
        };
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

#[cfg(test)]
mod tests {
    //! 876 号：跨引擎 transcript 兼容金标准。三条行与 TS 侧
    //! `packages/core/src/__tests__/session-transcript-cross-engine.test.ts`
    //! **逐字共用**——双端恒等锁：任一端改 schema 而另一端不跟，必有一端红。
    use super::*;
    use crate::interaction::session_restore::message_events_to_interaction_messages;

    /// 876 前 Rust（及 2.0 前 TS）写出的旧名形态。
    const GOLDEN_LEGACY_DISPLAY_LINE: &str = r#"{"type":"message","version":1,"sessionId":"s876","seq":3,"timestamp":1730000000000,"parentSeq":2,"requestId":"req-1","uuid":"u-3","parentUuid":null,"role":"assistant","legacyDisplay":{"toolExecutions":[{"id":"exec-1","tool":"draft_structure","label":"draft_structure","status":"completed"}]},"message":{"role":"assistant","content":[{"type":"text","text":"已生成结构"}]}}"#;
    /// 上游 2.0 TS 写出的新名形态（含 visibility 与 display 全字段）。
    const GOLDEN_TS20_MESSAGE_LINE: &str = r#"{"type":"message","version":1,"sessionId":"s876","seq":4,"timestamp":1730000000001,"parentSeq":3,"requestId":"req-1","uuid":"u-4","parentUuid":"u-3","role":"assistant","visibility":"model","display":{"completion":{"status":"answered","message":"完成"},"userInput":{"text":"写大纲","language":"zh","attachments":[]},"thinking":"先列人物","toolExecutions":[{"id":"exec-2","tool":"write_outline","status":"completed"}]},"message":{"role":"assistant","content":[]}}"#;
    /// 上游 2.0 TS 每轮末追加的交付账本事件。
    const GOLDEN_REQUEST_DELIVERY_LINE: &str = r#"{"type":"request_delivery","version":1,"sessionId":"s876","seq":5,"timestamp":1730000000002,"parentSeq":4,"requestId":"req-1","state":{"version":1,"authorRequest":"写大纲","declared":true,"steps":[],"receipts":[]}}"#;

    fn as_value(line: &str) -> Value {
        serde_json::from_str(line).expect("golden line is valid JSON")
    }

    #[test]
    fn legacy_display_alias_reads_and_writes_canonical_display() {
        let event: TranscriptEvent = serde_json::from_str(GOLDEN_LEGACY_DISPLAY_LINE).expect("旧名行可读");
        let TranscriptEvent::Message { display, .. } = &event else { panic!("应为 message") };
        assert_eq!(display.as_ref().map(|d| d.tool_executions.len()), Some(1));
        let written = serde_json::to_value(&event).unwrap();
        assert!(written.get("display").is_some(), "写出须为 TS 2.0 认的新名 display");
        assert!(written.get("legacyDisplay").is_none(), "不得再写旧名（TS 严格 schema 拒读）");
    }

    #[test]
    fn ts20_message_round_trips_losslessly_including_visibility() {
        let event: TranscriptEvent = serde_json::from_str(GOLDEN_TS20_MESSAGE_LINE).expect("2.0 行可读");
        let TranscriptEvent::Message { display, visibility, .. } = &event else { panic!("应为 message") };
        assert_eq!(visibility.as_deref(), Some("model"));
        let display = display.as_ref().expect("display 不得丢");
        assert!(display.completion.is_some() && display.user_input.is_some());
        assert_eq!(display.thinking.as_deref(), Some("先列人物"));
        assert_eq!(display.tool_executions.len(), 1);
        assert_eq!(serde_json::to_value(&event).unwrap(), as_value(GOLDEN_TS20_MESSAGE_LINE), "读写须无损");
    }

    #[test]
    fn request_delivery_line_parses_and_round_trips() {
        let event: TranscriptEvent = serde_json::from_str(GOLDEN_REQUEST_DELIVERY_LINE).expect("request_delivery 可读");
        assert!(matches!(event, TranscriptEvent::RequestDelivery { seq: 5, .. }));
        assert_eq!(serde_json::to_value(&event).unwrap(), as_value(GOLDEN_REQUEST_DELIVERY_LINE));
    }

    #[test]
    fn raw_max_seq_covers_lines_this_engine_cannot_parse() {
        let text = format!(
            "{}\n{}\n",
            r#"{"type":"session_created","version":1,"sessionId":"s876","seq":1,"timestamp":1,"parentSeq":null,"bookId":null,"title":null,"createdAt":1,"updatedAt":1}"#,
            r#"{"type":"some_future_event","version":1,"sessionId":"s876","seq":7,"timestamp":2}"#,
        );
        let (events, raw_max_seq) = parse_transcript(&text);
        assert_eq!(events.len(), 1, "未知类型行跳过");
        assert_eq!(raw_max_seq, Some(7), "但其 seq 必须计入，防追加复用");
    }

    #[tokio::test]
    async fn append_after_ts_request_delivery_tail_neither_reuses_seq_nor_forks_chain() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        tokio::fs::create_dir_all(sessions_dir(root)).await.unwrap();
        let ts_written = [
            r#"{"type":"session_created","version":1,"sessionId":"s876","seq":1,"timestamp":1,"parentSeq":null,"bookId":null,"title":null,"createdAt":1,"updatedAt":1}"#,
            r#"{"type":"request_started","version":1,"sessionId":"s876","seq":2,"timestamp":2,"parentSeq":1,"requestId":"req-1","input":"写大纲"}"#,
            &GOLDEN_TS20_MESSAGE_LINE.replace(r#""seq":4"#, r#""seq":3"#).replace(r#""parentSeq":3"#, r#""parentSeq":2"#),
            &GOLDEN_REQUEST_DELIVERY_LINE.replace(r#""seq":5"#, r#""seq":4"#).replace(r#""parentSeq":4"#, r#""parentSeq":3"#),
        ]
        .join("\n");
        tokio::fs::write(transcript_path(root, "s876"), format!("{ts_written}\n")).await.unwrap();

        let written = append_transcript_events(root, "s876", |_, next_seq| {
            vec![TranscriptEvent::RequestCommitted {
                version: 1,
                session_id: "s876".into(),
                seq: next_seq,
                timestamp: 3,
                parent_seq: None,
                request_id: "req-1".into(),
            }]
        })
        .await;
        assert_eq!(written.len(), 1);
        assert_eq!(written[0].seq(), 5, "seq 须越过 TS 末行 request_delivery(4)，不得复用");
        assert_eq!(written[0].parent_seq(), Some(Some(4)), "父须为 request_delivery(4)，不得从其前驱分叉");
    }

    #[test]
    fn model_visibility_messages_are_hidden_from_ui_projection() {
        let model_only: TranscriptEvent = serde_json::from_str(GOLDEN_TS20_MESSAGE_LINE).unwrap();
        let visible: TranscriptEvent = serde_json::from_str(&GOLDEN_TS20_MESSAGE_LINE.replace(r#""visibility":"model","#, "")).unwrap();
        assert!(message_events_to_interaction_messages(&[&model_only]).is_empty(), "visibility=model 不入 UI");
        assert!(!message_events_to_interaction_messages(&[&visible]).is_empty(), "无 visibility 正常投影");
    }
}
