// @vitest-environment jsdom
//! 457 号：ChapterWorkspacePanel 交互测试——本章提示词保存（PUT /brief 载荷）
//! 与灵感抽卡（POST /inspiration 卡片渲染）链路锁定。
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ChapterWorkspacePanel } from "./ChapterWorkspacePanel";
import type { TFunction } from "../hooks/use-i18n";

const t = ((key: string) => key) as TFunction;

const workspace = {
  chapterNumber: 1,
  brief: "保留碎镜钥匙设定",
  plan: "第 3 章系统计划",
  versions: [{ id: "v1", source: "manual", createdAt: "2026-09-15T00:00:00Z", characterCount: 61 }],
  canDelete: true,
};

const fetchJsonMock = vi.fn(async (path: string, init?: { method?: string; body?: string }) => {
  if (path.endsWith("/brief") && init?.method === "PUT") {
    return JSON.parse(init.body ?? "{}");
  }
  if (path.endsWith("/inspiration") && init?.method === "POST") {
    return { card: "灵感卡：让镜灵在雨夜首次开口。" };
  }
  if (path.includes("/versions/v1") && !path.includes("/restore")) {
    return { content: "旧版本全文内容。" };
  }
  if (path.includes("/restore")) {
    return {};
  }
  if (path.includes("/rewrite/")) {
    return {};
  }
  return {};
});

const useApiMock = vi.fn((_path: string) => ({
  data: workspace,
  loading: false,
  error: null,
  refetch: vi.fn(async () => {}),
}));

vi.mock("../hooks/use-api", () => ({
  fetchJson: (path: string, init?: { method?: string; body?: string }) => fetchJsonMock(path, init),
  useApi: (path: string) => useApiMock(path),
}));

afterEach(() => {
  cleanup();
  fetchJsonMock.mockClear();
  useApiMock.mockClear();
});

function renderPanel(onChapterChanged = vi.fn(), onChapterDeleted = vi.fn()) {
  return render(
    <ChapterWorkspacePanel
      bookId="b1"
      chapterNumber={1}
      t={t}
      onChapterChanged={onChapterChanged}
      onChapterDeleted={onChapterDeleted}
    />,
  );
}

describe("ChapterWorkspacePanel 交互（457 号）", () => {
  it("渲染工作台数据：brief 回填 + 系统计划 + 历史版本", () => {
    renderPanel();
    const brief = document.querySelector("textarea") as HTMLTextAreaElement;
    expect(brief.value).toBe("保留碎镜钥匙设定");
    expect(document.body.textContent).toContain("第 3 章系统计划");
    // 历史版本行渲染 source 与字数（不展示裸 id）
    expect(document.body.textContent).toContain("reader.versionHistory");
    expect(document.body.textContent).toContain("manual");
  });

  it("编辑提示词保存——PUT /brief 载荷为编辑后文本", async () => {
    const user = userEvent.setup();
    renderPanel();
    const brief = document.querySelector("textarea") as HTMLTextAreaElement;
    await user.clear(brief);
    await user.type(brief, "新增：本章不许出现现代词汇");
    const save = Array.from(document.querySelectorAll("button")).find(
      (b) => b.textContent?.trim() === "reader.saveBrief",
    ) as HTMLButtonElement;
    await user.click(save);
    await vi.waitFor(() => {
      const putCall = fetchJsonMock.mock.calls.find(
        ([path, init]) => path.endsWith("/brief") && (init as { method?: string })?.method === "PUT",
      );
      expect(putCall).toBeDefined();
      expect(JSON.parse((putCall![1] as { body: string }).body).brief).toBe(
        "新增：本章不许出现现代词汇",
      );
    });
  });

  it("灵感抽卡——POST /inspiration 并渲染卡片", async () => {
    const user = userEvent.setup();
    renderPanel();
    const draw = Array.from(document.querySelectorAll("button")).find(
      (b) => b.textContent?.trim() === "reader.inspiration",
    ) as HTMLButtonElement;
    await user.click(draw);
    await vi.waitFor(() => {
      expect(document.body.textContent).toContain("灵感卡：让镜灵在雨夜首次开口。");
    });
    const inspirationCall = fetchJsonMock.mock.calls.find(([path]) =>
      path.endsWith("/inspiration"),
    );
    expect(inspirationCall).toBeDefined();
  });

  it("按提示重写——POST /rewrite 载荷含当前 brief 并通知变更（462 号）", async () => {
    const user = userEvent.setup();
    const onChapterChanged = vi.fn();
    renderPanel(onChapterChanged);
    const brief = document.querySelector("textarea") as HTMLTextAreaElement;
    await user.clear(brief);
    await user.type(brief, "重写提示：保留碎镜钥匙设定");
    const rewrite = Array.from(document.querySelectorAll("button")).find(
      (b) => b.textContent?.trim() === "reader.rewriteFromBrief",
    ) as HTMLButtonElement;
    await user.click(rewrite);
    await vi.waitFor(() => {
      const rewriteCall = fetchJsonMock.mock.calls.find(
        ([path, init]) => path.includes("/rewrite/") && (init as { method?: string })?.method === "POST",
      );
      expect(rewriteCall).toBeDefined();
      expect(JSON.parse((rewriteCall![1] as { body: string }).body).brief).toBe(
        "重写提示：保留碎镜钥匙设定",
      );
      expect(onChapterChanged).toHaveBeenCalled();
    });
  });

  it("历史版本预览——查看版本拉取旧版全文渲染（462 号）", async () => {
    const user = userEvent.setup();
    renderPanel();
    const view = Array.from(document.querySelectorAll("button")).find(
      (b) => b.textContent?.trim() === "reader.viewVersion",
    ) as HTMLButtonElement;
    await user.click(view);
    await vi.waitFor(() => {
      expect(document.body.textContent).toContain("旧版本全文内容。");
    });
  });

  it("恢复版本——confirm 确认后 POST restore 并通知变更（462 号）", async () => {
    const confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(true);
    const user = userEvent.setup();
    const onChapterChanged = vi.fn();
    renderPanel(onChapterChanged);
    const restore = Array.from(document.querySelectorAll("button")).find(
      (b) => b.textContent?.trim() === "reader.restoreVersion",
    ) as HTMLButtonElement;
    await user.click(restore);
    await vi.waitFor(() => {
      expect(document.body.textContent).toContain("reader.restoreComplete");
    });
    const restoreCall = fetchJsonMock.mock.calls.find(
      ([path, init]) => path.includes("/restore") && (init as { method?: string })?.method === "POST",
    );
    expect(restoreCall).toBeDefined();
    expect(onChapterChanged).toHaveBeenCalled();
    confirmSpy.mockRestore();
  });

  it("恢复取消——confirm 拒绝则不调用 restore（462 号）", async () => {
    const confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(false);
    const user = userEvent.setup();
    renderPanel();
    const restore = Array.from(document.querySelectorAll("button")).find(
      (b) => b.textContent?.trim() === "reader.restoreVersion",
    ) as HTMLButtonElement;
    await user.click(restore);
    expect(fetchJsonMock.mock.calls.find(([path]) => path.includes("/restore"))).toBeUndefined();
    confirmSpy.mockRestore();
  });
});
