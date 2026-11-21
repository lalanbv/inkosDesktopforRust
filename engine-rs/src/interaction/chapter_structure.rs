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
    state
        .save_chapter_index_at(&root, &next_index, false)
        .await
        .map_err(|e| StructureError::bad_request(e.to_string()))?;

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
        match write_file_atomic(&keep_file.path, &format!("{merged}\n")).await {
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
    state
        .save_chapter_index_at(&root, &next_index, false)
        .await
        .map_err(|e| StructureError::bad_request(e.to_string()))?;

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
    let files = collect_chapter_files(&chapters_dir, &numbers, &index).await?;
    let artifacts = collect_artifact_files(&root, &numbers).await;
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
    state
        .save_chapter_index_at(&root, &next_index, false)
        .await
        .map_err(|e| StructureError::bad_request(e.to_string()))?;

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

#[cfg(test)]
mod tests {
    use super::*;

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
