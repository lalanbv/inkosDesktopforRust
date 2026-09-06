/**
 * 回填 diff 预览（186 号 C3 增强第一项）。
 *
 * apply 语义是**整体覆盖写入** `story/series_backfill.md`——未勾选的既有
 * 条目会被移除。diff 把这个语义显性化：写入前展示「保留 / 移除 / 写入」
 * 三类行。纯函数层（LCS 行级 diff + markdown 渲染）与 UI 解耦便于单测。
 */

/** 回填条目（与双端 schema items 元素一致）。 */
export interface BackfillItem {
  readonly id: string;
  readonly category: string;
  readonly title: string;
  readonly content: string;
}

export interface DiffLine {
  readonly kind: "kept" | "removed" | "added";
  readonly text: string;
}

/** 目标书现有 series_backfill.md 的将写入渲染（与双端 apply 渲染一致）。 */
export function renderBackfillMarkdown(
  sourceBookId: string,
  updatedAt: string,
  items: ReadonlyArray<BackfillItem>,
): string {
  // 头部与 Rust render_backfill_markdown 逐字对齐（双端渲染一致是 diff
  // 预览可信的前提——182/184 号 apply 渲染同款）。
  let out = `# 系列设定回填\n\n来源：《${sourceBookId}》（${sourceBookId}） · 抽取于 ${updatedAt} · 勾选 ${items.length} 条\n\n`;
  for (const item of items) {
    out += `## [${item.category}] ${item.title}\n\n${item.content}\n\n`;
  }
  return out;
}

/** LCS 行级 diff：返回按出现顺序合并的行分类（旧独有=removed，新独有=added，共有=kept）。 */
export function diffLines(
  oldLines: ReadonlyArray<string>,
  newLines: ReadonlyArray<string>,
): DiffLine[] {
  const n = oldLines.length;
  const m = newLines.length;
  // DP 表（行数 ≤ 数百，O(n·m) 可接受）。
  const dp: Uint32Array[] = Array.from({ length: n + 1 }, () => new Uint32Array(m + 1));
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      dp[i][j] = oldLines[i] === newLines[j] ? dp[i + 1][j + 1] + 1 : Math.max(dp[i + 1][j], dp[i][j + 1]);
    }
  }
  const out: DiffLine[] = [];
  let i = 0;
  let j = 0;
  while (i < n && j < m) {
    if (oldLines[i] === newLines[j]) {
      out.push({ kind: "kept", text: oldLines[i] });
      i++;
      j++;
    } else if (dp[i + 1][j] >= dp[i][j + 1]) {
      out.push({ kind: "removed", text: oldLines[i] });
      i++;
    } else {
      out.push({ kind: "added", text: newLines[j] });
      j++;
    }
  }
  while (i < n) {
    out.push({ kind: "removed", text: oldLines[i] });
    i++;
  }
  while (j < m) {
    out.push({ kind: "added", text: newLines[j] });
    j++;
  }
  return out;
}

/** 组装 diff 预览：现有内容（可 null=首次写入）× 勾选后的将写入内容。 */
export function buildBackfillDiff(
  existingContent: string | null,
  sourceBookId: string,
  updatedAt: string,
  selectedItems: ReadonlyArray<BackfillItem>,
): DiffLine[] {
  const next = renderBackfillMarkdown(sourceBookId, updatedAt, selectedItems);
  // 头部行（标题 + 来源/抽取时间元数据）不参与 diff——重新抽取必然刷新
  // 「抽取于」，算进差异只是噪音；条目行才是用户关心的内容。
  const isHeaderLine = (text: string): boolean =>
    text.startsWith("# 系列设定回填") || text.startsWith("来源：《");
  const oldLines = (existingContent ?? "").split("\n").filter((line) => !isHeaderLine(line));
  const newLines = next.split("\n").filter((line) => !isHeaderLine(line));
  return diffLines(oldLines, newLines);
}
