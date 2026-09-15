// @vitest-environment jsdom
//! 508 号（续）：DirectorPanel 方向候选生成接线——premise 必填守卫、
//! 生成结果渲染（标题/钩子/差异化/置信度）、选用写回会话、失败路径。
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { DirectorPanel } from "./DirectorPanel";

const directionCandidates = [
  { id: "d1", title: "雾都灯影", hook: "少年执灯入雾，雾中有他失踪的母亲。", genre: "悬疑", synopsis: "灯影分割两界，主角以灯为钥。", differentiator: "以灯火明暗做章节节奏。", confidence: 0.82 },
  { id: "d2", title: "雾都迷航", hook: "同源灵感。", genre: "悬疑", synopsis: "迷航线。", differentiator: "多线叙事。", confidence: 0.64 },
];

let directionsResponse: Array<{ id: string; title: string; hook: string; genre: string; synopsis: string; differentiator: string; confidence: number }> = [];
let failDirections = false;

const fetchJsonMock = vi.fn(async (path: string, init?: { method?: string; body?: string }) => {
  const method = init?.method ?? "GET";
  if (path === "/books/b1/director" && method === "GET") {
    return {
      session: { bookId: "b1", runMode: "ready-stop", stage: "directions", inspiration: { premise: "少年执灯入雾都。", keywords: [] } },
      savedChapters: 0,
      resumeAdvice: "",
    };
  }
  if (path === "/api/v1/director/directions" && method === "POST") {
    if (failDirections) throw new Error("HTTP 500：方向生成失败");
    return { directions: directionsResponse };
  }
  if (path === "/books/b1/director" && method === "PUT") {
    return { ok: true };
  }
  throw new Error(`unexpected ${method} ${path}`);
});

vi.mock("../hooks/use-api", () => ({
  fetchJson: (path: string, init?: { method?: string; body?: string }) => fetchJsonMock(path, init),
}));

function findPut() {
  return fetchJsonMock.mock.calls.find(
    ([path, init]) => path === "/books/b1/director" && init?.method === "PUT",
  );
}

afterEach(() => {
  cleanup();
  fetchJsonMock.mockClear();
  failDirections = false;
});

describe("DirectorPanel 方向候选（508 号）", () => {
  it("生成方向候选并渲染标题/钩子/差异化/置信度", async () => {
    directionsResponse = directionCandidates;
    const user = userEvent.setup();
    render(<DirectorPanel bookId="b1" />);
    const premiseInput = (await vi.waitFor(() => {
      const el = screen.getByPlaceholderText("一句话灵感…") as HTMLInputElement;
      expect(el.value).toBe("少年执灯入雾都。");
      return el;
    })) as HTMLInputElement;
    await user.click(screen.getByText("生成方向候选"));
    await vi.waitFor(() => {
      expect(screen.getByText("雾都灯影")).toBeTruthy();
      expect(screen.getByText("雾都迷航")).toBeTruthy();
    });
    expect(screen.getByText(/少年执灯入雾/)).toBeTruthy();
    expect(screen.getByText(/以灯火明暗做章节节奏/)).toBeTruthy();
  });

  it("选用候选：PUT patch 携带 selectedDirection", async () => {
    const user = userEvent.setup();
    render(<DirectorPanel bookId="b1" />);
    await vi.waitFor(() => {
      expect(screen.getByPlaceholderText("一句话灵感…")).toBeTruthy();
    });
    await user.click(screen.getByText("生成方向候选"));
    await vi.waitFor(() => {
      expect(screen.getByText("雾都灯影")).toBeTruthy();
    });
    await user.click(screen.getByText("雾都灯影"));
    const call = findPut();
    expect(call).toBeTruthy();
    expect(JSON.parse(call![1]!.body ?? "{}")).toEqual({
      patch: { selectedDirection: directionCandidates[0] },
    });
    await vi.waitFor(() => {
      expect(screen.getByText(/已选用方向/)).toBeTruthy();
    });
  });

  it("premise 为空时生成被客户端守卫拒绝", async () => {
    const user = userEvent.setup();
    render(<DirectorPanel bookId="b1" />);
    await vi.waitFor(() => {
      expect(screen.getByText("生成方向候选")).toBeTruthy();
    });
    await user.click(screen.getByText("生成方向候选"));
    expect(findPut()).toBeUndefined();
  });

  it("生成失败时错误信息透出", async () => {
    failDirections = true;
    const user = userEvent.setup();
    render(<DirectorPanel bookId="b1" />);
    await vi.waitFor(() => {
      expect(screen.getByText("生成方向候选")).toBeTruthy();
    });
    await user.click(screen.getByText("生成方向候选"));
    await vi.waitFor(() => {
      expect(screen.getByText("HTTP 500：方向生成失败")).toBeTruthy();
    });
  });
});
