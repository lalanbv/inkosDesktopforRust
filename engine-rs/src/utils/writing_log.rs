//! R51/1653 号：日产出账本存储（append-only jsonl，TS 镜像 =
//! packages/core/src/utils/writing-log-store.ts）。
//!
//! 路径 = `{book_dir}/.inkos/writing-log.jsonl`（书级 .inkos 子目录——双根
//! 对称：Rust book_dir=books/{id}、TS bookDir=works/{id}/source）；行 =
//! `WritingLogRow` 四键（ts=UTC ISO）。追加语义 = 读旧 + 拼接 + 原子写；
//! 读取宽容（坏行跳过）。仅元数据（无正文=隐私红线）。

use std::path::{Path, PathBuf};

use crate::utils::writing_goal::WritingLogRow;

pub fn writing_log_path(book_dir: &Path) -> PathBuf {
    book_dir.join(".inkos").join("writing-log.jsonl")
}

/// 读账本（无文件=空账本；坏行跳过——jsonl 宽容读）。
pub async fn load_writing_log(book_dir: &Path) -> Vec<WritingLogRow> {
    match tokio::fs::read_to_string(writing_log_path(book_dir)).await {
        Ok(text) => text
            .lines()
            .filter_map(|line| {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    serde_json::from_str(trimmed).ok()
                }
            })
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// 追加账本行（读旧+拼接+原子写；空批次 no-op）。
pub async fn append_writing_log(book_dir: &Path, rows: &[WritingLogRow]) -> Result<(), String> {
    if rows.is_empty() {
        return Ok(());
    }
    let mut lines: Vec<String> = load_writing_log(book_dir)
        .await
        .iter()
        .map(|row| serde_json::to_string(row).unwrap_or_default())
        .collect();
    for row in rows {
        lines.push(serde_json::to_string(row).unwrap_or_default());
    }
    let path = writing_log_path(book_dir);
    if let Some(parent) = path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    let text = lines.join("\n") + "\n";
    crate::utils::atomic_file_set::write_file_atomic(&path, &text)
        .await
        .map_err(|e| e.to_string())
}

/// 写点辅助：wordCount 前后对比 → 差异行（Δ≠0 才记；纯保存零变化不记）。
/// 事务成功路径尾部 best-effort 追加（账本为统计面，失败不 fail 保存事务）。
pub async fn record_word_count_delta(
    book_dir: &Path,
    chapter_number: u32,
    before: i64,
    after: i64,
    ts_iso: &str,
) {
    if before == after {
        return;
    }
    let row = WritingLogRow {
        ts: ts_iso.to_string(),
        chapter_num: chapter_number,
        before,
        after,
    };
    let _ = append_writing_log(book_dir, std::slice::from_ref(&row)).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn append_then_load_roundtrip_and_delta_gate() {
        let dir = tempfile::tempdir().unwrap();
        let book_dir = dir.path();
        // 无文件=空账本。
        assert!(load_writing_log(book_dir).await.is_empty());
        // Δ=0 不记。
        record_word_count_delta(book_dir, 1, 500, 500, "2026-11-22T00:00:00.000Z").await;
        assert!(load_writing_log(book_dir).await.is_empty());
        // Δ≠0 记 + 坏行宽容读。
        record_word_count_delta(book_dir, 1, 500, 800, "2026-11-22T01:00:00.000Z").await;
        record_word_count_delta(book_dir, 2, 0, 300, "2026-11-22T02:00:00.000Z").await;
        let rows = load_writing_log(book_dir).await;
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].chapter_num, 1);
        assert_eq!(rows[0].after - rows[0].before, 300);
        // 追加语义（读旧+拼接）。
        record_word_count_delta(book_dir, 1, 800, 700, "2026-11-22T03:00:00.000Z").await;
        assert_eq!(load_writing_log(book_dir).await.len(), 3);
    }
}
