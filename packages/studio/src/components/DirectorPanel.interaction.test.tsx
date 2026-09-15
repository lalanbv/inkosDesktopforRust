// @vitest-environment jsdom
//! 478 号：DirectorPanel 交互测试——加载回填（灵感/运行模式/阶段/已存章节/续跑
//! 建议）、保存载荷（patch 包裹 + keywords 回传回归）、编辑与失败路径。
//! 442 缺陷类防御：keywords 不在表单展示，保存必须原样回传而非清空。
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { DirectorPanel } from "./DirectorPanel";

const storedSession = {
  bookId: "b1",
  runMode: "range",
  stage: "writing",
  inspiration: { premise: "少年执灯入雾都", keywords: ["悬疑", "东方"] },
  selectedDirection: { title: "雾都灯影" },
};

let failPut = false;

const fetchJsonMock = vi.fn(async (path: string, init?: { method?: string; body?: string }) => {
  if (path === "/books/b1/director" && (init?.method ?? "GET") === "GET") {
    return { session: storedSession, savedChapters: 3, resumeAdvice: "从第 4 章续跑。" };
  }
  if (path === "/books/b1/director" && init?.method === "PUT") {
    if (failPut) throw new Error("HTTP 400：invalid runMode");
    return { ok: true };
  }
  throw new Error(`unexpected ${init?.method ?? "GET"} ${path}`);
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
  failPut = false;
});

describe("DirectorPanel 交互（478 号）", () => {
  it("加载回填灵感/运行模式/阶段/续跑建议，保存载荷 patch 包裹且 keywords 原样回传", async () => {
    const user = userEvent.setup();
    render(<DirectorPanel bookId="b1" />);
    await vi.waitFor(() => {
      expect((screen.getByPlaceholderText("一句话灵感…") as HTMLInputElement).value).toBe("少年执灯入雾都");
    });
    const select = screen.getByRole("combobox") as HTMLSelectElement;
    expect(select.value).toBe("range");
    expect(screen.getByText("writing")).toBeTruthy();
    expect(screen.getByText(/已存章节/)).toBeTruthy(); // 计数 3 与阶段同段渲染
    expect(screen.getByText(/从第 4 章续跑。/)).toBeTruthy();

    await user.click(screen.getByText("保存"));
    const call = findPut();
    expect(call).toBeTruthy();
    expect(JSON.parse(call![1]!.body ?? "{}")).toEqual({
      patch: {
        runMode: "range",
        stage: "writing",
        inspiration: { premise: "少年执灯入雾都", keywords: ["悬疑", "东方"] },
      },
    });
    await vi.waitFor(() => {
      expect(screen.getByText("已保存。")).toBeTruthy();
    });
  });

  it("编辑灵感与切换运行模式后保存，载荷携带新值且 keywords 不丢", async () => {
    const user = userEvent.setup();
    render(<DirectorPanel bookId="b1" />);
    await vi.waitFor(() => {
      expect((screen.getByPlaceholderText("一句话灵感…") as HTMLInputElement).value).toBe("少年执灯入雾都");
    });
    const input = screen.getByPlaceholderText("一句话灵感…") as HTMLInputElement;
    await user.clear(input);
    await user.type(input, "新灵感一句话");
    await user.selectOptions(screen.getByRole("combobox"), "full-book");
    await user.click(screen.getByText("保存"));
    const call = findPut();
    expect(JSON.parse(call![1]!.body ?? "{}")).toEqual({
      patch: {
        runMode: "full-book",
        stage: "writing",
        inspiration: { premise: "新灵感一句话", keywords: ["悬疑", "东方"] },
      },
    });
  });

  it("保存失败时错误信息经 notice 透出", async () => {
    failPut = true;
    const user = userEvent.setup();
    render(<DirectorPanel bookId="b1" />);
    await vi.waitFor(() => {
      expect(screen.getByPlaceholderText("一句话灵感…")).toBeTruthy();
    });
    await user.click(screen.getByText("保存"));
    await vi.waitFor(() => {
      expect(screen.getByText("HTTP 400：invalid runMode")).toBeTruthy();
    });
  });
});
