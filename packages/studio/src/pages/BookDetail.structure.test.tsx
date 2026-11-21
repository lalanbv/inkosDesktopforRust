// @vitest-environment jsdom
//! R48b/1647 号：BookDetail 拖拽重排链组件级活体——1646 号备案未结算项
//!（此前仅纯函数+路由层覆盖，组件内 dragStart→drop→reorder 事务链与
//! revision 链式/409 清链从未整链活过）。三链锁：
//! ①拖拽 drop→reorder body（order 重排 + 首笔 expectedRevision 缺省跳过）
//! ②revision 链式回填（操作响应 revision 供第二笔带链）
//! ③409 revision conflict → structureConflict 提示 + revision 清链（下一笔
//!   回到缺省跳过）。拖拽源=目标 no-op 一并锁。
import { afterEach, describe, expect, it, vi, beforeEach } from "vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { BookDetail } from "./BookDetail";
import type { Nav } from "../lib/nav";
import type { TFunction } from "../hooks/use-i18n";

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
const useApiMock = vi.fn((_path: string) => ({
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
