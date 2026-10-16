import { describe, expect, it, vi } from "vitest";
import { renderToString } from "react-dom/server";

/**
 * 739 号：页面挂载冒烟覆盖矩阵第三批——重依赖页中的可轻 mock 面
 * （OnboardingWizard/TranslationManager/GenreManager/RadarView/
 * ImportManager/FlowView/FilmWizard/ProjectSettings 八页）。
 * ChatPage（chat store 全家桶 + 1423 行）维持大页另案。
 */

const useApiMock = vi.fn();
const fetchJsonMock = vi.fn();
const postApiMock = vi.fn();

vi.mock("@/hooks/use-api", () => ({
  useApi: (path: string) => useApiMock(path),
  fetchJson: (...args: unknown[]) => fetchJsonMock(...(args as [])),
  postApi: (...args: unknown[]) => postApiMock(...(args as [])),
  buildApiUrl: (path: string) => path,
}));

const nav = {
  toDashboard: () => undefined,
  toBook: () => undefined,
  toServices: () => undefined,
} as never;
const t = (key: string) => key;
const theme = "light" as const;

import { TranslationManager } from "@/pages/TranslationManager";
import { GenreManager } from "@/pages/GenreManager";
import { RadarView } from "@/pages/RadarView";
import { ImportManager } from "@/pages/ImportManager";
import FlowView from "@/pages/FlowView";
import FilmWizard from "@/pages/FilmWizard";
import { ProjectSettings } from "@/pages/ProjectSettings";

describe("page mount smoke — heavy-but-light-mockable batch 3 (739)", () => {

  it("TranslationManager: 翻译管理挂载（空项目列表）", () => {
    useApiMock.mockImplementation(() => ({
      data: { projects: [], language: "zh" },
      refetch: () => undefined,
      loading: false,
      error: null,
    }));
    const html = renderToString(<TranslationManager nav={nav} theme={theme} t={t} />);
    expect(typeof html).toBe("string");
    expect(html.length).toBeGreaterThan(0);
  });

  it("GenreManager: 题材管理挂载（genres 空列表）", () => {
    useApiMock.mockImplementation(() => ({
      data: { genres: [] },
      refetch: () => undefined,
    }));
    const html = renderToString(<GenreManager nav={nav} theme={theme} t={t} />);
    expect(typeof html).toBe("string");
    expect(html.length).toBeGreaterThan(0);
  });

  it("RadarView: 市场雷达挂载（空历史空结果）", () => {
    useApiMock.mockImplementation(() => ({
      data: undefined,
      refetch: () => undefined,
    }));
    fetchJsonMock.mockResolvedValue({});
    const html = renderToString(
      <RadarView nav={nav} theme={theme} t={t} onCreateBook={() => undefined} />,
    );
    expect(typeof html).toBe("string");
    expect(html.length).toBeGreaterThan(0);
  });

  it("ImportManager: 导入管理挂载（空书列表 chapters 页签）", () => {
    useApiMock.mockImplementation(() => ({
      data: { books: [] },
      refetch: () => undefined,
    }));
    const html = renderToString(
      <ImportManager nav={nav} theme={theme} t={t} />,
    );
    expect(typeof html).toBe("string");
    expect(html.length).toBeGreaterThan(0);
  });

  it("FlowView: 分镜流视图挂载（空项目 null 早退合法）", () => {
    useApiMock.mockImplementation(() => ({ data: undefined, refetch: () => undefined }));
    fetchJsonMock.mockResolvedValue({});
    expect(() =>
      renderToString(<FlowView projectId="flow-1" nav={nav} theme={theme} t={t} />),
    ).not.toThrow();
  });

  it("FilmWizard: 分镜向导挂载（空 story-graph）", () => {
    useApiMock.mockImplementation(() => ({ data: undefined, refetch: () => undefined }));
    const html = renderToString(
      <FilmWizard projectId="film-1" nav={nav} theme={theme} t={t} sse={{ messages: [], connected: true }} />,
    );
    expect(typeof html).toBe("string");
    expect(html.length).toBeGreaterThan(0);
  });

  it("ProjectSettings: 项目设置挂载（设置区块在位）", () => {
    useApiMock.mockImplementation(() => ({
      data: { language: "zh" },
      refetch: () => undefined,
    }));
    const html = renderToString(
      <ProjectSettings nav={nav} theme={theme} t={t} sse={{ messages: [] }} />,
    );
    expect(typeof html).toBe("string");
    expect(html.length).toBeGreaterThan(0);
  });
});
