import { describe, expect, it, vi, beforeEach } from "vitest";
import { renderToString } from "react-dom/server";
import type { SSEMessage } from "@/hooks/use-sse";
import { Dashboard } from "@/pages/Dashboard";

const useApiMock = vi.fn();

vi.mock("@/hooks/use-api", () => ({
  useApi: (path: string) => useApiMock(path),
  fetchJson: vi.fn(),
  postApi: vi.fn(),
}));

const nav = new Proxy({}, { get: () => vi.fn() }) as Record<string, () => void>;
const t = (key: string) => key;
const sse = { messages: [] as ReadonlyArray<SSEMessage> };

describe("Dashboard loading skeleton", () => {
  beforeEach(() => {
    useApiMock.mockReset();
  });

  it("shows card skeletons while the library is loading without data", () => {
    useApiMock.mockReturnValue({ data: null, error: null, loading: true, refetch: vi.fn() });

    const html = renderToString(<Dashboard nav={nav as never} sse={sse} theme="light" t={t} />);
    expect(html).toContain('data-loading="skeleton"');
    expect(html).toContain('data-slot="skeleton-cards"');
    expect(html).not.toContain("animate-spin");
  });

  it("keeps showing content during background refetch instead of flashing a skeleton", () => {
    useApiMock.mockReturnValue({
      data: { books: [{ id: "b1", title: "长夜余火", genre: "玄幻", status: "writing", chaptersWritten: 3 }] },
      error: null,
      loading: true,
      refetch: vi.fn(),
    });

    const html = renderToString(<Dashboard nav={nav as never} sse={sse} theme="light" t={t} />);
    expect(html).not.toContain('data-loading="skeleton"');
    expect(html).toContain("长夜余火");
  });

  it("keeps a persistent series-backfill CTA in the header for populated libraries (193号)", () => {
    // 193 号：回填入口此前只在系列分组 header 出现——无 series 字段的散书
    // 项目在 Dashboard 上不可见；页头常驻按钮修复可发现性。
    useApiMock.mockReturnValue({
      data: { books: [{ id: "b1", title: "散书一本", genre: "玄幻", status: "active", chaptersWritten: 2 }] },
      error: null,
      loading: false,
      refetch: vi.fn(),
    });

    const html = renderToString(<Dashboard nav={nav as never} sse={sse} theme="light" t={t} />);
    expect(html).toContain('data-slot="dashboard-backfill-cta"');
    expect(html).toContain("dash.backfillEntry");
  });

  it("renders the empty library state when data is ready and empty", () => {
    useApiMock.mockReturnValue({ data: { books: [] }, error: null, loading: false, refetch: vi.fn() });

    const html = renderToString(<Dashboard nav={nav as never} sse={sse} theme="light" t={t} />);
    expect(html).not.toContain('data-loading="skeleton"');
    expect(html).toContain("dash.noBooks");
  });
});
