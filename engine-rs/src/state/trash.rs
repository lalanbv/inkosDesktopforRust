//! 回收站（R58/1676 号：破坏性操作护栏）——回滚与删书不再硬删。
//!
//! - 回滚：被废的章正文、草稿、快照、runtime 工件，连同会被覆盖或删除的真相文件、
//!   结构化状态与章节索引，整批移进 `chapters/.trash/rollback-{ts}-{target}/`，书内
//!   相对路径原样保留。sqlite 加速索引（可重建的派生物）不进回收站，照旧直接删。
//! - 删书：整个书目录移进项目级回收站 `{projectRoot}/.trash/books/{bookId}--{ts}/`。
//!   放在 books 根之外——作品发现、迁移、备份白名单都看不到它。
//!
//! 两个纯函数与 Node 回退腿同一条规则（`packages/core/src/state/trash.ts` 的
//! `trashBatchName` / `trashedBookName`），对跑用同一份夹具钉两端条目集。
//!
//! 本腿没有带日志的原子文件集，所以是 rename 批次：整批移完才做后面的恢复与索引
//! 改写；中途任何一步失败，已移项按逆序 rename 回原路径，回位失败的条目点名报出。

use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// 回滚会覆盖或删除的真相文件（两腿恢复逻辑的并集；在场的才进回收站）。
pub const ROLLBACK_TRUTH_FILES: [&str; 7] = [
    "current_state.md",
    "pending_hooks.md",
    "chapter_summaries.md",
    "particle_ledger.md",
    "subplot_board.md",
    "emotional_arcs.md",
    "character_matrix.md",
];

/// 回滚除了上面这些还会整份覆盖或移除的两样：结构化状态目录、章节索引。
pub const ROLLBACK_TRUTH_PATHS: [&str; 2] = ["story/state", "chapters/index.json"];

pub fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since_epoch| since_epoch.as_millis() as u64)
        .unwrap_or(0)
}

/// `yyyyMMddHHmmss`（UTC）——与 JS `new Date(ms).toISOString()` 去分隔取前 14 位一致。
pub fn compact_timestamp(timestamp_millis: u64) -> String {
    let iso = i64::try_from(timestamp_millis)
        .ok()
        .and_then(crate::state::chapter_workspace::millis_to_iso)
        .unwrap_or_else(|| "1970-01-01T00:00:00.000Z".to_string());
    iso.chars()
        .filter(|c| c.is_ascii_digit())
        .take(14)
        .collect()
}

/// 回滚批次目录名：`rollback-{yyyyMMddHHmmss}-{target}`。
pub fn trash_batch_name(timestamp_millis: u64, target_chapter: u32) -> String {
    format!(
        "rollback-{}-{target_chapter}",
        compact_timestamp(timestamp_millis)
    )
}

/// 被删的书在项目级回收站里的目录名：`{bookId}--{yyyyMMddHHmmss}`。
pub fn trashed_book_name(book_id: &str, timestamp_millis: u64) -> String {
    format!("{book_id}--{}", compact_timestamp(timestamp_millis))
}

/// 在 `parent` 下建出名为 `base` 的目录；同名已在时追加 `-2`、`-3`…（与章级回收站的
/// 重名后缀同一惯例）。返回最终的名字。
pub async fn reserve_dir(parent: &Path, base: &str) -> io::Result<String> {
    tokio::fs::create_dir_all(parent).await?;
    for attempt in 1u32.. {
        let candidate = if attempt == 1 {
            base.to_string()
        } else {
            format!("{base}-{attempt}")
        };
        match tokio::fs::create_dir(parent.join(&candidate)).await {
            Ok(()) => return Ok(candidate),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::other("trash name space exhausted"))
}

/// 把 `book_dir/<relative>` 移进 `book_dir/<batch>/<relative>`（目录随 rename 整体走），
/// 记进 `moved` 以便回位。不存在的路径跳过。
pub async fn move_into_batch(
    book_dir: &Path,
    batch: &str,
    relative: &str,
    moved: &mut Vec<(PathBuf, PathBuf)>,
) -> io::Result<()> {
    let source = book_dir.join(relative);
    if !tokio::fs::try_exists(&source).await.unwrap_or(false) {
        return Ok(());
    }
    let target = book_dir.join(batch).join(relative);
    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::rename(&source, &target).await?;
    moved.push((source, target));
    Ok(())
}

/// 回位：按移动的逆序逐个 rename 回原路径。返回回位失败的条目（仍留在回收站批次里）。
pub async fn undo_moves(moved: &[(PathBuf, PathBuf)]) -> Vec<String> {
    let mut stranded = Vec::new();
    for (source, target) in moved.iter().rev() {
        if tokio::fs::rename(target, source).await.is_err() {
            stranded.push(source.to_string_lossy().into_owned());
        }
    }
    stranded
}

/// 回位之后仍然留在回收站里的条目说明；全部回位了就是空串。
pub fn stranded_note(stranded: &[String]) -> String {
    if stranded.is_empty() {
        return String::new();
    }
    format!("；以下条目回位失败、仍在回收站里：{}", stranded.join("、"))
}

// ── R60/1679 号：回收站的列出 / 恢复 / 清空 ────────────────────────────────────
//
// 回收站是「把文件找回来」，不是撤销操作。恢复只把文件移回原位，状态与快照不动。
// - 书级：`{projectRoot}/.trash/books/{bookId}--{ts}/`。两腿共用这一个目录（桌面默认
//   产品引擎、Node 是回退，同一个项目根）。本腿只认自己删出来的形态——根上是
//   `book.json`；Node 腿删的书根上是 `work.json` 加 `source/`，由它自己列、自己恢复。
// - 章级：`{bookDir}/chapters/.trash/`。平铺的章文件（重名时带 `-2`、`-3` 后缀）与
//   回滚批次 `rollback-{ts}-{target}/`（书内相对路径原样）。
//
// 恢复的底线：目标已在原位 → [`TrashError::TargetExists`]，一个字节都不覆盖；整批恢复
// 全有或全无。条目识别的纯函数与 Node 腿 `state/trash.ts` 同一条规则
// （`packages/studio/src/__tests__/golden/trash-entry-vectors.json` 两端同读）。

use regex::Regex;
use std::sync::OnceLock;

/// 回收站操作的失败。`code()` 与 HTTP 状态同 Node 腿 `TrashError`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrashError {
    /// 请求不成形：条目名或路径越界、不合布局，字段类型不对。
    Invalid(&'static str),
    /// 条目不在回收站里——包括另一条腿删出来的形态。
    NotFound(String),
    /// 目标已在原位：一个字节都不覆盖。`conflicts` 是挡路的原路径。
    TargetExists {
        message: String,
        conflicts: Vec<String>,
    },
    /// 盘上的操作失败（权限、磁盘、目标被别人先建出来）。不是调用方能改请求解决的。
    Io(String),
}

impl TrashError {
    pub fn code(&self) -> &'static str {
        match self {
            TrashError::Invalid(_) => "INVALID_TRASH_REQUEST",
            TrashError::NotFound(_) => "TRASH_ENTRY_NOT_FOUND",
            TrashError::TargetExists { .. } => "TRASH_TARGET_EXISTS",
            TrashError::Io(_) => "TRASH_IO_ERROR",
        }
    }

    pub fn message(&self) -> String {
        match self {
            TrashError::Invalid(message) => (*message).to_string(),
            TrashError::NotFound(what) => format!("Trash entry not found: {what}"),
            TrashError::TargetExists { message, .. } => message.clone(),
            TrashError::Io(message) => message.clone(),
        }
    }

    /// HTTP 状态。
    pub fn status(&self) -> u16 {
        match self {
            TrashError::Invalid(_) => 400,
            TrashError::NotFound(_) => 404,
            TrashError::TargetExists { .. } => 409,
            TrashError::Io(_) => 500,
        }
    }
}

fn rollback_batch_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^rollback-[0-9]{14}-[0-9]+(?:-[0-9]+)?$").unwrap())
}

fn book_entry_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^(.+)--([0-9]{14})(?:-[0-9]+)?$").unwrap())
}

/// 进回收站时加的重名后缀：`base-2.ext`。
fn collision_suffix_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"-[0-9]+(\.[^./]*)$").unwrap())
}

const CHAPTER_TRASH_PREFIX: &str = "chapters/.trash/";

/// 章级回收站条目 → 它来自哪里。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrashPathInfo {
    pub original_path: String,
    /// 来自哪个回滚批次；平铺的条目是 `None`。
    pub batch: Option<String>,
}

/// 章级回收站条目的原路径。平铺条目去掉重名后缀；批次条目的原路径就是批次目录下那一串。
/// 只认这两种布局，路径里不许有 `..`、空段、反斜杠与 NUL——调用方拿到的原路径一定还是
/// 书目录里的相对路径。
pub fn original_path_of_trash_entry(path: &str) -> Option<TrashPathInfo> {
    let rest = path.strip_prefix(CHAPTER_TRASH_PREFIX)?;
    if path.contains('\\') || path.contains('\0') {
        return None;
    }
    let segments: Vec<&str> = rest.split('/').collect();
    if segments
        .iter()
        .any(|segment| segment.is_empty() || *segment == "." || *segment == "..")
    {
        return None;
    }
    let (first, tail) = segments.split_first()?;
    if rollback_batch_re().is_match(first) {
        return (!tail.is_empty()).then(|| TrashPathInfo {
            original_path: tail.join("/"),
            batch: Some((*first).to_string()),
        });
    }
    if !tail.is_empty() {
        return None;
    }
    Some(TrashPathInfo {
        original_path: format!("chapters/{}", collision_suffix_re().replace(first, "$1")),
        batch: None,
    })
}

/// 公历日期 → 自 1970-01-01 的天数（Howard Hinnant 的算法，`civil_from_days` 的逆）。
fn days_from_civil(year: i64, month: u32, day: i64) -> Option<i64> {
    if !(1..=12).contains(&month) {
        return None;
    }
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month_index = (i64::from(month) + 9) % 12;
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    Some(era * 146097 + day_of_era - 719468)
}

/// `yyyyMMddHHmmss`（UTC）→ ISO。折算回去必须逐位相同——月 13、时 24、2 月 30 这类不存在
/// 的时刻因此被拒（与 Node 腿同一条判定）。
fn compact_to_iso(digits: &str) -> Option<String> {
    if digits.len() != 14 || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let field = |from: usize, len: usize| digits.get(from..from + len)?.parse::<i64>().ok();
    let (year, month, day) = (
        field(0, 4)?,
        u32::try_from(field(4, 2)?).ok()?,
        field(6, 2)?,
    );
    let days = days_from_civil(year, month, day)?;
    let seconds_in_day = field(8, 2)? * 3600 + field(10, 2)? * 60 + field(12, 2)?;
    let millis = days
        .checked_mul(86_400_000)?
        .checked_add(seconds_in_day.checked_mul(1000)?)?;
    let iso = crate::state::chapter_workspace::millis_to_iso(millis)?;
    (compact_timestamp(u64::try_from(millis).ok()?) == digits).then_some(iso)
}

/// 回滚批次目录名里的时间（批次里的文件保留着回滚前的修改时间，删掉的时刻只在名字里）。
pub fn trashed_at_of_rollback_batch(batch: &str) -> Option<String> {
    if !rollback_batch_re().is_match(batch) {
        return None;
    }
    compact_to_iso(batch.get(9..23)?)
}

/// 书级回收站条目名里的时间。
pub fn trashed_at_of_trash_entry(entry: &str) -> Option<String> {
    compact_to_iso(book_entry_re().captures(entry)?.get(2)?.as_str())
}

/// 书级回收站条目名 → 书 id；名字不成形、时刻不存在、书 id 不是安全的路径段 → `None`。
pub fn book_id_of_trash_entry(entry: &str) -> Option<String> {
    if entry.contains('/') || entry.contains('\\') || entry.contains('\0') {
        return None;
    }
    let book_id = book_entry_re().captures(entry)?.get(1)?.as_str();
    (crate::utils::is_safe_book_id(book_id) && trashed_at_of_trash_entry(entry).is_some())
        .then(|| book_id.to_string())
}

async fn sorted_dir_names(dir: &Path) -> io::Result<Vec<String>> {
    let mut entries = match tokio::fs::read_dir(dir).await {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut names = Vec::new();
    while let Some(entry) = entries.next_entry().await? {
        names.push(entry.file_name().to_string_lossy().into_owned());
    }
    names.sort();
    Ok(names)
}

struct TrashFile {
    /// 相对所在目录的 POSIX 路径。
    path: String,
    bytes: u64,
    modified: Option<SystemTime>,
}

/// 目录下的全部普通文件；符号链接与其它类型跳过（不跟着链接走到回收站外面去）。
async fn regular_files(dir: &Path, prefix: &str) -> io::Result<Vec<TrashFile>> {
    let mut files = Vec::new();
    for name in sorted_dir_names(dir).await? {
        let full = dir.join(&name);
        let meta = tokio::fs::symlink_metadata(&full).await?;
        if meta.is_dir() {
            let nested = format!("{prefix}{name}/");
            files.extend(Box::pin(regular_files(&full, &nested)).await?);
        } else if meta.is_file() {
            files.push(TrashFile {
                path: format!("{prefix}{name}"),
                bytes: meta.len(),
                modified: meta.modified().ok(),
            });
        }
    }
    Ok(files)
}

fn iso_of(time: Option<SystemTime>) -> String {
    time.and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|since_epoch| {
            crate::state::chapter_workspace::millis_to_iso(
                i64::try_from(since_epoch.as_millis()).unwrap_or(i64::MAX),
            )
        })
        .unwrap_or(None)
        .unwrap_or_else(|| "1970-01-01T00:00:00.000Z".to_string())
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BookTrashEntry {
    pub entry: String,
    pub book_id: String,
    pub trashed_at: String,
    pub bytes: u64,
    pub file_count: usize,
    /// 书配置上的标题；读不出是 `None`。
    pub title: Option<String>,
}

/// 本腿删出来的形态：一个目录，根上有 `book.json`（不跟符号链接）。
async fn is_own_book_entry(dir: &Path) -> bool {
    tokio::fs::symlink_metadata(dir)
        .await
        .map(|meta| meta.is_dir())
        .unwrap_or(false)
        && tokio::fs::symlink_metadata(dir.join("book.json"))
            .await
            .map(|meta| meta.is_file())
            .unwrap_or(false)
}

async fn entry_title(dir: &Path) -> Option<String> {
    for file in ["book.json", "source/book.json"] {
        let title = tokio::fs::read_to_string(dir.join(file))
            .await
            .ok()
            .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
            .and_then(|value| value.get("title")?.as_str().map(str::to_owned))
            .filter(|title| !title.is_empty());
        if title.is_some() {
            return title;
        }
    }
    None
}

/// 列出本腿的书级回收站，删得晚的在前。目录不存在 → 空。
pub async fn list_book_trash(project_root: &Path) -> io::Result<Vec<BookTrashEntry>> {
    let trash_root = project_root.join(".trash").join("books");
    let mut entries = Vec::new();
    for name in sorted_dir_names(&trash_root).await? {
        let Some(book_id) = book_id_of_trash_entry(&name) else {
            continue;
        };
        let dir = trash_root.join(&name);
        if !is_own_book_entry(&dir).await {
            continue;
        }
        let files = regular_files(&dir, "").await?;
        entries.push(BookTrashEntry {
            entry: name.clone(),
            book_id,
            trashed_at: trashed_at_of_trash_entry(&name).unwrap_or_default(),
            bytes: files.iter().map(|file| file.bytes).sum(),
            file_count: files.len(),
            title: entry_title(&dir).await,
        });
    }
    entries.sort_by(|a, b| {
        b.trashed_at
            .cmp(&a.trashed_at)
            .then_with(|| b.entry.cmp(&a.entry))
    });
    Ok(entries)
}

/// 条目名必须是本腿认得的一个在册条目；否则按「请求不合法」或「找不到」拒绝。
async fn own_book_entry_dir(project_root: &Path, entry: &str) -> Result<PathBuf, TrashError> {
    if book_id_of_trash_entry(entry).is_none() {
        return Err(TrashError::Invalid("Invalid trash entry"));
    }
    let dir = project_root.join(".trash").join("books").join(entry);
    if !is_own_book_entry(&dir).await {
        return Err(TrashError::NotFound(entry.to_string()));
    }
    Ok(dir)
}

/// 把一本删掉的书搬回 `books/<id>`。书目录已被占用（删掉之后又建了一本同 id 的）→
/// [`TrashError::TargetExists`]。返回书 id。
pub async fn restore_book_from_trash(
    project_root: &Path,
    entry: &str,
) -> Result<String, TrashError> {
    let dir = own_book_entry_dir(project_root, entry).await?;
    let book_id = book_id_of_trash_entry(entry).unwrap_or_default();
    let target = project_root.join("books").join(&book_id);
    if tokio::fs::symlink_metadata(&target).await.is_ok() {
        return Err(TrashError::TargetExists {
            message: format!(
                "Book \"{book_id}\" already exists; the trashed copy was left in the trash."
            ),
            conflicts: vec![format!("books/{book_id}")],
        });
    }
    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(io_error)?;
    }
    tokio::fs::rename(&dir, &target).await.map_err(io_error)?;
    Ok(book_id)
}

/// 真空删除一个书级回收站条目。
pub async fn purge_book_trash(project_root: &Path, entry: &str) -> Result<(), TrashError> {
    let dir = own_book_entry_dir(project_root, entry).await?;
    match tokio::fs::remove_dir_all(&dir).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            Err(TrashError::NotFound(entry.to_string()))
        }
        Err(error) => Err(io_error(error)),
    }
}

fn io_error(error: io::Error) -> TrashError {
    TrashError::Io(format!("Trash operation failed: {error}"))
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChapterTrashEntry {
    /// 回收站里的路径（相对书目录）。
    pub path: String,
    pub original_path: String,
    pub batch: Option<String>,
    pub bytes: u64,
    /// 文件自己的修改时间（进回收站是 rename，改不了它）。
    pub modified_at: String,
    /// 删掉的时刻：批次条目取批次名里的时间；平铺的条目没有记录。
    pub trashed_at: Option<String>,
}

/// 列出一本书的章级回收站。回收站不存在 → 空。符号链接与认不出的布局跳过。
pub async fn list_chapter_trash(book_dir: &Path) -> io::Result<Vec<ChapterTrashEntry>> {
    let trash_dir = book_dir.join("chapters").join(".trash");
    let mut entries = Vec::new();
    for name in sorted_dir_names(&trash_dir).await? {
        let Ok(meta) = tokio::fs::symlink_metadata(trash_dir.join(&name)).await else {
            continue;
        };
        if meta.is_dir() {
            let Some(trashed_at) = trashed_at_of_rollback_batch(&name) else {
                continue;
            };
            for file in regular_files(&trash_dir.join(&name), "").await? {
                entries.push(ChapterTrashEntry {
                    path: format!("chapters/.trash/{name}/{}", file.path),
                    original_path: file.path,
                    batch: Some(name.clone()),
                    bytes: file.bytes,
                    modified_at: iso_of(file.modified),
                    trashed_at: Some(trashed_at.clone()),
                });
            }
        } else if meta.is_file() {
            let path = format!("chapters/.trash/{name}");
            let Some(info) = original_path_of_trash_entry(&path) else {
                continue;
            };
            entries.push(ChapterTrashEntry {
                path,
                original_path: info.original_path,
                batch: None,
                bytes: meta.len(),
                modified_at: iso_of(meta.modified().ok()),
                trashed_at: None,
            });
        }
    }
    entries.sort_by(|a, b| {
        let when = |entry: &ChapterTrashEntry| {
            entry
                .trashed_at
                .clone()
                .unwrap_or_else(|| entry.modified_at.clone())
        };
        when(b).cmp(&when(a)).then_with(|| a.path.cmp(&b.path))
    });
    Ok(entries)
}

/// 回到 `chapters/` 下的章文件：它的章号与该登记进目录的条目。
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoredChapter {
    pub number: u32,
    /// 回滚前的目录里存着就带出来；没有的按文件现做。
    pub meta: crate::models::chapter::ChapterMeta,
}

/// 找回的一条：回收站里的路径、原位，以及章文件该带的那条目录条目。
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoredTrashEntry {
    pub path: String,
    pub original_path: String,
    /// 章文件才有；别的文件是 `None`。
    pub chapter: Option<RestoredChapter>,
}

/// 选定、核对过的一条找回：目标不在原位，章条目已备好，还没搬。
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedTrashRestore {
    pub entry: ChapterTrashEntry,
    /// 章文件才有；别的文件是 `None`。
    pub chapter: Option<RestoredChapter>,
}

/// 这批找回里该登记进目录的章条目：一个章号一条，按章号升序。同章双形态一起回到原位时
/// （旧命名的章与重写后的正典章都在同一个回滚批次里）跟随读侧会打开的那一个文件——正典
/// 优先、同类内文件名序小者，与索引重建同一个裁决（`select_chapter_files`）。
/// 路径里的文件名（最后一个 `/` 之后；没有 `/` 就是整串）。择优看的是文件名——正典的判定
/// 锚在串首，传整条路径进去永远判不成正典。
fn file_name_of(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

pub fn chapter_metas_of<'a>(
    planned: &'a [PlannedTrashRestore],
) -> Vec<&'a crate::models::chapter::ChapterMeta> {
    let mut by_number: std::collections::BTreeMap<
        u32,
        (&'a crate::models::chapter::ChapterMeta, &'a str),
    > = std::collections::BTreeMap::new();
    for item in planned {
        let Some(chapter) = &item.chapter else {
            continue;
        };
        let candidate: (&crate::models::chapter::ChapterMeta, &str) =
            (&chapter.meta, item.entry.original_path.as_str());
        let take = match by_number.get(&chapter.number) {
            None => true,
            Some((_, current)) => crate::state::manager::chapter_file_candidate_wins(
                file_name_of(current),
                file_name_of(candidate.1),
            ),
        };
        if take {
            by_number.insert(chapter.number, candidate);
        }
    }
    by_number.into_values().map(|(meta, _)| meta).collect()
}

fn chapter_number_of(original_path: &str) -> Option<u32> {
    let name = original_path.strip_prefix("chapters/")?;
    if name.contains('/') {
        return None;
    }
    crate::state::manager::leading_number(name).filter(|number| *number > 0)
}

/// 回滚批次里存着回滚前的目录：这一章原来的条目（状态、词数都是回滚前的）。
async fn previous_meta(
    book_dir: &Path,
    batch: &str,
    number: u32,
) -> Option<crate::models::chapter::ChapterMeta> {
    let raw = tokio::fs::read_to_string(
        book_dir
            .join("chapters")
            .join(".trash")
            .join(batch)
            .join("chapters")
            .join("index.json"),
    )
    .await
    .ok()?;
    let index: Vec<crate::models::chapter::ChapterMeta> = serde_json::from_str(&raw).ok()?;
    index.into_iter().find(|meta| meta.number == number)
}

/// 找回的章文件该带哪条目录条目：回滚批次里存着回滚前的目录，就从那里取这一章原来的
/// 条目；取不到就按文件现做（与索引重建同口径，时间戳是文件的修改时间）。非章文件 → `None`。
async fn chapter_of(
    book_dir: &Path,
    entry: &ChapterTrashEntry,
) -> Result<Option<RestoredChapter>, TrashError> {
    let Some(number) = chapter_number_of(&entry.original_path) else {
        return Ok(None);
    };
    let previous = match &entry.batch {
        Some(batch) => previous_meta(book_dir, batch, number).await,
        None => None,
    };
    let meta = match previous {
        Some(meta) => Some(meta),
        None => {
            let content = tokio::fs::read_to_string(book_dir.join(&entry.path))
                .await
                .map_err(io_error)?;
            let name = entry
                .original_path
                .rsplit('/')
                .next()
                .unwrap_or(&entry.original_path);
            crate::state::manager::chapter_meta_from_file(name, &content, entry.modified_at.clone())
        }
    };
    Ok(meta.map(|meta| RestoredChapter { number, meta }))
}

/// 文件移走、删掉之后，把回收站里空出来的目录摘掉：自下而上只摘空目录，碰到非空就停；
/// 回收站目录本身留着。
async fn prune_empty_dirs(trash_dir: &Path, vacated: &[PathBuf]) {
    for start in vacated {
        let mut dir = start.clone();
        while dir.starts_with(trash_dir) && dir != trash_dir {
            if tokio::fs::remove_dir(&dir).await.is_err() {
                break;
            }
            match dir.parent() {
                Some(parent) => dir = parent.to_path_buf(),
                None => break,
            }
        }
    }
}

/// 选定要找回的条目并核对：`path` 一个条目，或 `batch` 整个回滚批次。
///
/// 目标已在原位 → [`TrashError::TargetExists`]（一个都不移）。条目的路径必须与列出来的
/// 逐字相同——列出来的才在回收站里。只选定、不搬：调用方先把章条目登记进目录
/// （[`chapter_metas_of`]），再 [`commit_planned_restore`]——目录写不下来就什么都不搬，
/// 回收站里原样留着。
pub async fn plan_chapter_trash_restore(
    book_dir: &Path,
    path: Option<&str>,
    batch: Option<&str>,
) -> Result<Vec<PlannedTrashRestore>, TrashError> {
    let entries = list_chapter_trash(book_dir).await.map_err(io_error)?;
    let picked: Vec<&ChapterTrashEntry> = match (path, batch) {
        (Some(_), Some(_)) | (None, None) => {
            return Err(TrashError::Invalid("Expected { path } or { batch }"));
        }
        (None, Some(batch)) => {
            if !rollback_batch_re().is_match(batch) {
                return Err(TrashError::Invalid("Invalid trash batch"));
            }
            let picked: Vec<_> = entries
                .iter()
                .filter(|entry| entry.batch.as_deref() == Some(batch))
                .collect();
            if picked.is_empty() {
                return Err(TrashError::NotFound(batch.to_string()));
            }
            picked
        }
        (Some(path), None) => {
            if original_path_of_trash_entry(path).is_none() {
                return Err(TrashError::Invalid("Invalid trash path"));
            }
            let picked: Vec<_> = entries.iter().filter(|entry| entry.path == path).collect();
            if picked.is_empty() {
                return Err(TrashError::NotFound(path.to_string()));
            }
            picked
        }
    };
    let mut in_place = Vec::new();
    for entry in &picked {
        if tokio::fs::symlink_metadata(book_dir.join(&entry.original_path))
            .await
            .is_ok()
        {
            in_place.push(entry.original_path.clone());
        }
    }
    if !in_place.is_empty() {
        return Err(TrashError::TargetExists {
            message: format!(
                "Already in place, nothing was restored: {}",
                in_place.join(", ")
            ),
            conflicts: in_place,
        });
    }
    let mut planned = Vec::new();
    for entry in picked {
        let chapter = chapter_of(book_dir, entry).await?;
        planned.push(PlannedTrashRestore {
            entry: entry.clone(),
            chapter,
        });
    }
    Ok(planned)
}

/// 把选定的一批搬回原位，摘掉回收站里空出来的目录。
pub async fn commit_planned_restore(
    book_dir: &Path,
    planned: &[PlannedTrashRestore],
) -> Result<Vec<RestoredTrashEntry>, TrashError> {
    let mut restored = Vec::new();
    let mut vacated = Vec::new();
    for item in planned {
        let source = book_dir.join(&item.entry.path);
        let destination = book_dir.join(&item.entry.original_path);
        if let Some(parent) = destination.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(io_error)?;
        }
        tokio::fs::rename(&source, &destination)
            .await
            .map_err(io_error)?;
        if let Some(parent) = source.parent() {
            vacated.push(parent.to_path_buf());
        }
        restored.push(RestoredTrashEntry {
            path: item.entry.path.clone(),
            original_path: item.entry.original_path.clone(),
            chapter: item.chapter.clone(),
        });
    }
    prune_empty_dirs(&book_dir.join("chapters").join(".trash"), &vacated).await;
    Ok(restored)
}

/// 真空删除章级回收站里的东西：`path` 一个条目、`batch` 一个回滚批次、`all` 全部列出来的
/// 条目（回收站目录本身留着）。返回删掉的条目数。
///
/// 只删列出来的条目：符号链接、认不出的布局不碰——删掉的一定是界面上列过的。全部清空之后
/// 回收站目录里可能剩看不见的残余，不占界面也不占找回的路。
pub async fn purge_chapter_trash(
    book_dir: &Path,
    path: Option<&str>,
    batch: Option<&str>,
    all: bool,
) -> Result<usize, TrashError> {
    if usize::from(path.is_some()) + usize::from(batch.is_some()) + usize::from(all) != 1 {
        return Err(TrashError::Invalid(
            "Expected exactly one of { path }, { batch }, { all: true }",
        ));
    }
    let trash_dir = book_dir.join("chapters").join(".trash");
    let entries = list_chapter_trash(book_dir).await.map_err(io_error)?;
    let picked: Vec<&ChapterTrashEntry> = if all {
        entries.iter().collect()
    } else if let Some(batch) = batch {
        if !rollback_batch_re().is_match(batch) {
            return Err(TrashError::Invalid("Invalid trash batch"));
        }
        let picked: Vec<_> = entries
            .iter()
            .filter(|entry| entry.batch.as_deref() == Some(batch))
            .collect();
        if picked.is_empty() {
            return Err(TrashError::NotFound(batch.to_string()));
        }
        picked
    } else {
        let path = path.unwrap_or_default();
        if original_path_of_trash_entry(path).is_none() {
            return Err(TrashError::Invalid("Invalid trash path"));
        }
        let picked: Vec<&ChapterTrashEntry> =
            entries.iter().filter(|entry| entry.path == path).collect();
        if picked.is_empty() {
            return Err(TrashError::NotFound(path.to_string()));
        }
        picked
    };
    let mut vacated = Vec::new();
    for entry in &picked {
        let source = book_dir.join(&entry.path);
        tokio::fs::remove_file(&source).await.map_err(io_error)?;
        if let Some(parent) = source.parent() {
            vacated.push(parent.to_path_buf());
        }
    }
    prune_empty_dirs(&trash_dir, &vacated).await;
    Ok(picked.len())
}

// ── R60 第三批/1685 号：保留期（到期自动真空删除）────────────────────────────
//
// 只有「有删除时刻记录」的条目参与保留期：
// - 书级条目 `{bookId}--{ts}`：时刻在条目名里；
// - 回滚批次 `rollback-{ts}-{target}`：时刻在批次名里。
// 平铺条目（章删除 / 结构合并放进来的散文件）没有删除时刻记录——文件的 mtime
// 是内容时刻：今天驳回一章，正文可能是四十天前写的；拿 mtime 判保留期会让
// 「刚进回收站的内容」下次启动就被清掉。它们仍走界面上的删除按钮（两步确认）。
//
// 判定式两腿同一条（`packages/studio/src/__tests__/golden/trash-retention-vectors.json`
// 两端同读）：回收时刻（UTC，秒粒度）≤ compact_timestamp(now − days×86400000)
// 即过期——回收时刻本来就是秒粒度，与毫秒比较式精确等价，也免去这边的 ISO 解析。
// 清扫只在服务启动装配点跑（读端点零写，1682 号定线）；清不掉的逐条跳过，
// 清扫永不阻塞启动。

pub const DEFAULT_TRASH_RETENTION_DAYS: u64 = 30;
/// 保留期上限（100 年）：配置写得再大也封到这一档——cutoff 计算保持有界。
pub const MAX_TRASH_RETENTION_DAYS: u64 = 36500;
const DAY_MILLIS: u64 = 86_400_000;

/// 只有正整数认账（超上限封顶）；其余一律缺省 30（与 TS normalizeTrashRetentionDays 同式）。
pub fn normalize_trash_retention_days(value: &serde_json::Value) -> u64 {
    value
        .as_u64()
        .filter(|days| *days > 0)
        .map(|days| days.min(MAX_TRASH_RETENTION_DAYS))
        .unwrap_or(DEFAULT_TRASH_RETENTION_DAYS)
}

/// raw 配置（inkos.json 原样对象）→ 保留天数；缺失 / 形态不对回落缺省。
pub fn trash_retention_days_from_raw(raw: &serde_json::Value) -> u64 {
    normalize_trash_retention_days(
        raw.get("trash")
            .and_then(|section| section.get("retentionDays"))
            .unwrap_or(&serde_json::Value::Null),
    )
}

/// ISO（本模块列表函数的产出，UTC 固定格式）→ `yyyyMMddHHmmss` 秒粒度名时刻。
fn iso_to_compact(iso: &str) -> String {
    iso.chars()
        .filter(|c| c.is_ascii_digit())
        .take(14)
        .collect()
}

/// 已过保留期？恰等于 cutoff = 过期（到点就走；下限 0 与 TS `Math.max(0, …)` 同式）。
pub fn trash_sweep_expired(trashed_at_iso: &str, retention_days: u64, now_millis: u64) -> bool {
    let cutoff =
        compact_timestamp(now_millis.saturating_sub(retention_days.saturating_mul(DAY_MILLIS)));
    iso_to_compact(trashed_at_iso) <= cutoff
}

/// 启动清扫清掉的东西：书级条目数与回滚批次数。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct SweptTrash {
    pub books: usize,
    pub batches: usize,
}

/// 清掉已过保留期的书级条目与回滚批次。只清「列出来的」（与清空端点同一纪律：
/// 本腿认不出的形态、另一腿删的书都不碰）；失败逐条吞掉继续，返回各类清掉的
/// 数目——调用方记日志用。回收站目录本身留着。
pub async fn sweep_expired_trash(
    project_root: &Path,
    book_dirs: &[PathBuf],
    retention_days: u64,
    now_millis: u64,
) -> io::Result<SweptTrash> {
    let mut swept = SweptTrash {
        books: 0,
        batches: 0,
    };
    for entry in list_book_trash(project_root).await? {
        if !trash_sweep_expired(&entry.trashed_at, retention_days, now_millis) {
            continue;
        }
        let dir = project_root.join(".trash").join("books").join(&entry.entry);
        if !tokio::fs::remove_dir_all(&dir).await.is_ok() {
            continue;
        }
        swept.books += 1;
    }
    for book_dir in book_dirs {
        let mut seen = std::collections::HashSet::new();
        for entry in list_chapter_trash(book_dir).await? {
            // 批次条目一定带着批次名的时刻；真缺（不该发生）就跳过——绝不拿 mtime 兜底。
            let Some(batch) = entry.batch.as_deref() else {
                continue;
            };
            let Some(trashed_at) = entry.trashed_at.as_deref() else {
                continue;
            };
            if !seen.insert(batch.to_string())
                || !trash_sweep_expired(trashed_at, retention_days, now_millis)
            {
                continue;
            }
            let dir = book_dir.join("chapters").join(".trash").join(batch);
            if !tokio::fs::remove_dir_all(&dir).await.is_ok() {
                continue;
            }
            swept.batches += 1;
        }
    }
    Ok(swept)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-11-25T09:30:07.123Z 的毫秒时间戳（与 JS `Date.parse` 同值）。
    const T: u64 = 1_795_599_007_123;

    #[test]
    fn names_match_the_node_leg_rule() {
        assert_eq!(compact_timestamp(T), "20261125093007");
        assert_eq!(trash_batch_name(T, 3), "rollback-20261125093007-3");
        assert_eq!(trashed_book_name("b-1", T), "b-1--20261125093007");
    }

    #[tokio::test]
    async fn reserve_dir_appends_a_suffix_when_the_name_is_taken() {
        let dir = tempfile::tempdir().unwrap();
        let parent = dir.path().join("trash");
        assert_eq!(reserve_dir(&parent, "x").await.unwrap(), "x");
        assert_eq!(reserve_dir(&parent, "x").await.unwrap(), "x-2");
        assert_eq!(reserve_dir(&parent, "x").await.unwrap(), "x-3");
        assert_eq!(reserve_dir(&parent, "y").await.unwrap(), "y");
    }

    #[tokio::test]
    async fn moving_into_a_batch_keeps_book_relative_paths_and_undoes_in_reverse() {
        let dir = tempfile::tempdir().unwrap();
        let book = dir.path().join("book");
        let batch = "chapters/.trash/rollback-20261125093007-1";
        std::fs::create_dir_all(book.join("chapters")).unwrap();
        std::fs::create_dir_all(book.join("story").join("snapshots").join("2")).unwrap();
        std::fs::write(
            book.join("story").join("snapshots").join("2").join("a.md"),
            "a",
        )
        .unwrap();
        std::fs::write(book.join("chapters").join("0002_x.md"), "x").unwrap();

        let mut moved = Vec::new();
        move_into_batch(&book, batch, "chapters/0002_x.md", &mut moved)
            .await
            .unwrap();
        move_into_batch(&book, batch, "story/snapshots/2", &mut moved)
            .await
            .unwrap();
        // 不存在的路径跳过，不算已移。
        move_into_batch(&book, batch, "story/pending_hooks.md", &mut moved)
            .await
            .unwrap();
        assert_eq!(moved.len(), 2);
        assert_eq!(
            std::fs::read_to_string(book.join(batch).join("chapters").join("0002_x.md")).unwrap(),
            "x"
        );
        assert!(!book.join("chapters").join("0002_x.md").exists());

        assert!(undo_moves(&moved).await.is_empty());
        assert_eq!(
            std::fs::read_to_string(book.join("chapters").join("0002_x.md")).unwrap(),
            "x"
        );
        assert!(book
            .join("story")
            .join("snapshots")
            .join("2")
            .join("a.md")
            .is_file());
    }

    #[tokio::test]
    async fn undo_reports_items_that_cannot_be_moved_back() {
        let dir = tempfile::tempdir().unwrap();
        let book = dir.path().join("book");
        let batch = "chapters/.trash/rollback-20261125093007-1";
        std::fs::create_dir_all(book.join("chapters")).unwrap();
        std::fs::write(book.join("chapters").join("0002_x.md"), "x").unwrap();
        let mut moved = Vec::new();
        move_into_batch(&book, batch, "chapters/0002_x.md", &mut moved)
            .await
            .unwrap();
        // 原路径上冒出一个非空目录——rename 回不去。
        std::fs::create_dir_all(book.join("chapters").join("0002_x.md").join("blocker")).unwrap();

        let stranded = undo_moves(&moved).await;
        assert_eq!(stranded.len(), 1);
        assert!(stranded[0].ends_with("chapters/0002_x.md"), "{stranded:?}");
        assert!(stranded_note(&stranded).contains("回位失败"));
        assert_eq!(stranded_note(&[]), "");
    }

    // --- R60/1679 号：列出 / 恢复 / 清空 ──────────────────────────────────────

    /// 两端同读的向量表（Node 腿 `trash-api.test.ts` 读同一个文件）：条目识别规则对同一
    /// 输入的判定必须与 Node 腿恒等。
    const VECTORS: &str =
        include_str!("../../../packages/studio/src/__tests__/golden/trash-entry-vectors.json");

    #[test]
    fn entry_recognition_matches_the_shared_vectors() {
        let vectors: serde_json::Value = serde_json::from_str(VECTORS).unwrap();
        for case in vectors["originalPath"]["accept"].as_array().unwrap() {
            let path = case["path"].as_str().unwrap();
            let info =
                original_path_of_trash_entry(path).unwrap_or_else(|| panic!("应认得：{path}"));
            assert_eq!(
                info.original_path,
                case["originalPath"].as_str().unwrap(),
                "{path}"
            );
            assert_eq!(info.batch.as_deref(), case["batch"].as_str(), "{path}");
        }
        for path in vectors["originalPath"]["reject"].as_array().unwrap() {
            let path = path.as_str().unwrap();
            assert!(
                original_path_of_trash_entry(path).is_none(),
                "不该认得：{path:?}"
            );
        }
        for case in vectors["bookEntry"]["accept"].as_array().unwrap() {
            let entry = case["entry"].as_str().unwrap();
            assert_eq!(
                book_id_of_trash_entry(entry).as_deref(),
                Some(case["bookId"].as_str().unwrap()),
                "{entry}"
            );
            assert_eq!(
                trashed_at_of_trash_entry(entry).as_deref(),
                Some(case["trashedAt"].as_str().unwrap()),
                "{entry}"
            );
        }
        for entry in vectors["bookEntry"]["reject"].as_array().unwrap() {
            let entry = entry.as_str().unwrap();
            assert!(book_id_of_trash_entry(entry).is_none(), "{entry:?}");
        }
        assert_eq!(
            trashed_at_of_rollback_batch("rollback-20261125093007-1").as_deref(),
            Some("2026-11-25T09:30:07.000Z")
        );
        assert!(trashed_at_of_rollback_batch("rollback-20261301093007-1").is_none());
    }

    fn write(path: &Path, content: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    fn tree(root: &Path, dir: &Path, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                tree(root, &path, out);
            } else {
                out.push(
                    path.strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/"),
                );
            }
        }
        out.sort();
    }

    fn listing(root: &Path) -> Vec<String> {
        let mut out = Vec::new();
        tree(root, root, &mut out);
        out
    }

    #[tokio::test]
    async fn book_trash_lists_own_shape_newest_first_and_restores_without_overwriting() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let trash = root.join(".trash").join("books");
        write(
            &trash.join("b1--20261125093001").join("book.json"),
            r#"{"id":"b1","title":"被删的书"}"#,
        );
        write(
            &trash
                .join("b1--20261125093001")
                .join("chapters")
                .join("0001.md"),
            "正文",
        );
        // Node 腿删的书：根上是 work.json 加 source/。本腿不认。
        write(
            &trash.join("t1--20261125093002").join("work.json"),
            r#"{"id":"t1"}"#,
        );
        write(
            &trash
                .join("t1--20261125093002")
                .join("source")
                .join("book.json"),
            "{}",
        );
        write(
            &trash.join("b1--20261125093007-3").join("book.json"),
            r#"{"id":"b1","title":"第二次删的"}"#,
        );
        write(&trash.join("notes.txt"), "不是条目");

        let listed = list_book_trash(root).await.unwrap();
        assert_eq!(
            serde_json::to_value(&listed).unwrap(),
            serde_json::json!([
                { "entry": "b1--20261125093007-3", "bookId": "b1", "trashedAt": "2026-11-25T09:30:07.000Z",
                  "bytes": 37, "fileCount": 1, "title": "第二次删的" },
                { "entry": "b1--20261125093001", "bookId": "b1", "trashedAt": "2026-11-25T09:30:01.000Z",
                  "bytes": 40, "fileCount": 2, "title": "被删的书" },
            ])
        );

        // 找回：整本回到 books/<id>。
        assert_eq!(
            restore_book_from_trash(root, "b1--20261125093001")
                .await
                .unwrap(),
            "b1"
        );
        assert!(root
            .join("books")
            .join("b1")
            .join("chapters")
            .join("0001.md")
            .is_file());
        assert!(!trash.join("b1--20261125093001").exists());

        // 书目录已被占用：不覆盖，条目留在回收站里。
        assert_eq!(
            restore_book_from_trash(root, "b1--20261125093007-3").await,
            Err(TrashError::TargetExists {
                message: "Book \"b1\" already exists; the trashed copy was left in the trash."
                    .to_string(),
                conflicts: vec!["books/b1".to_string()],
            })
        );
        assert!(trash
            .join("b1--20261125093007-3")
            .join("book.json")
            .is_file());
        assert_eq!(
            read_to_string(&root.join("books").join("b1").join("book.json")).await,
            r#"{"id":"b1","title":"被删的书"}"#
        );

        // 另一条腿的形态、不存在的条目、不成形的名字。
        assert_eq!(
            restore_book_from_trash(root, "t1--20261125093002").await,
            Err(TrashError::NotFound("t1--20261125093002".to_string()))
        );
        assert_eq!(
            purge_book_trash(root, "t1--20261125093002").await,
            Err(TrashError::NotFound("t1--20261125093002".to_string()))
        );
        assert_eq!(
            restore_book_from_trash(root, "b9--20261125093001").await,
            Err(TrashError::NotFound("b9--20261125093001".to_string()))
        );
        for bad in ["../b1--20261125093001", "notes.txt", "b1--20261301000000"] {
            assert_eq!(
                purge_book_trash(root, bad).await,
                Err(TrashError::Invalid("Invalid trash entry")),
                "{bad}"
            );
        }
        assert!(trash.join("t1--20261125093002").join("work.json").is_file());

        // 真空删除：只删点名的那一个。
        purge_book_trash(root, "b1--20261125093007-3")
            .await
            .unwrap();
        assert!(!trash.join("b1--20261125093007-3").exists());
        assert_eq!(
            purge_book_trash(root, "b1--20261125093007-3").await,
            Err(TrashError::NotFound("b1--20261125093007-3".to_string()))
        );
    }

    const BATCH: &str = "rollback-20261125093007-1";
    /// 回滚前的目录（批次里存着的那份）。
    const PRE_INDEX: &str = r#"[{"number":1,"title":"风起","status":"approved","wordCount":3,"createdAt":"2026-08-17T00:00:00.000Z","updatedAt":"2026-08-17T00:00:00.000Z","auditIssues":[],"lengthWarnings":[]},{"number":3,"title":"收束","status":"approved","wordCount":5,"createdAt":"2026-08-17T00:00:00.000Z","updatedAt":"2026-08-17T00:00:00.000Z","auditIssues":[],"lengthWarnings":[]}]"#;

    /// 一本书：第 1 章在位；回收站里有平铺的两份第 2 章、一个回滚批次（第 3 章 + 快照 + 回滚前的目录）。
    fn seed_chapter_trash(book: &Path) {
        write(
            &book.join("chapters").join("0001_风起.md"),
            "# 第1章 风起\n\n正文。",
        );
        write(
            &book.join("chapters").join("index.json"),
            r#"[{"number":1,"title":"风起","status":"approved","wordCount":3,"createdAt":"2026-08-17T00:00:00.000Z","updatedAt":"2026-08-17T00:00:00.000Z","auditIssues":[],"lengthWarnings":[]}]"#,
        );
        write(
            &book.join("chapters").join(".trash").join("0002_云涌.md"),
            "# 第2章 云涌\n\n第一份。",
        );
        write(
            &book.join("chapters").join(".trash").join("0002_云涌-2.md"),
            "# 第2章 云涌\n\n第二份。",
        );
        write(
            &book
                .join("chapters")
                .join(".trash")
                .join(BATCH)
                .join("chapters")
                .join("0003.md"),
            "# 第3章 收束\n\n正文三。",
        );
        write(
            &book
                .join("chapters")
                .join(".trash")
                .join(BATCH)
                .join("chapters")
                .join("index.json"),
            PRE_INDEX,
        );
        write(
            &book
                .join("chapters")
                .join(".trash")
                .join(BATCH)
                .join("story")
                .join("snapshots")
                .join("2")
                .join("current_state.md"),
            "state-2",
        );
        // 回滚之后原位还在的当前状态。
        write(&book.join("story").join("current_state.md"), "state-1");
    }

    async fn read_to_string(path: &Path) -> String {
        tokio::fs::read_to_string(path).await.unwrap()
    }

    /// 不登记目录的整步找回（目录登记另测）：选定后直接搬。
    async fn restore(
        book: &Path,
        path: Option<&str>,
        batch: Option<&str>,
    ) -> Result<Vec<RestoredTrashEntry>, TrashError> {
        let planned = plan_chapter_trash_restore(book, path, batch).await?;
        commit_planned_restore(book, &planned).await
    }

    #[tokio::test]
    async fn chapter_trash_lists_flat_and_batch_entries() {
        let dir = tempfile::tempdir().unwrap();
        let book = dir.path().join("book");
        seed_chapter_trash(&book);
        let listed = list_chapter_trash(&book).await.unwrap();
        let shape: Vec<_> = listed
            .iter()
            .map(|entry| {
                (
                    entry.path.clone(),
                    entry.original_path.clone(),
                    entry.batch.clone(),
                    entry.bytes,
                    entry.trashed_at.clone(),
                )
            })
            .collect();
        // 平铺的条目刚进回收站（按文件修改时间排在前面），批次里的按批次时间、批次内按路径。
        assert_eq!(
            shape,
            vec![
                (
                    "chapters/.trash/0002_云涌-2.md".to_string(),
                    "chapters/0002_云涌.md".to_string(),
                    None,
                    30,
                    None
                ),
                (
                    "chapters/.trash/0002_云涌.md".to_string(),
                    "chapters/0002_云涌.md".to_string(),
                    None,
                    30,
                    None
                ),
                (
                    format!("chapters/.trash/{BATCH}/chapters/0003.md"),
                    "chapters/0003.md".to_string(),
                    Some(BATCH.to_string()),
                    30,
                    Some("2026-11-25T09:30:07.000Z".to_string())
                ),
                (
                    format!("chapters/.trash/{BATCH}/chapters/index.json"),
                    "chapters/index.json".to_string(),
                    Some(BATCH.to_string()),
                    PRE_INDEX.len() as u64,
                    Some("2026-11-25T09:30:07.000Z".to_string())
                ),
                (
                    format!("chapters/.trash/{BATCH}/story/snapshots/2/current_state.md"),
                    "story/snapshots/2/current_state.md".to_string(),
                    Some(BATCH.to_string()),
                    7,
                    Some("2026-11-25T09:30:07.000Z".to_string())
                ),
            ]
        );
        assert!(listed.iter().all(|entry| entry.modified_at.ends_with('Z')));
    }

    #[tokio::test]
    async fn chapter_restore_brings_files_back_and_never_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let book = dir.path().join("book");
        seed_chapter_trash(&book);
        let trash = book.join("chapters").join(".trash");

        // 平铺条目：回到去掉重名后缀的原名。
        let restored = restore(&book, Some("chapters/.trash/0002_云涌-2.md"), None)
            .await
            .unwrap();
        assert_eq!(restored.len(), 1);
        assert_eq!(restored[0].original_path, "chapters/0002_云涌.md");
        // 平铺的条目没有回滚前的目录可取：条目按文件现做（词数 = 去空白后的 UTF-16 码元数）。
        let chapter = restored[0].chapter.as_ref().unwrap();
        assert_eq!((chapter.number, chapter.meta.title.as_str()), (2, "云涌"));
        assert_eq!(
            (
                chapter.meta.word_count,
                serde_json::to_value(chapter.meta.status).unwrap()
            ),
            (10, serde_json::json!("ready-for-review"))
        );
        assert_eq!(
            read_to_string(&book.join("chapters").join("0002_云涌.md")).await,
            "# 第2章 云涌\n\n第二份。"
        );
        assert!(!trash.join("0002_云涌-2.md").exists());

        // 批次里的章文件带着回滚前的目录条目回来。
        let restored = restore(
            &book,
            Some(&format!("chapters/.trash/{BATCH}/chapters/0003.md")),
            None,
        )
        .await
        .unwrap();
        let chapter = restored[0].chapter.as_ref().unwrap();
        assert_eq!(chapter.number, 3);
        assert_eq!(chapter.meta.title, "收束");
        // 非章文件没有条目可带。
        let restored = restore(
            &book,
            Some(&format!(
                "chapters/.trash/{BATCH}/story/snapshots/2/current_state.md"
            )),
            None,
        )
        .await
        .unwrap();
        assert!(restored[0].chapter.is_none());
        assert_eq!(
            read_to_string(
                &book
                    .join("story")
                    .join("snapshots")
                    .join("2")
                    .join("current_state.md")
            )
            .await,
            "state-2"
        );

        // 目标已在原位：一个都不移。
        assert_eq!(
            restore(
                &book,
                Some(&format!("chapters/.trash/{BATCH}/chapters/index.json")),
                None
            )
            .await,
            Err(TrashError::TargetExists {
                message: "Already in place, nothing was restored: chapters/index.json".to_string(),
                conflicts: vec!["chapters/index.json".to_string()],
            })
        );
        assert!(trash
            .join(BATCH)
            .join("chapters")
            .join("index.json")
            .is_file());

        // 不是条目的路径、不存在的条目。
        for bad in [
            "chapters/0001_风起.md",
            "chapters/.trash/../0001_风起.md",
            &format!("chapters/.trash/{BATCH}"),
        ] {
            assert_eq!(
                restore(&book, Some(bad), None).await,
                Err(TrashError::Invalid("Invalid trash path")),
                "{bad}"
            );
        }
        assert_eq!(
            restore(&book, Some("chapters/.trash/0009.md"), None).await,
            Err(TrashError::NotFound("chapters/.trash/0009.md".to_string()))
        );
        assert_eq!(
            restore(&book, None, Some("rollback-20261125093008-9")).await,
            Err(TrashError::NotFound(
                "rollback-20261125093008-9".to_string()
            ))
        );
        assert_eq!(
            restore(&book, None, Some("../")).await,
            Err(TrashError::Invalid("Invalid trash batch"))
        );
        assert_eq!(
            restore(&book, None, None).await,
            Err(TrashError::Invalid("Expected { path } or { batch }"))
        );

        // 一个没有冲突的批次：全部回去，空出来的批次目录不留空壳。
        let free = "rollback-20261125093008-2";
        write(
            &trash
                .join(free)
                .join("story")
                .join("drafts")
                .join("0004.md"),
            "草稿",
        );
        write(
            &trash
                .join(free)
                .join("story")
                .join("drafts")
                .join("0005.md"),
            "草稿五",
        );
        // 原位已有的文件挡住整批：一个都不动。
        write(
            &book.join("story").join("drafts").join("0005.md"),
            "原位的五",
        );
        assert_eq!(
            restore(&book, None, Some(free)).await,
            Err(TrashError::TargetExists {
                message: "Already in place, nothing was restored: story/drafts/0005.md".to_string(),
                conflicts: vec!["story/drafts/0005.md".to_string()],
            })
        );
        assert_eq!(
            read_to_string(
                &trash
                    .join(free)
                    .join("story")
                    .join("drafts")
                    .join("0004.md")
            )
            .await,
            "草稿"
        );
        std::fs::remove_file(book.join("story").join("drafts").join("0005.md")).unwrap();

        let restored = restore(&book, None, Some(free)).await.unwrap();
        let mut paths: Vec<&str> = restored
            .iter()
            .map(|entry| entry.original_path.as_str())
            .collect();
        paths.sort();
        assert_eq!(paths, ["story/drafts/0004.md", "story/drafts/0005.md"]);
        assert!(!trash.join(free).exists());
        assert!(book.join("story").join("drafts").join("0004.md").is_file());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn chapter_trash_ignores_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let book = dir.path().join("book");
        seed_chapter_trash(&book);
        let outside = dir.path().join("outside.md");
        std::fs::write(&outside, "外面的文件").unwrap();
        std::os::unix::fs::symlink(
            &outside,
            book.join("chapters").join(".trash").join("0007_链接.md"),
        )
        .unwrap();
        assert!(list_chapter_trash(&book)
            .await
            .unwrap()
            .iter()
            .all(|entry| !entry.path.contains("0007_链接")));
        assert_eq!(
            restore(&book, Some("chapters/.trash/0007_链接.md"), None).await,
            Err(TrashError::NotFound(
                "chapters/.trash/0007_链接.md".to_string()
            ))
        );
        purge_chapter_trash(&book, Some("chapters/.trash/0007_链接.md"), None, false)
            .await
            .unwrap_err();
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), "外面的文件");
    }

    #[tokio::test]
    async fn chapter_purge_removes_one_entry_one_batch_or_all() {
        let dir = tempfile::tempdir().unwrap();
        let book = dir.path().join("book");
        seed_chapter_trash(&book);
        let trash = book.join("chapters").join(".trash");

        assert_eq!(
            purge_chapter_trash(&book, Some("chapters/.trash/0002_云涌.md"), None, false)
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            purge_chapter_trash(&book, Some("chapters/.trash/0002_云涌.md"), None, false).await,
            Err(TrashError::NotFound(
                "chapters/.trash/0002_云涌.md".to_string()
            ))
        );
        // 批次里的单个文件：删掉后空出来的目录摘掉。
        assert_eq!(
            purge_chapter_trash(
                &book,
                Some(&format!(
                    "chapters/.trash/{BATCH}/story/snapshots/2/current_state.md"
                )),
                None,
                false
            )
            .await
            .unwrap(),
            1
        );
        assert!(!trash.join(BATCH).join("story").exists());
        assert_eq!(
            purge_chapter_trash(&book, None, Some(BATCH), false)
                .await
                .unwrap(),
            2
        );
        assert!(!trash.join(BATCH).exists());

        for (path, batch, all) in [
            (Some("chapters/0001_风起.md"), None, false),
            (None, Some(".."), false),
            (Some("x"), Some("y"), false),
            (None, None, false),
            (Some("x"), None, true),
        ] {
            let refusal = purge_chapter_trash(&book, path, batch, all)
                .await
                .unwrap_err();
            assert!(
                matches!(refusal, TrashError::Invalid(_)),
                "{path:?} {batch:?} {all} → {refusal:?}"
            );
            assert_eq!(refusal.code(), "INVALID_TRASH_REQUEST");
        }
        assert_eq!(
            purge_chapter_trash(&book, None, None, true).await.unwrap(),
            1
        );
        assert!(listing(&trash).is_empty());
        assert!(
            std::fs::read_to_string(book.join("chapters").join("0001_风起.md"))
                .unwrap()
                .contains("正文")
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn chapter_purge_removes_only_what_it_listed() {
        let dir = tempfile::tempdir().unwrap();
        let book = dir.path().join("book");
        seed_chapter_trash(&book);
        let trash = book.join("chapters").join(".trash");
        let outside = dir.path().join("outside.md");
        write(&outside, "外面的文件");
        write(&trash.join("rollback-x").join("keep.md"), "认不出的布局");
        std::os::unix::fs::symlink(&outside, trash.join(BATCH).join("link.md")).unwrap();
        // 按批次清空同理：只删批次里列出来的，批次里的符号链接不碰。
        let half = "rollback-20261125093009-3";
        write(&trash.join(half).join("chapters").join("0004.md"), "草稿");
        std::os::unix::fs::symlink(&outside, trash.join(half).join("link.md")).unwrap();
        assert_eq!(
            purge_chapter_trash(&book, None, Some(half), false)
                .await
                .unwrap(),
            1
        );
        assert!(trash.join(half).join("link.md").symlink_metadata().is_ok());

        // 两条平铺 + 批次里的三个条目；符号链接与 `rollback-x` 从没被列出来过。
        assert_eq!(
            purge_chapter_trash(&book, None, None, true).await.unwrap(),
            5
        );
        let mut left = listing(&trash);
        left.retain(|path| !path.starts_with(half));
        assert_eq!(
            left,
            vec![format!("{BATCH}/link.md"), "rollback-x/keep.md".to_string()]
        );
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), "外面的文件");
        assert!(list_chapter_trash(&book).await.unwrap().is_empty());
        assert_eq!(
            purge_chapter_trash(&book, None, None, true).await.unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn chapter_metas_follow_the_file_the_reader_would_open_when_both_forms_come_back() {
        let dir = tempfile::tempdir().unwrap();
        let book = dir.path().join("book");
        let batch = "rollback-20261125093009-1";
        write(&book.join("chapters").join("index.json"), "[]");
        // 旧命名的章与重写后的正典章在同一个回滚批次里（批次里没存回滚前的目录）。
        let inside = book
            .join("chapters")
            .join(".trash")
            .join(batch)
            .join("chapters");
        write(&inside.join("0002-旧名.md"), "# 第2章 旧名\n\n旧稿。");
        write(&inside.join("0002_新名.md"), "# 第2章 新名\n\n新稿。");
        let planned = plan_chapter_trash_restore(&book, None, Some(batch))
            .await
            .unwrap();
        assert_eq!(planned.len(), 2);
        // 一个章号只出一条，跟随读侧会打开的正典文件；条目按文件现做。
        let titles: Vec<&str> = chapter_metas_of(&planned)
            .iter()
            .map(|meta| meta.title.as_str())
            .collect();
        assert_eq!(titles, ["新名"]);
        // 结论与输入顺序无关。
        let mut reordered = planned.clone();
        reordered.swap(0, 1);
        assert_eq!(chapter_metas_of(&reordered)[0].title, "新名");
        // 非章文件不出条目。
        write(
            &book
                .join("chapters")
                .join(".trash")
                .join(batch)
                .join("story")
                .join("drafts")
                .join("0009.md"),
            "草稿",
        );
        let planned = plan_chapter_trash_restore(&book, None, Some(batch))
            .await
            .unwrap();
        assert_eq!(
            planned.iter().filter(|item| item.chapter.is_none()).count(),
            1
        );
        assert_eq!(chapter_metas_of(&planned).len(), 1);
    }

    #[tokio::test]
    async fn planning_moves_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let book = dir.path().join("book");
        seed_chapter_trash(&book);
        let trash = book.join("chapters").join(".trash");
        let before = listing(&trash);
        let planned = plan_chapter_trash_restore(&book, None, Some(BATCH))
            .await
            .unwrap_err();
        assert!(matches!(planned, TrashError::TargetExists { .. }));
        let planned =
            plan_chapter_trash_restore(&book, Some("chapters/.trash/0002_云涌-2.md"), None)
                .await
                .unwrap();
        assert_eq!(planned.len(), 1);
        // 选定之后盘上没有任何变化：目录写不下来就停在这里。
        assert_eq!(listing(&trash), before);
        assert!(!book.join("chapters").join("0002_云涌.md").exists());
        let restored = commit_planned_restore(&book, &planned).await.unwrap();
        assert_eq!(restored[0].original_path, "chapters/0002_云涌.md");
        assert_eq!(
            read_to_string(&book.join("chapters").join("0002_云涌.md")).await,
            "# 第2章 云涌\n\n第二份。"
        );
    }

    // ── R60 第三批/1685 号：保留期 ────────────────────────────────

    fn retention_vectors() -> serde_json::Value {
        serde_json::from_str(include_str!(
            "../../../packages/studio/src/__tests__/golden/trash-retention-vectors.json"
        ))
        .unwrap()
    }

    #[test]
    fn retention_vectors_match_the_shared_table() {
        let vectors = retention_vectors();
        assert_eq!(
            vectors["defaultDays"].as_u64().unwrap(),
            DEFAULT_TRASH_RETENTION_DAYS
        );
        assert_eq!(
            vectors["maxDays"].as_u64().unwrap(),
            MAX_TRASH_RETENTION_DAYS
        );
        for vector in vectors["normalization"].as_array().unwrap() {
            assert_eq!(
                normalize_trash_retention_days(&vector["value"]),
                vector["days"].as_u64().unwrap(),
                "normalization {:?}",
                vector["value"]
            );
        }
        for vector in vectors["expiry"].as_array().unwrap() {
            assert_eq!(
                trash_sweep_expired(
                    vector["trashedAt"].as_str().unwrap(),
                    vector["days"].as_u64().unwrap(),
                    vector["nowMillis"].as_u64().unwrap(),
                ),
                vector["expired"].as_bool().unwrap(),
                "expiry {}",
                vector["name"].as_str().unwrap()
            );
        }
    }

    #[test]
    fn retention_days_from_raw_falls_back_to_the_default() {
        assert_eq!(
            trash_retention_days_from_raw(&serde_json::json!({"trash": {"retentionDays": 45}})),
            45
        );
        assert_eq!(
            trash_retention_days_from_raw(&serde_json::json!({})),
            DEFAULT_TRASH_RETENTION_DAYS
        );
        assert_eq!(
            trash_retention_days_from_raw(&serde_json::json!({"trash": 5})),
            DEFAULT_TRASH_RETENTION_DAYS
        );
        assert_eq!(
            trash_retention_days_from_raw(&serde_json::json!({"trash": {"retentionDays": "45"}})),
            DEFAULT_TRASH_RETENTION_DAYS
        );
    }

    /// 本腿删出来的书级条目（根上有 `book.json`）。
    fn seed_book_trash_entry(root: &Path, entry: &str) {
        std::fs::create_dir_all(root.join(".trash").join("books").join(entry)).unwrap();
        std::fs::write(
            root.join(".trash")
                .join("books")
                .join(entry)
                .join("book.json"),
            format!(r#"{{"id":"{}"}}"#, entry.split("--").next().unwrap()),
        )
        .unwrap();
    }

    fn seed_book_trash(root: &Path, id: &str, batch: Option<&str>, flat: Option<&str>) -> PathBuf {
        let book = root.join("works").join(id).join("source");
        std::fs::create_dir_all(book.join("chapters").join(".trash")).unwrap();
        if let Some(batch) = batch {
            let inner = book
                .join("chapters")
                .join(".trash")
                .join(batch)
                .join("chapters");
            std::fs::create_dir_all(&inner).unwrap();
            std::fs::write(inner.join("0003_回.md"), "旧正文").unwrap();
        }
        if let Some(flat) = flat {
            std::fs::write(
                book.join("chapters").join(".trash").join(flat),
                "更旧的正文",
            )
            .unwrap();
        }
        book
    }

    const NOW: u64 = 1_795_651_200_000; // 2026-11-26T00:00:00Z，cutoff = 2026-10-27

    #[tokio::test]
    async fn sweep_expires_book_entries_and_batches_but_never_flat_entries() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        seed_book_trash_entry(root, "gone--20260901000000"); // 过期
        seed_book_trash_entry(root, "kept--20261125000000"); // 未过期
        std::fs::create_dir_all(
            root.join(".trash")
                .join("books")
                .join("weird-form--20260101000000"),
        )
        .unwrap(); // 认不出（无 book.json）
        let b1 = seed_book_trash(root, "b1", Some("rollback-20260901000000-3"), None); // 过期批次
        let b2 = seed_book_trash(
            root,
            "b2",
            Some("rollback-20261125000000-3"),
            Some("0009_旧形态.md"),
        );
        // 平铺条目的 mtime 推到 1970——保留期不得拿内容时刻判它（删除时刻无记录）。
        let flat = b2.join("chapters").join(".trash").join("0009_旧形态.md");
        let handle = std::fs::File::options().write(true).open(&flat).unwrap();
        handle.set_modified(std::time::UNIX_EPOCH).unwrap();

        let swept = sweep_expired_trash(root, &[b1.clone(), b2.clone()], 30, NOW)
            .await
            .unwrap();
        assert_eq!(
            swept,
            SweptTrash {
                books: 1,
                batches: 1
            }
        );

        let books = list_book_trash(root).await.unwrap();
        let names: Vec<_> = books.iter().map(|entry| entry.entry.as_str()).collect();
        assert_eq!(names, ["kept--20261125000000"]);
        // 认不出的形态不进列表也就不清扫——它必须原样留在磁盘上。
        assert!(root
            .join(".trash")
            .join("books")
            .join("weird-form--20260101000000")
            .is_dir());
        assert!(list_chapter_trash(&b1).await.unwrap().is_empty()); // 过期批次整批清走
        assert_eq!(list_chapter_trash(&b2).await.unwrap().len(), 2); // 未过期批次 + 平铺都在
    }

    #[tokio::test]
    async fn sweep_on_missing_trash_dirs_is_zero_and_silent() {
        let dir = tempfile::tempdir().unwrap();
        let swept = sweep_expired_trash(
            dir.path(),
            &[dir.path().join("works/ghost/source")],
            30,
            NOW,
        )
        .await
        .unwrap();
        assert_eq!(
            swept,
            SweptTrash {
                books: 0,
                batches: 0
            }
        );
    }

    #[tokio::test]
    async fn a_swept_batch_leaves_no_empty_shell_behind() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let book = seed_book_trash(root, "b1", Some("rollback-20260601000000-2"), None);
        sweep_expired_trash(root, std::slice::from_ref(&book), 30, NOW)
            .await
            .unwrap();
        let trash = book.join("chapters").join(".trash");
        let names: Vec<_> = sorted_dir_names(&trash).await.unwrap();
        assert!(names.is_empty());
    }

    #[tokio::test]
    async fn a_one_day_window_sweeps_by_exact_seconds() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        seed_book_trash_entry(root, "gone--20261125090000"); // 昨天 09:00
        seed_book_trash_entry(root, "kept--20261125120000"); // 昨天 12:00
        let now = 1_795_687_200_000; // 2026-11-26T10:00Z，days=1 的 cutoff = 昨天 10:00
        let swept = sweep_expired_trash(root, &[], 1, now).await.unwrap();
        assert_eq!(
            swept,
            SweptTrash {
                books: 1,
                batches: 0
            }
        );
        let books = list_book_trash(root).await.unwrap();
        let names: Vec<_> = books.iter().map(|entry| entry.entry.as_str()).collect();
        assert_eq!(names, ["kept--20261125120000"]);
    }
}
