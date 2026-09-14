// @vitest-environment jsdom
//! 458 号：ChapterReader 动作链交互测试——通过/驳回调用与导航、对照分屏
//! 开关（divider 出现/消失）。
import { afterEach, describe, expect, it, vi, beforeEach } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ChapterReader } from "@/pages/ChapterReader";
import type { Nav } from "@/lib/nav";
import type { TFunction } from "@/hooks/use-i18n";

const postApiMock = vi.fn(async (path: string) => ({}));
const toBookMock = vi.fn();

const useApiMock = vi.fn<(path: string) => unknown>((path: string) => {
  if (path === "/books/b1") {
    return { data: { book: { id: "b1", title: "冒烟书" } }, error: null, loading: false, refetch: vi.fn() };
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
  postApi: (path: string) => postApiMock(path),
}));

const nav = { toBook: toBookMock } as unknown as Nav;
const t = ((key: string) => key) as TFunction;

beforeEach(() => {
  postApiMock.mockClear();
  toBookMock.mockClear();
});

afterEach(() => cleanup());

describe("ChapterReader 动作链（458 号）", () => {
  it("通过——POST approve 并导航回书页", async () => {
    const user = userEvent.setup();
    render(<ChapterReader bookId="b1" chapterNumber={1} nav={nav} theme="light" t={t} />);
    await user.click(screen.getByText("reader.approve"));
    expect(postApiMock).toHaveBeenCalledWith("/books/b1/chapters/1/approve");
    expect(toBookMock).toHaveBeenCalledWith("b1");
  });

  it("驳回——POST reject 并导航回书页", async () => {
    const user = userEvent.setup();
    render(<ChapterReader bookId="b1" chapterNumber={1} nav={nav} theme="light" t={t} />);
    await user.click(screen.getByText("reader.reject"));
    expect(postApiMock).toHaveBeenCalledWith("/books/b1/chapters/1/reject");
    expect(toBookMock).toHaveBeenCalledWith("b1");
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
