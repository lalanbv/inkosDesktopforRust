//! 章节结构操作事务（R48a，1641 号）。
//!
//! 三事务（拆分/合并/重排）+ 章集合 revision 乐观并发，纯函数面镜像
//! `packages/core/src/state/chapter-structure.ts`（golden
//! chapter-structure-vectors.json 锁双端：重编号/拆分边界/合并分隔/revision hash）。
//!
//! 事务原子性（staging 两相）：受影响章文件 + runtime/drafts 产物先整体
//! rename 入 `chapters/.tmp-structure/`（失败即中断零落盘），随后按新号
//! 回名 + 前段/新章原子写 + 索引原子替换（最后落盘）——任一步失败从
//! staging 全量还原。快照（story/snapshots/）与版本归档（.versions/）是
//! 冻结历史**不随事务迁移**（entity-rename 先例同语义）；
//! runtime/chapter-NNNN.* 与 drafts 前导号产物属当前章工作面**随号迁移**。
//! 被删章（合并）正文进 `.trash/`（chapter_delete 惯例，永不硬删）。
//!
//! 乐观并发：`chapter_set_revision` = 章集合指纹（sha256 前 16 hex，
//! 双端 golden 锁定）；写请求携带 `expectedRevision` 不符 → 409
//! `CHAPTER_SET_CONFLICT`，缺省跳过校验（逐步采纳面）。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::models::chapter::{ChapterMeta, ChapterStatus};
use crate::state::chapter_delete::pick_available_name;
use crate::state::chapter_workspace::{archive_chapter_version, ChapterVersionSource};
use crate::state::manager::StateManager;
use crate::state::store::FsStateStore;
use crate::utils::atomic_file_set::write_file_atomic;

/// staging 目录前缀（R48a 固定名 `.tmp-structure` 在 Rust 无锁面下并发
/// 双结构操作互撞——1646 号审查修复改 per-op UUID 后缀，restore 参数化）。
const STAGING_PREFIX: &str = ".tmp-structure-";
const TRASH_DIR: &str = ".trash";
const REVISION_CONFLICT_MESSAGE: &str = "Chapter set changed since last read (revision conflict).";
pub const REVISION_CONFLICT_CODE: &str = "CHAPTER_SET_CONFLICT";

/// 结构事务错误（status 供端点层映射 HTTP 码）。
#[derive(Debug, Clone)]
pub struct StructureError {
    pub status: u16,
    pub message: String,
    pub code: Option<&'static str>,
}

impl StructureError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: 400,
            message: message.into(),
            code: None,
        }
    }
    fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: 404,
            message: message.into(),
            code: None,
        }
    }
    fn conflict() -> Self {
        Self {
            status: 409,
            message: REVISION_CONFLICT_MESSAGE.to_string(),
            code: Some(REVISION_CONFLICT_CODE),
        }
    }
}

/// 重编号条目（from→to，实际发生移位的章）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct RenumberedEntry {
    pub from: u32,
    pub to: u32,
}

/// 结构事务结果（camelCase 序列化，TS 镜像逐键对齐）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StructureOutcome {
    pub op: &'static str,
    pub book_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub split_from: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_chapter: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub merged_into: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub removed_chapter: Option<u32>,
    pub renumbered: Vec<RenumberedEntry>,
    pub revision: String,
    pub touched_files: Vec<String>,
    pub summary: String,
}

// ── 纯函数面（golden 锁双端）─────────────────────────────────────

/// 章集合指纹：`[[number, wordCount, updatedAt], ...]`（number 升序）的
/// JSON 串 sha256 前 16 hex。空索引 → sha256("[]")。双端逐位一致。
pub fn chapter_set_revision(index: &[ChapterMeta]) -> String {
    let mut numbers: Vec<u32> = index.iter().map(|meta| meta.number).collect();
    numbers.sort_unstable();
    let mut parts: Vec<serde_json::Value> = Vec::with_capacity(numbers.len());
    for number in numbers {
        if let Some(meta) = index.iter().find(|meta| meta.number == number) {
            parts.push(serde_json::json!([
                meta.number,
                meta.word_count,
                meta.updated_at,
            ]));
        }
    }
    let payload = serde_json::to_string(&parts).unwrap_or_else(|_| "[]".to_string());
    let digest = Sha256::digest(payload.as_bytes());
    format!("{:x}", digest)[..16].to_string()
}

/// 拆分重编号：`at` 之后全部 +1（新章占 at+1）。返回移位对（升序）。
pub fn renumber_after_split(numbers: &[u32], at: u32) -> Vec<(u32, u32)> {
    numbers
        .iter()
        .filter(|&&n| n > at)
        .map(|&n| (n, n + 1))
        .collect()
}

/// 合并重编号：`removed` 之后全部 -1。返回移位对（升序）。
pub fn renumber_after_merge(numbers: &[u32], removed: u32) -> Vec<(u32, u32)> {
    numbers
        .iter()
        .filter(|&&n| n > removed)
        .map(|&n| (n, n - 1))
        .collect()
}

/// 重排映射：`order`（新序的章号排列）须为当前章号集合的双射。
/// 返回实际移位对（from→to，旧号→新号=order 下标+1；恒位章不出现）。
pub fn reorder_plan(numbers: &[u32], order: &[u32]) -> Result<Vec<(u32, u32)>, StructureError> {
    let current: std::collections::BTreeSet<u32> = numbers.iter().copied().collect();
    let requested: std::collections::BTreeSet<u32> = order.iter().copied().collect();
    if order.len() != current.len() || requested != current {
        return Err(StructureError::bad_request(
            "Reorder payload must be a permutation of the current chapter numbers.",
        ));
    }
    let mut moved: Vec<(u32, u32)> = order
        .iter()
        .enumerate()
        .map(|(index, &old)| (old, (index + 1) as u32))
        .filter(|&(old, new)| old != new)
        .collect();
    moved.sort_by_key(|&(from, _)| from);
    Ok(moved)
}

/// 拆分切点（UTF-16 code unit 偏移，对齐前端光标语义）：
/// 返回 (前段, 后段)；offset 落在代理对中间 → 非法。
pub fn split_utf16(content: &str, offset: usize) -> Result<(String, String), StructureError> {
    let total = content.encode_utf16().count();
    if offset == 0 || offset >= total {
        return Err(StructureError::bad_request(
            "Split offset must be inside the chapter content.",
        ));
    }
    // 落点必须在 char 边界（代理对中间=非法）。
    let mut unit = 0usize;
    for (byte_index, ch) in content.char_indices() {
        if unit == offset {
            let (head, tail) = content.split_at(byte_index);
            return Ok((head.to_string(), tail.to_string()));
        }
        if unit > offset {
            return Err(StructureError::bad_request(
                "Split offset falls inside a surrogate pair.",
            ));
        }
        unit += ch.len_utf16();
    }
    Err(StructureError::bad_request(
        "Split offset falls inside a surrogate pair.",
    ))
}

/// 合并拼接：`a + separator + b`（separator 可为空串）。
pub fn merge_content(a: &str, b: &str, separator: &str) -> String {
    format!("{a}{separator}{b}")
}

// ── 共享事务骨架 ─────────────────────────────────────────────────

struct ChapterFile {
    meta: ChapterMeta,
    /// 当前盘上文件名（数值等值定位的胜者，可能为 legacy 连字符形态）。
    file_name: String,
    path: PathBuf,
}

/// 收集受影响章的盘上文件（数值等值定位 + 正典择优）。
async fn collect_chapter_files(
    chapters_dir: &Path,
    numbers: &[u32],
    index: &[ChapterMeta],
) -> Result<BTreeMap<u32, ChapterFile>, StructureError> {
    let mut files: Vec<String> = Vec::new();
    let mut entries = tokio::fs::read_dir(chapters_dir)
        .await
        .map_err(|e| StructureError::bad_request(format!("chapters dir unreadable: {e}")))?;
    while let Ok(Some(entry)) = entries.next_entry().await {
        files.push(entry.file_name().to_string_lossy().into_owned());
    }
    let mut map = BTreeMap::new();
    for &number in numbers {
        let Some(meta) = index.iter().find(|meta| meta.number == number) else {
            return Err(StructureError::not_found(format!(
                "Chapter {number} not found"
            )));
        };
        let file_name = crate::state::chapter_filename::find_chapter_file_by_number(&files, number)
            .ok_or_else(|| StructureError::not_found(format!("Chapter {number} file not found")))?;
        map.insert(
            number,
            ChapterFile {
                meta: meta.clone(),
                file_name: file_name.clone(),
                path: chapters_dir.join(&file_name),
            },
        );
    }
    Ok(map)
}

/// 受影响章的 runtime/drafts 产物清单（chapter-NNNN.* 前缀 / NNNN_*.md 前导号）。
async fn collect_artifact_files(root: &Path, numbers: &[u32]) -> Vec<(PathBuf, String)> {
    let mut out = Vec::new();
    let runtime_dir = root.join("story").join("runtime");
    let drafts_dir = root.join("story").join("drafts");
    for (dir, prefix_of) in [(&runtime_dir, true), (&drafts_dir, false)] {
        let Ok(mut entries) = tokio::fs::read_dir(dir).await else {
            continue;
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            let name = entry.file_name().to_string_lossy().into_owned();
            let hit = numbers.iter().any(|&n| {
                if prefix_of {
                    name.starts_with(&format!("chapter-{n:04}."))
                } else {
                    crate::state::chapter_filename::parse_chapter_file_number(&name) == Some(n)
                }
            });
            if hit {
                out.push((entry.path(), name));
            }
        }
    }
    out
}

/// staging 两相第一步：受影响章文件 + 产物整体入 staging（原名存放）。
/// 任一 rename 失败 → 立即回滚已入 staging 部分（原名还原）并返回 Err。
async fn stage_affected(
    chapters_dir: &Path,
    root: &Path,
    chapter_files: &BTreeMap<u32, ChapterFile>,
    artifacts: &[(PathBuf, String)],
) -> Result<PathBuf, StructureError> {
    // 1646 号审查修复：per-op 唯一名——Rust 侧结构操作无跨进程锁（manager
    // 暂缓件备案），固定名在并发双操作下互撞（第二个 stage 前置清场会破坏
    // 第一个的已入 staging 面）。UUID 后缀隔离，restore 参数化回收。
    let staging = chapters_dir.join(format!("{STAGING_PREFIX}{}", uuid::Uuid::new_v4().simple()));
    tokio::fs::create_dir_all(&staging)
        .await
        .map_err(|e| StructureError::bad_request(format!("staging mkdir failed: {e}")))?;
    let staging_runtime = staging.join("runtime");
    let staging_drafts = staging.join("drafts");
    let mut staged: Vec<(PathBuf, PathBuf)> = Vec::new();
    let staging_for_rollback = staging.clone();
    let rollback = |staged: Vec<(PathBuf, PathBuf)>| async move {
        for (from, to) in staged {
            let _ = tokio::fs::rename(&from, &to).await;
        }
        let _ = tokio::fs::remove_dir_all(&staging_for_rollback).await;
    };
    for file in chapter_files.values() {
        let dest = staging.join(&file.file_name);
        if let Err(e) = tokio::fs::rename(&file.path, &dest).await {
            rollback(staged).await;
            return Err(StructureError::bad_request(format!(
                "staging rename failed: {e}"
            )));
        }
        staged.push((dest, file.path.clone()));
    }
    for (path, name) in artifacts {
        let is_runtime = path.starts_with(root.join("story").join("runtime"));
        let dest_dir = if is_runtime {
            &staging_runtime
        } else {
            &staging_drafts
        };
        let _ = tokio::fs::create_dir_all(dest_dir).await;
        let dest = dest_dir.join(name);
        if let Err(e) = tokio::fs::rename(path, &dest).await {
            rollback(staged).await;
            return Err(StructureError::bad_request(format!(
                "staging artifact rename failed: {e}"
            )));
        }
        staged.push((dest, path.clone()));
    }
    Ok(staging)
}

/// 失败还原：staging 全量回原位 + 清 staging（索引在最后才写，零落盘）。
/// R61：不变量守卫失败的回位——删本轮新写文件 → 逆序撤销已回名 rename →
/// staging 全量回原位（与既有 rename_fail 路径同一序列）。
async fn rollback_structure_tx(
    written: &[PathBuf],
    executed_renames: &[(PathBuf, PathBuf)],
    chapters_dir: &Path,
    staging: &Path,
) {
    for path in written {
        let _ = tokio::fs::remove_file(path).await;
    }
    for (dest, staged_src) in executed_renames.iter().rev() {
        let _ = tokio::fs::rename(dest, staged_src).await;
    }
    restore_from_staging(chapters_dir, staging).await;
}

async fn restore_from_staging(chapters_dir: &Path, staging: &Path) {
    for sub in ["", "runtime", "drafts"] {
        let dir = if sub.is_empty() {
            staging.to_path_buf()
        } else {
            staging.join(sub)
        };
        let Ok(mut entries) = tokio::fs::read_dir(&dir).await else {
            continue;
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            // 1646 号审查修复：staging 根段跳过子目录（runtime/drafts 留给
            // 对应段处理——否则子目录被整体 rename 进 chapters/ 产物错位）。
            if sub.is_empty() && entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let dest = if sub.is_empty() {
                chapters_dir.join(&name)
            } else {
                chapters_dir
                    .parent()
                    .map(|root| root.join("story").join(sub).join(&name))
                    .unwrap_or_else(|| entry.path())
            };
            // 回位父目录可能已被外部删除（活体注入实证）——restore 必须
            // 自建父目录，否则还原失败=数据卡 staging。
            if let Some(parent) = dest.parent() {
                let _ = tokio::fs::create_dir_all(parent).await;
            }
            let _ = tokio::fs::rename(entry.path(), dest).await;
        }
    }
    let _ = tokio::fs::remove_dir_all(staging).await;
}

/// 索引行重编号应用：映射 number + updatedAt 刷新 + （可选）字数覆写。
fn apply_renumber(
    index: &[ChapterMeta],
    mapping: &BTreeMap<u32, u32>,
    word_overrides: &BTreeMap<u32, u32>,
    now_iso: &str,
) -> Vec<ChapterMeta> {
    let mut rows: Vec<ChapterMeta> = index
        .iter()
        .map(|meta| {
            let new_number = mapping.get(&meta.number).copied().unwrap_or(meta.number);
            let mut next = meta.clone();
            if next.number != new_number {
                next.number = new_number;
                next.updated_at = now_iso.to_string();
            }
            if let Some(&count) = word_overrides.get(&new_number) {
                next.word_count = count;
                next.updated_at = now_iso.to_string();
            }
            next
        })
        .collect();
    rows.sort_by_key(|meta| meta.number);
    rows
}

/// 章文件名（正典形态：`NNNN_title.md`；空 title → `NNNN_.md`）。
fn canonical_file_name(number: u32, title: &str) -> String {
    format!("{:04}_{}.md", number, title)
}

// ── 三事务 ───────────────────────────────────────────────────────

/// 拆分：`at` 号章在 UTF-16 offset 处切两章，新章占 at+1，后章起全量重编号。
#[allow(clippy::too_many_arguments)]
pub async fn split_chapter(
    state: &StateManager,
    book_id: &str,
    chapter_number: u32,
    offset: usize,
    expected_revision: Option<&str>,
    now_millis: i64,
    now_iso: &str,
) -> Result<StructureOutcome, StructureError> {
    let root = state.book_dir(book_id);
    let chapters_dir = root.join("chapters");
    let index = state
        .load_chapter_index(book_id)
        .await
        .map_err(|e| StructureError::bad_request(e.to_string()))?;
    if expected_revision.is_some_and(|expected| chapter_set_revision(&index) != expected) {
        return Err(StructureError::conflict());
    }
    let numbers: Vec<u32> = index.iter().map(|meta| meta.number).collect();
    let mut files = collect_chapter_files(&chapters_dir, &[chapter_number], &index).await?;
    let source = files
        .remove(&chapter_number)
        .ok_or_else(|| StructureError::not_found(format!("Chapter {chapter_number} not found")))?;
    let content = tokio::fs::read_to_string(&source.path)
        .await
        .map_err(|e| StructureError::bad_request(format!("chapter unreadable: {e}")))?;
    // R61：操作前全书指纹清单（staging/归档之前采集，内容未动）。
    let before_list = collect_fingerprints(&chapters_dir, &numbers, &index).await?;
    let (head, tail) = split_utf16(&content, offset)?;
    let renumber = renumber_after_split(&numbers, chapter_number);
    // 新号 = at+1：重编号把全部 >at 的章 +1 后该位必然空出（重复号坏数据
    // 在 collect_chapter_files 的 BTreeMap 键合并且索引读侧自愈面外——
    // 无需冲突检查，中段拆分不再误报）。
    let new_number = chapter_number + 1;

    // 归档先于落盘（chapter-replace 惯例）：前章旧文 → 旧号下，被拆后段 → 新号下。
    let book_dir = root.to_string_lossy().into_owned();
    archive_chapter_version(
        &FsStateStore,
        &book_dir,
        chapter_number,
        &content,
        ChapterVersionSource::Structure,
        now_millis,
        now_iso,
    )
    .await
    .map_err(|e| StructureError::bad_request(e.to_string()))?;
    archive_chapter_version(
        &FsStateStore,
        &book_dir,
        new_number,
        &tail,
        ChapterVersionSource::Structure,
        now_millis,
        now_iso,
    )
    .await
    .map_err(|e| StructureError::bad_request(e.to_string()))?;

    let affected_numbers: Vec<u32> = std::iter::once(chapter_number)
        .chain(numbers.iter().copied().filter(|&n| n > chapter_number))
        .collect();
    let affected = collect_chapter_files(&chapters_dir, &affected_numbers, &index).await?;
    let artifacts = collect_artifact_files(&root, &affected_numbers).await;
    let staging = stage_affected(&chapters_dir, &root, &affected, &artifacts).await?;

    // 新名规划（旧号→新号；新章文件名 canonical）。
    let mapping: BTreeMap<u32, u32> = renumber.iter().copied().collect();
    let mut rename_fail: Option<String> = None;
    let mut written: Vec<PathBuf> = Vec::new();
    let mut renumbered: Vec<RenumberedEntry> = Vec::new();
    // 受影响章：旧号本身保留原名（内容截断覆写），其余回名新号。
    // 1646 号审查修复：已执行 rename 台账（dest→staged 源）——中断时逆序
    // 撤销回 staging（restore 只能回收 staging 内文件，已回名章不可达）。
    let mut executed_renames: Vec<(PathBuf, PathBuf)> = Vec::new();
    for (old, file) in affected.iter() {
        let Some(&new) = mapping.get(old) else {
            continue;
        };
        let staged_path = staging.join(&file.file_name);
        let dest = chapters_dir.join(canonical_file_name(new, &file.meta.title));
        if old != &new {
            match tokio::fs::rename(&staged_path, &dest).await {
                Ok(()) => {
                    // dest 归 executed_renames 撤销面（逆序 rename 回 staging），
                    // 不入 written 删除面——先删后撤顺序会丢已回名章内容。
                    executed_renames.push((dest.clone(), staged_path.clone()));
                    renumbered.push(RenumberedEntry {
                        from: *old,
                        to: new,
                    });
                }
                Err(e) => {
                    rename_fail = Some(e.to_string());
                    break;
                }
            }
        }
    }
    if rename_fail.is_none() {
        // 前段覆写旧号原文件（staging 已移走，原子写新建）。
        let normalized_head = if head.ends_with('\n') {
            head.clone()
        } else {
            format!("{head}\n")
        };
        match write_file_atomic(&source.path, &normalized_head).await {
            Ok(()) => written.push(source.path.clone()),
            Err(e) => rename_fail = Some(e.to_string()),
        }
    }
    if rename_fail.is_none() {
        // 新章 = 拆出的后段（title 沿用原章，读侧空 title 回退章号显示）。
        let normalized_tail = if tail.ends_with('\n') {
            tail.clone()
        } else {
            format!("{tail}\n")
        };
        let new_path = chapters_dir.join(canonical_file_name(new_number, &source.meta.title));
        match write_file_atomic(&new_path, &normalized_tail).await {
            Ok(()) => written.push(new_path.clone()),
            Err(e) => rename_fail = Some(e.to_string()),
        }
    }
    if rename_fail.is_none() {
        // runtime/drafts 产物按映射回位（chapter-NNNN.* / NNNN_* 前导号）。
        for sub in ["runtime", "drafts"] {
            let staging_sub = staging.join(sub);
            let Ok(mut entries) = tokio::fs::read_dir(&staging_sub).await else {
                continue;
            };
            while let Ok(Some(entry)) = entries.next_entry().await {
                let name = entry.file_name().to_string_lossy().into_owned();
                let dest_dir = root.join("story").join(sub);
                let _ = tokio::fs::create_dir_all(&dest_dir).await;
                let dest_name = if name.starts_with("chapter-") {
                    let suffix = name.trim_start_matches("chapter-");
                    let old_padded: String = suffix.chars().take(4).collect();
                    let old_number: u32 = old_padded.parse().unwrap_or(0);
                    let new_local = mapping.get(&old_number).copied().unwrap_or(old_number);
                    format!("chapter-{new_local:04}{}", &suffix[4..])
                } else {
                    let old_number =
                        crate::state::chapter_filename::parse_chapter_file_number(&name)
                            .unwrap_or(0);
                    let new_local = mapping.get(&old_number).copied().unwrap_or(old_number);
                    name.replacen(&format!("{old_number:04}"), &format!("{new_local:04}"), 1)
                };
                if let Err(e) = tokio::fs::rename(entry.path(), dest_dir.join(&dest_name)).await {
                    rename_fail = Some(e.to_string());
                    break;
                }
            }
        }
    }
    if let Some(error) = rename_fail {
        // 还原：删本轮新写文件 → 逆序撤销已回名 rename → staging 全量回原位。
        for path in &written {
            let _ = tokio::fs::remove_file(path).await;
        }
        for (dest, staged_src) in executed_renames.iter().rev() {
            let _ = tokio::fs::rename(dest, staged_src).await;
        }
        restore_from_staging(&chapters_dir, &staging).await;
        return Err(StructureError::bad_request(format!(
            "split transaction aborted (no changes persisted): {error}"
        )));
    }
    let _ = tokio::fs::remove_dir_all(&staging).await;

    // 索引：新章行（ReadyForReview 惯例）+ 移位行 + 字数覆写，最后落盘。
    let mut word_overrides: BTreeMap<u32, u32> = BTreeMap::new();
    word_overrides.insert(
        chapter_number,
        crate::utils::language::utf16_len(head.trim()) as u32,
    );
    word_overrides.insert(
        new_number,
        crate::utils::language::utf16_len(tail.trim()) as u32,
    );
    let mut next_index = apply_renumber(&index, &mapping, &word_overrides, now_iso);
    next_index.push(ChapterMeta {
        number: new_number,
        title: source.meta.title.clone(),
        status: ChapterStatus::ReadyForReview,
        word_count: crate::utils::language::utf16_len(tail.trim()) as u32,
        created_at: now_iso.to_string(),
        updated_at: now_iso.to_string(),
        audit_issues: Vec::new(),
        length_warnings: Vec::new(),
        volume_id: None,
        review_note: None,
        detection_score: None,
        detection_provider: None,
        detected_at: None,
        length_telemetry: None,
        token_usage: None,
    });
    next_index.sort_by_key(|meta| meta.number);

    // R61：提交前不变量守卫（违反=从 staging 全量回位、零落盘、错误点名缺失项）。
    let next_numbers: Vec<u32> = next_index.iter().map(|meta| meta.number).collect();
    let hook = async {
        let after_list = collect_fingerprints(&chapters_dir, &next_numbers, &next_index).await?;
        let mut violations = structure_invariant(
            &StructureOp::Split { at: chapter_number },
            &before_list,
            &after_list,
        );
        // 拼回校验用未规范化的原始切分（head+tail 恒等于 content——slice 无损）；
        // 写入侧的补尾换行是展示规范化，不参与「两片拼回」语义。
        if head.clone() + tail.as_str() != content {
            violations.push("split pieces do not reassemble the chapter content".to_string());
        }
        Ok::<_, StructureError>((after_list, violations))
    }
    .await;
    let (after_list, violations) = match hook {
        Ok(pair) => pair,
        Err(error) => {
            rollback_structure_tx(&written, &executed_renames, &chapters_dir, &staging).await;
            return Err(StructureError::bad_request(format!(
                "structure invariant violated: {}",
                error.message
            )));
        }
    };
    if !violations.is_empty() {
        rollback_structure_tx(&written, &executed_renames, &chapters_dir, &staging).await;
        return Err(StructureError::bad_request(format!(
            "structure invariant violated: {}",
            violations.join("; ")
        )));
    }

    state
        .save_chapter_index_at(&root, &next_index, false)
        .await
        .map_err(|e| StructureError::bad_request(e.to_string()))?;

    // 操作日志（append-only；撤销批二的回位依据）。写失败不回滚已提交的事务
    // （内容面已落盘），记 stderr——批二撤销遇缺日志会拒绝并说明原因。
    if let Err(error) = append_structure_ops(
        &root,
        "split",
        &before_list,
        &after_list,
        now_iso,
        StructureOpsExtra {
            at: Some(chapter_number),
            ..Default::default()
        },
    )
    .await
    {
        eprintln!("structure ops log write failed: {error}");
    }

    Ok(StructureOutcome {
        op: "split",
        book_id: book_id.to_string(),
        split_from: Some(chapter_number),
        new_chapter: Some(new_number),
        merged_into: None,
        removed_chapter: None,
        renumbered,
        revision: chapter_set_revision(&next_index),
        touched_files: vec!["chapters/index.json".to_string()],
        summary: format!("Split chapter {chapter_number} into {chapter_number}+{new_number}."),
    })
}

/// 合并：`chapter_number` 与 `merge_with` 须相邻，序号保留较小者、删较大者
/// （正文进 .trash），后续章前移。`separator` 缺省 "\n\n"（可传空串）。
#[allow(clippy::too_many_arguments)]
pub async fn merge_chapter(
    state: &StateManager,
    book_id: &str,
    chapter_number: u32,
    merge_with: u32,
    separator: Option<&str>,
    expected_revision: Option<&str>,
    now_millis: i64,
    now_iso: &str,
) -> Result<StructureOutcome, StructureError> {
    if chapter_number == merge_with {
        return Err(StructureError::bad_request(
            "Merge requires two distinct adjacent chapters.",
        ));
    }
    if chapter_number.abs_diff(merge_with) != 1 {
        return Err(StructureError::bad_request(
            "Merge requires adjacent chapters.",
        ));
    }
    let root = state.book_dir(book_id);
    let chapters_dir = root.join("chapters");
    let index = state
        .load_chapter_index(book_id)
        .await
        .map_err(|e| StructureError::bad_request(e.to_string()))?;
    if expected_revision.is_some_and(|expected| chapter_set_revision(&index) != expected) {
        return Err(StructureError::conflict());
    }
    let keep = chapter_number.min(merge_with);
    let removed = chapter_number.max(merge_with);
    let numbers: Vec<u32> = index.iter().map(|meta| meta.number).collect();
    let mut files = collect_chapter_files(&chapters_dir, &[keep, removed], &index).await?;
    let keep_file = files
        .remove(&keep)
        .ok_or_else(|| StructureError::not_found(format!("Chapter {keep} not found")))?;
    let removed_file = files
        .remove(&removed)
        .ok_or_else(|| StructureError::not_found(format!("Chapter {removed} not found")))?;
    let keep_content = tokio::fs::read_to_string(&keep_file.path)
        .await
        .map_err(|e| StructureError::bad_request(format!("chapter unreadable: {e}")))?;
    let removed_content = tokio::fs::read_to_string(&removed_file.path)
        .await
        .map_err(|e| StructureError::bad_request(format!("chapter unreadable: {e}")))?;
    let sep = separator.unwrap_or("\n\n");
    let merged = merge_content(keep_content.trim_end(), removed_content.trim_start(), sep);
    let renumber = renumber_after_merge(&numbers, removed);
    // R61：操作前全书指纹清单（归档/提交之前采集）。
    let before_list = collect_fingerprints(&chapters_dir, &numbers, &index).await?;

    // 归档先于落盘：保留章旧文（structure 来源）。
    let book_dir = root.to_string_lossy().into_owned();
    archive_chapter_version(
        &FsStateStore,
        &book_dir,
        keep,
        &keep_content,
        ChapterVersionSource::Structure,
        now_millis,
        now_iso,
    )
    .await
    .map_err(|e| StructureError::bad_request(e.to_string()))?;

    let affected_numbers: Vec<u32> = std::iter::once(keep)
        .chain(std::iter::once(removed))
        .chain(numbers.iter().copied().filter(|&n| n > removed))
        .collect();
    let affected = collect_chapter_files(&chapters_dir, &affected_numbers, &index).await?;
    let artifacts = collect_artifact_files(&root, &affected_numbers).await;
    let staging = stage_affected(&chapters_dir, &root, &affected, &artifacts).await?;

    let mapping: BTreeMap<u32, u32> = renumber.iter().copied().collect();
    let mut rename_fail: Option<String> = None;
    let mut written: Vec<PathBuf> = Vec::new();
    let mut renumbered: Vec<RenumberedEntry> = Vec::new();
    // 被删章正文 → .trash（重名规避），其余受影响章回名新号。
    let trash_dir = chapters_dir.join(TRASH_DIR);
    let _ = tokio::fs::create_dir_all(&trash_dir).await;
    let trash_name = pick_available_name(&trash_dir, &removed_file.file_name).await;
    let mut executed_renames: Vec<(PathBuf, PathBuf)> = Vec::new();
    match tokio::fs::rename(
        &staging.join(&removed_file.file_name),
        trash_dir.join(&trash_name),
    )
    .await
    {
        Ok(()) => {
            executed_renames.push((
                trash_dir.join(&trash_name),
                staging.join(&removed_file.file_name),
            ));
        }
        Err(e) => rename_fail = Some(e.to_string()),
    }
    for (old, file) in affected.iter() {
        if rename_fail.is_some() || *old == removed {
            continue;
        }
        let staged_path = staging.join(&file.file_name);
        if *old == keep {
            // 保留章文件暂留 staging，待合并内容覆写后写回。
            continue;
        }
        let Some(&new) = mapping.get(old) else {
            continue;
        };
        let dest = chapters_dir.join(canonical_file_name(new, &file.meta.title));
        match tokio::fs::rename(&staged_path, &dest).await {
            Ok(()) => {
                written.push(dest.clone());
                renumbered.push(RenumberedEntry {
                    from: *old,
                    to: new,
                });
            }
            Err(e) => {
                rename_fail = Some(e.to_string());
                break;
            }
        }
    }
    if rename_fail.is_none() {
        // 1666 号：合并产物恒以单个换行收尾。此前直接补 `\n`——被并章以换行
        // 收尾（写入惯例）时其尾部换行与补的收尾换行叠加成一个多余空行；被并章
        // 为空白时分隔符的尾随空白同样在此剪掉。
        match write_file_atomic(&keep_file.path, &format!("{}\n", merged.trim_end())).await {
            Ok(()) => written.push(keep_file.path.clone()),
            Err(e) => rename_fail = Some(e.to_string()),
        }
    }
    if rename_fail.is_none() {
        // 被删章 runtime/drafts 产物直接移 trash（跟随正文永不硬删语义）；
        // 保留章与其余章产物按映射回位。
        for sub in ["runtime", "drafts"] {
            let staging_sub = staging.join(sub);
            let Ok(mut entries) = tokio::fs::read_dir(&staging_sub).await else {
                continue;
            };
            while let Ok(Some(entry)) = entries.next_entry().await {
                let name = entry.file_name().to_string_lossy().into_owned();
                let old_number = if name.starts_with("chapter-") {
                    name.trim_start_matches("chapter-")
                        .chars()
                        .take(4)
                        .collect::<String>()
                        .parse()
                        .unwrap_or(0)
                } else {
                    crate::state::chapter_filename::parse_chapter_file_number(&name).unwrap_or(0)
                };
                let dest_dir = root.join("story").join(sub);
                let _ = tokio::fs::create_dir_all(&dest_dir).await;
                if old_number == removed {
                    let trash_sub = trash_dir.join(sub);
                    let _ = tokio::fs::create_dir_all(&trash_sub).await;
                    let _ = tokio::fs::rename(entry.path(), trash_sub.join(&name)).await;
                    continue;
                }
                let new_local = mapping.get(&old_number).copied().unwrap_or(old_number);
                let dest_name = if name.starts_with("chapter-") {
                    let suffix = name.trim_start_matches("chapter-");
                    format!("chapter-{new_local:04}{}", &suffix[4..])
                } else {
                    name.replacen(&format!("{old_number:04}"), &format!("{new_local:04}"), 1)
                };
                if let Err(e) = tokio::fs::rename(entry.path(), dest_dir.join(&dest_name)).await {
                    rename_fail = Some(e.to_string());
                    break;
                }
            }
        }
    }
    if let Some(error) = rename_fail {
        for path in &written {
            let _ = tokio::fs::remove_file(path).await;
        }
        // 1646 号审查修复：逆序撤销已回名 rename（含 trash 首步）后再 restore。
        for (dest, staged_src) in executed_renames.iter().rev() {
            let _ = tokio::fs::rename(dest, staged_src).await;
        }
        // trash 内本轮移入的产物移回 staging 对应子目录（零丢失），再整体还原。
        for sub in ["runtime", "drafts"] {
            let trash_sub = trash_dir.join(sub);
            let staging_sub = staging.join(sub);
            if let Ok(mut entries) = tokio::fs::read_dir(&trash_sub).await {
                while let Ok(Some(entry)) = entries.next_entry().await {
                    let _ =
                        tokio::fs::rename(entry.path(), staging_sub.join(entry.file_name())).await;
                }
            }
            let _ = tokio::fs::remove_dir_all(&trash_sub).await;
        }
        let _ = tokio::fs::rename(
            trash_dir.join(&trash_name),
            staging.join(&removed_file.file_name),
        )
        .await;
        restore_from_staging(&chapters_dir, &staging).await;
        return Err(StructureError::bad_request(format!(
            "merge transaction aborted (no changes persisted): {error}"
        )));
    }
    let _ = tokio::fs::remove_dir_all(&staging).await;

    let mut word_overrides: BTreeMap<u32, u32> = BTreeMap::new();
    word_overrides.insert(
        keep,
        crate::utils::language::utf16_len(merged.trim()) as u32,
    );
    // 1647 号 bug 修复：先滤掉被删章行、再应用重编号映射。此前顺序相反——
    // 「原 removed 行」与「重编后恰好落到 removed 号的行」并存同号，随后的
    // number != removed 过滤把重编章一并滤杀（3 章书 merge 3→2 后 index 只剩
    // {1,2}、文件面却正确——index/文件面发散，duel 双端同错通过=回声室，
    // 逐功能「拆→并→集合复原」live 断言逼出）。
    let kept: Vec<ChapterMeta> = index
        .iter()
        .filter(|meta| meta.number != removed)
        .cloned()
        .collect();
    let next_index: Vec<ChapterMeta> = apply_renumber(&kept, &mapping, &word_overrides, now_iso);

    // R61：提交前不变量守卫（违反=从 staging 全量回位、零落盘、错误点名缺失项）。
    let next_numbers: Vec<u32> = next_index.iter().map(|meta| meta.number).collect();
    let hook = async {
        let after_list = collect_fingerprints(&chapters_dir, &next_numbers, &next_index).await?;
        let mut violations =
            structure_invariant(&StructureOp::Merge { removed }, &before_list, &after_list);
        // 内容级：keep 章写入值 == 合并拼接的收尾规范形（与写入表达式同源，防御未来改动）。
        let keep_write = keep_file.path.clone();
        let actual = tokio::fs::read_to_string(&keep_write)
            .await
            .map_err(|e| StructureError::bad_request(format!("chapter unreadable: {e}")))?;
        if actual != format!("{}\n", merged.trim_end()) {
            violations.push(
                "merged chapter content does not match merge_content(keep, removed)".to_string(),
            );
        }
        Ok::<_, StructureError>((after_list, violations))
    }
    .await;
    let (after_list, violations) = match hook {
        Ok(pair) => pair,
        Err(error) => {
            rollback_structure_tx(&written, &executed_renames, &chapters_dir, &staging).await;
            return Err(StructureError::bad_request(format!(
                "structure invariant violated: {}",
                error.message
            )));
        }
    };
    if !violations.is_empty() {
        rollback_structure_tx(&written, &executed_renames, &chapters_dir, &staging).await;
        return Err(StructureError::bad_request(format!(
            "structure invariant violated: {}",
            violations.join("; ")
        )));
    }

    state
        .save_chapter_index_at(&root, &next_index, false)
        .await
        .map_err(|e| StructureError::bad_request(e.to_string()))?;

    // 操作日志（append-only；撤销批二的回位依据）。写失败不回滚已提交的事务
    // （内容面已落盘），记 stderr——批二撤销遇缺日志会拒绝并说明原因。
    if let Err(error) = append_structure_ops(
        &root,
        "merge",
        &before_list,
        &after_list,
        now_iso,
        StructureOpsExtra {
            removed: Some(removed),
            ..Default::default()
        },
    )
    .await
    {
        eprintln!("structure ops log write failed: {error}");
    }

    Ok(StructureOutcome {
        op: "merge",
        book_id: book_id.to_string(),
        split_from: None,
        new_chapter: None,
        merged_into: Some(keep),
        removed_chapter: Some(removed),
        renumbered,
        revision: chapter_set_revision(&next_index),
        touched_files: vec!["chapters/index.json".to_string()],
        summary: format!("Merged chapter {removed} into {keep}."),
    })
}

/// 重排：`order`（新序章号全排列）→ 批量重编号事务。
pub async fn reorder_chapters(
    state: &StateManager,
    book_id: &str,
    order: &[u32],
    expected_revision: Option<&str>,
    now_iso: &str,
) -> Result<StructureOutcome, StructureError> {
    let root = state.book_dir(book_id);
    let chapters_dir = root.join("chapters");
    let index = state
        .load_chapter_index(book_id)
        .await
        .map_err(|e| StructureError::bad_request(e.to_string()))?;
    if expected_revision.is_some_and(|expected| chapter_set_revision(&index) != expected) {
        return Err(StructureError::conflict());
    }
    let numbers: Vec<u32> = index.iter().map(|meta| meta.number).collect();
    let renumber = reorder_plan(&numbers, order)?;
    if renumber.is_empty() {
        return Err(StructureError::bad_request("Reorder produces no change."));
    }
    // 1649 号 bug 修复（IAB 走查逼出）：staging 收走面=**变号章**，与
    // split/merge 的受影响语义对齐。此前传入全部章号——恒等成员（号不变
    // 的章）被收进 staging 后，rename 循环 `mapping.get(old)` 未命中而
    // continue，恒等章滞留 staging，成功路径 remove_dir_all 将其**永久
    // 删除**（reorder [1,3,2,4,5] 后 0001/0004/0005 静默消失而 index 仍在
    // =数据丢失级 index/文件面发散；既有测试全用全员变号排列 [3,2,1]/
    // [3,1,2]/[2,1]，恒等成员从未被测）。
    let affected: Vec<u32> = renumber.iter().map(|(from, _)| *from).collect();
    let files = collect_chapter_files(&chapters_dir, &affected, &index).await?;
    let artifacts = collect_artifact_files(&root, &affected).await;
    // R61：操作前全书指纹清单（staging 之前采集，内容未动）。
    let before_list = collect_fingerprints(&chapters_dir, &numbers, &index).await?;
    let staging = stage_affected(&chapters_dir, &root, &files, &artifacts).await?;

    let mapping: BTreeMap<u32, u32> = renumber.iter().copied().collect();
    let mut rename_fail: Option<String> = None;
    let mut renumbered: Vec<RenumberedEntry> = Vec::new();
    let mut executed_renames: Vec<(PathBuf, PathBuf)> = Vec::new();
    for (old, file) in files.iter() {
        let Some(&new) = mapping.get(old) else {
            continue;
        };
        let staged_path = staging.join(&file.file_name);
        let dest = chapters_dir.join(canonical_file_name(new, &file.meta.title));
        match tokio::fs::rename(&staged_path, &dest).await {
            Ok(()) => {
                executed_renames.push((dest.clone(), staged_path.clone()));
                renumbered.push(RenumberedEntry {
                    from: *old,
                    to: new,
                });
            }
            Err(e) => {
                rename_fail = Some(e.to_string());
                break;
            }
        }
    }
    if rename_fail.is_none() {
        for sub in ["runtime", "drafts"] {
            let staging_sub = staging.join(sub);
            let Ok(mut entries) = tokio::fs::read_dir(&staging_sub).await else {
                continue;
            };
            while let Ok(Some(entry)) = entries.next_entry().await {
                let name = entry.file_name().to_string_lossy().into_owned();
                let old_number = if name.starts_with("chapter-") {
                    name.trim_start_matches("chapter-")
                        .chars()
                        .take(4)
                        .collect::<String>()
                        .parse()
                        .unwrap_or(0)
                } else {
                    crate::state::chapter_filename::parse_chapter_file_number(&name).unwrap_or(0)
                };
                let new_local = mapping.get(&old_number).copied().unwrap_or(old_number);
                let dest_dir = root.join("story").join(sub);
                let _ = tokio::fs::create_dir_all(&dest_dir).await;
                let dest_name = if name.starts_with("chapter-") {
                    let suffix = name.trim_start_matches("chapter-");
                    format!("chapter-{new_local:04}{}", &suffix[4..])
                } else {
                    name.replacen(&format!("{old_number:04}"), &format!("{new_local:04}"), 1)
                };
                if let Err(e) = tokio::fs::rename(entry.path(), dest_dir.join(&dest_name)).await {
                    rename_fail = Some(e.to_string());
                    break;
                }
            }
        }
    }
    if let Some(error) = rename_fail {
        for (dest, staged_src) in executed_renames.iter().rev() {
            let _ = tokio::fs::rename(dest, staged_src).await;
        }
        restore_from_staging(&chapters_dir, &staging).await;
        return Err(StructureError::bad_request(format!(
            "reorder transaction aborted (no changes persisted): {error}"
        )));
    }
    let _ = tokio::fs::remove_dir_all(&staging).await;

    let next_index = apply_renumber(&index, &mapping, &BTreeMap::new(), now_iso);

    // R61：提交前不变量守卫（reorder 不得改写任何章正文；违反=全量回位）。
    let next_numbers: Vec<u32> = next_index.iter().map(|meta| meta.number).collect();
    let hook = async {
        let after_list = collect_fingerprints(&chapters_dir, &next_numbers, &next_index).await?;
        let violations = structure_invariant(&StructureOp::Reorder, &before_list, &after_list);
        Ok::<_, StructureError>((after_list, violations))
    }
    .await;
    let (after_list, violations) = match hook {
        Ok(pair) => pair,
        Err(error) => {
            rollback_structure_tx(&[], &executed_renames, &chapters_dir, &staging).await;
            return Err(StructureError::bad_request(format!(
                "structure invariant violated: {}",
                error.message
            )));
        }
    };
    if !violations.is_empty() {
        rollback_structure_tx(&[], &executed_renames, &chapters_dir, &staging).await;
        return Err(StructureError::bad_request(format!(
            "structure invariant violated: {}",
            violations.join("; ")
        )));
    }

    state
        .save_chapter_index_at(&root, &next_index, false)
        .await
        .map_err(|e| StructureError::bad_request(e.to_string()))?;

    // 操作日志（append-only；撤销批二的回位依据）。写失败不回滚已提交的事务
    // （内容面已落盘），记 stderr——批二撤销遇缺日志会拒绝并说明原因。
    if let Err(error) = append_structure_ops(
        &root,
        "reorder",
        &before_list,
        &after_list,
        now_iso,
        StructureOpsExtra::default(),
    )
    .await
    {
        eprintln!("structure ops log write failed: {error}");
    }

    Ok(StructureOutcome {
        op: "reorder",
        book_id: book_id.to_string(),
        split_from: None,
        new_chapter: None,
        merged_into: None,
        removed_chapter: None,
        renumbered,
        revision: chapter_set_revision(&next_index),
        touched_files: vec!["chapters/index.json".to_string()],
        summary: format!("Reordered {} chapters.", order.len()),
    })
}

// ── R61 批一（1687 号）：结构事务不变量守卫 + 操作日志 ─────────────────
// 镜像 `packages/core/src/state/chapter-structure.ts` 同名段；纯函数双端 golden
// 同读（packages/studio/src/__tests__/golden/structure-invariant-vectors.json，
// TS 测试 readFileSync、本侧 include_str!）。撤销端点与 undo_eligible 归批二。

/// 章内容指纹：UTF-8 sha256 前 16 hex（chapter_set_revision 同族；只在结构操作时计算）。
pub fn content_fingerprint(content: &str) -> String {
    let digest = Sha256::digest(content.as_bytes());
    format!("{:x}", digest)[..16].to_string()
}

/// 清单项：章号 + 现文件名 + 内容指纹（只有这三样，无正文）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChapterFingerprint {
    pub number: u32,
    pub file_name: String,
    pub fingerprint: String,
}

/// 结构操作类型（golden 向量同形：`{"kind":"split","at":2}`）。
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum StructureOp {
    Reorder,
    Split { at: u32 },
    Merge { removed: u32 },
}

/// 操作日志行（`story/structure-ops.jsonl`；append-only，撤销批二消费）。
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructureOperationRecord {
    pub ts: String,
    pub op_id: String,
    pub kind: String,
    pub before: Vec<ChapterFingerprint>,
    pub after: Vec<ChapterFingerprint>,
    pub undone: bool,
    /// split 行：被拆章号（撤销推导逆 op 用）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at: Option<u32>,
    /// merge 行：被并章号（同上）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub removed: Option<u32>,
    /// undo 行指向被撤销那笔的 opId。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub undo_of: Option<String>,
}

/// 清单级不变量判定（镜像 TS `structureInvariant` 逐字消息；多指纹违反按指纹
/// 字典序输出，两端同序）：reorder = 号集合相等 + 指纹多重集相等（内容只换位
/// 不改写）；split 被拆号允许变、新号 at+1 出现、其余 +1 移位保指纹；merge 被
/// 并号消失、keep 号（removed-1）允许变、其余 -1 移位保指纹。内容级校验（两片
/// 拼回 / 合并拼接）由事务钩子携带内容做，不进本函数。
pub fn structure_invariant(
    op: &StructureOp,
    before: &[ChapterFingerprint],
    after: &[ChapterFingerprint],
) -> Vec<String> {
    let mut violations: Vec<String> = Vec::new();
    let expected_count = match op {
        StructureOp::Split { .. } => before.len() + 1,
        StructureOp::Merge { .. } => before.len().saturating_sub(1),
        StructureOp::Reorder => before.len(),
    };
    if after.len() != expected_count {
        let expectation = match op {
            StructureOp::Split { .. } => "split must add exactly one",
            StructureOp::Merge { .. } => "merge must remove exactly one",
            StructureOp::Reorder => "reorder must keep the count",
        };
        violations.push(format!(
            "chapter count changed: {} -> {} ({expectation})",
            before.len(),
            after.len()
        ));
    }
    let find_after = |number: u32| after.iter().find(|item| item.number == number);
    match op {
        StructureOp::Reorder => {
            let before_numbers: std::collections::BTreeSet<u32> =
                before.iter().map(|item| item.number).collect();
            let after_numbers: std::collections::BTreeSet<u32> =
                after.iter().map(|item| item.number).collect();
            for number in &before_numbers {
                if !after_numbers.contains(number) {
                    let name = before
                        .iter()
                        .find(|item| item.number == *number)
                        .map(|item| item.file_name.clone())
                        .unwrap_or_default();
                    violations.push(format!(
                        "missing chapter {number} ({name}) has no counterpart in the after set"
                    ));
                }
            }
            for number in &after_numbers {
                if !before_numbers.contains(number) {
                    let name = after
                        .iter()
                        .find(|item| item.number == *number)
                        .map(|item| item.file_name.clone())
                        .unwrap_or_default();
                    violations.push(format!(
                        "unexpected chapter {number} ({name}) appeared in the after set"
                    ));
                }
            }
            let mut before_counts: BTreeMap<&str, usize> = BTreeMap::new();
            for item in before {
                *before_counts.entry(item.fingerprint.as_str()).or_default() += 1;
            }
            let mut after_counts: BTreeMap<&str, usize> = BTreeMap::new();
            for item in after {
                *after_counts.entry(item.fingerprint.as_str()).or_default() += 1;
            }
            for (fingerprint, count) in &before_counts {
                if after_counts.get(fingerprint).copied().unwrap_or(0) < *count {
                    let owners = before
                        .iter()
                        .filter(|item| item.fingerprint == *fingerprint)
                        .map(|item| format!("{} ({})", item.number, item.file_name))
                        .collect::<Vec<_>>()
                        .join(", ");
                    violations.push(format!(
                        "fingerprint {fingerprint} (chapter {owners}) is missing from the after set but reorder must not rewrite any chapter"
                    ));
                }
            }
            for (fingerprint, count) in &after_counts {
                if before_counts.get(fingerprint).copied().unwrap_or(0) < *count {
                    let owners = after
                        .iter()
                        .filter(|item| item.fingerprint == *fingerprint)
                        .map(|item| format!("{} ({})", item.number, item.file_name))
                        .collect::<Vec<_>>()
                        .join(", ");
                    violations.push(format!(
                        "fingerprint {fingerprint} (after chapter {owners}) matches no before chapter"
                    ));
                }
            }
        }
        StructureOp::Split { at } => {
            let mut expected_numbers: std::collections::BTreeSet<u32> =
                [*at, *at + 1].into_iter().collect();
            for item in before {
                if item.number == *at {
                    continue; // 被拆章允许变（截断成前段）
                }
                let target = if item.number > *at {
                    item.number + 1
                } else {
                    item.number
                };
                expected_numbers.insert(target);
                let Some(counterpart) = find_after(target) else {
                    violations.push(format!(
                        "missing chapter {target} ({}) after split renumbering",
                        item.file_name
                    ));
                    continue;
                };
                if counterpart.fingerprint != item.fingerprint {
                    violations.push(format!(
                        "chapter {} ({}) content changed ({} -> {}) but split only renumbers it",
                        item.number, item.file_name, item.fingerprint, counterpart.fingerprint
                    ));
                }
            }
            for item in after {
                if !expected_numbers.contains(&item.number) {
                    violations.push(format!(
                        "unexpected chapter {} ({}) appeared in the after set",
                        item.number, item.file_name
                    ));
                }
            }
        }
        StructureOp::Merge { removed } => {
            let mut expected_numbers: std::collections::BTreeSet<u32> =
                std::collections::BTreeSet::new();
            for item in before {
                if item.number == *removed {
                    continue; // 被并章消失
                }
                let target = if item.number > *removed {
                    item.number - 1
                } else {
                    item.number
                };
                expected_numbers.insert(target);
                let Some(counterpart) = find_after(target) else {
                    violations.push(format!(
                        "missing chapter {target} ({}) after merge renumbering",
                        item.file_name
                    ));
                    continue;
                };
                // keep 号（removed-1）被合并覆写，允许变。
                if item.number != removed.saturating_sub(1)
                    && counterpart.fingerprint != item.fingerprint
                {
                    violations.push(format!(
                        "chapter {} ({}) content changed ({} -> {}) but merge only renumbers it",
                        item.number, item.file_name, item.fingerprint, counterpart.fingerprint
                    ));
                }
            }
            for item in after {
                if !expected_numbers.contains(&item.number) {
                    violations.push(format!(
                        "unexpected chapter {} ({}) survived the merge",
                        item.number, item.file_name
                    ));
                }
            }
        }
    }
    violations
}

/// 操作前全书指纹清单（撤销批二的全量回位依据；结构操作低频，逐章重读可承受）。
async fn collect_fingerprints(
    chapters_dir: &Path,
    numbers: &[u32],
    index: &[ChapterMeta],
) -> Result<Vec<ChapterFingerprint>, StructureError> {
    let files = collect_chapter_files(chapters_dir, numbers, index).await?;
    let mut sorted = numbers.to_vec();
    sorted.sort_unstable();
    let mut out = Vec::with_capacity(sorted.len());
    for number in sorted {
        let file = files
            .get(&number)
            .ok_or_else(|| StructureError::not_found(format!("Chapter {number} not found")))?;
        let content = tokio::fs::read_to_string(&file.path)
            .await
            .map_err(|e| StructureError::bad_request(format!("chapter unreadable: {e}")))?;
        out.push(ChapterFingerprint {
            number,
            file_name: file.file_name.clone(),
            fingerprint: content_fingerprint(&content),
        });
    }
    Ok(out)
}

/// 操作日志行附加参数（undo 行的 undoOf；split/merge 行的操作参数——撤销推导逆 op 用）。
#[derive(Debug, Clone, Default)]
pub struct StructureOpsExtra {
    pub undo_of: Option<String>,
    pub at: Option<u32>,
    pub removed: Option<u32>,
}

/// 成功提交后追加操作日志（读改写整文件 + 原子写，writing-log 同族）。
pub async fn append_structure_ops(
    book_dir: &Path,
    kind: &str,
    before: &[ChapterFingerprint],
    after: &[ChapterFingerprint],
    ts: &str,
    extra: StructureOpsExtra,
) -> std::io::Result<StructureOperationRecord> {
    let record = StructureOperationRecord {
        ts: ts.to_string(),
        op_id: format!("{kind}-{}", uuid::Uuid::new_v4().simple()),
        kind: kind.to_string(),
        before: before.to_vec(),
        after: after.to_vec(),
        undone: false,
        at: extra.at,
        removed: extra.removed,
        undo_of: extra.undo_of.clone(),
    };
    let path = book_dir.join("story").join("structure-ops.jsonl");
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let mut lines: Vec<String> = match tokio::fs::read_to_string(&path).await {
        Ok(raw) => raw
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(String::from)
            .collect(),
        Err(_) => Vec::new(),
    };
    lines.push(serde_json::to_string(&record).unwrap_or_default());
    write_file_atomic(&path, &format!("{}\n", lines.join("\n"))).await?;
    Ok(record)
}

/// 读操作日志（文件缺 → 空；坏行忽略）。
async fn load_structure_ops(book_dir: &Path) -> Vec<StructureOperationRecord> {
    match tokio::fs::read_to_string(book_dir.join("story").join("structure-ops.jsonl")).await {
        Ok(raw) => raw
            .lines()
            .filter(|line| !line.trim().is_empty())
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// 原笔标已撤销（读改写整文件；行按 opId 匹配）。
async fn mark_undone(book_dir: &Path, op_id: &str) -> std::io::Result<()> {
    let mut rows = load_structure_ops(book_dir).await;
    for row in rows.iter_mut() {
        if row.op_id == op_id {
            row.undone = true;
        }
    }
    let path = book_dir.join("story").join("structure-ops.jsonl");
    let text = format!(
        "{}\n",
        rows.iter()
            .map(|row| serde_json::to_string(row).unwrap_or_default())
            .collect::<Vec<_>>()
            .join("\n")
    );
    write_file_atomic(&path, &text).await
}

/// 可撤销判定（镜像 TS `undoEligible` 逐字 reason）：当且仅当尾笔存在、
/// undone:false、kind 不是 undo、且尾笔 after 与当前盘面（号集合 + 指纹多重集）
/// 一致——其后没有编辑改写。
pub fn undo_eligible(
    tail: Option<&StructureOperationRecord>,
    current: &[ChapterFingerprint],
) -> (bool, Option<&'static str>) {
    let Some(tail) = tail else {
        return (false, Some("no structure operations logged"));
    };
    if tail.undone {
        return (false, Some("last operation is already undone"));
    }
    if tail.kind == "undo" {
        return (false, Some("last entry is itself an undo"));
    }
    let same_numbers = tail.after.len() == current.len()
        && tail.after.iter().all(|item| {
            current
                .iter()
                .any(|candidate| candidate.number == item.number)
        })
        && current.iter().all(|candidate| {
            tail.after
                .iter()
                .any(|item| item.number == candidate.number)
        });
    let multiset = |list: &[ChapterFingerprint]| -> Vec<String> {
        let mut keys: Vec<String> = list
            .iter()
            .map(|item| format!("{}\u{0}{}", item.number, item.fingerprint))
            .collect();
        keys.sort();
        keys
    };
    if !same_numbers || multiset(&tail.after) != multiset(current) {
        return (false, Some("disk state changed since the last operation"));
    }
    (true, None)
}

/// 目录内按内容指纹找正文（.trash 根 / .versions/<号>/；找不到 → None）。
async fn find_content_by_fingerprint(dir: &Path, fingerprint: &str) -> Option<String> {
    let mut entries = tokio::fs::read_dir(dir).await.ok()?;
    let mut names: Vec<String> = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        names.push(entry.file_name().to_string_lossy().into_owned());
    }
    names.sort();
    names.reverse();
    for name in names {
        if !name.ends_with(".md") {
            continue;
        }
        let Ok(content) = tokio::fs::read_to_string(dir.join(&name)).await else {
            continue;
        };
        if content_fingerprint(&content) == fingerprint {
            return Some(content);
        }
    }
    None
}

/// 撤销上一笔结构事务（R61 批二；镜像 TS `undoLastStructureOperation`）：
/// undoEligible 守卫 → 内容来源链（现盘指纹配对 → .trash → .versions/<号>
/// structure 存档；缺失拒绝点名章）→ 逆 op 过同一清单守卫 → 提交（写面=
/// 目标号原子写/回名 + 非目标号删除；staging 两相失败回位与批一同构）→
/// undo 行 + 原笔标 undone:true。
#[allow(clippy::too_many_arguments)]
pub async fn undo_last_structure_operation(
    state: &StateManager,
    book_id: &str,
    expected_revision: Option<&str>,
    now_millis: i64,
    now_iso: &str,
) -> Result<StructureOutcome, StructureError> {
    let _ = now_millis;
    let root = state.book_dir(book_id);
    let chapters_dir = root.join("chapters");
    let index = state
        .load_chapter_index(book_id)
        .await
        .map_err(|e| StructureError::bad_request(e.to_string()))?;
    if expected_revision.is_some_and(|expected| chapter_set_revision(&index) != expected) {
        return Err(StructureError::conflict());
    }
    let numbers: Vec<u32> = index.iter().map(|meta| meta.number).collect();
    let current = collect_fingerprints(&chapters_dir, &numbers, &index).await?;
    let rows = load_structure_ops(&root).await;
    let tail = rows.last();
    let (eligible, reason) = undo_eligible(tail, &current);
    let Some(tail) = tail else {
        return Err(StructureError::bad_request(format!(
            "Undo refused: {}",
            reason.unwrap_or("unknown")
        )));
    };
    if !eligible {
        return Err(StructureError {
            status: 409,
            message: format!("Undo refused: {}", reason.unwrap_or("unknown")),
            code: None,
        });
    }

    // 逆 op 从日志行的操作参数推导（行携带 at/removed——指纹推导不唯一）。
    let inverse_op = match tail.kind.as_str() {
        "split" => StructureOp::Merge {
            removed: tail.at.map(|at| at + 1).unwrap_or(0),
        },
        "merge" => StructureOp::Split {
            at: tail
                .removed
                .map(|removed| removed.saturating_sub(1))
                .unwrap_or(0),
        },
        _ => StructureOp::Reorder,
    };

    // 当前内容的指纹索引（回位时「内容只搬位置」的来源）。
    let mut current_by_fingerprint: BTreeMap<String, String> = BTreeMap::new();
    let mut current_paths: BTreeMap<u32, PathBuf> = BTreeMap::new();
    let mut current_names: BTreeMap<u32, String> = BTreeMap::new();
    for item in &current {
        if let Some(file) = collect_chapter_files(&chapters_dir, &[item.number], index.as_slice())
            .await?
            .get(&item.number)
        {
            let content = tokio::fs::read_to_string(&file.path)
                .await
                .map_err(|e| StructureError::bad_request(format!("chapter unreadable: {e}")))?;
            current_by_fingerprint.insert(item.fingerprint.clone(), content);
            current_paths.insert(item.number, file.path.clone());
            current_names.insert(item.number, file.file_name.clone());
        }
    }

    // 目标章内容来源链：现盘 → .trash（根文件）→ .versions/<号>/ structure 存档。
    let mut resolved: BTreeMap<u32, String> = BTreeMap::new();
    for target in &tail.before {
        if let Some(content) = current_by_fingerprint.get(&target.fingerprint) {
            resolved.insert(target.number, content.clone());
            continue;
        }
        let trash_dir = chapters_dir.join(TRASH_DIR);
        if let Some(content) = find_content_by_fingerprint(&trash_dir, &target.fingerprint).await {
            resolved.insert(target.number, content);
            continue;
        }
        let padded = format!("{:04}", target.number);
        let versions_dir = chapters_dir.join(".versions").join(&padded);
        if let Some(content) = find_content_by_fingerprint(&versions_dir, &target.fingerprint).await
        {
            resolved.insert(target.number, content);
            continue;
        }
        return Err(StructureError {
            status: 409,
            message: format!(
                "Undo refused: cannot restore chapter {} ({}): no matching content in disk, .trash or .versions",
                target.number, target.file_name
            ),
            code: None,
        });
    }

    // 守卫（逆 op 过同一清单判定）——抛在提交之前=零落盘。
    let violations = structure_invariant(&inverse_op, &current, &tail.before);
    if !violations.is_empty() {
        return Err(StructureError::bad_request(format!(
            "structure invariant violated: {}",
            violations.join("; ")
        )));
    }

    // 写面：目标号 → 目标名（内容变更则原子写；名不同则删旧名）；当前号
    // ∉ 目标号集 → 该章内容已按指纹配对搬走，旧文件删除。
    let mut written: Vec<PathBuf> = Vec::new();
    let mut executed_renames: Vec<(PathBuf, PathBuf)> = Vec::new();
    let target_numbers: std::collections::BTreeSet<u32> =
        tail.before.iter().map(|item| item.number).collect();
    for target in &tail.before {
        let content = resolved.get(&target.number).expect("resolved above");
        let dest = chapters_dir.join(&target.file_name);
        let current_path = current_paths.get(&target.number);
        let needs_write = match current_path {
            Some(path) => tokio::fs::read_to_string(path).await.unwrap_or_default() != *content,
            None => true,
        };
        if needs_write {
            if let Err(e) = write_file_atomic(&dest, content).await {
                rollback_undo(&written, &executed_renames).await;
                return Err(StructureError::bad_request(format!(
                    "undo transaction aborted (no changes persisted): {e}"
                )));
            }
            written.push(dest.clone());
        }
        if let Some(path) = current_path {
            if current_names.get(&target.number).map(String::as_str)
                != Some(target.file_name.as_str())
            {
                executed_renames.push((dest.clone(), path.clone()));
            }
        }
    }
    for (number, path) in &current_paths {
        if !target_numbers.contains(number) {
            if let Err(e) = tokio::fs::remove_file(path).await {
                rollback_undo(&written, &executed_renames).await;
                return Err(StructureError::bad_request(format!(
                    "undo transaction aborted (no changes persisted): {e}"
                )));
            }
        }
    }

    // index 回位：当前行按号落到目标集（title 保留，字数按恢复内容重算）；
    // 目标有而当前无的号按 before 文件名解析 title 新建行（ReadyForReview 惯例）。
    let title_of = |file_name: &str| -> String {
        let stem = file_name.strip_suffix(".md").unwrap_or(file_name);
        match stem.find('_') {
            Some(dash) => stem[dash + 1..].to_string(),
            None => stem.to_string(),
        }
    };
    let mut next_index: Vec<ChapterMeta> = Vec::new();
    for target in &tail.before {
        if let Some(meta) = index.iter().find(|meta| meta.number == target.number) {
            let mut row = meta.clone();
            let content = resolved.get(&target.number).expect("resolved above");
            row.word_count = crate::utils::language::utf16_len(content.trim()) as u32;
            row.updated_at = now_iso.to_string();
            next_index.push(row);
        } else {
            let content = resolved.get(&target.number).expect("resolved above");
            next_index.push(ChapterMeta {
                number: target.number,
                title: title_of(&target.file_name),
                status: ChapterStatus::ReadyForReview,
                word_count: crate::utils::language::utf16_len(content.trim()) as u32,
                created_at: now_iso.to_string(),
                updated_at: now_iso.to_string(),
                audit_issues: Vec::new(),
                length_warnings: Vec::new(),
                volume_id: None,
                review_note: None,
                detection_score: None,
                detection_provider: None,
                detected_at: None,
                length_telemetry: None,
                token_usage: None,
            });
        }
    }
    next_index.sort_by_key(|meta| meta.number);
    state
        .save_chapter_index_at(&root, &next_index, false)
        .await
        .map_err(|e| StructureError::bad_request(e.to_string()))?;

    if let Err(error) = append_structure_ops(
        &root,
        "undo",
        &current,
        &tail.before,
        now_iso,
        StructureOpsExtra {
            undo_of: Some(tail.op_id.clone()),
            ..Default::default()
        },
    )
    .await
    {
        eprintln!("structure ops log write failed: {error}");
    }
    if let Err(error) = mark_undone(&root, &tail.op_id).await {
        eprintln!("structure ops log mark-undone failed: {error}");
    }

    Ok(StructureOutcome {
        op: "undo",
        book_id: book_id.to_string(),
        split_from: None,
        new_chapter: None,
        merged_into: None,
        removed_chapter: None,
        renumbered: Vec::new(),
        revision: chapter_set_revision(&next_index),
        touched_files: vec!["chapters/index.json".to_string()],
        summary: format!("Undid {} ({}).", tail.kind, tail.op_id),
    })
}

/// undo 事务失败回位：删本轮新写文件 → 逆序撤销已回名。
async fn rollback_undo(written: &[PathBuf], executed_renames: &[(PathBuf, PathBuf)]) {
    for path in written {
        let _ = tokio::fs::remove_file(path).await;
    }
    for (dest, staged_src) in executed_renames.iter().rev() {
        let _ = tokio::fs::rename(dest, staged_src).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// R61 批一/1687 号：不变量纯函数双端向量（与 TS 测试同一份文件）。
    const INVARIANT_VECTORS: &str = include_str!(
        "../../../packages/studio/src/__tests__/golden/structure-invariant-vectors.json"
    );

    #[test]
    fn content_fingerprint_matches_golden_vectors() {
        let vectors: serde_json::Value = serde_json::from_str(INVARIANT_VECTORS).unwrap();
        for vector in vectors["contentFingerprint"].as_array().unwrap() {
            assert_eq!(
                content_fingerprint(vector["content"].as_str().unwrap()),
                vector["fingerprint"].as_str().unwrap(),
                "content {}",
                vector["content"]
            );
        }
    }

    #[test]
    fn structure_invariant_matches_golden_vectors() {
        let vectors: serde_json::Value = serde_json::from_str(INVARIANT_VECTORS).unwrap();
        for vector in vectors["structureInvariant"].as_array().unwrap() {
            let op: StructureOp = serde_json::from_value(vector["op"].clone()).unwrap();
            let before: Vec<ChapterFingerprint> =
                serde_json::from_value(vector["before"].clone()).unwrap();
            let after: Vec<ChapterFingerprint> =
                serde_json::from_value(vector["after"].clone()).unwrap();
            let violations = structure_invariant(&op, &before, &after);
            let expected: Vec<String> = vector["violations"]
                .as_array()
                .unwrap()
                .iter()
                .map(|item| item.as_str().unwrap().to_string())
                .collect();
            assert_eq!(violations, expected, "vector: {}", vector["name"]);
            assert_eq!(
                violations.is_empty(),
                vector["ok"].as_bool().unwrap(),
                "vector: {}",
                vector["name"]
            );
        }
    }

    #[tokio::test]
    async fn structure_ops_log_appends_and_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let book_dir = dir.path().to_path_buf();
        let before = vec![ChapterFingerprint {
            number: 1,
            file_name: "0001_甲.md".into(),
            fingerprint: "fp-1".into(),
        }];
        let after = before.clone();
        let record = append_structure_ops(
            &book_dir,
            "reorder",
            &before,
            &after,
            "2026-11-26T00:00:00.000Z",
            StructureOpsExtra::default(),
        )
        .await
        .unwrap();
        assert!(!record.undone);
        let raw =
            std::fs::read_to_string(book_dir.join("story").join("structure-ops.jsonl")).unwrap();
        let line: StructureOperationRecord = serde_json::from_str(raw.trim()).unwrap();
        assert_eq!(line.kind, "reorder");
        assert!(!line.undone);
        assert_eq!(line.ts, "2026-11-26T00:00:00.000Z");
        assert_eq!(line.before, before);
        assert_eq!(line.after, after);
    }

    #[tokio::test]
    async fn split_appends_structure_ops_log() {
        // 事务级接线咬合：摘除 split 内的守卫/日志接线，本测试与守卫注入测试一起红。
        let dir = tempfile::tempdir().unwrap();
        let state = seed_book(dir.path()).await;
        let original = tokio::fs::read_to_string(dir.path().join("books/b1/chapters/0002_灵脉.md"))
            .await
            .unwrap();
        let revision_before = chapter_set_revision(&state.load_chapter_index("b1").await.unwrap());
        split_chapter(
            &state,
            "b1",
            2,
            8,
            Some(&revision_before),
            1700000000000,
            "2026-11-21T01:00:00.000Z",
        )
        .await
        .expect("split");
        let raw =
            std::fs::read_to_string(dir.path().join("books/b1/story/structure-ops.jsonl")).unwrap();
        let line: StructureOperationRecord = serde_json::from_str(raw.trim()).unwrap();
        assert_eq!(line.kind, "split");
        assert!(!line.undone);
        assert_eq!(
            line.before
                .iter()
                .map(|item| item.number)
                .collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        assert_eq!(
            line.after
                .iter()
                .map(|item| item.number)
                .collect::<Vec<_>>(),
            vec![1, 2, 3, 4]
        );
        // before 指纹 = 操作前原文（拆分截断只是提交后的事）。
        assert_eq!(
            line.before
                .iter()
                .find(|item| item.number == 2)
                .unwrap()
                .fingerprint,
            content_fingerprint(&original)
        );
    }

    #[test]
    fn undo_eligible_matches_golden_vectors() {
        let vectors: serde_json::Value = serde_json::from_str(INVARIANT_VECTORS).unwrap();
        for vector in vectors["undoEligible"].as_array().unwrap() {
            let tail: Option<StructureOperationRecord> = if vector["tail"].is_null() {
                None
            } else {
                Some(serde_json::from_value(vector["tail"].clone()).unwrap())
            };
            let current: Vec<ChapterFingerprint> =
                serde_json::from_value(vector["current"].clone()).unwrap();
            let (eligible, reason) = undo_eligible(tail.as_ref(), &current);
            assert_eq!(
                eligible,
                vector["eligible"].as_bool().unwrap(),
                "vector: {}",
                vector["name"]
            );
            let expected_reason = vector["reason"].as_str();
            assert_eq!(reason, expected_reason, "vector: {}", vector["name"]);
        }
    }

    #[tokio::test]
    async fn undo_undoes_split_and_marks_original() {
        let dir = tempfile::tempdir().unwrap();
        let state = seed_book(dir.path()).await;
        let book_dir = dir.path().join("books/b1");
        let original = tokio::fs::read_to_string(book_dir.join("chapters/0002_灵脉.md"))
            .await
            .unwrap();
        let revision_before = chapter_set_revision(&state.load_chapter_index("b1").await.unwrap());
        split_chapter(
            &state,
            "b1",
            2,
            8,
            Some(&revision_before),
            1700000000000,
            "2026-11-21T01:00:00.000Z",
        )
        .await
        .expect("split");
        let outcome = undo_last_structure_operation(
            &state,
            "b1",
            None,
            1700000001000,
            "2026-11-21T01:10:00.000Z",
        )
        .await
        .expect("undo");
        assert_eq!(outcome.op, "undo");
        // 盘面复原：2 号原文回位、4 号消失、index 号集 [1,2,3]。
        let restored = tokio::fs::read_to_string(book_dir.join("chapters/0002_灵脉.md"))
            .await
            .unwrap();
        assert_eq!(restored, original);
        assert!(!book_dir.join("chapters/0004_收束.md").exists());
        let index = state.load_chapter_index("b1").await.unwrap();
        assert_eq!(
            index.iter().map(|meta| meta.number).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        // 日志：两行——undo 行指向原笔，原笔 undone:true。
        let rows = load_structure_ops(&book_dir).await;
        assert_eq!(rows.len(), 2);
        assert!(rows[0].undone, "原笔应标已撤销");
        assert_eq!(rows[1].kind, "undo");
        assert_eq!(rows[1].undo_of.as_deref(), Some(rows[0].op_id.as_str()));
        assert!(!rows[1].undone);
        assert_eq!(
            rows[1]
                .after
                .iter()
                .map(|item| item.number)
                .collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        // 再撤销 → 拒绝（尾笔是 undo 行）。
        let error = undo_last_structure_operation(
            &state,
            "b1",
            None,
            1700000002000,
            "2026-11-21T01:20:00.000Z",
        )
        .await
        .unwrap_err();
        assert!(
            error.message.contains("last entry is itself an undo"),
            "{error:?}"
        );
    }

    #[tokio::test]
    async fn undo_refuses_without_log_and_after_drift() {
        let dir = tempfile::tempdir().unwrap();
        let state = seed_book(dir.path()).await;
        let error = undo_last_structure_operation(
            &state,
            "b1",
            None,
            1700000000000,
            "2026-11-21T01:00:00.000Z",
        )
        .await
        .unwrap_err();
        assert!(
            error.message.contains("no structure operations logged"),
            "{error:?}"
        );

        let revision_before = chapter_set_revision(&state.load_chapter_index("b1").await.unwrap());
        split_chapter(
            &state,
            "b1",
            2,
            8,
            Some(&revision_before),
            1700000000000,
            "2026-11-21T01:00:00.000Z",
        )
        .await
        .expect("split");
        // 事后编辑：盘面漂移 → after 指纹不符。
        tokio::fs::write(
            dir.path().join("books/b1/chapters/0002_灵脉.md"),
            "# 灵脉\n\n被改过的正文\n",
        )
        .await
        .unwrap();
        let error = undo_last_structure_operation(
            &state,
            "b1",
            None,
            1700000001000,
            "2026-11-21T01:10:00.000Z",
        )
        .await
        .unwrap_err();
        assert!(error.message.contains("disk state changed"), "{error:?}");
    }

    #[tokio::test]
    async fn undo_refuses_naming_chapter_when_no_source() {
        let dir = tempfile::tempdir().unwrap();
        let state = seed_book(dir.path()).await;
        let revision_before = chapter_set_revision(&state.load_chapter_index("b1").await.unwrap());
        split_chapter(
            &state,
            "b1",
            2,
            8,
            Some(&revision_before),
            1700000000000,
            "2026-11-21T01:00:00.000Z",
        )
        .await
        .expect("split");
        // 抹掉来源链：.versions 拆分归档清掉 → 目标指纹无来源。
        tokio::fs::remove_dir_all(dir.path().join("books/b1/chapters/.versions"))
            .await
            .unwrap();
        let error = undo_last_structure_operation(
            &state,
            "b1",
            None,
            1700000001000,
            "2026-11-21T01:10:00.000Z",
        )
        .await
        .unwrap_err();
        assert!(
            error.message.contains("cannot restore chapter"),
            "{error:?}"
        );
        // 零落盘：盘面保持 split 后状态（守卫抛在提交之前）。
        assert!(dir.path().join("books/b1/chapters/0004_收束.md").exists());
    }

    #[test]
    fn invariant_hook_names_the_lost_member() {
        // 故障注入（施工图验收）：after 清单丢一章 → 违反项点名缺失章。
        let before = vec![
            ChapterFingerprint {
                number: 1,
                file_name: "0001_甲.md".into(),
                fingerprint: "fp-1".into(),
            },
            ChapterFingerprint {
                number: 2,
                file_name: "0002_乙.md".into(),
                fingerprint: "fp-2".into(),
            },
            ChapterFingerprint {
                number: 3,
                file_name: "0003_丙.md".into(),
                fingerprint: "fp-3".into(),
            },
        ];
        let after = vec![
            ChapterFingerprint {
                number: 1,
                file_name: "0001_甲.md".into(),
                fingerprint: "fp-1".into(),
            },
            ChapterFingerprint {
                number: 3,
                file_name: "0003_丙.md".into(),
                fingerprint: "fp-3".into(),
            },
        ];
        let violations = structure_invariant(&StructureOp::Reorder, &before, &after);
        assert!(
            violations.iter().any(|item| item.contains("chapter 2")),
            "violations must name the lost member: {violations:?}"
        );
        assert!(!violations.is_empty());
    }

    fn meta(number: u32, title: &str, word_count: u32) -> ChapterMeta {
        ChapterMeta {
            number,
            title: title.to_string(),
            status: ChapterStatus::ReadyForReview,
            word_count,
            created_at: "2026-11-21T00:00:00.000Z".to_string(),
            updated_at: "2026-11-21T00:00:00.000Z".to_string(),
            audit_issues: Vec::new(),
            length_warnings: Vec::new(),
            volume_id: None,
            review_note: None,
            detection_score: None,
            detection_provider: None,
            detected_at: None,
            length_telemetry: None,
            token_usage: None,
        }
    }

    fn index3() -> Vec<ChapterMeta> {
        vec![
            meta(1, "风起", 10),
            meta(2, "灵脉", 20),
            meta(3, "收束", 30),
        ]
    }

    #[test]
    fn revision_stable_and_order_insensitive() {
        let index = index3();
        let a = chapter_set_revision(&index);
        let mut shuffled = index.clone();
        shuffled.reverse();
        assert_eq!(
            a,
            chapter_set_revision(&shuffled),
            "revision 对输入序不敏感（内部按号升序）"
        );
        let mut changed = index.clone();
        changed[1].word_count = 21;
        assert_ne!(a, chapter_set_revision(&changed), "wordCount 变化即变值");
        assert_eq!(a.len(), 16);
        assert_eq!(chapter_set_revision(&[]).len(), 16);
    }

    #[test]
    fn renumber_split_and_merge_plans() {
        assert_eq!(renumber_after_split(&[1, 2, 3], 1), vec![(2, 3), (3, 4)]);
        assert_eq!(
            renumber_after_split(&[1, 2, 3], 3),
            Vec::<(u32, u32)>::new()
        );
        assert_eq!(renumber_after_merge(&[1, 2, 3], 2), vec![(3, 2)]);
        assert_eq!(
            renumber_after_merge(&[1, 2, 3], 3),
            Vec::<(u32, u32)>::new()
        );
    }

    #[test]
    fn reorder_plan_requires_permutation() {
        assert_eq!(
            reorder_plan(&[1, 2, 3], &[3, 1, 2]).unwrap(),
            vec![(1, 2), (2, 3), (3, 1)]
        );
        assert_eq!(reorder_plan(&[1, 2, 3], &[1, 2, 3]).unwrap(), vec![]);
        assert!(reorder_plan(&[1, 2, 3], &[1, 2]).is_err());
        assert!(reorder_plan(&[1, 2, 3], &[1, 2, 2]).is_err());
        assert!(reorder_plan(&[1, 2, 3], &[1, 2, 4]).is_err());
    }

    #[test]
    fn split_utf16_boundaries_and_surrogate_reject() {
        let content = "ab龙c\u{1F420}尾".to_string(); // 🐠 = 2 units，总 7
        assert_eq!(content.encode_utf16().count(), 7);
        let (head, tail) = split_utf16(&content, 2).unwrap();
        assert_eq!(head, "ab");
        assert_eq!(tail, "龙c🐠尾");
        // 落在 🐠 双 unit 内（unit=5）→ 非法（代理对中间）。
        assert!(split_utf16(&content, 5).is_err());
        let (head, tail) = split_utf16(&content, 4).unwrap();
        assert_eq!(head, "ab龙c");
        assert_eq!(tail, "🐠尾");
        assert!(split_utf16(&content, 0).is_err());
        assert!(split_utf16(&content, 7).is_err());
    }

    #[test]
    fn merge_content_separator_variants() {
        assert_eq!(merge_content("甲。", "乙。", "\n\n"), "甲。\n\n乙。");
        assert_eq!(merge_content("甲。", "乙。", ""), "甲。乙。");
    }

    // ── 事务（tempdir fixture）────────────────────────────────────

    async fn seed_book(root: &std::path::Path) -> StateManager {
        let book_dir = root.join("books").join("b1");
        tokio::fs::create_dir_all(book_dir.join("chapters"))
            .await
            .unwrap();
        tokio::fs::create_dir_all(book_dir.join("story").join("runtime"))
            .await
            .unwrap();
        tokio::fs::create_dir_all(book_dir.join("story").join("drafts"))
            .await
            .unwrap();
        for (number, title, body) in [
            (1u32, "风起", "第一章正文甲。"),
            (2, "灵脉", "第二章正文乙。"),
            (3, "收束", "第三章正文丙。"),
        ] {
            tokio::fs::write(
                book_dir
                    .join("chapters")
                    .join(format!("{number:04}_{title}.md")),
                format!("# {title}\n\n{body}\n"),
            )
            .await
            .unwrap();
        }
        tokio::fs::write(
            book_dir
                .join("story")
                .join("runtime")
                .join("chapter-0002.plan.md"),
            "二号章的规划产物\n",
        )
        .await
        .unwrap();
        tokio::fs::write(
            book_dir
                .join("story")
                .join("drafts")
                .join("0002_灵脉草稿.md"),
            "二号章草稿\n",
        )
        .await
        .unwrap();
        tokio::fs::write(
            book_dir.join("chapters").join("index.json"),
            serde_json::to_string(&index3()).unwrap(),
        )
        .await
        .unwrap();
        StateManager::new(root.to_path_buf())
    }

    #[tokio::test]
    async fn split_renames_renumbers_migrates_artifacts_and_archives() {
        let dir = tempfile::tempdir().unwrap();
        let state = seed_book(dir.path()).await;
        let revision_before = chapter_set_revision(&state.load_chapter_index("b1").await.unwrap());

        let outcome = split_chapter(
            &state,
            "b1",
            2,
            8,
            Some(&revision_before),
            1700000000000,
            "2026-11-21T01:00:00.000Z",
        )
        .await
        .expect("split");
        assert_eq!(outcome.op, "split");
        assert_eq!(outcome.split_from, Some(2));
        assert_eq!(outcome.new_chapter, Some(3));
        assert_eq!(
            outcome
                .renumbered
                .iter()
                .map(|e| (e.from, e.to))
                .collect::<Vec<_>>(),
            vec![(3, 4)]
        );
        assert_ne!(outcome.revision, revision_before);
        assert_eq!(
            outcome.revision,
            chapter_set_revision(&state.load_chapter_index("b1").await.unwrap())
        );

        let chapters = dir.path().join("books/b1/chapters");
        let tail = tokio::fs::read_to_string(chapters.join("0003_灵脉.md"))
            .await
            .unwrap();
        let moved = tokio::fs::read_to_string(chapters.join("0004_收束.md"))
            .await
            .unwrap();
        assert!(
            tail.starts_with("章正文乙。"),
            "新章=被拆后段（offset 切在第二|章间）"
        );
        assert!(moved.contains("正文丙。"));
        assert!(!chapters.join("0003_收束.md").exists(), "旧 3 号位让位");
        // runtime/drafts 产物原位（2 号不变号）。
        assert!(dir
            .path()
            .join("books/b1/story/runtime/chapter-0002.plan.md")
            .exists());
        assert!(dir
            .path()
            .join("books/b1/story/drafts/0002_灵脉草稿.md")
            .exists());
        // 归档：旧号下旧文 + 新号下后段（structure 来源）。
        let versions = chapters.join(".versions");
        assert!(versions.join("0002").read_dir().unwrap().count() >= 1);
        assert!(versions.join("0003").read_dir().unwrap().count() >= 1);
        assert!(
            !chapters.read_dir().unwrap().any(|entry| entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".tmp-structure")),
            "staging 清理（per-op 唯一名无残留）"
        );
        // 索引：4 行（1/2/3/4），3 号=拆出新章（同 title）、4 号=原 3 号。
        let index = state.load_chapter_index("b1").await.unwrap();
        assert_eq!(
            index.iter().map(|m| m.number).collect::<Vec<_>>(),
            vec![1, 2, 3, 4]
        );
        assert_eq!(index[2].title, "灵脉");
        assert_eq!(index[3].title, "收束");
        assert!(index[2].word_count > 0 && index[1].word_count > 0);
    }

    #[tokio::test]
    async fn split_rejects_bad_offset_and_revision_conflict() {
        let dir = tempfile::tempdir().unwrap();
        let state = seed_book(dir.path()).await;
        let err = split_chapter(&state, "b1", 2, 0, None, 1, "now")
            .await
            .unwrap_err();
        assert_eq!(err.status, 400);
        let content_len =
            tokio::fs::read_to_string(dir.path().join("books/b1/chapters/0002_灵脉.md"))
                .await
                .unwrap()
                .encode_utf16()
                .count();
        let err = split_chapter(&state, "b1", 2, content_len, None, 1, "now")
            .await
            .unwrap_err();
        assert_eq!(err.status, 400);
        // 乐观并发：expectedRevision 不符 → 409（缺省=跳过校验）。
        let err = split_chapter(&state, "b1", 2, 4, Some("deadbeefdeadbeef"), 1, "now")
            .await
            .unwrap_err();
        assert_eq!(err.status, 409);
        assert_eq!(err.code, Some(REVISION_CONFLICT_CODE));
        assert_eq!(
            state.load_chapter_index("b1").await.unwrap().len(),
            3,
            "冲突路径零变更"
        );
    }

    #[tokio::test]
    async fn merge_adjacent_trashes_removed_and_shifts() {
        let dir = tempfile::tempdir().unwrap();
        let state = seed_book(dir.path()).await;
        let outcome = merge_chapter(
            &state,
            "b1",
            2,
            3,
            Some("\n\n"),
            None,
            1700000000000,
            "2026-11-21T01:00:00.000Z",
        )
        .await
        .expect("merge");
        assert_eq!(outcome.op, "merge");
        assert_eq!(outcome.merged_into, Some(2));
        assert_eq!(outcome.removed_chapter, Some(3));
        let chapters = dir.path().join("books/b1/chapters");
        let merged = tokio::fs::read_to_string(chapters.join("0002_灵脉.md"))
            .await
            .unwrap();
        assert!(merged.contains("正文乙。") && merged.contains("正文丙。"));
        assert!(!chapters.join("0003_收束.md").exists());
        assert!(
            chapters.join(".trash/0003_收束.md").exists(),
            "被删章正文进 .trash"
        );
        let index = state.load_chapter_index("b1").await.unwrap();
        assert_eq!(
            index.iter().map(|m| m.number).collect::<Vec<_>>(),
            vec![1, 2]
        );
        // 非相邻拒绝。
        let err = merge_chapter(&state, "b1", 1, 3, None, None, 1, "now")
            .await
            .unwrap_err();
        assert_eq!(err.status, 400);
    }

    /// 1647 号回归：merge 触发重编号路径（removed 之后仍有章）时索引不得丢
    /// 章——此前「先映射后过滤」把重编后恰好落到 removed 号的行一并滤杀
    /// （removed=2 时 3→2 的重编行被滤，index 丢收束章而文件面在位=
    /// 双面发散；live 逐功能「拆→并→集合复原」断言逼出）。
    #[tokio::test]
    async fn merge_with_renumber_keeps_shifted_chapter_in_index() {
        let dir = tempfile::tempdir().unwrap();
        let state = seed_book(dir.path()).await;
        // merge 2 into 1：removed=2、keep=1、3→2 重编号路径。
        let outcome = merge_chapter(
            &state,
            "b1",
            2,
            1,
            Some("\n\n"),
            None,
            1700000000000,
            "2026-11-21T01:00:00.000Z",
        )
        .await
        .expect("merge");
        assert_eq!(outcome.merged_into, Some(1));
        assert_eq!(outcome.removed_chapter, Some(2));
        assert_eq!(
            outcome
                .renumbered
                .iter()
                .map(|e| (e.from, e.to))
                .collect::<Vec<_>>(),
            vec![(3, 2)]
        );
        let index = state.load_chapter_index("b1").await.unwrap();
        assert_eq!(
            index.iter().map(|m| m.number).collect::<Vec<_>>(),
            vec![1, 2],
            "重编章必须保留在索引（修复前只剩 [1]）"
        );
        assert_eq!(
            index.iter().map(|m| m.title.as_str()).collect::<Vec<_>>(),
            vec!["风起", "收束"]
        );
        let chapters = dir.path().join("books/b1/chapters");
        assert!(chapters.join("0002_收束.md").exists(), "重编号文件在位");
        assert!(!chapters.join("0003_收束.md").exists());
        assert!(chapters.join(".trash/0002_灵脉.md").exists());
    }

    /// 1666 号（章节结构对跑真执行逼出）：被并章以换行收尾（写入惯例）时，
    /// 合并产物恒以单个换行收尾——此前只剪被并章头部，其尾部换行与补的收尾
    /// 换行叠加成一个多余空行。
    #[tokio::test]
    async fn merge_output_ends_with_exactly_one_newline() {
        for (removed_body, expected) in [
            (
                "# 收束\n\n第三章正文丙。\n",
                "# 灵脉\n\n第二章正文乙。\n\n# 收束\n\n第三章正文丙。\n",
            ),
            (
                "# 收束\n\n第三章正文丙。",
                "# 灵脉\n\n第二章正文乙。\n\n# 收束\n\n第三章正文丙。\n",
            ),
            ("  \n", "# 灵脉\n\n第二章正文乙。\n"),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let state = seed_book(dir.path()).await;
            let chapters = dir.path().join("books/b1/chapters");
            tokio::fs::write(chapters.join("0003_收束.md"), removed_body)
                .await
                .unwrap();
            merge_chapter(
                &state,
                "b1",
                2,
                3,
                None,
                None,
                1700000000000,
                "2026-11-21T01:00:00.000Z",
            )
            .await
            .expect("merge");
            let merged = tokio::fs::read_to_string(chapters.join("0002_灵脉.md"))
                .await
                .unwrap();
            assert_eq!(merged, expected, "被并章 = {removed_body:?}");
        }
    }

    /// 1666 号：重编号只改名——未被拆分的后续章正文逐字节不动。
    #[tokio::test]
    async fn split_renumber_keeps_shifted_chapter_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let state = seed_book(dir.path()).await;
        let chapters = dir.path().join("books/b1/chapters");
        let original = "# 收束\n\n第三章正文丙。";
        tokio::fs::write(chapters.join("0003_收束.md"), original)
            .await
            .unwrap();
        split_chapter(
            &state,
            "b1",
            2,
            6,
            None,
            1700000000000,
            "2026-11-21T01:00:00.000Z",
        )
        .await
        .expect("split");
        assert_eq!(
            tokio::fs::read_to_string(chapters.join("0004_收束.md"))
                .await
                .unwrap(),
            original
        );
        // 被拆章自身的两段照旧各以单个换行收尾。
        assert_eq!(
            tokio::fs::read_to_string(chapters.join("0002_灵脉.md"))
                .await
                .unwrap(),
            "# 灵脉\n\n"
        );
        assert_eq!(
            tokio::fs::read_to_string(chapters.join("0003_灵脉.md"))
                .await
                .unwrap(),
            "第二章正文乙。\n"
        );
    }

    #[tokio::test]
    async fn reorder_permutes_files_and_index() {
        let dir = tempfile::tempdir().unwrap();
        let state = seed_book(dir.path()).await;
        let outcome = reorder_chapters(&state, "b1", &[3, 1, 2], None, "2026-11-21T01:00:00.000Z")
            .await
            .expect("reorder");
        assert_eq!(
            outcome
                .renumbered
                .iter()
                .map(|e| (e.from, e.to))
                .collect::<Vec<_>>(),
            vec![(1, 2), (2, 3), (3, 1)]
        );
        let chapters = dir.path().join("books/b1/chapters");
        assert!(chapters.join("0001_收束.md").exists());
        assert!(chapters.join("0002_风起.md").exists());
        assert!(chapters.join("0003_灵脉.md").exists());
        let index = state.load_chapter_index("b1").await.unwrap();
        assert_eq!(
            index.iter().map(|m| m.title.as_str()).collect::<Vec<_>>(),
            vec!["收束", "风起", "灵脉"]
        );
        let err = reorder_chapters(&state, "b1", &[1, 2], None, "now")
            .await
            .unwrap_err();
        assert_eq!(err.status, 400);
    }

    /// 1649 号回归（IAB 走查逼出）：reorder 含恒等成员的排列——恒等章
    /// 文件必须原位保留。此前 staging 收走全部章而 rename 循环只回位变号
    /// 章，恒等章滞留 staging 被成功路径 remove_dir_all **永久删除**
    /// （[1,3,2,4,5] 后 0001/0004/0005 静默消失、index 却全在=数据丢失）。
    /// 既有测试全用全员变号排列，恒等成员从未被测。
    #[tokio::test]
    async fn reorder_with_identity_members_keeps_unchanged_files() {
        let dir = tempfile::tempdir().unwrap();
        let state = seed_book(dir.path()).await;
        // [1,3,2]：章 1 恒等（风起不动），2/3 交换。
        let outcome = reorder_chapters(&state, "b1", &[1, 3, 2], None, "2026-11-22T00:00:00.000Z")
            .await
            .expect("reorder");
        assert_eq!(
            outcome
                .renumbered
                .iter()
                .map(|e| (e.from, e.to))
                .collect::<Vec<_>>(),
            vec![(2, 3), (3, 2)]
        );
        let chapters = dir.path().join("books/b1/chapters");
        // 恒等章文件原位（修复前被 staging 永删）。
        assert!(
            chapters.join("0001_风起.md").exists(),
            "恒等章 1 文件必须保留"
        );
        // 变号章交换落位。
        assert!(chapters.join("0002_收束.md").exists());
        assert!(chapters.join("0003_灵脉.md").exists());
        let index = state.load_chapter_index("b1").await.unwrap();
        assert_eq!(
            index.iter().map(|m| m.title.as_str()).collect::<Vec<_>>(),
            vec!["风起", "收束", "灵脉"]
        );
        // 二次 reorder（再交换 1/2）可行——修复前此时报 Chapter file not found
        // （首轮恒等章 1 文件已被 staging 永删，collect_chapter_files 404）。
        let again =
            reorder_chapters(&state, "b1", &[2, 1, 3], None, "2026-11-22T00:00:00.000Z").await;
        assert!(again.is_ok(), "二次 reorder 不应因文件缺失失败: {again:?}");
    }

    /// 事务中断原子性（1646 号审查修复后的活体注入）：新章回写位被预放
    /// **同名目录**（同 title 撞名，candidate_wins 择文件为胜者→staging 不收
    /// 目录）→ 新章覆写 write_file_atomic(目录位) 失败 → 中断 → restore
    /// 回位 → 零落盘（索引未写、章文件回原位、产物原位、staging 无残留）。
    /// 旧固定名预占注入在 per-op UUID 名下失效，故换此确定性注入。
    #[tokio::test]
    async fn split_aborts_atomically_when_new_chapter_slot_occupied() {
        let dir = tempfile::tempdir().unwrap();
        let state = seed_book(dir.path()).await;
        // 注入：3 号新章回写位（0003_灵脉.md，与 2 号章同 title=拆分新章名）
        // 预放为目录——candidate_wins 择 0003_收束.md 文件为 3 号胜者，
        // 该目录不被 staging 收走，覆写必失败。
        let chapters = dir.path().join("books/b1/chapters");
        tokio::fs::create_dir(chapters.join("0003_灵脉.md"))
            .await
            .unwrap();
        let index_before = state.load_chapter_index("b1").await.unwrap();

        let err = split_chapter(&state, "b1", 2, 8, None, 1, "now")
            .await
            .unwrap_err();
        assert_eq!(err.status, 400);
        assert!(
            err.message.contains("aborted"),
            "中断语义文案: {}",
            err.message
        );

        // restore 完整性：受影响章回原位、索引零变更、产物原位、staging 无残留。
        assert!(chapters.join("0002_灵脉.md").exists(), "2 号章回原位");
        assert!(chapters.join("0003_收束.md").exists(), "3 号章回原位");
        let index_after = state.load_chapter_index("b1").await.unwrap();
        assert_eq!(
            chapter_set_revision(&index_before),
            chapter_set_revision(&index_after),
            "索引零变更"
        );
        let runtime_listing: Vec<String> =
            std::fs::read_dir(dir.path().join("books/b1/story/runtime"))
                .unwrap()
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect();
        eprintln!(
            "DEBUG runtime={runtime_listing:?} staging_exists={}",
            chapters.read_dir().unwrap().any(|e| e
                .ok()
                .map(|e| e
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".tmp-structure"))
                .unwrap_or(false))
        );
        assert!(
            dir.path()
                .join("books/b1/story/runtime/chapter-0002.plan.md")
                .exists(),
            "产物原位"
        );
        assert!(
            !chapters.read_dir().unwrap().any(|entry| entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".tmp-structure")),
            "staging 无残留（per-op 唯一名回收）"
        );
    }
}
