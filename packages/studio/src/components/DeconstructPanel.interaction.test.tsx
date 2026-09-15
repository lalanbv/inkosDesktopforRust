// @vitest-environment jsdom
//! 477 号：DeconstructPanel 交互测试——逐章证据行解析（人物中英顿号逗号混合
//! 拆分+可选字段条件携带）进 POST 载荷、无效输入不发包、发布链（publish+name）
//! 与失败路径 notice 透出。446 号已真机端到端走查，此处锁组件解析与载荷契约。
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { DeconstructPanel } from "./DeconstructPanel";

const fetchJsonMock = vi.fn(async (path: string, init?: { method?: string; body?: string }) => {
  if (path === "/books/b1/deconstruct" && init?.method === "POST") {
    const payload = JSON.parse(init.body ?? "{}") as { publish?: boolean };
    return {
      markdown: "# 拆书产物\n## 人物档案",
      path: ".inkos/story/deconstruction/拆书.md",
      ...(payload.publish ? { publishedMaterialId: "mat_1" } : {}),
    };
  }
  throw new Error(`unexpected ${init?.method ?? "GET"} ${path}`);
});

vi.mock("../hooks/use-api", () => ({
  fetchJson: (path: string, init?: { method?: string; body?: string }) => fetchJsonMock(path, init),
}));

const TWO_LINES = "1 | 林动、应欢欢,绫清竹 | 初入大荒 | 开局章 | 强钩\n2 |  | 试炼开始";

async function typeLines(user: ReturnType<typeof userEvent.setup>, text: string) {
  await user.type(screen.getByPlaceholderText("每行一章：章号 | 出场人物 | 事件梗概 | 章型 | 伏笔动静"), text);
}

function findPost() {
  return fetchJsonMock.mock.calls.find(
    ([path, init]) => path === "/books/b1/deconstruct" && init?.method === "POST",
  );
}

afterEach(() => {
  cleanup();
  fetchJsonMock.mockClear();
});

describe("DeconstructPanel 交互（477 号）", () => {
  it("分析：逐章行解析进 POST 载荷（人物拆分+可选字段条件携带），产物渲染", async () => {
    const user = userEvent.setup();
    render(<DeconstructPanel bookId="b1" />);
    await typeLines(user, TWO_LINES);
    await user.click(screen.getByText("分析"));
    const call = findPost();
    expect(call).toBeTruthy();
    expect(JSON.parse(call![1]!.body ?? "{}")).toEqual({
      chapters: [
        { chapter: 1, characters: ["林动", "应欢欢", "绫清竹"], events: "初入大荒", chapterType: "开局章", hookActivity: "强钩" },
        { chapter: 2, characters: [], events: "试炼开始" },
      ],
      depth: "full",
      language: "zh",
    });
    await vi.waitFor(() => {
      expect(screen.getByText("已生成并落盘。")).toBeTruthy();
    });
    expect(screen.getByText(/## 人物档案/)).toBeTruthy();
  });

  it("无可解析章节行时不发包并提示格式", async () => {
    const user = userEvent.setup();
    render(<DeconstructPanel bookId="b1" />);
    await user.type(screen.getByPlaceholderText("每行一章：章号 | 出场人物 | 事件梗概 | 章型 | 伏笔动静"), "abc | 不是章号");
    await user.click(screen.getByText("分析"));
    expect(findPost()).toBeUndefined();
    expect(screen.getByText(/没有可解析的章节行/)).toBeTruthy();
  });

  it("发布到材料池：载荷带 publish+可选名称，产物 id 进通知", async () => {
    const user = userEvent.setup();
    render(<DeconstructPanel bookId="b1" />);
    await typeLines(user, "1 | 林动 | 初入大荒");
    await user.type(screen.getByPlaceholderText("产物名称（可选）"), "我的拆书");
    await user.click(screen.getByText("发布到材料池"));
    const call = findPost();
    expect(call).toBeTruthy();
    expect(JSON.parse(call![1]!.body ?? "{}")).toEqual({
      chapters: [{ chapter: 1, characters: ["林动"], events: "初入大荒" }],
      depth: "full",
      publish: true,
      name: "我的拆书",
      language: "zh",
    });
    await vi.waitFor(() => {
      expect(screen.getByText("已发布到材料池：mat_1")).toBeTruthy();
    });
  });

  it("端点失败时错误信息经 notice 透出", async () => {
    fetchJsonMock.mockRejectedValueOnce(new Error("HTTP 500：引擎内部错误"));
    const user = userEvent.setup();
    render(<DeconstructPanel bookId="b1" />);
    await typeLines(user, "1 | 林动 | 初入大荒");
    await user.click(screen.getByText("分析"));
    await vi.waitFor(() => {
      expect(screen.getByText("HTTP 500：引擎内部错误")).toBeTruthy();
    });
  });
});
