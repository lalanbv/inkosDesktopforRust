/**
 * R53/1657 号：章版本比较 LCS 行级 diff 纯函数（Rust 镜像 =
 * engine-rs/src/utils/text_diff.rs，golden text-diff-vectors.json 锁定）。
 *
 * 算法与 studio `series-backfill-diff.ts` 的 `diffLines` 同构（186 号
 * LCS DP 形态——O(n·m) 行数 ≤ 数千可接受）；抽出 core 单源供章历史比较
 * 面消费（series-backfill 未来可切本源，备案不强制）。行切分 = `\n` split
 * （保留 `\r`？——章正文正典 LF，CR 视作行内容一部分）。
 */

export interface TextDiffLine {
  readonly kind: "kept" | "removed" | "added";
  readonly text: string;
}

/** LCS 行级 diff：旧独有=removed（红）、新独有=added（绿）、共有=kept。 */
export function diffChapterText(oldText: string, newText: string): ReadonlyArray<TextDiffLine> {
  const oldLines = oldText.split("\n");
  const newLines = newText.split("\n");
  const n = oldLines.length;
  const m = newLines.length;
  // DP 表（Uint32Array 行级复用——数千行 O(n·m) 内存量级可控）。
  const dp: Uint32Array[] = Array.from({ length: n + 1 }, () => new Uint32Array(m + 1));
  for (let i = n - 1; i >= 0; i -= 1) {
    for (let j = m - 1; j >= 0; j -= 1) {
      dp[i][j] =
        oldLines[i] === newLines[j]
          ? dp[i + 1][j + 1] + 1
          : Math.max(dp[i + 1][j], dp[i][j + 1]);
    }
  }
  const out: TextDiffLine[] = [];
  let i = 0;
  let j = 0;
  while (i < n && j < m) {
    if (oldLines[i] === newLines[j]) {
      out.push({ kind: "kept", text: oldLines[i] });
      i += 1;
      j += 1;
    } else if (dp[i + 1][j] >= dp[i][j + 1]) {
      out.push({ kind: "removed", text: oldLines[i] });
      i += 1;
    } else {
      out.push({ kind: "added", text: newLines[j] });
      j += 1;
    }
  }
  while (i < n) {
    out.push({ kind: "removed", text: oldLines[i] });
    i += 1;
  }
  while (j < m) {
    out.push({ kind: "added", text: newLines[j] });
    j += 1;
  }
  return out;
}

/** diff 汇总（UI 引导与测试断言用）。 */
export function diffSummary(lines: ReadonlyArray<TextDiffLine>): {
  readonly kept: number;
  readonly removed: number;
  readonly added: number;
} {
  let kept = 0;
  let removed = 0;
  let added = 0;
  for (const line of lines) {
    if (line.kind === "kept") kept += 1;
    else if (line.kind === "removed") removed += 1;
    else added += 1;
  }
  return { kept, removed, added };
}
