//! 会话类型 + 创建件。
//!
//! 移植自 `packages/core/src/interaction/session.ts` 的持久层子集：
//! SessionKind（10 值）/ PlayMode（2 值）/ BookSession / InteractionMessage /
//! ToolExecution / createBookSession。交互运行时状态机部分（runtime.ts）随
//! agent 端点（65 号）移植。

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// 会话种类。对齐 TS `SessionKindSchema`（10 值）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionKind {
    #[serde(rename = "chat")]
    Chat,
    #[serde(rename = "book-create")]
    BookCreate,
    #[serde(rename = "book")]
    Book,
    #[serde(rename = "short")]
    Short,
    #[serde(rename = "play")]
    Play,
    #[serde(rename = "script")]
    Script,
    #[serde(rename = "storyboard")]
    Storyboard,
    #[serde(rename = "interactive-film")]
    InteractiveFilm,
    #[serde(rename = "edit")]
    Edit,
    #[serde(rename = "interactive-film-authoring")]
    InteractiveFilmAuthoring,
}

impl SessionKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            SessionKind::Chat => "chat",
            SessionKind::BookCreate => "book-create",
            SessionKind::Book => "book",
            SessionKind::Short => "short",
            SessionKind::Play => "play",
            SessionKind::Script => "script",
            SessionKind::Storyboard => "storyboard",
            SessionKind::InteractiveFilm => "interactive-film",
            SessionKind::Edit => "edit",
            SessionKind::InteractiveFilmAuthoring => "interactive-film-authoring",
        }
    }

    pub fn parse(value: &str) -> Option<SessionKind> {
        match value {
            "chat" => Some(SessionKind::Chat),
            "book-create" => Some(SessionKind::BookCreate),
            "book" => Some(SessionKind::Book),
            "short" => Some(SessionKind::Short),
            "play" => Some(SessionKind::Play),
            "script" => Some(SessionKind::Script),
            "storyboard" => Some(SessionKind::Storyboard),
            "interactive-film" => Some(SessionKind::InteractiveFilm),
            "edit" => Some(SessionKind::Edit),
            "interactive-film-authoring" => Some(SessionKind::InteractiveFilmAuthoring),
            _ => None,
        }
    }
}

/// 游玩模式。对齐 TS `PlayModeSchema`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlayMode {
    #[serde(rename = "open")]
    Open,
    #[serde(rename = "guided")]
    Guided,
}

impl PlayMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            PlayMode::Open => "open",
            PlayMode::Guided => "guided",
        }
    }

    pub fn parse(value: &str) -> Option<PlayMode> {
        match value {
            "open" => Some(PlayMode::Open),
            "guided" => Some(PlayMode::Guided),
            _ => None,
        }
    }
}

/// 书籍会话（持久层形态；messages 为宽松 Value 列表，展示层自 derive）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BookSession {
    #[serde(rename = "sessionId")]
    pub session_id: String,
    /// null 表示未绑定书籍。
    #[serde(rename = "bookId", default)]
    pub book_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_kind: Option<SessionKind>,
    #[serde(rename = "playMode", skip_serializing_if = "Option::is_none", default)]
    pub play_mode: Option<PlayMode>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub messages: Vec<Value>,
    #[serde(rename = "createdAt")]
    pub created_at: u64,
    #[serde(rename = "updatedAt")]
    pub updated_at: u64,
}

impl BookSession {
    /// 端点响应形态：补 zod default（draftRounds/events 空数组）。
    pub fn to_response_json(&self) -> Value {
        let mut map = serde_json::to_value(self).unwrap_or(Value::Null);
        if let Some(obj) = map.as_object_mut() {
            obj.entry("draftRounds".to_string())
                .or_insert_with(|| json!([]));
            obj.entry("events".to_string()).or_insert_with(|| json!([]));
        }
        map
    }
}

/// `isSafeBookId`（book-id.ts 的路径安全校验子集：非空 + 无路径分隔/空字节 + 防 `..`）。
pub fn is_safe_book_id(book_id: &str) -> bool {
    !book_id.is_empty()
        && !book_id.contains('/')
        && !book_id.contains('\\')
        && !book_id.contains('\0')
        && book_id != "."
        && book_id != ".."
}

/// `createBookSession`：新会话（sessionId 缺省 `毫秒时间戳-6位随机36`）。
pub fn create_book_session(
    book_id: Option<&str>,
    session_id: Option<&str>,
    session_kind: Option<SessionKind>,
    play_mode: Option<PlayMode>,
) -> BookSession {
    let now = utc_now_ms();
    let session_id = session_id
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("{now}-{}", random_suffix()));
    BookSession {
        session_id,
        book_id: book_id.filter(|id| is_safe_book_id(id)).map(|s| s.to_string()),
        session_kind,
        play_mode,
        title: None,
        messages: Vec::new(),
        created_at: now,
        updated_at: now,
    }
}

pub fn utc_now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// `Math.random().toString(36).slice(2, 8)`（6 位 base36 随机串）。
pub fn random_suffix() -> String {
    // xorshift 从纳秒时钟播种（会话 id 仅要求唯一性，密码学强度不必要）
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64 ^ d.as_secs())
        .unwrap_or(0x9E3779B9);
    let mut state = seed | 1;
    let mut out = String::with_capacity(6);
    for _ in 0..6 {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        out.push(std::char::from_digit((state % 36) as u32, 36).unwrap_or('0'));
    }
    out
}
