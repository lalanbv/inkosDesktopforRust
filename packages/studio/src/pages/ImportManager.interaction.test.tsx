// @vitest-environment jsdom
//! 911 号：ImportManager 写面交互——章节导入主路径（POST body+success/error
//! 三态呈现）与番外 init 全链（postApi→waitForStudioBookReady 全局 fetch
//! stub→invalidate+nav.toBook 导航）。
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { ImportManager } from "./ImportManager";

const { fetchJsonMock, postApiMock, invalidateMock, toBookMock } = vi.hoisted(() => ({
  fetchJsonMock: vi.fn(),
  postApiMock: vi.fn(),
  invalidateMock: vi.fn(),
  toBookMock: vi.fn(),
}));

vi.mock("../hooks/use-api", () => ({
  fetchJson: (path: string, init?: RequestInit) => fetchJsonMock(path, init),
  postApi: (path: string, body?: unknown) => postApiMock(path, body),
  invalidateApiPaths: (paths: ReadonlyArray<string>) => invalidateMock(paths),
  useApi: (url: string) => {
    if (url === "/books") {
      return {
        data: { books: [{ id: "b1", title: "镜花水月" }] },
        loading: false,
        error: null,
        refetch: vi.fn(),
      };
    }
    return { data: undefined, loading: false, error: null, refetch: vi.fn() };
  },
}));

vi.mock("../hooks/use-colors", () => ({ useColors: () => ({ link: "", btnPrimary: "", btnSecondary: "", btnDanger: "", cardStatic: "" }) }));
vi.mock("../hooks/use-i18n", () => ({ useI18n: () => ({ lang: "zh" }) }));

const t = (key: string) => key;

beforeEach(() => {
  fetchJsonMock.mockReset();
  postApiMock.mockReset();
  invalidateMock.mockReset();
  toBookMock.mockReset();
  // waitForStudioBookReady 默认 fetchImpl 的就绪轮询（返回即就绪形态）。
  vi.stubGlobal("fetch", vi.fn(async () => ({
    ok: true,
    json: async () => ({ book: { id: "nb1" }, chapters: [], nextChapter: 1 }),
  }) as unknown as Response));
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe("ImportManager 写面交互（911 号）", () => {
  it("章节导入：POST body {text,splitRegex} + success 回执（Imported N chapters）", async () => {
    fetchJsonMock.mockResolvedValue({ importedCount: 3 });
    render(<ImportManager nav={{ toDashboard: () => {}, toBook: toBookMock }} theme="dark" t={t} />);
    fireEvent.change(screen.getByLabelText(/import\.selectTarget/), { target: { value: "b1" } });
    fireEvent.change(screen.getByPlaceholderText(/import\.pasteChapters/), { target: { value: "第一章 开端\n第二章 转折" } });
    fireEvent.click(screen.getAllByRole("button", { name: /import\.chapters/ }).at(-1)!);
    await waitFor(() => expect(screen.getByText(/Imported 3 chapters/)).toBeTruthy());
    const post = fetchJsonMock.mock.calls.find(([path]) => String(path).includes("/import/chapters"));
    expect(post).toBeTruthy();
    expect(JSON.parse(String((post![1] as RequestInit).body))).toEqual({
      text: "第一章 开端\n第二章 转折",
      splitRegex: undefined,
    });
  });

  it("章节导入失败 → error 三态呈现（destructive 形态+消息透传）", async () => {
    fetchJsonMock.mockRejectedValue(new Error("chapter split failed"));
    render(<ImportManager nav={{ toDashboard: () => {}, toBook: toBookMock }} theme="dark" t={t} />);
    fireEvent.change(screen.getByLabelText(/import\.selectTarget/), { target: { value: "b1" } });
    fireEvent.change(screen.getByPlaceholderText(/import\.pasteChapters/), { target: { value: "正文" } });
    fireEvent.click(screen.getAllByRole("button", { name: /import\.chapters/ }).at(-1)!);
    await waitFor(() => expect(screen.getByText(/chapter split failed/)).toBeTruthy());
    const banner = screen.getByText(/chapter split failed/);
    expect(banner.className).toContain("text-destructive");
  });

  it("番外 init 全链：postApi body → 就绪轮询 → invalidate + nav.toBook 导航", async () => {
    postApiMock.mockResolvedValue({ bookId: "nb1" });
    render(<ImportManager nav={{ toDashboard: () => {}, toBook: toBookMock }} theme="dark" t={t} initialTab="spinoff" />);
    fireEvent.change(screen.getByPlaceholderText(/import\.spinoffTitle/), { target: { value: "番外：坊市旧事" } });
    fireEvent.change(screen.getByRole("combobox"), { target: { value: "b1" } });
    fireEvent.change(screen.getByPlaceholderText(/import\.spinoffDirection/), { target: { value: "配角视角" } });
    fireEvent.click(screen.getAllByRole("button", { name: /import\.spinoff/ }).at(-1)!);
    await waitFor(() => expect(toBookMock).toHaveBeenCalledWith("nb1"));
    expect(postApiMock.mock.calls.some(([path, body]) => path === "/spinoff/init" && JSON.stringify(body).includes("坊市旧事"))).toBe(true);
    expect(invalidateMock).toHaveBeenCalled();
    // 成功回执（spinoffDone: nb1）。
    expect(screen.getByText(/import\.spinoffDone/).textContent).toContain("nb1");
  });

  it("同人 init 全链：POST /fanfic/init body 五字段 → fanficDone 导航", async () => {
    fetchJsonMock.mockImplementation((path: string, init?: RequestInit) => {
      if (path === "/fanfic/init" && init?.method === "POST") return Promise.resolve({ bookId: "nb2" });
      return Promise.reject(new Error(`unrouted: ${init?.method ?? "GET"} ${path}`));
    });
    render(<ImportManager nav={{ toDashboard: () => {}, toBook: toBookMock }} theme="dark" t={t} initialTab="fanfic" />);
    fireEvent.change(screen.getByPlaceholderText(/import\.fanficTitle/), { target: { value: "同人：坊市之女" } });
    fireEvent.change(screen.getByPlaceholderText(/import\.pasteMaterial/), { target: { value: "同人正文素材" } });
    fireEvent.click(screen.getAllByRole("button", { name: /import\.fanfic/ }).at(-1)!);
    await waitFor(() => expect(toBookMock).toHaveBeenCalledWith("nb2"));
    const fanfic = fetchJsonMock.mock.calls.find(([path, init]) => path === "/fanfic/init");
    expect(fanfic).toBeTruthy();
    // body 五字段契约：title/sourceText/mode/genre/language（mode/genre 取默认首项）。
    expect(JSON.parse(String((fanfic![1] as RequestInit).body))).toEqual({
      title: "同人：坊市之女",
      sourceText: "同人正文素材",
      mode: "canon",
      genre: "other",
      language: "zh",
    });
    expect(screen.getByText(/import\.fanficDone/).textContent).toContain("nb2");
    expect(invalidateMock).toHaveBeenCalled();
  });

  it("仿写 init 全链：postApi /imitation/init body 四字段 → imitationDone 导航", async () => {
    postApiMock.mockResolvedValue({ bookId: "nb3" });
    render(<ImportManager nav={{ toDashboard: () => {}, toBook: toBookMock }} theme="dark" t={t} initialTab="imitation" />);
    fireEvent.change(screen.getByPlaceholderText(/import\.imitationTitle/), { target: { value: "仿写：市井长卷" } });
    fireEvent.change(screen.getByPlaceholderText(/import\.imitationIdea/), { target: { value: "保留双线结构" } });
    fireEvent.change(screen.getByPlaceholderText(/import\.imitationRef/), { target: { value: "参考文本全文" } });
    fireEvent.click(screen.getAllByRole("button", { name: /import\.imitation/ }).at(-1)!);
    await waitFor(() => expect(toBookMock).toHaveBeenCalledWith("nb3"));
    const imitation = postApiMock.mock.calls.find(([path]) => path === "/imitation/init");
    expect(imitation![1]).toEqual({
      title: "仿写：市井长卷",
      referenceText: "参考文本全文",
      storyIdea: "保留双线结构",
      genre: "other",
      language: "zh",
    });
    expect(screen.getByText(/import\.imitationDone/).textContent).toContain("nb3");
  });

  it("母本导入（book 来源）：postApi /books/:id/import/canon body {fromBookId} + 成功回执", async () => {
    render(<ImportManager nav={{ toDashboard: () => {}, toBook: toBookMock }} theme="dark" t={t} initialTab="canon" />);
    // canon 段两个 combobox：第一个=canonFrom（来源书），第二个=canonTarget（衍生目标书）。
    const selects = screen.getAllByRole("combobox");
    fireEvent.change(selects[0]!, { target: { value: "b1" } });
    fireEvent.change(selects[1]!, { target: { value: "b1" } });
    fireEvent.click(screen.getAllByRole("button", { name: /import\.canon/ }).at(-1)!);
    await waitFor(() => expect(postApiMock.mock.calls.some(([path, body]) => path === "/books/b1/import/canon" && JSON.stringify(body) === JSON.stringify({ fromBookId: "b1" }))).toBe(true));
    // tr 真双语：zh 环境回执为中文文案。
    expect(screen.getByText(/母本导入成功|Canon imported successfully/)).toBeTruthy();
  });
});
