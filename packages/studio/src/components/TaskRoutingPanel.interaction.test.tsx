// @vitest-environment jsdom
//! 479 号：TaskRoutingPanel 交互测试——加载回填、保存载荷（defaults 保留+新任务
//! 合并）、清空任务模型后从 tasks 剔除、失败路径 notice 透出。
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { TaskRoutingPanel } from "./TaskRoutingPanel";

let failPut = false;

const storedRouting = {
  defaults: { model: "global-x" },
  tasks: { writing: { model: "m-writer" } },
};

const fetchJsonMock = vi.fn(async (path: string, init?: { method?: string; body?: string }) => {
  if (path === "/task-routing" && (init?.method ?? "GET") === "GET") {
    return { routing: storedRouting };
  }
  if (path === "/task-routing" && init?.method === "PUT") {
    if (failPut) throw new Error("HTTP 500：写入失败");
    return { ok: true };
  }
  throw new Error(`unexpected ${init?.method ?? "GET"} ${path}`);
});

vi.mock("../hooks/use-api", () => ({
  fetchJson: (path: string, init?: { method?: string; body?: string }) => fetchJsonMock(path, init),
}));

/** 行结构：label span 与 input 同行——按任务名取同行输入框。 */
function inputFor(task: string) {
  const label = screen.getByText(task);
  const row = label.closest("div");
  if (!row) throw new Error(`row for ${task} not found`);
  return row.querySelector("input") as HTMLInputElement;
}

function findPut() {
  return fetchJsonMock.mock.calls.find(
    ([path, init]) => path === "/task-routing" && init?.method === "PUT",
  );
}

afterEach(() => {
  cleanup();
  fetchJsonMock.mockClear();
  failPut = false;
});

describe("TaskRoutingPanel 交互（479 号）", () => {
  it("加载回填任务模型，未覆盖任务显示占位（留空=全局）", async () => {
    render(<TaskRoutingPanel />);
    await vi.waitFor(() => {
      expect((inputFor("写作") as HTMLInputElement).value).toBe("m-writer");
    });
    expect((inputFor("审改") as HTMLInputElement).value).toBe("");
    expect(inputFor("审改").placeholder).toBe("使用全局模型");
  });

  it("新任务填模型后保存，载荷保留 defaults 并合并新任务", async () => {
    const user = userEvent.setup();
    render(<TaskRoutingPanel />);
    await vi.waitFor(() => {
      expect(inputFor("写作").value).toBe("m-writer");
    });
    await user.type(inputFor("审改"), "m-review");
    await user.click(screen.getByText("保存路由"));
    const call = findPut();
    expect(call).toBeTruthy();
    expect(JSON.parse(call![1]!.body ?? "{}")).toEqual({
      routing: {
        defaults: { model: "global-x" },
        tasks: { writing: { model: "m-writer" }, review: { model: "m-review" } },
      },
    });
    await vi.waitFor(() => {
      expect(screen.getByText("已保存。新任务即时生效。")).toBeTruthy();
    });
  });

  it("清空任务模型后保存，该任务从 tasks 剔除（不落空字段）", async () => {
    const user = userEvent.setup();
    render(<TaskRoutingPanel />);
    await vi.waitFor(() => {
      expect(inputFor("写作").value).toBe("m-writer");
    });
    await user.clear(inputFor("写作"));
    await user.click(screen.getByText("保存路由"));
    const call = findPut();
    expect(JSON.parse(call![1]!.body ?? "{}")).toEqual({
      routing: { defaults: { model: "global-x" }, tasks: {} },
    });
  });

  it("保存失败时错误信息经 notice 透出", async () => {
    failPut = true;
    const user = userEvent.setup();
    render(<TaskRoutingPanel />);
    await vi.waitFor(() => {
      expect(inputFor("写作")).toBeTruthy();
    });
    await user.click(screen.getByText("保存路由"));
    await vi.waitFor(() => {
      expect(screen.getByText("HTTP 500：写入失败")).toBeTruthy();
    });
  });
});
