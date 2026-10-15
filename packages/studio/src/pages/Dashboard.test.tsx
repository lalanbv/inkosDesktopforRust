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

  it("keeps the dashboard body reachable when the works domain fails (712号)", () => {
    // 712 号：Rust 生产腿 /works 404（上游新增未迁域）此前把 worksQuery.error
    // 并入致命早退——书卡/写作数据/运行遥测/备份恢复面板在 Rust 腿整页不可达
    //（模拟真人走查实锤）。works 域失败须降级为无 otherWorks 分组。
    useApiMock.mockImplementation((path: string) =>
      path === "/works"
        ? { data: null, error: "404 Not Found", loading: false, refetch: vi.fn() }
        : {
          data: { books: [{ id: "b1", title: "镜花水月", genre: "玄幻", status: "active", chaptersWritten: 2 }] },
          error: null,
          loading: false,
          refetch: vi.fn(),
        },
    );

    const html = renderToString(<Dashboard nav={nav as never} sse={sse} theme="light" t={t} />);
    expect(html).not.toContain("Failed to load library");
    expect(html).toContain("镜花水月");
    // 备份恢复面板（R18/386）随主体在位
    expect(html).not.toContain("dash.noBooks");
  });

  it("still shows the library error when /books itself fails", () => {
    // 核心数据面失败仍致命（与降级面区分锁定）
    useApiMock.mockImplementation((path: string) =>
      path === "/works"
        ? { data: { works: [] }, error: null, loading: false, refetch: vi.fn() }
        : { data: null, error: "boom", loading: false, refetch: vi.fn() },
    );

    const html = renderToString(<Dashboard nav={nav as never} sse={sse} theme="light" t={t} />);
    expect(html).toContain("Failed to load library");
  });
});
