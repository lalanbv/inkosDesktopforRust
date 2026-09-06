import { describe, expect, it, vi, beforeEach } from "vitest";
import { renderToString } from "react-dom/server";
import { BookTimeline, TimelineEditDialog } from "@/pages/BookTimeline";

const { useApiMock, putApiMock } = vi.hoisted(() => ({
  useApiMock: vi.fn(),
  putApiMock: vi.fn(),
}));

vi.mock("@/hooks/use-api", () => ({
  useApi: (path: string) => useApiMock(path),
  fetchJson: vi.fn(),
  postApi: vi.fn(),
  putApi: putApiMock,
}));

// 两个 useApi 调用按 path 分流：book 详情 / timeline。
function mockSources(source: { book?: unknown; timeline?: unknown }) {
  useApiMock.mockImplementation((path: string) => {
    if (path.endsWith("/timeline")) {
      return { data: { timeline: source.timeline ?? null }, error: null, loading: false, refetch: vi.fn() };
    }
    return { data: source.book ?? null, error: null, loading: source.book === undefined, refetch: vi.fn() };
  });
}

const nav = new Proxy({}, { get: () => vi.fn() }) as Record<string, (a?: unknown, b?: unknown) => void>;
const t = (key: string) => key;

describe("BookTimeline（180 号 W-C4-a 只读时间线）", () => {
  beforeEach(() => {
    useApiMock.mockReset();
  });

  it("loading 态渲染骨架", () => {
    useApiMock.mockImplementation((path: string) =>
      path.endsWith("/timeline")
        ? { data: { timeline: null }, error: null, loading: false, refetch: vi.fn() }
        : { data: null, error: null, loading: true, refetch: vi.fn() },
    );
    const html = renderToString(
      <BookTimeline bookId="b1" nav={nav as never} theme="light" t={t} />,
    );
    expect(html).toContain('data-loading="skeleton"');
    expect(html).not.toContain('data-tone="done"');
  });

  it("单线兜底网格：每章一格、状态映射 tone、列头按章号", () => {
    mockSources({
      book: {
        book: { id: "b1", title: "时间线之书" },
        nextChapter: 4,
        chapters: [
          { number: 2, title: "第二章", status: "approved", wordCount: 3200 },
          { number: 1, title: "第一章", status: "ready-for-review", wordCount: 2800 },
          { number: 3, title: "第三章", status: "audit-failed", wordCount: 0 },
        ],
      },
    });
    const html = renderToString(
      <BookTimeline bookId="b1" nav={nav as never} theme="light" t={t} />,
    );
    // 单线兜底：恰好一行情节线标签。
    expect(html).toContain("timeline.mainPlotline");
    // 章乱序输入 → 渲染按章号升序。
    const order = [...html.matchAll(/data-timeline-cell="(\d+)"/g)].map((m) => Number(m[1]));
    expect(order).toEqual([1, 2, 3]);
    // 状态 tone：approved→done / ready-for-review→review / audit-failed→failed。
    expect(html).toContain('data-timeline-cell="1" data-tone="review"');
    expect(html).toContain('data-timeline-cell="2" data-tone="done"');
    expect(html).toContain('data-timeline-cell="3" data-tone="failed"');
    // 统计行。
    expect(html).toContain("timeline.stats");
  });

  it("空章节渲染空态提示", () => {
    mockSources({
      book: { book: { id: "b1", title: "空书" }, nextChapter: 1, chapters: [] },
      timeline: null,
    });
    const html = renderToString(
      <BookTimeline bookId="b1" nav={nav as never} theme="light" t={t} />,
    );
    expect(html).toContain("timeline.empty");
    expect(html).not.toContain("data-timeline-cell");
  });

  it("timeline.json 非空时切换多线（planned tone + note 副标题）", () => {
    mockSources({
      book: {
        book: { id: "b1", title: "多线书" },
        nextChapter: 3,
        chapters: [{ number: 1, title: "第一章", status: "approved", wordCount: 1000 }],
      },
      timeline: {
        version: 1,
        bookId: "b1",
        updatedAt: "2026-09-07T00:00:00.000Z",
        plotlines: [
          { id: "main", name: "主线", cells: [{ chapter: 1, title: "风起", note: "主角入场" }] },
          { id: "side", name: "支线", cells: [{ chapter: 2, note: "伏笔埋设" }] },
        ],
      },
    });
    const html = renderToString(
      <BookTimeline bookId="b1" nav={nav as never} theme="light" t={t} />,
    );
    // 两行情节线。
    expect(html).toContain("主线");
    expect(html).toContain("支线");
    // timeline 格 → planned tone；无 title 的格用章号兜底标题。
    expect(html).toContain('data-tone="planned"');
    expect(html).toContain("风起");
    expect(html).toContain("伏笔埋设");
    expect(html).toContain("timeline.chapter");
    // 章列轴来自 timeline cells（含第 2 章，即使 chapters 无第 2 章）。
    // 每线渲染全列轴：主线 1,2 + 支线 1,2（占位格以 empty tone 补齐）。
    const order = [...html.matchAll(/data-timeline-cell="(\d+)"/g)].map((m) => Number(m[1]));
    expect(order).toEqual([1, 2, 1, 2]);
    // timeline 格可编辑；占位格（该线该章无节拍）以 empty tone 暴露。
    expect(html).toContain('data-editable="true"');
    expect(html).toContain('data-tone="empty"');
  });

  it("timeline.json 为 null 时回退单线兜底", () => {
    mockSources({
      book: {
        book: { id: "b1", title: "兜底书" },
        nextChapter: 2,
        chapters: [{ number: 1, title: "第一章", status: "approved", wordCount: 1000 }],
      },
      timeline: null,
    });
    const html = renderToString(
      <BookTimeline bookId="b1" nav={nav as never} theme="light" t={t} />,
    );
    expect(html).toContain("timeline.mainPlotline");
    expect(html).toContain('data-tone="done"');
    expect(html).not.toContain('data-tone="planned"');
    // 兜底格只读（点击去阅读），但提供「编辑时间线」初始化入口。
    expect(html).toContain('data-editable="false"');
    expect(html).toContain('data-slot="timeline-init"');
  });

  it("imported 章节归 imported tone", () => {
    mockSources({
      book: {
        book: { id: "b1", title: "导入书" },
        nextChapter: 2,
        chapters: [{ number: 1, title: "导入章", status: "imported", wordCount: 900 }],
      },
    });
    const html = renderToString(
      <BookTimeline bookId="b1" nav={nav as never} theme="light" t={t} />,
    );
    expect(html).toContain('data-tone="imported"');
  });
});

describe("TimelineEditDialog（182 号 C4-c 编辑弹窗）", () => {
  it("表单结构：标题/备注输入 + 打开章节 + 保存/取消", () => {
    const html = renderToString(
      <TimelineEditDialog
        bookId="b1"
        target={{ plotlineId: "main", plotlineName: "主线", chapter: 3, title: "危机", note: "宗门大比" }}
        saving={false}
        onSubmit={() => {}}
        onCancel={() => {}}
        nav={nav as never}
        t={t}
      />,
    );
    expect(html).toContain('data-testid="timeline-edit-dialog"');
    expect(html).toContain('data-slot="timeline-beat-title"');
    expect(html).toContain('data-slot="timeline-beat-note"');
    expect(html).toContain("主线");
    expect(html).toContain("timeline.editBeat");
    expect(html).toContain("timeline.openChapter");
    expect(html).toContain('data-slot="timeline-save"');
  });

  it("saving 态保存按钮禁用并显示保存中文案", () => {
    const html = renderToString(
      <TimelineEditDialog
        bookId="b1"
        target={{ plotlineId: "main", plotlineName: "主线", chapter: 3, title: "危机", note: "" }}
        saving
        onSubmit={() => {}}
        onCancel={() => {}}
        nav={nav as never}
        t={t}
      />,
    );
    expect(html).toContain("timeline.saving");
    expect(html).toContain("disabled");
  });
});
