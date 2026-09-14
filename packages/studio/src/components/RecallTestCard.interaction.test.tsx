// @vitest-environment jsdom
//! 457 号：RecallTestCard 交互测试——召回测试查询载荷与融合结果渲染
//! （语义+词法双源标记 / 词法降级模式 / 无命中空态）。
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { RecallTestCard } from "./RecallTestCard";

const fetchJsonMock = vi.fn(async (path: string, init?: { method?: string; body?: string }) => {
  if (path.includes("/hybrid-search")) {
    return {
      fts: [],
      fused: [
        { id: "ch-0002", score: 0.8734, sources: ["semantic", "fts5"] },
        { id: "ch-0001", score: 0.4121, sources: ["fts5"] },
      ],
      mode: "semantic",
    };
  }
  return {};
});

vi.mock("../hooks/use-api", () => ({
  fetchJson: (path: string, init?: { method?: string; body?: string }) => fetchJsonMock(path, init),
}));

afterEach(() => {
  cleanup();
  fetchJsonMock.mockClear();
});

describe("RecallTestCard 交互（457 号）", () => {
  it("查询→POST 载荷 k=10→融合排名渲染双源标记", async () => {
    const user = userEvent.setup();
    render(<RecallTestCard bookId="b1" />);
    const input = document.querySelector("input") as HTMLInputElement;
    await user.type(input, "碎镜的秘密");
    await user.click(screenSearch());
    await vi.waitFor(() => {
      expect(document.body.textContent).toContain("语义+词法混合");
    });
    expect(document.body.textContent).toContain("ch-0002");
    expect(document.body.textContent).toContain("0.8734");
    expect(document.body.textContent).toContain("·语义");
    expect(document.body.textContent).toContain("·词法");
    const call = fetchJsonMock.mock.calls.find(([path]) => path.includes("/hybrid-search"));
    expect(JSON.parse((call![1] as { body: string }).body)).toEqual({
      query: "碎镜的秘密",
      k: 10,
    });
  });

  it("空结果渲染词法降级标记与无命中空态", async () => {
    fetchJsonMock.mockImplementationOnce(async () => ({ fts: [], fused: [], mode: "fts5-fallback" }));
    const user = userEvent.setup();
    render(<RecallTestCard bookId="b1" />);
    const input = document.querySelector("input") as HTMLInputElement;
    await user.type(input, "不存在的查询");
    await user.click(screenSearch());
    await vi.waitFor(() => {
      expect(document.body.textContent).toContain("词法降级");
    });
    expect(document.body.textContent).toContain("无命中");
  });

  function screenSearch(): HTMLElement {
    return Array.from(document.querySelectorAll("button")).find(
      (b) => b.textContent?.trim() === "检索",
    ) as HTMLElement;
  }
});
