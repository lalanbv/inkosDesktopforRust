// @vitest-environment jsdom
//! 922 号：StyleManager 写面交互——分析流（POST /style/analyze 原文+默认
//! sourceName "sample"）与导入到书流（postApi /books/:id/style/import）+
//! 三态 notice 呈现。
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { StyleManager } from "./StyleManager";

const { fetchJsonMock, postApiMock } = vi.hoisted(() => ({ fetchJsonMock: vi.fn(), postApiMock: vi.fn() }));

vi.mock("../hooks/use-api", () => ({
  fetchJson: (path: string, init?: RequestInit) => fetchJsonMock(path, init),
  postApi: (path: string, body?: unknown) => postApiMock(path, body),
  useApi: () => ({
    data: { books: [{ id: "b1", title: "镜花水月" }] },
    loading: false,
    error: null,
    refetch: vi.fn(),
  }),
}));

vi.mock("../hooks/use-colors", () => ({ useColors: () => ({ link: "", btnPrimary: "", btnSecondary: "", btnDanger: "", cardStatic: "" }) }));
vi.mock("../hooks/use-i18n", () => ({ useI18n: () => ({ lang: "zh" }) }));

const t = (key: string) => key;

beforeEach(() => {
  fetchJsonMock.mockReset();
  postApiMock.mockReset();
  fetchJsonMock.mockImplementation((path: string, init?: RequestInit) => {
    if (path === "/style/analyze" && init?.method === "POST") {
      return Promise.resolve({ guide: "风格指南正文（mock 分析产物）" });
    }
    return Promise.reject(new Error(`unrouted fetch: ${init?.method ?? "GET"} ${path}`));
  });
  postApiMock.mockResolvedValue({ ok: true });
});

afterEach(() => cleanup());

describe("StyleManager 写面交互（922 号）", () => {
  it("分析流：POST /style/analyze body 含 text+默认 sourceName，产物 guide 渲染并出现导入区", async () => {
    render(<StyleManager nav={{ toDashboard: () => {} }} theme="dark" t={t} />);
    const analyze = screen.getByRole("button", { name: /style\.analyze/ });
    expect((analyze as HTMLButtonElement).disabled).toBe(true); // 空文本守卫
    fireEvent.change(screen.getByPlaceholderText(/style\.pasteHint/), { target: { value: "样例正文" } });
    fireEvent.click(analyze);
    await waitFor(() => expect(screen.getByText(/风格指南正文（mock 分析产物）/)).toBeTruthy());
    const post = fetchJsonMock.mock.calls.find(([path]) => path === "/style/analyze");
    expect(JSON.parse(String((post![1] as RequestInit).body))).toEqual({ text: "样例正文", sourceName: "sample" });
    expect(screen.getByText(/style\.importToBook/)).toBeTruthy();
  });

  it("导入到书：postApi /books/:id/style/import body 一致 + success 回执", async () => {
    render(<StyleManager nav={{ toDashboard: () => {} }} theme="dark" t={t} />);
    fireEvent.change(screen.getByPlaceholderText(/style\.pasteHint/), { target: { value: "样例正文" } });
    fireEvent.click(screen.getByRole("button", { name: /style\.analyze/ }));
    await waitFor(() => expect(screen.getByText(/风格指南正文/)).toBeTruthy());
    fireEvent.change(screen.getByRole("combobox"), { target: { value: "b1" } });
    fireEvent.click(screen.getByRole("button", { name: /style\.importGuide/ }));
    await waitFor(() => expect(screen.getByText(/Style guide imported successfully!/)).toBeTruthy());
    expect(postApiMock.mock.calls.some(([path, body]) => path === "/books/b1/style/import" && JSON.stringify(body) === JSON.stringify({ text: "样例正文", sourceName: "sample" }))).toBe(true);
  });

  it("分析失败 → error notice destructive 形态", async () => {
    fetchJsonMock.mockRejectedValue(new Error("style engine offline"));
    render(<StyleManager nav={{ toDashboard: () => {} }} theme="dark" t={t} />);
    fireEvent.change(screen.getByPlaceholderText(/style\.pasteHint/), { target: { value: "样例正文" } });
    fireEvent.click(screen.getByRole("button", { name: /style\.analyze/ }));
    await waitFor(() => expect(screen.getByText(/style engine offline/)).toBeTruthy());
    expect(screen.getByText(/style engine offline/).className).toContain("text-destructive");
  });
});
