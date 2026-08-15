//! 删除最新章（chapter-delete）。
//!
//! 移植自 `packages/core/src/state/chapter-delete.ts`（117 行）。只有最新章
//! 可删：正文移入 `chapters/.trash/`（永不硬删），随后经审核 reject 同款
//! 回滚链（快照恢复 → 后续章/草稿/快照/runtime 工件清除 → 索引回写）回滚
//! 到上一章。快照可用性在动任何文件**之前**校验——失败时书籍零变更。

use std::path::Path;

use regex::Regex;
use serde::Serialize;
use std::sync::OnceLock;

use crate::state::manager::StateManager;

/// 请求删除的章节号。`NaN` 变体对齐 TS `parseInt` 失败时 NaN !== 任何数
/// 的比较语义（必然落「只能删最新章」错误，文案显示 NaN）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DeleteRequest {
    /// 缺省：删最新章。
    Latest,
    Chapter(i64),
    NaN,
}

/// 删除结果。对齐 TS `DeleteLatestChapterResult`（camelCase 序列化）。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteLatestChapterResult {
    pub book_id: String,
    pub deleted_chapter: u32,
    pub title: String,
    /// 保留在 `chapters/.trash/` 下的书内相对 POSIX 路径。
    pub trashed_files: Vec<String>,
    pub rolled_back_to: i64,
    pub discarded: Vec<u32>,
}

/// 删除书籍的最新章。对齐 TS `deleteLatestChapter`（错误文案逐字）。
pub async fn delete_latest_chapter(
    state: &StateManager,
    book_id: &str,
    request: DeleteRequest,
) -> Result<DeleteLatestChapterResult, String> {
    let index = state.load_chapter_index(book_id).await.map_err(|e| e.to_string())?;
    if index.is_empty() {
        return Err(format!("Book \"{book_id}\" has no chapters to delete."));
    }
    let latest = index.iter().map(|m| m.number as i64).max().unwrap_or(0);
    let allowed = match request {
        DeleteRequest::Latest => true,
        DeleteRequest::Chapter(n) => n == latest,
        DeleteRequest::NaN => false,
    };
    if !allowed {
        let requested = match request {
            DeleteRequest::Chapter(n) => n.to_string(),
            _ => "NaN".to_string(),
        };
        return Err(format!(
            "Only the latest chapter ({latest}) can be deleted, but chapter {requested} was requested. \
Deleting a middle chapter would require renumbering later chapters and replaying state."
        ));
    }

    let book_dir = state.book_dir(book_id);
    let rollback_target = latest - 1;

    // 快照可用性前置校验：失败时不动任何文件。
    for required in ["current_state.md", "pending_hooks.md"] {
        let snapshot_file = book_dir
            .join("story")
            .join("snapshots")
            .join(rollback_target.to_string())
            .join(required);
        if tokio::fs::metadata(&snapshot_file).await.is_err() {
            return Err(format!(
                "Cannot delete chapter {latest}: the state snapshot for chapter {rollback_target} is missing \
(story/snapshots/{rollback_target}/{required}). Nothing was changed."
            ));
        }
    }

    // 最新章正文移入 .trash（重名追加 -2/-3… 后缀）。
    let chapters_dir = book_dir.join("chapters");
    let trash_dir = chapters_dir.join(".trash");
    let mut chapter_files: Vec<String> = Vec::new();
    if let Ok(mut entries) = tokio::fs::read_dir(&chapters_dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Some(number) = chapter_number_from_file(&name) {
                if number == latest {
                    chapter_files.push(name);
                }
            }
        }
    }
    if !chapter_files.is_empty() {
        tokio::fs::create_dir_all(&trash_dir)
            .await
            .map_err(|e| e.to_string())?;
    }
    let mut trashed_files: Vec<String> = Vec::new();
    for file in chapter_files {
        let trashed_name = pick_available_name(&trash_dir, &file).await;
        tokio::fs::rename(chapters_dir.join(&file), trash_dir.join(&trashed_name))
            .await
            .map_err(|e| e.to_string())?;
        trashed_files.push(format!("chapters/.trash/{trashed_name}"));
    }

    let discarded = state
        .rollback_to_chapter(book_id, rollback_target as u32)
        .await?;
    let title = index
        .iter()
        .find(|m| m.number as i64 == latest)
        .map(|m| m.title.clone())
        .unwrap_or_else(|| format!("第{latest}章"));

    Ok(DeleteLatestChapterResult {
        book_id: book_id.to_string(),
        deleted_chapter: latest as u32,
        title,
        trashed_files,
        rolled_back_to: rollback_target,
        discarded,
    })
}

/// 章节文件名前缀解析：`^([0-9]+)[_-]?.*\.md$`（对齐 TS 正则；`\d` 限定
/// ASCII 以避免 Rust 正则的 Unicode 数字类偏差）。
fn chapter_number_from_file(name: &str) -> Option<i64> {
    let caps = delete_file_re().captures(name)?;
    caps[1].parse::<i64>().ok()
}

fn delete_file_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^([0-9]+)[_-]?.*\.md$").expect("delete file regex"))
}

/// 重名规避：`base-2.ext`、`base-3.ext`…（对齐 TS `pickAvailableName`）。
async fn pick_available_name(dir: &Path, file_name: &str) -> String {
    let (base, ext) = match file_name.rfind('.') {
        Some(dot) => (&file_name[..dot], &file_name[dot..]),
        None => (file_name, ""),
    };
    let mut candidate = file_name.to_string();
    let mut suffix = 2;
    while tokio::fs::try_exists(dir.join(&candidate)).await.unwrap_or(false) {
        candidate = format!("{base}-{suffix}{ext}");
        suffix += 1;
    }
    candidate
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(root: &Path) -> StateManager {
        StateManager::new(root.to_path_buf())
    }

    async fn fixture(root: &Path) {
        let book = root.join("books").join("b1");
        let story = book.join("story");
        tokio::fs::create_dir_all(book.join("chapters")).await.unwrap();
        tokio::fs::create_dir_all(story.join("runtime")).await.unwrap();
        tokio::fs::create_dir_all(story.join("snapshots").join("1")).await.unwrap();
        tokio::fs::write(book.join("chapters").join("0001_风起.md"), "# 第1章").await.unwrap();
        tokio::fs::write(book.join("chapters").join("0002_云涌.md"), "# 第2章").await.unwrap();
        tokio::fs::write(story.join("current_state.md"), "状态v1").await.unwrap();
        tokio::fs::write(story.join("pending_hooks.md"), "| 伏笔 |").await.unwrap();
        tokio::fs::write(story.join("snapshots").join("1").join("current_state.md"), "快照v1").await.unwrap();
        tokio::fs::write(story.join("snapshots").join("1").join("pending_hooks.md"), "钩子v1").await.unwrap();
        tokio::fs::write(story.join("runtime").join("chapter-0002.plan.md"), "plan2").await.unwrap();
        let now = "2026-01-01T00:00:00.000Z";
        let meta = |number: u32| {
            serde_json::json!({
                "number": number, "title": format!("第{number}章"), "status": "ready-for-review",
                "wordCount": 4, "auditIssues": [], "lengthWarnings": [],
                "createdAt": now, "updatedAt": now,
            })
        };
        tokio::fs::write(
            book.join("chapters").join("index.json"),
            serde_json::to_string(&vec![meta(1), meta(2)]).unwrap(),
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn deletes_latest_trashes_file_and_rolls_back() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture(&root).await;
        let book = root.join("books").join("b1");

        let result = delete_latest_chapter(&state(&root), "b1", DeleteRequest::Latest)
            .await
            .unwrap();
        assert_eq!(result.deleted_chapter, 2);
        assert_eq!(result.title, "第2章");
        assert_eq!(result.rolled_back_to, 1);
        assert_eq!(result.discarded, vec![2]);
        assert_eq!(result.trashed_files, vec!["chapters/.trash/0002_云涌.md".to_string()]);
        // 正文进 .trash、原位消失；快照恢复；runtime 工件清除。
        assert!(book.join("chapters").join(".trash").join("0002_云涌.md").exists());
        assert!(!book.join("chapters").join("0002_云涌.md").exists());
        assert_eq!(
            tokio::fs::read_to_string(book.join("story").join("current_state.md")).await.unwrap(),
            "快照v1"
        );
        assert!(!book.join("story").join("runtime").join("chapter-0002.plan.md").exists());
        // 索引只剩第 1 章。
        let index: Vec<serde_json::Value> = serde_json::from_str(
            &tokio::fs::read_to_string(book.join("chapters").join("index.json")).await.unwrap(),
        )
        .unwrap();
        assert_eq!(index.len(), 1);
        assert_eq!(index[0]["number"], 1);
    }

    #[tokio::test]
    async fn trash_collision_appends_suffix() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture(&root).await;
        let trash = root.join("books").join("b1").join("chapters").join(".trash");
        tokio::fs::create_dir_all(&trash).await.unwrap();
        tokio::fs::write(trash.join("0002_云涌.md"), "旧废稿").await.unwrap();

        let result = delete_latest_chapter(&state(&root), "b1", DeleteRequest::Latest)
            .await
            .unwrap();
        assert_eq!(result.trashed_files[0], "chapters/.trash/0002_云涌-2.md");
        assert!(trash.join("0002_云涌-2.md").exists());
        assert_eq!(
            tokio::fs::read_to_string(trash.join("0002_云涌.md")).await.unwrap(),
            "旧废稿"
        );
    }

    #[tokio::test]
    async fn middle_chapter_and_nan_rejected_with_exact_messages() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture(&root).await;
        let err = delete_latest_chapter(&state(&root), "b1", DeleteRequest::Chapter(1))
            .await
            .unwrap_err();
        assert_eq!(
            err,
            "Only the latest chapter (2) can be deleted, but chapter 1 was requested. \
Deleting a middle chapter would require renumbering later chapters and replaying state."
        );
        let err = delete_latest_chapter(&state(&root), "b1", DeleteRequest::NaN)
            .await
            .unwrap_err();
        assert!(err.contains("but chapter NaN was requested."), "{err}");
        // 中间章请求失败后文件原样。
        assert!(root.join("books").join("b1").join("chapters").join("0001_风起.md").exists());
    }

    #[tokio::test]
    async fn missing_snapshot_aborts_before_any_change() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture(&root).await;
        let book = root.join("books").join("b1");
        tokio::fs::remove_file(book.join("story").join("snapshots").join("1").join("pending_hooks.md"))
            .await
            .unwrap();
        let err = delete_latest_chapter(&state(&root), "b1", DeleteRequest::Latest)
            .await
            .unwrap_err();
        assert_eq!(
            err,
            "Cannot delete chapter 2: the state snapshot for chapter 1 is missing \
(story/snapshots/1/pending_hooks.md). Nothing was changed."
        );
        assert!(book.join("chapters").join("0002_云涌.md").exists());
        assert!(!book.join("chapters").join(".trash").exists());
    }

    #[tokio::test]
    async fn empty_book_error() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        tokio::fs::create_dir_all(root.join("books").join("b1").join("chapters")).await.unwrap();
        let err = delete_latest_chapter(&state(&root), "b1", DeleteRequest::Latest)
            .await
            .unwrap_err();
        assert_eq!(err, "Book \"b1\" has no chapters to delete.");
    }
}
