//! R53/1657 号：章版本比较 LCS 行级 diff 纯函数（TS 镜像 =
//! packages/core/src/utils/text-diff.ts，golden text-diff-vectors.json
//! 锁定）。算法与 studio series-backfill `diffLines` 同构（186 号形态）；
//! 章历史比较面为纯前端功能（S 为主），Rust 侧 golden 纪律镜像。

/// 行分类（TS `TextDiffLine` 镜像）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TextDiffKind {
    Kept,
    Removed,
    Added,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TextDiffLine {
    pub kind: TextDiffKind,
    pub text: String,
}

/// LCS 行级 diff：旧独有=removed、新独有=added、共有=kept。行切分 = `\n`
/// （章正文正典 LF，CR 视作行内容一部分）。TS 镜像 = `diffChapterText`。
pub fn diff_chapter_text(old_text: &str, new_text: &str) -> Vec<TextDiffLine> {
    let old_lines: Vec<&str> = old_text.split('\n').collect();
    let new_lines: Vec<&str> = new_text.split('\n').collect();
    let n = old_lines.len();
    let m = new_lines.len();
    // DP 表（Vec<u32> 行级——数千行 O(n·m) 可控）。
    let mut dp = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i][j] = if old_lines[i] == new_lines[j] {
                dp[i + 1][j + 1] + 1
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }
    let mut out = Vec::new();
    let (mut i, mut j) = (0usize, 0usize);
    while i < n && j < m {
        if old_lines[i] == new_lines[j] {
            out.push(TextDiffLine {
                kind: TextDiffKind::Kept,
                text: old_lines[i].to_string(),
            });
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            out.push(TextDiffLine {
                kind: TextDiffKind::Removed,
                text: old_lines[i].to_string(),
            });
            i += 1;
        } else {
            out.push(TextDiffLine {
                kind: TextDiffKind::Added,
                text: new_lines[j].to_string(),
            });
            j += 1;
        }
    }
    while i < n {
        out.push(TextDiffLine {
            kind: TextDiffKind::Removed,
            text: old_lines[i].to_string(),
        });
        i += 1;
    }
    while j < m {
        out.push(TextDiffLine {
            kind: TextDiffKind::Added,
            text: new_lines[j].to_string(),
        });
        j += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modify_block_and_tail_add() {
        let diff = diff_chapter_text("# 第1章\n\n旧正文。\n", "# 第1章\n\n新正文更长。\n\n尾段。");
        let summary: Vec<&str> = diff
            .iter()
            .map(|line| match line.kind {
                TextDiffKind::Kept => "k",
                TextDiffKind::Removed => "-",
                TextDiffKind::Added => "+",
            })
            .collect();
        // 保留头两行 + 改一行（-旧+新）+ 旧尾空行 kept 对齐 + 尾段 added。
        assert_eq!(summary, vec!["k", "k", "-", "+", "k", "+"]);
    }
}
