// @vitest-environment jsdom
//! 457 号：RecallTestCard 交互测试——召回测试查询载荷与融合结果渲染
//! （语义+词法双源标记 / 词法降级模式 / 无命中空态）。
import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render } from "@testing-library/react";
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

// 1169 号：键盘路径守卫——IME 组合期回车吞没 + 普通回车放行 + busy 飞行互斥
// （Enter 路径不经过按钮 disabled，run() 入口互斥是唯一在途闸门）。
describe("RecallTestCard 键盘守卫（1169 号）", () => {
  function hybridCalls(): number {
    return fetchJsonMock.mock.calls.filter(([path]) => String(path).includes("/hybrid-search")).length;
  }

  it("IME 组合期回车（选词上屏）不触发检索，普通回车触发", async () => {
    const user = userEvent.setup();
    render(<RecallTestCard bookId="b1" />);
    const input = document.querySelector("input") as HTMLInputElement;
    await user.type(input, "碎镜之谜");
    fireEvent.keyDown(input, { key: "Enter", isComposing: true });
    expect(hybridCalls()).toBe(0);
    fireEvent.keyDown(input, { key: "Enter" });
    await vi.waitFor(() => {
      expect(hybridCalls()).toBe(1);
    });
  });

  it("busy 飞行互斥——在途回车不重复发检索（run() 入口守卫）", async () => {
    let release!: (v: unknown) => void;
    fetchJsonMock.mockImplementationOnce(
      // resolve 形参类型比 unknown 窄（逆变失败），断言放行（测试门栓）。
      () => new Promise((resolve) => { release = resolve as unknown as (v: unknown) => void; }),
    );
    const user = userEvent.setup();
    render(<RecallTestCard bookId="b1" />);
    const input = document.querySelector("input") as HTMLInputElement;
    await user.type(input, "镜中影");
    fireEvent.keyDown(input, { key: "Enter" });
    // 真实双击/双按=两个独立事件任务——任务间让 React 完成重渲染，
    // 第二发 handler 才读到 fresh busy=true（同任务连发不构成真实输入面）。
    await act(async () => {});
    fireEvent.keyDown(input, { key: "Enter" });
    expect(hybridCalls()).toBe(1);
    release({ fts: [], fused: [], mode: "semantic" });
    await vi.waitFor(() => {
      expect(document.body.textContent).toContain("语义+词法混合");
    });
  });
});
