// @vitest-environment jsdom
//! 922 号：TranslationManager 写面交互——列表默认选中首项 + run 流
//! （POST /translations/:id/run body {batchSize:8} → 成功回执）+
//! 错误呈现。上传/create 链依赖 File 对象（jsdom 无 DataTransfer）不在此
//! 覆盖，备案留手动走查。
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { TranslationManager } from "./TranslationManager";

const { fetchJsonMock, refetchMock } = vi.hoisted(() => ({
  fetchJsonMock: vi.fn(),
  refetchMock: vi.fn(),
}));

vi.mock("../hooks/use-api", () => ({
  fetchJson: (path: string, init?: RequestInit) => fetchJsonMock(path, init),
  postApi: vi.fn(),
  invalidateApiPaths: vi.fn(),
  useApi: (url: string) => {
    if (url === "/translations") {
      return {
        data: { translations: [{ projectId: "p1", title: "试作翻译", sourceLanguage: "zh", targetLanguage: "en", chapters: 3 }] },
        loading: false,
        error: null,
        refetch: refetchMock,
      };
    }
    if (url === "/translations/p1") {
      return {
        data: {
          manifest: { projectId: "p1", title: "试作翻译", sourceLanguage: "zh", targetLanguage: "en", chapters: [{ number: 1, title: "首章", translatedSegments: 4 }] },
          report: "试作报告",
          chapters: [{ number: 1, title: "首章", translatedSegments: 4, segments: [{ index: 0, source: "源文", target: "译文" }] }],
        },
        loading: false,
        error: null,
        refetch: refetchMock,
      };
    }
    return { data: undefined, loading: false, error: null, refetch: refetchMock };
  },
}));

vi.mock("../hooks/use-colors", () => ({ useColors: () => ({ link: "", btnPrimary: "", btnSecondary: "", btnDanger: "", cardStatic: "" }) }));
vi.mock("../hooks/use-i18n", () => ({ useI18n: () => ({ lang: "zh" }) }));

const t = (key: string) => key;

const detailBody = () => ({
  manifest: { projectId: "p1", title: "试作翻译", sourceLanguage: "zh", targetLanguage: "en", chapters: [{ number: 1, title: "首章", translatedSegments: 4 }] },
  report: "试作报告",
  chapters: [{ number: 1, title: "首章", translatedSegments: 4, segments: [{ index: 0, source: "源文", target: "译文" }] }],
});

beforeEach(() => {
  fetchJsonMock.mockReset();
  refetchMock.mockReset();
  fetchJsonMock.mockImplementation((path: string, init?: RequestInit) => {
    if (path === "/translations/p1" && (!init?.method || init.method === "GET")) return Promise.resolve(detailBody());
    if (path === "/translations/p1/run" && init?.method === "POST") {
      return Promise.resolve({ translatedSegments: 12, reviewedChapters: 1, reportPath: "/report.md" });
    }
    return Promise.reject(new Error(`unrouted fetch: ${init?.method ?? "GET"} ${path}`));
  });
});

afterEach(() => cleanup());

describe("TranslationManager 写面交互（922 号）", () => {
  it("列表默认选中首项并拉取详情（manifest 章节渲染）", async () => {
    render(<TranslationManager nav={{ toDashboard: () => {} }} theme="dark" t={t} />);
    await waitFor(() => expect(screen.getByRole("button", { name: /translation\.run/ })).toBeTruthy());
    // 「试作翻译」同时出现在列表卡片与详情面板=列表选中与详情拉取双在位。
    expect(screen.getAllByText("试作翻译").length).toBeGreaterThanOrEqual(2);
    expect(screen.getAllByText(/首章/).length).toBeGreaterThanOrEqual(2); // 列表 manifest + 详情章节
  });

  it("run 流：POST /translations/p1/run body {batchSize:8} → 成功回执", async () => {
    render(<TranslationManager nav={{ toDashboard: () => {} }} theme="dark" t={t} />);
    await waitFor(() => expect(screen.getByRole("button", { name: /translation\.run/ })).toBeTruthy());
    fireEvent.click(screen.getByRole("button", { name: /translation\.run/ }));
    await waitFor(() => expect(fetchJsonMock.mock.calls.some(([path, init]) => path === "/translations/p1/run" && init?.method === "POST")).toBe(true));
    const run = fetchJsonMock.mock.calls.find(([path, init]) => path === "/translations/p1/run");
    expect(JSON.parse(String((run![1] as RequestInit).body))).toEqual({ batchSize: 8 });
  });

  it("run 失败 → error status 呈现", async () => {
    fetchJsonMock.mockImplementation((path: string, init?: RequestInit) => {
      if (init?.method === "POST" && path === "/translations/p1/run") return Promise.reject(new Error("translation worker offline"));
      if (path === "/translations/p1") return Promise.resolve(detailBody());
      return Promise.reject(new Error(`unrouted: ${path}`));
    });
    render(<TranslationManager nav={{ toDashboard: () => {} }} theme="dark" t={t} />);
    await waitFor(() => expect(screen.getByRole("button", { name: /translation\.run/ })).toBeTruthy());
    fireEvent.click(screen.getByRole("button", { name: /translation\.run/ }));
    await waitFor(() => expect(screen.getByText(/translation worker offline/)).toBeTruthy());
  });
});
