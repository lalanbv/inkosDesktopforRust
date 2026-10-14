//! 章节文件名前导号解析与同号定位——`packages/core/src/state/chapter-filename.ts`
//! 逐字镜像。
//!
//! 682 号在 TS 侧确立「能列出（索引宽松侧）即可打开（详情侧）」单一事实源：
//! 引擎写入侧恒 4 位补零（`NNNN_title.md`），但导入/手工放置的章节文件位数
//! 不限；读侧按「前导号数值等值」定位，同号多形态（0007/007/7 并存）取
//! 4 位补零（引擎主形态）优先、字典序兜底。旧读侧 `startsWith(补零串)` 的
//! 前缀碰撞（num=7 误配 `00071_`）与「列表可见、详情 404」（`7_手记.md`）
//! 按数值等值在此根除——687 号把引擎读侧消费点（章节读取/审计/修稿/检测/
//! workspace 等 8 处）统一到本模块；写侧自产工件定位不受此影响（683 号裁决：
//! `starts_with("{padded}_")` 逐字镜像维持）。

use regex::Regex;
use std::path::Path;
use std::sync::OnceLock;

fn chapter_file_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^(\d+)[_-]?(.*)\.md$").unwrap())
}

/// TS `parseChapterFileName`：前导号数值（须 > 0）；不匹配返回 `None`。
pub fn parse_chapter_file_number(file_name: &str) -> Option<u32> {
    let captures = chapter_file_re().captures(file_name)?;
    let number = captures[1].parse::<u32>().ok()?;
    (number > 0).then_some(number)
}

/// TS `/^0{3}\d/`：4 位补零（引擎主形态）判定。
fn is_padded_4(file_name: &str) -> bool {
    let bytes = file_name.as_bytes();
    bytes.len() >= 4
        && bytes[0] == b'0'
        && bytes[1] == b'0'
        && bytes[2] == b'0'
        && bytes[3].is_ascii_digit()
}

/// TS `findChapterFileByNumber`：数值等值候选按字典序，4 位补零优先、字典序兜底。
pub fn find_chapter_file_by_number(files: &[String], chapter_number: u32) -> Option<String> {
    let mut candidates: Vec<&String> = files
        .iter()
        .filter(|file| parse_chapter_file_number(file) == Some(chapter_number))
        .collect();
    candidates.sort();
    candidates
        .iter()
        .find(|file| is_padded_4(file))
        .or_else(|| candidates.first())
        .map(|file| (*file).clone())
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
    fn find_prefers_padded4_then_lexicographic() {
        // 同号多形态同属 padded-4 类：类内字典序（'-' 0x2D < '_' 0x5F），
        // 与 TS findChapterFileByNumber 逐字一致（确定性是目的，非「下划线规范名」）。
        let files = list(&["0001_镜中醒来.md", "0001-镜中醒来.md"]);
        assert_eq!(
            find_chapter_file_by_number(&files, 1),
            Some("0001-镜中醒来.md".to_string())
        );
        // 非 4 位形态并存：字典序兜底（"007_" < "7_"）。
        let files = list(&["7_手记.md", "007_手记.md"]);
        assert_eq!(
            find_chapter_file_by_number(&files, 7),
            Some("007_手记.md".to_string())
        );
        // 数值等值：00071 是 71 章，与 7 无关（前缀碰撞根除）。
        let files = list(&["00071_长名.md"]);
        assert_eq!(find_chapter_file_by_number(&files, 7), None);
        assert_eq!(
            find_chapter_file_by_number(&files, 71),
            Some("00071_长名.md".to_string())
        );
        // 无候选。
        assert_eq!(find_chapter_file_by_number(&list(&["0002_风起.md"]), 1), None);
    }
}
