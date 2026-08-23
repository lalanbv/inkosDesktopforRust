import { describe, expect, it, vi, beforeEach } from "vitest";
import { renderToString } from "react-dom/server";
import type { SSEMessage } from "@/hooks/use-sse";
import { Sidebar } from "@/components/Sidebar";

// renderToString 不跑 effect，useDelayedVisible 恒为 false；测试里直接
// 把延迟出现视为已过阈值，验证"超过阈值后骨架可见"这一终态。
vi.mock("@/components/skeletons", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/components/skeletons")>()),
  useDelayedVisible: () => true,
}));

const useApiMock = vi.fn();

vi.mock("@/hooks/use-api", () => ({
  useApi: (path: string) => useApiMock(path),
}));

const nav = new Proxy({}, { get: () => vi.fn() }) as Record<string, () => void>;
const t = (key: string) => key;
const sse = { messages: [] as ReadonlyArray<SSEMessage> };

function occurrences(html: string, needle: string): number {
  return html.split(needle).length - 1;
}

describe("Sidebar bookshelf skeleton", () => {
  beforeEach(() => {
    useApiMock.mockReset();
  });

  it("shows row skeletons while /books and /interactive-films are not ready", () => {
    useApiMock.mockImplementation((path: string) => {
      if (path === "/books" || path === "/interactive-films") {
        return { data: null, error: null, loading: true, refetch: vi.fn(), mutate: vi.fn() };
      }
      return { data: null, error: null, loading: false, refetch: vi.fn(), mutate: vi.fn() };
    });

    const html = renderToString(<Sidebar nav={nav as never} activePage="dashboard" sse={sse} t={t} />);
    expect(html).toContain('data-loading="skeleton"');
    expect(occurrences(html, 'data-slot="skeleton-rows"')).toBe(2);
    // 书架 6 行 + 影游 3 行。行图标类组合（size-8 shrink-0 rounded-full）为骨架行
    // 专属；会话区 EmptyState 的图标泡也是 rounded-full，不能按裸类名全局计数。
    expect(occurrences(html, "size-8 shrink-0 rounded-full")).toBe(9);
    // 未就绪时不得误报"书架空空如也"
    expect(html).not.toContain("dash.noBooks");
  });

  it("renders the empty state once data is ready with zero books", () => {
    useApiMock.mockImplementation((path: string) => {
      if (path === "/books") {
        return { data: { books: [] }, error: null, loading: false, refetch: vi.fn(), mutate: vi.fn() };
      }
      if (path === "/interactive-films") {
        return { data: { films: [] }, error: null, loading: false, refetch: vi.fn(), mutate: vi.fn() };
      }
      return { data: null, error: null, loading: false, refetch: vi.fn(), mutate: vi.fn() };
    });

    const html = renderToString(<Sidebar nav={nav as never} activePage="dashboard" sse={sse} t={t} />);
    expect(html).not.toContain('data-loading="skeleton"');
    expect(html).toContain("dash.noBooks");
  });

  it("zone=create renders only creation sections (P3-1 按区渲染)", () => {
    useApiMock.mockImplementation((path: string) => {
      if (path === "/books" || path === "/interactive-films") {
        return { data: null, error: null, loading: true, refetch: vi.fn(), mutate: vi.fn() };
      }
      return { data: null, error: null, loading: false, refetch: vi.fn(), mutate: vi.fn() };
    });

    const html = renderToString(
      <Sidebar nav={nav as never} activePage="dashboard" sse={sse} t={t} zone="create" />,
    );
    expect(html).toContain("nav.createSection");
    expect(html).toContain("nav.myBooks");
    expect(html).toContain("nav.history");
    // 其它三区不出现
    expect(html).not.toContain("nav.tools");
    expect(html).not.toContain("nav.system");
    expect(html).not.toContain("film-projects-section");
  });

  it("zone=tools merges genres into the tools group and hides manage items", () => {
    useApiMock.mockImplementation((path: string) => {
      if (path === "/books" || path === "/interactive-films") {
        return { data: null, error: null, loading: false, refetch: vi.fn(), mutate: vi.fn() };
      }
      return { data: null, error: null, loading: false, refetch: vi.fn(), mutate: vi.fn() };
    });

    const html = renderToString(
      <Sidebar nav={nav as never} activePage="genres" sse={sse} t={t} zone="tools" />,
    );
    expect(html).toContain("nav.tools");
    expect(html).toContain("create.genre");
    expect(html).not.toContain("nav.system");
    expect(html).not.toContain("nav.config");
    expect(html).not.toContain("nav.createSection");
  });

  it("zone=manage shows system items without genres, zone=film shows film projects", () => {
    useApiMock.mockImplementation((path: string) => {
      if (path === "/books" || path === "/interactive-films") {
        return { data: null, error: null, loading: false, refetch: vi.fn(), mutate: vi.fn() };
      }
      return { data: null, error: null, loading: false, refetch: vi.fn(), mutate: vi.fn() };
    });

    const manageHtml = renderToString(
      <Sidebar nav={nav as never} activePage="services" sse={sse} t={t} zone="manage" />,
    );
    expect(manageHtml).toContain("nav.system");
    expect(manageHtml).toContain("nav.config");
    expect(manageHtml).not.toContain("create.genre");
    expect(manageHtml).not.toContain("nav.tools");

    const filmHtml = renderToString(
      <Sidebar nav={nav as never} activePage="film-studio:p1" sse={sse} t={t} zone="film" />,
    );
    expect(filmHtml).toContain("film-projects-section");
    expect(filmHtml).not.toContain("nav.createSection");
  });
});
