//! markdown 表格单元格的转义/还原单一真源（812/816 号）。
//!
//! 契约（与 TS `utils/markdown-table.ts` 逐字同集）：
//! - 渲染：`|` → `\|`、`\r\n`/`\n` → `<br>`、首尾空白 trim——单元格不破行不破列；
//! - 解析：按未转义 `|` 拆分（`\|` 不作分隔），单元格 `<br>`（三形态、忽略大小写）→ 换行、
//!   `\|` → `|`。
//!
//! 历史：此逻辑曾以物理拷贝散布多处（projections / chapter_analyzer / story_markdown /
//! writer 摘要行 / TS state-projections + story-markdown），810/811/812 三轮各修一处后
//! 才收敛——新渲染/解析点必须走本模块，不得再抄内联正则。forecast/render 的
//! 「换行→空格」是另一语义（双侧一致），不在此列。

use regex::Regex;
use std::sync::OnceLock;

fn restore_br_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)<br\s*/?>").unwrap())
}

/// 渲染：单元格转义（竖线 → `\|`、换行 → `<br>`、首尾空白 trim）。
///
/// 817 号：单次字节扫描快速路径——无 `|`/`\n`/`\r` 的常见单元格直接 trim 返回，避免
/// 逐替换扫描。810 号补换行转义后 projections 渲染（每章落盘热路径，100 钩子×14 格）
/// 逐格多两次子串扫描，bench 实测 hooks +54%/summaries +70%（基线 11-01，先于 810）
/// ——本快速路径恢复基线并略优（常见格：1 次扫描+trim，旧形态为 1 次扫描+2 次分配）。
pub fn escape_table_cell(value: &str) -> String {
    let needs_escape = value
        .as_bytes()
        .iter()
        .any(|&b| b == b'|' || b == b'\n' || b == b'\r');
    if !needs_escape {
        return value.trim().to_string();
    }
    value
        .replace('|', "\\|")
        .replace("\r\n", "<br>")
        .replace('\n', "<br>")
        .trim()
        .to_string()
}

/// 解析：单元格回读还原（`<br>` 三形态 → 换行、`\|` → `|`）。
pub fn restore_table_cell(cell: &str) -> String {
    restore_br_re().replace_all(cell, "\n").replace("\\|", "|")
}

/// 按未转义的 `|` 拆分表格行（`\|` 不作分隔符，留给 [`restore_table_cell`] 还原）。
/// 等价 JS `line.split(/(?<!\\)\|/)`（regex crate 无反向预查，手写扫描）。
pub fn split_table_row_escaped(line: &str) -> Vec<&str> {
    let mut segs = Vec::new();
    let mut start = 0;
    let mut prev_backslash = false;
    for (idx, ch) in line.char_indices() {
        if ch == '|' && !prev_backslash {
            segs.push(&line[start..idx]);
            start = idx + 1;
        }
        prev_backslash = ch == '\\';
    }
    segs.push(&line[start..]);
    segs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_covers_pipe_crlf_and_trims() {
        assert_eq!(escape_table_cell("a|b"), "a\\|b");
        assert_eq!(escape_table_cell("既有。\n依据。"), "既有。<br>依据。");
        assert_eq!(escape_table_cell("a\r\nb"), "a<br>b");
        assert_eq!(escape_table_cell("  x  "), "x");
    }

    #[test]
    fn restore_round_trips_escape() {
        let original = "A|B\nC";
        assert_eq!(restore_table_cell(&escape_table_cell(original)), original);
        assert_eq!(restore_table_cell("x<BR/>y<br />z"), "x\ny\nz");
    }

    #[test]
    fn split_skips_escaped_pipes() {
        assert_eq!(split_table_row_escaped("| a\\|b | c |"), vec!["", " a\\|b ", " c ", ""]);
        assert_eq!(split_table_row_escaped("| a | b |"), vec!["", " a ", " b ", ""]);
    }
}
