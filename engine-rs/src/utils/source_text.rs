//! 审稿源文本行工具。移植自 `packages/core/src/utils/source-text.ts`
//! `splitSourceLines`/`sourceLineBodies`/`numberSourceLines`（纯函数）。
//!
//! 行语义：**物理行保留精确终止符**（`/[^\n]*\n|[^\n]+$/g`）——scoped 编辑
//! 面依赖终止符保真；展示面（numberSourceLines）剥 `\n` 后 `行号\t正文`。

/// TS `splitSourceLines`：按 `\n` 切割、每行保留终止符（末行无终止符时原样）。
pub fn split_source_lines(source: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let bytes = source.as_bytes();
    let mut start = 0usize;
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'\n' {
            lines.push(source[start..=index].to_string());
            start = index + 1;
        }
    }
    if start < source.len() {
        lines.push(source[start..].to_string());
    }
    lines
}

/// TS `sourceLineBodies`：剥尾部 `\n`（\r 保留——TS 同只剥 \n）。
pub fn source_line_bodies(source: &str) -> Vec<String> {
    split_source_lines(source)
        .into_iter()
        .map(|line| line.strip_suffix('\n').map(str::to_string).unwrap_or(line))
        .collect()
}

/// TS `numberSourceLines`：`{startLine + index}\t{line}` 逐行拼接。
pub fn number_source_lines(source: &str, start_line: u32) -> String {
    source_line_bodies(source)
        .iter()
        .enumerate()
        .map(|(index, line)| format!("{}\t{line}", start_line as u64 + index as u64))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_keeps_terminators_and_tail() {
        assert_eq!(split_source_lines("a\nb\nc"), vec!["a\n", "b\n", "c"]);
        assert_eq!(split_source_lines("a\nb\n"), vec!["a\n", "b\n"]);
        assert_eq!(split_source_lines(""), Vec::<String>::new());
    }

    #[test]
    fn numbering_matches_ts_layout() {
        let source = "# 标题\n\n正文一行";
        assert_eq!(number_source_lines(source, 1), "1\t# 标题\n2\t\n3\t正文一行");
    }
}
