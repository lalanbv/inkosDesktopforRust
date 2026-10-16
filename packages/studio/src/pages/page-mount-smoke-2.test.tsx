import { describe, expect, it, vi } from "vitest";
import { renderToString } from "react-dom/server";

/**
 * 738 号：页面挂载冒烟覆盖矩阵第二批——StyleManager / StoryPlayer /
 * WorkInspector / CreativeMethodsEditor / StoryGraphTree 五页（依赖
 * useApi/fetchJson 单点 + tr 双语 + 轻 props）。
 */

const useApiMock = vi.fn();
const fetchJsonMock = vi.fn();

vi.mock("@/hooks/use-api", () => ({
  useApi: (path: string) => useApiMock(path),
  fetchJson: (...args: unknown[]) => fetchJsonMock(...(args as [])),
  buildApiUrl: (path: string) => path,
}));

const nav = { toDashboard: () => undefined, toBook: () => undefined } as never;
const t = (key: string) => key;
const theme = "light" as const;

import { StyleManager } from "@/pages/StyleManager";
import { StoryPlayer } from "@/pages/StoryPlayer";
import { WorkInspector } from "@/pages/WorkInspector";
import { CreativeMethodsEditor } from "@/pages/CreativeMethodsEditor";
import { StoryGraphTree } from "@/pages/StoryGraphTree";

describe("page mount smoke — small pages batch 2 (738)", () => {
  it("StyleManager: 文风管理页挂载（分析表单骨架在位）", () => {
    useApiMock.mockImplementation(() => ({ data: undefined, refetch: () => undefined }));
    fetchJsonMock.mockResolvedValue({});
    const html = renderToString(<StyleManager nav={nav} theme={theme} t={t} />);
    expect(html).toContain("style.title");
  });

  it("StoryPlayer: 影游播放页挂载（空项目数据不崩）", () => {
    useApiMock.mockImplementation(() => ({ data: undefined, refetch: () => undefined }));
    fetchJsonMock.mockResolvedValue({});
    // data 未就绪时组件早退 null 分支——挂载不崩即通过（null 分支合法）。
    expect(() =>
      renderToString(<StoryPlayer projectId="film-1" nav={nav} theme={theme} t={t} />),
    ).not.toThrow();
  });

  it("WorkInspector: 工作检查器挂载（work 详情样本渲染）", () => {
    useApiMock.mockImplementation((path: string) =>
      path.startsWith("/works/")
        ? {
            data: {
              work: {
                id: "w1",
                title: "镜花缘·互动版",
                profileId: "interactive-film",
                status: "active",
                artifacts: [],
              },
              episodes: [],
            },
            refetch: () => undefined,
          }
        : { data: undefined, refetch: () => undefined },
    );
    const html = renderToString(
      <WorkInspector
        workId="w1"
        onBack={() => undefined}
        onChat={() => undefined}
      />,
    );
    expect(html).toContain("镜花缘");
  });

  it("CreativeMethodsEditor: 创作方法编辑器挂载（profiles 空态）", () => {
    useApiMock.mockImplementation(() => ({ data: { profiles: [] }, refetch: () => undefined }));
    fetchJsonMock.mockResolvedValue({});
    const html = renderToString(<CreativeMethodsEditor profileId="p1" />);
    expect(typeof html).toBe("string");
  });

  it("StoryGraphTree: 故事图树挂载（空分支数据不崩）", () => {
    useApiMock.mockImplementation(() => ({ data: undefined, refetch: () => undefined }));
    fetchJsonMock.mockResolvedValue({});
    // 同上：null 早退分支合法。
    expect(() =>
      renderToString(<StoryGraphTree projectId="film-1" nav={nav} theme={theme} t={t} />),
    ).not.toThrow();
  });
});
