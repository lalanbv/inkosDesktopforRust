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
});
