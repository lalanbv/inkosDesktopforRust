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
}
