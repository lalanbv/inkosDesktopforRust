//! 章节工作区（chapter-workspace）。
//!
//! 移植自 `packages/core/src/state/chapter-workspace.ts`（163 行）。章节级 fs 编排：
//! 用户备注 / 计划文档 / 版本归档的读写。纯 fs，无业务依赖，复用 [`StateStore`] trait。
//!
//! ## 时间戳/UUID 注入
//! `archive_chapter_version` 的 TS 版用 `new Date()` 与 `crypto.randomUUID()`；
//! Rust 由调用方注入 `now_millis` + `created_at_iso`（对齐「纯内核 + I/O 注入」范式，内核不依赖时钟），
//! 版本 id 的 UUID 部分用 `uuid` crate v4（与 TS 对齐）。

use regex::Regex;
use std::sync::OnceLock;
use uuid::Uuid;

use crate::state::store::{join_path, StateStore};

/// 版本来源（对齐 TS `ChapterVersionSource` 联合类型）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChapterVersionSource {
    Manual,
    Agent,
    Revision,
    Regeneration,
    Restore,
}

impl ChapterVersionSource {
    /// 序列化为 id 片段（对齐 TS 版本 id 的 source 段）。
    fn as_id_segment(self) -> &'static str {
        match self {
            ChapterVersionSource::Manual => "manual",
            ChapterVersionSource::Agent => "agent",
            ChapterVersionSource::Revision => "revision",
            ChapterVersionSource::Regeneration => "regeneration",
            ChapterVersionSource::Restore => "restore",
        }
    }

    /// 由 id 片段解析。
    fn from_id_segment(s: &str) -> Option<Self> {
        Some(match s {
            "manual" => ChapterVersionSource::Manual,
            "agent" => ChapterVersionSource::Agent,
            "revision" => ChapterVersionSource::Revision,
            "regeneration" => ChapterVersionSource::Regeneration,
            "restore" => ChapterVersionSource::Restore,
            _ => return None,
        })
    }
}

/// 章节版本元信息。对齐 TS `ChapterVersion`。
#[derive(Debug, Clone, PartialEq)]
pub struct ChapterVersion {
    pub id: String,
    pub chapter_number: u32,
    pub source: ChapterVersionSource,
    pub created_at: String,
    pub character_count: usize,
}

const _: () = {
    // 编译期确认 source 枚举五变体齐全（与 TS 联合类型对齐）。
    let sources = [
        ChapterVersionSource::Manual,
        ChapterVersionSource::Agent,
        ChapterVersionSource::Revision,
        ChapterVersionSource::Regeneration,
        ChapterVersionSource::Restore,
    ];
    let _ = sources;
};

fn version_id_re() -> &'static Regex {
    // TS: /^(\d{13})_(manual|agent|revision|regeneration|restore)_([0-9a-f-]{36})$/
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^(\d{13})_(manual|agent|revision|regeneration|restore)_([0-9a-f-]{36})$").expect("version id regex"))
}

/// 读章节用户备注；缺失 → 空串。对齐 TS `readChapterUserBrief`。
pub async fn read_chapter_user_brief(
    store: &dyn StateStore,
    book_dir: &str,
    chapter_number: u32,
) -> crate::Result<String> {
    let path = user_brief_path(book_dir, chapter_number);
    Ok(store.read_to_string(&path).await?.unwrap_or_default().trim().to_string())
}

/// 保存章节用户备注；空串则删除文件。对齐 TS `saveChapterUserBrief`。
pub async fn save_chapter_user_brief(
    store: &dyn StateStore,
    book_dir: &str,
    chapter_number: u32,
    brief: &str,
) -> crate::Result<()> {
    let path = user_brief_path(book_dir, chapter_number);
    let normalized = brief.trim();
    if normalized.is_empty() {
        store.remove_file(&path).await?;
        return Ok(());
    }
    store
        .mkdir_p(&join_path(&join_path(book_dir, "story"), "runtime"))
        .await?;
    store.write_string(&path, &format!("{normalized}\n")).await?;
    Ok(())
}

/// 读章节计划文档；缺失 → None。对齐 TS `readChapterPlanDocument`。
pub async fn read_chapter_plan_document(
    store: &dyn StateStore,
    book_dir: &str,
    chapter_number: u32,
) -> crate::Result<Option<String>> {
    store.read_to_string(&plan_path(book_dir, chapter_number)).await
}

/// 归档章节版本，返回版本元信息。
///
/// 对齐 TS `archiveChapterVersion`。`now_millis`（13 位毫秒时间戳）+ `created_at_iso` 由调用方注入；
/// 版本 id = `{now_millis}_{source}_{uuid_v4}`。`character_count` 按 UTF-16 码元计数（对齐 TS `.length`）。
pub async fn archive_chapter_version(
    store: &dyn StateStore,
    book_dir: &str,
    chapter_number: u32,
    content: &str,
    source: ChapterVersionSource,
    now_millis: i64,
    created_at_iso: &str,
) -> crate::Result<ChapterVersion> {
    assert_chapter_number(chapter_number)?;
    let id = format!("{}_{}_{}", now_millis, source.as_id_segment(), Uuid::new_v4());
    let dir = versions_dir(book_dir, chapter_number);
    store.mkdir_p(&dir).await?;
    store.write_string(&join_path(&dir, &format!("{id}.md")), content).await?;
    Ok(ChapterVersion {
        id,
        chapter_number,
        source,
        created_at: created_at_iso.to_string(),
        character_count: content.encode_utf16().count(),
    })
}

/// 列出章节的所有版本（按 createdAt 降序）。对齐 TS `listChapterVersions`。
pub async fn list_chapter_versions(
    store: &dyn StateStore,
    book_dir: &str,
    chapter_number: u32,
) -> crate::Result<Vec<ChapterVersion>> {
    assert_chapter_number(chapter_number)?;
    let dir = versions_dir(book_dir, chapter_number);
    let files = store.list_dir(&dir).await?;
    let mut versions: Vec<ChapterVersion> = Vec::new();
    for file in files {
        if !file.ends_with(".md") {
            continue;
        }
        let id = &file[..file.len() - 3];
        let Some(parsed) = parse_version_id(id) else { continue };
        let path = join_path(&dir, &file);
        let content = store.read_to_string(&path).await?.unwrap_or_default();
        versions.push(ChapterVersion {
            id: id.to_string(),
            chapter_number,
            source: parsed.source,
            created_at: parsed.created_at_iso,
            character_count: content.encode_utf16().count(),
        });
    }
    // createdAt 降序（与 TS localeCompare 一致——ISO8601 字典序 = 时间序）。
    versions.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(versions)
}

/// 读指定版本内容。对齐 TS `readChapterVersion`（非法 id → Constraint 错误）。
pub async fn read_chapter_version(
    store: &dyn StateStore,
    book_dir: &str,
    chapter_number: u32,
    version_id: &str,
) -> crate::Result<String> {
    assert_chapter_number(chapter_number)?;
    if parse_version_id(version_id).is_none() {
        return Err(crate::EngineError::Constraint(format!(
            "Invalid chapter version id: {version_id}"
        )));
    }
    let path = join_path(&versions_dir(book_dir, chapter_number), &format!("{version_id}.md"));
    match store.read_to_string(&path).await? {
        Some(content) => Ok(content),
        None => Err(crate::EngineError::Constraint(format!(
            "chapter version not found: {version_id}"
        ))),
    }
}

// --- 路径与解析辅助 -----------------------------------------------------------

fn user_brief_path(book_dir: &str, chapter_number: u32) -> String {
    assert_chapter_number(chapter_number).ok();
    join_path(
        &join_path(book_dir, "story"),
        &format!("runtime/chapter-{}.user-brief.md", pad_chapter(chapter_number)),
    )
}

fn plan_path(book_dir: &str, chapter_number: u32) -> String {
    join_path(
        &join_path(book_dir, "story"),
        &format!("runtime/chapter-{}.plan.md", pad_chapter(chapter_number)),
    )
}

fn versions_dir(book_dir: &str, chapter_number: u32) -> String {
    join_path(&join_path(book_dir, "chapters"), &format!(".versions/{}", pad_chapter(chapter_number)))
}

fn pad_chapter(chapter_number: u32) -> String {
    format!("{chapter_number:04}")
}

fn assert_chapter_number(chapter_number: u32) -> crate::Result<()> {
    if chapter_number < 1 {
        return Err(crate::EngineError::Constraint(format!(
            "Invalid chapter number: {chapter_number}"
        )));
    }
    Ok(())
}

/// 解析版本 id 为 `(source, created_at_iso)`。timestamp（13 位毫秒）转 ISO8601 由调用方负责——
/// 这里仅校验格式与 source，created_at 用原始 timestamp 的字符串形式（list 时由调用方格式化）。
fn parse_version_id(version_id: &str) -> Option<ParsedVersion> {
    let caps = version_id_re().captures(version_id)?;
    let timestamp: i64 = caps.get(1)?.as_str().parse().ok()?;
    let source = ChapterVersionSource::from_id_segment(caps.get(2)?.as_str())?;
    let _ = caps.get(3)?;
    // created_at_iso：TS 用 new Date(timestamp).toISOString()。这里无法调时钟格式化，
    // 保留 timestamp 转 ISO 的纯函数（无时区，UTC）。
    Some(ParsedVersion {
        source,
        created_at_iso: millis_to_iso(timestamp)?,
    })
}

struct ParsedVersion {
    source: ChapterVersionSource,
    created_at_iso: String,
}

/// 13 位毫秒时间戳 → ISO8601 UTC 字符串（`YYYY-MM-DDTHH:mm:ss.sssZ`）。
/// 对齐 JS `new Date(ms).toISOString()`。无外部 chrono 依赖，手写格式化。
fn millis_to_iso(millis: i64) -> Option<String> {
    if millis < 0 {
        return None;
    }
    let secs = millis / 1000;
    let ms = (millis % 1000) as u32;
    let (year, month, day, hour, minute, second) = epoch_secs_to_components(secs)?;
    Some(format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{ms:03}Z"
    ))
}

/// Unix 秒 → (年, 月, 日, 时, 分, 秒) UTC。用公历算法（civil_from_days 反推）。
fn epoch_secs_to_components(secs: i64) -> Option<(i64, u32, u32, u32, u32, u32)> {
    if secs < 0 {
        return None;
    }
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    let hour = (rem / 3600) as u32;
    let minute = ((rem % 3600) / 60) as u32;
    let second = (rem % 60) as u32;
    let (year, month, day) = civil_from_days(days)?;
    Some((year, month, day, hour, minute, second))
}

/// Howard Hinnant 的 civil_from_days 算法：Unix day count → (year, month, day)。
fn civil_from_days(z: i64) -> Option<(i64, u32, u32)> {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    Some((if m <= 2 { y + 1 } else { y }, m, d))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::store::InMemoryStateStore;

    #[test]
    fn version_id_regex_parses_well_formed_ids() {
        let id = "1700000000000_manual_550e8400-e29b-41d4-a716-446655440000";
        let parsed = parse_version_id(id).expect("合法 id 应解析");
        assert_eq!(parsed.source, ChapterVersionSource::Manual);
        assert!(parsed.created_at_iso.starts_with("2023"));
    }

    #[test]
    fn version_id_regex_rejects_malformed() {
        assert!(parse_version_id("not-an-id").is_none());
        assert!(parse_version_id("1700000000000_unknown_550e8400-e29b-41d4-a716-446655440000").is_none());
        assert!(parse_version_id("1700000000000_manual_short").is_none());
    }

    #[test]
    fn millis_to_iso_matches_js_iso_format() {
        // 2023-01-01T00:00:00.000Z 的毫秒时间戳。
        assert_eq!(millis_to_iso(1672531200000).as_deref(), Some("2023-01-01T00:00:00.000Z"));
        // 含毫秒。
        assert_eq!(millis_to_iso(1672531200123).as_deref(), Some("2023-01-01T00:00:00.123Z"));
    }

    #[test]
    fn civil_from_days_known_dates() {
        // 1970-01-01 = day 0。
        assert_eq!(civil_from_days(0), Some((1970, 1, 1)));
        // 2023-01-01 = day 19358。
        assert_eq!(civil_from_days(19358), Some((2023, 1, 1)));
    }

    #[tokio::test]
    async fn user_brief_roundtrip_and_delete_on_empty() {
        let store = InMemoryStateStore::new();
        // 缺失 → 空串。
        assert_eq!(read_chapter_user_brief(&store, "book", 1).await.unwrap(), "");
        // 写入。
        save_chapter_user_brief(&store, "book", 1, "  hello  ").await.unwrap();
        assert_eq!(read_chapter_user_brief(&store, "book", 1).await.unwrap(), "hello");
        // 空串 → 删除。
        save_chapter_user_brief(&store, "book", 1, "   ").await.unwrap();
        assert_eq!(read_chapter_user_brief(&store, "book", 1).await.unwrap(), "");
    }

    #[tokio::test]
    async fn plan_document_none_when_absent() {
        let store = InMemoryStateStore::new();
        assert_eq!(read_chapter_plan_document(&store, "book", 1).await.unwrap(), None);
        store.set(&join_path(&join_path("book", "story"), "runtime/chapter-0001.plan.md"), "plan");
        assert_eq!(
            read_chapter_plan_document(&store, "book", 1).await.unwrap(),
            Some("plan".to_string())
        );
    }

    #[tokio::test]
    async fn archive_version_writes_file_and_returns_metadata() {
        let store = InMemoryStateStore::new();
        let v = archive_chapter_version(
            &store,
            "book",
            5,
            "章节内容",
            ChapterVersionSource::Agent,
            1700000000000,
            "2023-11-14T22:13:20.000Z",
        )
        .await
        .unwrap();
        assert_eq!(v.chapter_number, 5);
        assert_eq!(v.source, ChapterVersionSource::Agent);
        assert!(v.id.starts_with("1700000000000_agent_"));
        // 文件写入正确路径。
        let path = join_path(&versions_dir("book", 5), &format!("{}.md", v.id));
        assert_eq!(store.read_to_string(&path).await.unwrap(), Some("章节内容".to_string()));
    }

    #[tokio::test]
    async fn list_versions_returns_sorted_desc() {
        let store = InMemoryStateStore::new();
        // 归档两个版本（手动构造文件名以控制时间戳）。
        let dir = versions_dir("book", 1);
        store.mkdir_p(&dir).await.unwrap();
        store
            .write_string(&join_path(&dir, "1700000000000_manual_550e8400-e29b-41d4-a716-446655440000.md"), "a")
            .await
            .unwrap();
        store
            .write_string(&join_path(&dir, "1700000001000_agent_660e8400-e29b-41d4-a716-446655440000.md"), "bb")
            .await
            .unwrap();
        let versions = list_chapter_versions(&store, "book", 1).await.unwrap();
        assert_eq!(versions.len(), 2);
        // 降序：较晚的（1700000001000）在前。
        assert_eq!(versions[0].source, ChapterVersionSource::Agent);
        assert_eq!(versions[1].source, ChapterVersionSource::Manual);
    }

    #[tokio::test]
    async fn read_version_rejects_invalid_id() {
        let store = InMemoryStateStore::new();
        let err = read_chapter_version(&store, "book", 1, "bad-id").await.unwrap_err();
        assert!(matches!(err, crate::EngineError::Constraint(_)));
    }

    #[tokio::test]
    async fn assert_chapter_number_rejects_zero() {
        assert!(super::assert_chapter_number(0).is_err());
        assert!(super::assert_chapter_number(1).is_ok());
    }
}
