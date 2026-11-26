// @vitest-environment jsdom
//! R48b/1647 号：BookDetail 拖拽重排链组件级活体——1646 号备案未结算项
//!（此前仅纯函数+路由层覆盖，组件内 dragStart→drop→reorder 事务链与
//! revision 链式/409 清链从未整链活过）。三链锁：
//! ①拖拽 drop→reorder body（order 重排 + 首笔 expectedRevision 缺省跳过）
//! ②revision 链式回填（操作响应 revision 供第二笔带链）
//! ③409 revision conflict → structureConflict 提示 + revision 清链（下一笔
//!   回到缺省跳过）。拖拽源=目标 no-op 一并锁。
import { afterEach, describe, expect, it, vi, beforeEach } from "vitest";
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { BookDetail } from "./BookDetail";
import type { Nav } from "../lib/nav";
import type { TFunction } from "../hooks/use-i18n";
import { engineKind, resetEngineKindCache } from "../hooks/use-engine-kind";

// 卡片面全部置空——本测只活章节表结构操作链（卡片各有自家测试面）。
vi.mock("../components/QualityDebtsCard", () => ({ QualityDebtsCard: () => null }));
vi.mock("../components/PromiseTimelineCard", () => ({ PromiseTimelineCard: () => null }));
vi.mock("../components/StyleBindingCard", () => ({ StyleBindingCard: () => null }));
vi.mock("../components/RosterCandidatesCard", () => ({ RosterCandidatesCard: () => null }));
vi.mock("../components/RecallTestCard", () => ({ RecallTestCard: () => null }));
vi.mock("../components/TensionCurveCard", () => ({ TensionCurveCard: () => null }));
vi.mock("../components/QualityTrendCard", () => ({ QualityTrendCard: () => null }));
vi.mock("../components/AssetLibraryPanel", () => ({ AssetLibraryPanel: () => null }));
vi.mock("../components/AntiAiAndExperiencePanel", () => ({ AntiAiAndExperiencePanel: () => null }));
vi.mock("../components/CodexPanel", () => ({ CodexPanel: () => null }));
vi.mock("../components/SearchPanel", () => ({ SearchPanel: () => null }));
vi.mock("../components/VolumeManageCard", () => ({ VolumeManageCard: () => null }));
vi.mock("../components/ExportPreviewCard", () => ({ ExportPreviewCard: () => null }));
vi.mock("../components/SeriesCanonPanel", () => ({ SeriesCanonPanel: () => null }));
vi.mock("../components/ContextLensPanel", () => ({ ContextLensPanel: () => null }));
vi.mock("../components/DeconstructPanel", () => ({ DeconstructPanel: () => null }));
vi.mock("../components/DirectorPanel", () => ({ DirectorPanel: () => null }));

const postApiMock = vi.fn(async (_path: string, _body?: unknown) => ({ ok: true, revision: "0000000000000000" }));
const fetchJsonMock = vi.fn(async (_path: string, _init?: unknown) => ({}));
const refetchMock = vi.fn();
// data: unknown——useApi 按 path 分流（书面数据 / structure/undo 状态），泛型由调用方定。
const useApiMock = vi.fn((_path: string) => ({
  data: {
    book: { id: "b1", title: "拖拽测试书", genre: "xuanhuan", status: "active", chapterWordCount: 2000, targetChapters: 10, language: "zh" },
    chapters: [
      { number: 1, title: "风起", status: "approved", wordCount: 200 },
      { number: 2, title: "云涌", status: "ready-for-review", wordCount: 210 },
      { number: 3, title: "收束", status: "drafting", wordCount: 190 },
    ],
    nextChapter: 4,
  } as unknown,
  error: null,
  loading: false,
  refetch: refetchMock,
}));

vi.mock("../hooks/use-api", () => ({
  useApi: (path: string) => useApiMock(path),
  postApi: (path: string, body?: unknown) => postApiMock(path, body),
  // mount 即调 chapter-review-mode——须返回 Promise（undefined.then 崩）。
  fetchJson: (path: string, init?: unknown) => fetchJsonMock(path, init),
}));

const nav = { toDashboard: vi.fn(), toChapter: vi.fn() } as unknown as Nav;
const t = ((key: string) => key) as TFunction;

function renderPage() {
  return render(<BookDetail bookId="b1" nav={nav} theme="light" t={t} sse={{ messages: [] }} />);
}

function row(number: number) {
  const row = document.querySelector(`[data-chapter-row="${number}"]`);
  expect(row).not.toBeNull();
  return row!;
}

beforeEach(() => {
  // BookDetail mount 即建 EventSource（写作任务流）并 addEventListener——
  // jsdom 无实现，打桩最小面（close + addEventListener 空实现）。
  vi.stubGlobal("EventSource", class {
    onerror: unknown = null;
    onmessage: unknown = null;
    onopen: unknown = null;
    addEventListener() {}
    close() {}
  });
  postApiMock.mockReset();
  postApiMock.mockResolvedValue({ ok: true, revision: "0000000000000000" });
  refetchMock.mockClear();
});

afterEach(() => {
  vi.unstubAllGlobals();
  cleanup();
});

describe("BookDetail 拖拽重排链（R48b/1647 号组件级活体）", () => {
  it("dragStart→drop 触发 reorder 事务（首笔缺省跳过校验）+ 源=目标 no-op", async () => {
    renderPage();
    await vi.waitFor(() => expect(document.querySelector('[data-chapter-row="1"]')).not.toBeNull());

    // 拖第 3 章放到第 1 章位：order=[3,1,2]，首笔无 expectedRevision（缺省跳过）。
    fireEvent.dragStart(row(3));
    fireEvent.drop(row(1));
    await vi.waitFor(() => {
      expect(postApiMock).toHaveBeenCalledWith("/books/b1/chapters/reorder", {
        order: [3, 1, 2],
        expectedRevision: undefined,
      });
    });

    // 源=目标 drop：不触发请求。
    postApiMock.mockClear();
    fireEvent.dragStart(row(2));
    fireEvent.drop(row(2));
    expect(postApiMock).not.toHaveBeenCalled();
  });

  it("revision 链式回填：第一笔响应 revision 供第二笔带链", async () => {
    postApiMock.mockResolvedValueOnce({ ok: true, revision: "aaaa1111bbbb2222" });
    renderPage();
    await vi.waitFor(() => expect(document.querySelector('[data-chapter-row="1"]')).not.toBeNull());

    fireEvent.dragStart(row(3));
    fireEvent.drop(row(1));
    await vi.waitFor(() => expect(postApiMock).toHaveBeenCalledTimes(1));
    // 冲刷第一笔的 finally（setRestructuring(false)+revision 回填）——
    // restructuring 在途时第二击被双击风暴守卫吞掉（真实行为）。
    await act(async () => {});

    // 第二笔（move-up 第 2 章）：expectedRevision=第一笔回填值。
    fireEvent.click(screen.getByTestId("chapter-move-up-2"));
    await vi.waitFor(() => {
      expect(postApiMock).toHaveBeenCalledWith("/books/b1/chapters/reorder", {
        order: [2, 1, 3],
        expectedRevision: "aaaa1111bbbb2222",
      });
    });
  });

  it("409 revision conflict：页内提示 + revision 清链（下一笔回到缺省跳过）", async () => {
    postApiMock
      .mockResolvedValueOnce({ ok: true, revision: "aaaa1111bbbb2222" })
      .mockRejectedValueOnce(new Error("409 revision conflict: chapter set changed"))
      .mockResolvedValueOnce({ ok: true, revision: "cccc3333dddd4444" });
    renderPage();
    await vi.waitFor(() => expect(document.querySelector('[data-chapter-row="1"]')).not.toBeNull());

    // 第一笔成功入链。
    fireEvent.click(screen.getByTestId("chapter-move-up-3"));
    await vi.waitFor(() => expect(postApiMock).toHaveBeenCalledTimes(1));
    await act(async () => {});

    // 第二笔带链 → 409 文案命中 revision conflict 分支（1651 号：壳内 alert
    // 静默——提示改页内 notice 文本条 role=status）。
    fireEvent.click(screen.getByTestId("chapter-move-up-2"));
    await vi.waitFor(() => expect(postApiMock).toHaveBeenCalledTimes(2));
    await vi.waitFor(() => {
      const notice = document.querySelector('[role="status"]');
      expect(notice?.textContent).toBe("book.structureConflict");
    });
    await act(async () => {});

    // 第三笔：revision 已清 → 回到缺省跳过。
    fireEvent.click(screen.getByTestId("chapter-move-down-1"));
    await vi.waitFor(() => {
      expect(postApiMock).toHaveBeenCalledWith("/books/b1/chapters/reorder", {
        order: [2, 1, 3],
        expectedRevision: undefined,
      });
    });
    expect(document.querySelectorAll('[role="status"]').length).toBeLessThanOrEqual(1);
  });

  // 1651 号回归：人工反馈输入弹层链（壳内 prompt 哑=死按钮修复）——以
  // rewrite 面为代表：点重写 → PromptDialog（multiline）→ 输入确认 →
  // POST rewrite 携 brief；取消 → 零请求。
  it("rewrite 弹层链：PromptDialog 输入 → POST 携 brief；取消零请求", async () => {
    renderPage();
    await vi.waitFor(() => expect(document.querySelector('[data-chapter-row="1"]')).not.toBeNull());

    // 触发 rewrite：handleRewrite 挂在章行重写钮（title=book.rewrite）。
    const rewriteBtn = document.querySelector('button[data-testid="chapter-rewrite-1"]')
      ?? [...document.querySelectorAll("button")].find((b) => b.getAttribute("title") === "book.rewrite");
    expect(rewriteBtn).toBeDefined();
    fireEvent.click(rewriteBtn as HTMLButtonElement);

    // 弹层出现（multiline textarea）。
    const area = await vi.waitFor(() => {
      const el = document.querySelector("div.fixed.inset-0 textarea") as HTMLTextAreaElement;
      expect(el).not.toBeNull();
      return el;
    });
    fireEvent.change(area, { target: { value: "加强节奏" } });
    const confirmBtn = [...document.querySelectorAll("div.fixed.inset-0 button")].find(
      (b) => b.textContent === "common.confirm",
    ) as HTMLButtonElement;
    expect(confirmBtn).toBeDefined();
    fireEvent.click(confirmBtn);

    await vi.waitFor(() => {
      const call = fetchJsonMock.mock.calls.find((args) => String(args[0]).includes("/rewrite/1"));
      expect(call).toBeDefined();
      expect((call![1] as { body: string }).body).toContain("加强节奏");
    });

    // 取消路径：再开弹层直接取消 → 无新请求。
    fetchJsonMock.mockClear();
    fireEvent.click(rewriteBtn as HTMLButtonElement);
    const cancelBtn = await vi.waitFor(() => {
      const el = [...document.querySelectorAll("div.fixed.inset-0 button")].find(
        (b) => b.textContent === "common.cancel",
      ) as HTMLButtonElement;
      expect(el).toBeDefined();
      return el;
    });
    fireEvent.click(cancelBtn);
    expect(fetchJsonMock.mock.calls.find((args) => String(args[0]).includes("/rewrite/"))).toBeUndefined();
  });
});

// 1668 号：best-of-N 的执行方只在产品引擎管线（2.0 core 无调用点）——Node
// 回退腿上按钮如实标注不生效；产品引擎在位不标、开关照常可用。腿型桩：
// /api/v1/health 404 = Node 回退腿 / 200 = 产品引擎。
describe("BookDetail best-of-N 腿型标注（1668 号）", () => {
  const stubEngine = (healthStatus: number) =>
    vi.stubGlobal("fetch", vi.fn(async () => ({ ok: healthStatus === 200, status: healthStatus }) as Response));
  const toggle = () => screen.getByTestId("book-best-of-n-toggle");

  // 腿型探测整页共享（模块级缓存）：本文件前面的用例已把缓存落成 unknown
  // （jsdom 相对 URL 不可 fetch），每例前后都重置，防读到陈旧腿型。
  beforeEach(() => resetEngineKindCache());
  afterEach(() => resetEngineKindCache());

  it("Node 回退腿——title 追加不生效提示 + 徽标", async () => {
    stubEngine(404);
    renderPage();
    await vi.waitFor(() => expect(toggle().getAttribute("title")).toBe("book.bestOfNHint book.bestOfNNodeLegHint"));
    expect(toggle().textContent).toContain("book.nodeLegBadge");
  });

  it("产品引擎在位——title 仅原提示、无徽标（探测已落定，非 checking 中间态）", async () => {
    stubEngine(200);
    renderPage();
    expect(await engineKind()).toBe("rust");
    await act(async () => {});
    expect(toggle().getAttribute("title")).toBe("book.bestOfNHint");
    expect(toggle().textContent).not.toContain("book.nodeLegBadge");
  });
});

// R61 批三/1689 号：结构操作撤销——①重排成功通知条带「撤销」按钮，点击打
// undo 端点并把通知换成已撤销文案；②undoStatus eligible → 状态行渲染（含
// 尾笔类型名）；③不可撤销时不渲染状态行。undoStatus 的 useApi 按 path 分流。
describe("BookDetail 结构撤销（R61 批三/1689 号）", () => {
  const refetchUndoMock = vi.fn();
  let undoStatusPayload: { eligible: boolean; reason: string | null; lastKind: string | null; lastOpId: string | null };

  beforeEach(() => {
    undoStatusPayload = { eligible: true, reason: null, lastKind: "split", lastOpId: "split-1" };
    useApiMock.mockImplementation((path: string) => {
      if (path.endsWith("/chapters/structure/undo")) {
        return { data: undoStatusPayload, error: null, loading: false, refetch: refetchUndoMock };
      }
      return {
        data: {
          book: { id: "b1", title: "拖拽测试书", genre: "xuanhuan", status: "active", chapterWordCount: 2000, targetChapters: 10, language: "zh" },
          chapters: [
            { number: 1, title: "风起", status: "approved", wordCount: 200 },
            { number: 2, title: "云涌", status: "ready-for-review", wordCount: 210 },
            { number: 3, title: "收束", status: "drafting", wordCount: 190 },
          ],
          nextChapter: 4,
        },
        error: null,
        loading: false,
        refetch: refetchMock,
      };
    });
  });

  afterEach(() => {
    useApiMock.mockImplementation((path: string) => useApiMock(path));
  });

  it("重排成功后通知条带撤销按钮，点击撤销打 undo 端点并换已撤销文案", async () => {
    postApiMock.mockImplementation(async (path: string) => {
      if (path.endsWith("/chapters/structure/undo")) return { ok: true, revision: "1111111111111111" };
      return { ok: true, revision: "0000000000000000" };
    });
    renderPage();
    await vi.waitFor(() => expect(document.querySelector('[data-chapter-row="1"]')).not.toBeNull());
    // 拖拽 3 → 1 触发 reorder（既有链同款手法）。
    fireEvent.dragStart(row(3));
    fireEvent.drop(row(1));
    await vi.waitFor(() =>
      expect(postApiMock).toHaveBeenCalledWith(
        "/books/b1/chapters/reorder",
        expect.objectContaining({ order: [3, 1, 2] }),
      ),
    );
    // 成功通知条带「撤销」按钮（按文案定位通知条容器——状态行同名按钮不算；
    // findByText 自带等待与 act 包裹）。
    const notice = (await screen.findByText("book.structureUndoAvailable")).closest("div");
    expect(notice).not.toBeNull();
    const undoButton = within(notice!).getByRole("button", { name: "book.structureUndo" });
    // 点击撤销 → undo 端点 + 通知换已撤销文案 + undoStatus 刷新。
    await act(async () => {
      fireEvent.click(undoButton);
    });
    await vi.waitFor(() =>
      expect(postApiMock).toHaveBeenCalledWith("/books/b1/chapters/structure/undo", {}),
    );
    await vi.waitFor(() => expect(screen.getByText("book.structureUndone")).toBeTruthy());
    expect(refetchUndoMock).toHaveBeenCalled();
    postApiMock.mockImplementation(async (_path: string) => ({ ok: true, revision: "0000000000000000" }));
  });

  it("undoStatus eligible → 状态行渲染尾笔类型", async () => {
    renderPage();
    const statusRow = await screen.findByTestId("structure-undo-status");
    expect(statusRow.textContent).toContain("book.structureUndoAvailable");
    expect(statusRow.textContent).toContain("book.structureUndoKindSplit");
  });

  it("undoStatus ineligible（盘面漂移）→ 状态行不渲染", async () => {
    undoStatusPayload = {
      eligible: false,
      reason: "disk state changed since the last operation",
      lastKind: "split",
      lastOpId: "split-1",
    };
    renderPage();
    await vi.waitFor(() =>
      expect(document.querySelector('[data-chapter-row="1"]')).not.toBeNull(),
    );
    expect(screen.queryByTestId("structure-undo-status")).toBeNull();
  });
});
