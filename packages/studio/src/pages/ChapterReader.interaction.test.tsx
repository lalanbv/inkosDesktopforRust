// @vitest-environment jsdom
//! 458 号：ChapterReader 动作链交互测试——通过/驳回调用与导航、对照分屏
//! 开关（divider 出现/消失）。
import { afterEach, describe, expect, it, vi, beforeEach } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ChapterReader } from "@/pages/ChapterReader";
import type { Nav } from "@/lib/nav";
import type { TFunction } from "@/hooks/use-i18n";

const postApiMock = vi.fn(async (path: string, body?: unknown) => ({}));
const toBookMock = vi.fn();

const useApiMock = vi.fn<(path: string) => unknown>((path: string) => {
  if (path === "/books/b1") {
    return { data: { book: { id: "b1", title: "冒烟书" } }, error: null, loading: false, refetch: vi.fn() };
  }
  if (path.startsWith("/books/b1/chapters/2")) {
    return {
      data: { chapterNumber: 2, filename: "chapter-002.md", content: "# 第二章 灯下续\n\n后续正文。" },
      error: null,
      loading: false,
      refetch: vi.fn(),
    };
  }
  if (path.startsWith("/books/b1/chapters/1")) {
    return {
      data: { chapterNumber: 1, filename: "chapter-001.md", content: "# 第一章 灯下\n\n墨迹未干，故事已开。" },
      error: null,
      loading: false,
      refetch: vi.fn(),
    };
  }
  return { data: null, error: null, loading: false, refetch: vi.fn() };
});

vi.mock("@/hooks/use-api", () => ({
  useApi: (path: string) => useApiMock(path),
  fetchJson: vi.fn(),
  postApi: (path: string, body?: unknown) => postApiMock(path, body),
}));

const nav = { toBook: toBookMock } as unknown as Nav;
const t = ((key: string) => key) as TFunction;

beforeEach(() => {
  postApiMock.mockClear();
  toBookMock.mockClear();
});

afterEach(() => cleanup());

describe("ChapterReader 动作链（458 号）", () => {
  it("对照分屏换章——splitNext 触发对照章新 fetch（462 号）", async () => {
    const user = userEvent.setup();
    // 第 2 章起：初始对照章 = 前一章（1），splitNext 前进到 2
    render(<ChapterReader bookId="b1" chapterNumber={2} nav={nav} theme="light" t={t} />);
    await user.click(screen.getByTestId("reader-split-toggle"));
    useApiMock.mockClear();
    const next = screen.getByLabelText("reader.splitNext");
    await user.click(next);
    await vi.waitFor(() => {
      expect(useApiMock).toHaveBeenCalledWith("/books/b1/chapters/2");
    });
    expect(document.body.textContent).toContain("reader.compareChapter · 2");
  });

  it("对照分屏跳转输入——数字 Enter 触发对照章切换，非法输入不生效（462/468 号）", async () => {
    const user = userEvent.setup();
    render(<ChapterReader bookId="b1" chapterNumber={2} nav={nav} theme="light" t={t} />);
    await user.click(screen.getByTestId("reader-split-toggle"));
    useApiMock.mockClear();

    const jump = screen.getByTestId("reader-split-jump") as HTMLInputElement;
    await user.type(jump, "1");
    await user.keyboard("{Enter}");
    await vi.waitFor(() => {
      expect(useApiMock).toHaveBeenCalledWith("/books/b1/chapters/1");
    });
    expect(jump.value).toBe("");

    await user.type(jump, "abc");
    await user.keyboard("{Enter}");
    // 非数字不切换：未出现非法路径请求；commitJump 对非法输入同样清空输入框
    expect(useApiMock.mock.calls.filter(([p]) => String(p).includes("abc"))).toHaveLength(0);
    expect(jump.value).toBe("");
  });

  it("对照分屏开关——toggle 出现分隔条与对照栏，再点关闭", async () => {
    const user = userEvent.setup();
    render(<ChapterReader bookId="b1" chapterNumber={1} nav={nav} theme="light" t={t} />);
    expect(document.querySelector('[data-testid="reader-split-divider"]')).toBeNull();

    await user.click(screen.getByTestId("reader-split-toggle"));
    expect(document.querySelector('[data-testid="reader-split-divider"]')).not.toBeNull();
    expect(screen.getByText("reader.splitClose")).toBeTruthy();

    await user.click(screen.getByTestId("reader-split-toggle"));
    expect(document.querySelector('[data-testid="reader-split-divider"]')).toBeNull();
  });
});

// ── R48b/1643 号：编辑态光标拆分（R48a split 事务消费面）────────────
// 光标 UTF-16 偏移 = textarea.selectionStart（splitUtf16 契约同语义）；
// 光标在边界（0/文本尾）→ 提示不发请求；合法中段 → postApi 两参形态。
// ── R47/1640 号焦点链回归（1646 号走查 #9 定性面）：⌘P 命中→跳章→
// data-search-hit 段落高亮——同组件实例 chapterNumber 变化的 effect 链。
describe("ChapterReader 焦点高亮链（1646 号走查定性）", () => {
  it("chapterNumber 切换后 takeChapterFocus 定位命中段落", async () => {
    const { setChapterFocus } = await import("../lib/chapter-focus");
    setChapterFocus("b1", 2, "后续");
    render(<ChapterReader bookId="b1" chapterNumber={2} nav={nav} theme="light" t={t} />);
    // 分段探针：先证正文渲染面（命中段落存在），再证焦点属性面。
    await vi.waitFor(() => {
      expect(document.querySelector("article p")?.textContent).toContain("后续正文");
    }, { timeout: 3000 });
    await vi.waitFor(
      () => {
        expect(document.querySelector('[data-search-hit="true"]')).not.toBeNull();
      },
      { timeout: 3000 },
    );
    const hit = document.querySelector('[data-search-hit="true"]');
    expect(hit?.textContent).toContain("后续正文");
  });
});

describe("ChapterReader 光标拆分（R48b/1643 号）", () => {
  function renderChapter1() {
    return render(<ChapterReader bookId="b1" chapterNumber={1} nav={nav} theme="light" t={t} />);
  }

  async function enterEditAndPlaceCursor(user: ReturnType<typeof userEvent.setup>, offset: number) {
    await user.click(screen.getByText("reader.edit"));
    // 1643 号备案：页面含多只 textarea（ChapterWorkspacePanel 的 brief 编辑
    // 器先渲染、value 空）——编辑态主编辑器按「value 非空」甄别，且光标设
    // 置后须同步 fireEvent.click（userEvent 微任务链会重置 selection）。
    const area = [...document.querySelectorAll("textarea")].find(
      (candidate) => candidate.value.length > 0,
    ) as HTMLTextAreaElement;
    fireEvent.focus(area);
    area.setSelectionRange(offset, offset);
    return area;
  }

  it("编辑态拆分按钮：光标中段 → postApi 两参（path+offset）并回书目", async () => {
    const user = userEvent.setup();
    const alertMock = vi.spyOn(window, "alert").mockImplementation(() => {});
    try {
    renderChapter1();
    const area = await enterEditAndPlaceCursor(user, 8);
    expect(area.selectionStart).toBe(8);
    // jsdom 光标时序坑：userEvent.click 的微任务链会重置 textarea selection
    // （sanity 实证 selectionStart 归 0）——fireEvent 原生同步点击保 selection。
    fireEvent.click(screen.getByTestId("reader-split-at-cursor"));
    await vi.waitFor(() => {
      expect(postApiMock).toHaveBeenCalledWith("/books/b1/chapters/1/split", { offset: 8 });
    });
    expect(toBookMock).toHaveBeenCalledWith("b1");
    expect(alertMock).not.toHaveBeenCalled();
    } finally {
      alertMock.mockRestore();
    }
  });

  it("光标在文本尾边界 → 提示不请求（offset 无切点负路径）", async () => {
    const user = userEvent.setup();
    const alertMock = vi.spyOn(window, "alert").mockImplementation(() => {});
    try {
      renderChapter1();
      const area = await enterEditAndPlaceCursor(user, 0);
      area.selectionStart = area.value.length;
      await user.click(screen.getByTestId("reader-split-at-cursor"));
      expect(postApiMock).not.toHaveBeenCalledWith("/books/b1/chapters/1/split", expect.anything());
      expect(alertMock).toHaveBeenCalled();
    } finally {
      alertMock.mockRestore();
    }
  });
});
