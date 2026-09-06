/**
 * 时间线编辑核心逻辑（182 号 C4-c）。
 *
 * 与 UI 解耦的纯函数层：BookTimeline 组件的编辑/初始化/新增情节线都经由
 * 这里生成下一个 Timeline 文档，再交给 PUT 端点回写。纯函数便于单测
 * （乐观更新的正确性在 UI 渲染之外先被锁定）。

/** 本地宽松 Timeline 文档形态（与 core TimelineSchema 的成功解析面一致）。 */
export interface TimelineDoc {
  version: 1;
  bookId: string;
  updatedAt: string;
  plotlines: ReadonlyArray<{
    id: string;
    name: string;
    cells: ReadonlyArray<{ chapter: number; title?: string; note?: string }>;
  }>;
}

/** 章节元数据的最小形态（chapters/index.json 派生）。 */
export interface ChapterLike {
  readonly number: number;
  readonly title: string;
}

function sortCells<T extends { chapter: number }>(cells: ReadonlyArray<T>): T[] {
  return [...cells].sort((a, b) => a.chapter - b.chapter);
}

/** 刷新 updatedAt（ISO，本地时钟——文档级元数据，非叙事时间）。 */
function withRefreshedUpdatedAt(doc: TimelineDoc): TimelineDoc {
  return { ...doc, updatedAt: new Date().toISOString() };
}

/**
 * 单元格编辑：更新指定情节线指定章的 cell（不存在则创建），cells 保持章序。
 * 语义：patch 中出现的字段（弹窗总是同时传 title/note）**留空即清除**、
 * 非空取 trim 后值；未出现的字段保留原值。仅 chapter 的空 cell 合法
 * （schema 允许，标记「该章在该线占位」）。
 */
export function buildTimelineAfterCellEdit(
  doc: TimelineDoc,
  plotlineId: string,
  chapter: number,
  patch: { title?: string; note?: string },
): TimelineDoc {
  const clean = (value: string | undefined): string | undefined => {
    if (value === undefined) return undefined;
    const trimmed = value.trim();
    return trimmed ? trimmed : undefined;
  };
  const plotlines = doc.plotlines.map((line) => {
    if (line.id !== plotlineId) return line;
    const existing = line.cells.find((cell) => cell.chapter === chapter);
    const merged: { chapter: number; title?: string; note?: string } = { chapter };
    const title = patch.title !== undefined ? clean(patch.title) : existing?.title;
    if (title !== undefined) merged.title = title;
    const note = patch.note !== undefined ? clean(patch.note) : existing?.note;
    if (note !== undefined) merged.note = note;
    return { ...line, cells: sortCells([...line.cells.filter((c) => c.chapter !== chapter), merged]) };
  });
  return withRefreshedUpdatedAt({ ...doc, plotlines });
}

/**
 * 兜底单线 → 初始 timeline：每章一个 cell（title = 章节标题），单条「主线」。
 * 保存后页面即切多线可编辑态。
 */
export function initializeTimelineFromChapters(
  bookId: string,
  chapters: ReadonlyArray<ChapterLike>,
  mainPlotlineName: string,
): TimelineDoc {
  return {
    version: 1,
    bookId,
    updatedAt: new Date().toISOString(),
    plotlines: [
      {
        id: "main",
        name: mainPlotlineName,
        cells: sortCells(
          chapters.map((ch) => ({ chapter: ch.number, title: ch.title })),
        ),
      },
    ],
  };
}

/** 新增情节线（id 用时间戳保证唯一；name 必非空——调用方保证）。 */
export function buildTimelineAfterAddPlotline(
  doc: TimelineDoc,
  name: string,
): TimelineDoc {
  const id = `line-${Date.now()}`;
  return withRefreshedUpdatedAt({
    ...doc,
    plotlines: [...doc.plotlines, { id, name: name.trim(), cells: [] }],
  });
}
