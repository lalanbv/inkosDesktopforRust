//! 章节文件名前导号解析与同号定位——`packages/core/src/utils/chapter-files.ts`
//!（`parseChapterFileName` / `findChapterFileByNumber`）镜像。
//!
//! 682 号在 TS 侧确立「能列出（索引宽松侧）即可打开（详情侧）」单一事实源：
//! 引擎写入侧恒 4 位补零（`NNNN_title.md`），但导入/手工放置的章节文件位数
//! 不限；读侧按「前导号数值等值」定位。旧读侧 `startsWith(补零串)` 的前缀碰撞
//!（num=7 误配 `00071_`）与「列表可见、详情 404」（`7_手记.md`）按数值等值在此
//! 根除——687 号把引擎读侧消费点（章节读取/审计/修稿/检测/workspace 等 8 处）
//! 统一到本模块；845 号补齐漏网的 book_eval / continuity / planning_materials /
//! edit_controller 四处 `starts_with(padded)` 读侧残留。
//!
//! 845 号：同号多形态择优与索引重建侧（`state::manager::select_chapter_files`）合并为
//! 唯一裁决 [`manager::chapter_file_candidate_wins`]（正典 `NNNN_` 优先，同类内字节序
//! 小者）——此前本模块按 padded-4 类内字典序取 `'-'<'_'` 的连字符旧章，与索引侧正典
//! 优先互斥，同一本书出现「索引列 `0001_新`、详情开 `0001-旧`」缺口。正则亦归源到
//! `manager::leading_number`，本模块不再持有私有拷贝。

use crate::state::manager::{chapter_file_candidate_wins, leading_number};
use std::path::Path;

/// TS `parseChapterFileName(...)?.number`：前导号数值（须 > 0）；不匹配 / 0 号返回 `None`。
pub fn parse_chapter_file_number(file_name: &str) -> Option<u32> {
    leading_number(file_name).filter(|number| *number > 0)
}

/// TS `findChapterFileByNumber`：数值等值候选按 [`chapter_file_candidate_wins`] 择一
///（与 `select_chapter_files` 对该章的胜者恒等）；单趟零中间分配，仅在命中时克隆一次。
pub fn find_chapter_file_by_number(files: &[String], chapter_number: u32) -> Option<String> {
    if chapter_number == 0 {
        return None;
    }
    let mut best: Option<&str> = None;
    for file in files {
        if parse_chapter_file_number(file) != Some(chapter_number) {
            continue;
        }
        if best.is_none_or(|current| chapter_file_candidate_wins(current, file)) {
            best = Some(file.as_str());
        }
    }
    best.map(str::to_owned)
}

/// 读 `chapters_dir` 并按数值等值定位章节文件（路由读侧消费形态；目录缺失/
/// 不可读 → `None`，由调用方落各自的 404/软失败语义）。
pub async fn resolve_chapter_file(chapters_dir: &Path, chapter_number: u32) -> Option<String> {
    let mut entries = tokio::fs::read_dir(chapters_dir).await.ok()?;
    let mut files: Vec<String> = Vec::new();
    loop {
        let entry = match entries.next_entry().await {
            Ok(Some(entry)) => entry,
            Ok(None) => break,
            Err(_) => return None,
        };
        files.push(entry.file_name().to_string_lossy().into_owned());
    }
    find_chapter_file_by_number(&files, chapter_number)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::manager::select_chapter_files;

    fn list(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parse_matches_ts_semantics() {
        assert_eq!(parse_chapter_file_number("0001_镜中醒来.md"), Some(1));
        assert_eq!(parse_chapter_file_number("0001-镜中醒来.md"), Some(1));
        assert_eq!(parse_chapter_file_number("7_手记.md"), Some(7));
        assert_eq!(parse_chapter_file_number("00071_长名.md"), Some(71));
        // 前导 0 的位数不限；数值等值而非字符串前缀。
        assert_eq!(parse_chapter_file_number("007_短.md"), Some(7));
        // 0 与不匹配形态排除。
        assert_eq!(parse_chapter_file_number("0000_零.md"), None);
        assert_eq!(parse_chapter_file_number("手记.md"), None);
        assert_eq!(parse_chapter_file_number("0001_手记.txt"), None);
    }

    #[test]
    fn find_prefers_canonical_then_lexicographic() {
        // 845 号：同号双形态正典 `_` 胜出（与索引重建 select_chapter_files 同胜者），
        // 不再按 padded 类内字典序取 '-'（0x2D）<'_'（0x5F）的连字符旧章。输入顺序无关。
        let files = list(&["0001-镜中醒来.md", "0001_镜中醒来.md"]);
        assert_eq!(
            find_chapter_file_by_number(&files, 1),
            Some("0001_镜中醒来.md".to_string())
        );
        let files = list(&["0001_镜中醒来.md", "0001-镜中醒来.md"]);
        assert_eq!(
            find_chapter_file_by_number(&files, 1),
            Some("0001_镜中醒来.md".to_string())
        );
        // 同类内字节序兜底：均正典 → "007_" < "7_"；均非正典 → "0002-a" < "0002-b"。
        let files = list(&["7_手记.md", "007_手记.md"]);
        assert_eq!(
            find_chapter_file_by_number(&files, 7),
            Some("007_手记.md".to_string())
        );
        let files = list(&["0002-b.md", "0002-a.md"]);
        assert_eq!(
            find_chapter_file_by_number(&files, 2),
            Some("0002-a.md".to_string())
        );
        // 数值等值：00071 是 71 章，与 7 无关（前缀碰撞根除）。
        let files = list(&["00071_长名.md"]);
        assert_eq!(find_chapter_file_by_number(&files, 7), None);
        assert_eq!(
            find_chapter_file_by_number(&files, 71),
            Some("00071_长名.md".to_string())
        );
        // 无候选 / 0 号。
        assert_eq!(find_chapter_file_by_number(&list(&["0002_风起.md"]), 1), None);
        assert_eq!(find_chapter_file_by_number(&list(&["0000_零.md"]), 0), None);
    }

    #[test]
    fn find_agrees_with_select_chapter_files_for_every_chapter() {
        // 845 号：读侧定位与索引去重对任一章的胜者恒等，且与输入顺序无关。
        let files = list(&[
            "0002-b.md", "0001_新.md", "0001-旧.md", "0003.md", "0000_zero.md", "index.json",
            "0002-a.md", "7_a.md", "0007_b.md", "007_c.md", "00071_七十一.md",
        ]);
        let mut reversed = files.clone();
        reversed.reverse();
        let selected = select_chapter_files(files.clone());
        assert_eq!(selected, select_chapter_files(reversed.clone()));
        for name in &selected {
            let number = parse_chapter_file_number(name).expect("selected files carry a chapter number");
            assert_eq!(find_chapter_file_by_number(&files, number).as_deref(), Some(name.as_str()));
            assert_eq!(find_chapter_file_by_number(&reversed, number).as_deref(), Some(name.as_str()));
        }
    }
}
