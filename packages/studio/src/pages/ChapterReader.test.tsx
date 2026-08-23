import { describe, expect, it, vi, beforeEach } from "vitest";
import { renderToString } from "react-dom/server";
import { ChapterReader } from "@/pages/ChapterReader";

const useApiMock = vi.fn();

vi.mock("@/hooks/use-api", () => ({
  useApi: (path: string) => useApiMock(path),
  fetchJson: vi.fn(),
  postApi: vi.fn(),
}));

const nav = new Proxy({}, { get: () => vi.fn() }) as Record<string, () => void>;
const t = (key: string) => key;

describe("ChapterReader loading skeleton", () => {
  beforeEach(() => {
    useApiMock.mockReset();
  });

  it("shows paragraph skeletons while the chapter is loading", () => {
    useApiMock.mockReturnValue({ data: null, error: null, loading: true, refetch: vi.fn() });

    const html = renderToString(
      <ChapterReader bookId="b1" chapterNumber={1} nav={nav as never} theme="light" t={t} />,
    );
    expect(html).toContain('data-loading="skeleton"');
    expect(html).toContain('data-slot="skeleton-paragraphs"');
    expect(html).not.toContain("animate-spin");
  });

  it("renders the manuscript once data arrives", () => {
    useApiMock.mockImplementation((path: string) => {
      if (path.startsWith("/books/b1/chapters/1")) {
        return {
          data: { chapterNumber: 1, filename: "chapter-001.md", content: "# 第一章 灯下\n\n墨迹未干，故事已开。" },
          error: null,
          loading: false,
          refetch: vi.fn(),
        };
      }
      // 工作台面板等其他路径：数据未就绪
      return { data: null, error: null, loading: false, refetch: vi.fn() };
    });

    const html = renderToString(
      <ChapterReader bookId="b1" chapterNumber={1} nav={nav as never} theme="light" t={t} />,
    );
    expect(html).not.toContain('data-loading="skeleton"');
    expect(html).toContain("第一章 灯下");
  });
});
