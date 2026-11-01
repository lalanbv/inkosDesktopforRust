import { describe, expect, it, vi } from "vitest";
import { renderToString } from "react-dom/server";

/**
 * 737 号：页面挂载冒烟覆盖矩阵第一批——TruthFiles / DaemonControl /
 * Analytics / LogViewer 四小页（依赖仅 useApi 单点 + 轻 props）。
 * 覆盖矩阵审计实锤四页零直接测试；大页（ChatPage 1423 行等）依赖
 * chat store 全家桶，mock 成本另案。
 */

const useApiMock = vi.fn();
const postApiMock = vi.fn();

vi.mock("@/hooks/use-api", () => ({
  useApi: (path: string) => useApiMock(path),
  postApi: (...args: unknown[]) => postApiMock(...(args as [])),
  fetchJson: vi.fn(),
}));

const nav = {
  toDashboard: () => undefined,
  toServices: () => undefined,
  toBook: () => undefined,
} as Parameters<typeof TruthFiles>[0]["nav"];
const t = (key: string) => key;
const theme = "light" as const;

import { TruthFiles } from "@/pages/TruthFiles";
import { DaemonControl } from "@/pages/DaemonControl";
import { Analytics } from "@/pages/Analytics";
import { LogViewer } from "@/pages/LogViewer";

describe("page mount smoke — small pages batch 1 (737)", () => {
  it("TruthFiles: 真相文件列表渲染 + 条目在位", () => {
    useApiMock.mockReturnValue({
      data: {
        files: [
          { name: "story_bible.md", size: 128, preview: "# 故事圣经\n\n设定正文。" },
          { name: "pending_hooks.md", size: 16, preview: "# 伏笔池" },
        ],
      },
      refetch: () => undefined,
    });
    const html = renderToString(
      <TruthFiles bookId="b1" nav={nav} theme={theme} t={t} />,
    );
    // 列表条目名渲染（preview 为点选后详情面板，初始未选中不渲染）。
    expect(html).toContain("story_bible.md");
    expect(html).toContain("pending_hooks.md");
  });

  it("DaemonControl: 守护状态与事件日志渲染（sse 空流）", () => {
    useApiMock.mockImplementation((path: string) =>
      path === "/daemon"
        ? { data: { running: true }, refetch: () => undefined }
        : { data: { items: [] }, refetch: () => undefined },
    );
    const html = renderToString(
      <DaemonControl nav={nav} theme={theme} t={t} sse={{ messages: [] }} />,
    );
    expect(html).toContain("daemon");
  });

  it("Analytics: 分析指标渲染（样本数据）", () => {
    useApiMock.mockReturnValue({
      data: {
        bookId: "b1",
        totalChapters: 3,
        totalWords: 7200,
        avgWordsPerChapter: 2400,
        auditPassRate: 100,
        // 792 号：形态对齐 Rust compute_analytics 权威（observation 两键已删）。
        topIssueCategories: [{ category: "pacing", count: 2 }],
        chaptersWithMostIssues: [{ chapter: 1, issueCount: 2 }],
        statusDistribution: { "ready-for-review": 2, drafted: 1 },
      },
      refetch: () => undefined,
    });
    const html = renderToString(
      <Analytics bookId="b1" nav={nav} theme={theme} t={t} />,
    );
    expect(html).toContain("analytics");
  });

  it("LogViewer: 日志条目渲染 + 空数据不崩", () => {
    useApiMock.mockImplementation((path: string) =>
      path === "/logs"
        ? {
            data: {
              entries: [
                { level: "info", tag: "studio", message: "引擎就绪", timestamp: Date.now() },
              ],
            },
            refetch: () => undefined,
          }
        : { data: undefined, refetch: () => undefined },
    );
    const html = renderToString(<LogViewer nav={nav} theme={theme} t={t} />);
    expect(html).toContain("logs");
  });
});
