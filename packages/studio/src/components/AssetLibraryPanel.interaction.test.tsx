// @vitest-environment jsdom
//! 475 号：AssetLibraryPanel 交互测试——三库 tab 切换拉取、新增（id 派生+行拆分
//! PUT 载荷）、删除（失败提示内置种子）、便携包导入（upload 直注）、采用到本书、
//! 导出包；kind 切换清展开态回归（本号修复：stale expandedId 跨库串显）。
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { AssetLibraryPanel } from "./AssetLibraryPanel";

let failDelete = false;

const genreSnapshot = {
  kind: "genre-base",
  seeded: true,
  assets: [
    {
      id: "west_fantasy",
      kind: "genre-base",
      name: "西方奇幻",
      body: "剑与魔法的世界",
      expectations: ["升级打怪", "地图拓展"],
      taboos: ["无系统"],
      samples: ["样例一"],
    },
  ],
};
// 与题材基底同 id 的异库资产：展开态跨库串显的复现载体
const progressionSnapshot = {
  kind: "progression-mode",
  seeded: false,
  assets: [
    { id: "west_fantasy", kind: "progression-mode", name: "同名异库", body: "异库正文", expectations: [], taboos: [], samples: [] },
  ],
};

const fetchJsonMock = vi.fn(async (path: string, init?: { method?: string; body?: string }) => {
  const method = init?.method ?? "GET";
  if (path === "/asset-library/genre-base") return genreSnapshot;
  if (path === "/asset-library/progression-mode") return progressionSnapshot;
  if (path === "/asset-library/genre-base/assets" && method === "PUT") return {};
  if (path === "/asset-library/genre-base/assets/west_fantasy" && method === "DELETE") {
    if (failDelete) throw new Error("builtin seed protected");
    return {};
  }
  if (path === "/asset-library/genre-base/export") return { assets: [] };
  if (path === "/asset-library/genre-base/import") return { added: 1, overwritten: 2, skipped: 3 };
  if (path === "/books/b1/adopt-library-assets") return { published: [{}] };
  throw new Error(`unexpected ${method} ${path}`);
});

vi.mock("../hooks/use-api", () => ({
  fetchJson: (path: string, init?: { method?: string; body?: string }) => fetchJsonMock(path, init),
}));

// jsdom 无 createObjectURL——导出链需 Blob 下载
(URL as unknown as { createObjectURL: (blob: Blob) => string }).createObjectURL = vi.fn(() => "blob:mock");
(URL as unknown as { revokeObjectURL: (url: string) => void }).revokeObjectURL = vi.fn();

function findCall(method: string, path: string) {
  return fetchJsonMock.mock.calls.find(([p, init]) => (init?.method ?? "GET") === method && p === path);
}

afterEach(() => {
  cleanup();
  fetchJsonMock.mockClear();
  failDelete = false;
});

describe("AssetLibraryPanel 交互（475 号）", () => {
  it("三库 tab 切换分别拉取对应快照，种子徽标随之显隐", async () => {
    const user = userEvent.setup();
    render(<AssetLibraryPanel bookId="b1" />);
    await vi.waitFor(() => {
      expect(screen.getByText("西方奇幻")).toBeTruthy();
    });
    expect(screen.getByText("当前为内置种子（未落盘）")).toBeTruthy();
    await user.click(screen.getByText("推进模式"));
    await vi.waitFor(() => {
      expect(screen.getByText("同名异库")).toBeTruthy();
    });
    expect(findCall("GET", "/asset-library/progression-mode")).toBeTruthy();
    expect(screen.queryByText("当前为内置种子（未落盘）")).toBeNull();
  });

  it("新增资产：id 由名称派生、多行字段拆分数组进 PUT 载荷，保存后清空表单", async () => {
    const user = userEvent.setup();
    render(<AssetLibraryPanel />);
    await vi.waitFor(() => {
      expect(screen.getByText("西方奇幻")).toBeTruthy();
    });
    const save = screen.getByText("保存资产") as HTMLButtonElement;
    expect(save.disabled).toBe(true);
    await user.type(screen.getByPlaceholderText("名称（新增/覆盖同 id）"), "Epic Saga");
    await user.type(screen.getByPlaceholderText("正文：核心引擎/范式说明"), "史诗引擎说明");
    await user.type(screen.getByPlaceholderText("读者期待（每行一条）"), "期待甲\n期待乙");
    await user.type(screen.getByPlaceholderText("禁忌（每行一条）"), "禁忌甲");
    expect(save.disabled).toBe(false);
    await user.click(save);
    const call = findCall("PUT", "/asset-library/genre-base/assets");
    expect(call).toBeTruthy();
    const payload = JSON.parse(call![1]!.body ?? "{}") as { asset: { id: string; expectations: string[]; taboos: string[] } };
    expect(payload.asset.id).toBe("epic_saga");
    expect(payload.asset.expectations).toEqual(["期待甲", "期待乙"]);
    expect(payload.asset.taboos).toEqual(["禁忌甲"]);
    await vi.waitFor(() => {
      expect(screen.getByText("已保存")).toBeTruthy();
    });
    expect((screen.getByPlaceholderText("名称（新增/覆盖同 id）") as HTMLInputElement).value).toBe("");
  });

  it("删除资产走 DELETE 并重拉；失败时提示内置种子受保护", async () => {
    const user = userEvent.setup();
    render(<AssetLibraryPanel />);
    await vi.waitFor(() => {
      expect(screen.getByText("西方奇幻")).toBeTruthy();
    });
    await user.click(screen.getByText("删"));
    await vi.waitFor(() => {
      expect(findCall("DELETE", "/asset-library/genre-base/assets/west_fantasy")).toBeTruthy();
    });
    // 失败路径：DELETE 抛错 → 保护提示（编码 decodeURIComponent 前后路径一致）
    failDelete = true;
    await user.click(screen.getByText("删"));
    await vi.waitFor(() => {
      expect(screen.getByText("删除失败（内置种子不可删）")).toBeTruthy();
    });
  });

  it("导入包：文件原文作 POST body，结果计数进通知", async () => {
    const user = userEvent.setup();
    render(<AssetLibraryPanel />);
    await vi.waitFor(() => {
      expect(screen.getByText("西方奇幻")).toBeTruthy();
    });
    const file = new File(['{"assets":[]}'], "pack.json", { type: "application/json" });
    await user.upload(document.querySelector('input[type="file"]') as HTMLInputElement, file);
    const call = findCall("POST", "/asset-library/genre-base/import");
    expect(call).toBeTruthy();
    expect(call![1]!.body).toBe('{"assets":[]}');
    await vi.waitFor(() => {
      expect(screen.getByText("导入完成：新增 1 / 覆盖 2 / 跳过 3")).toBeTruthy();
    });
  });

  it("采用到本书：refs 携带当前库与资产 id；无 bookId 时按钮不渲染", async () => {
    const user = userEvent.setup();
    const { unmount } = render(<AssetLibraryPanel bookId="b1" />);
    await vi.waitFor(() => {
      expect(screen.getByText("采用到本书")).toBeTruthy();
    });
    await user.click(screen.getByText("采用到本书"));
    const call = findCall("POST", "/books/b1/adopt-library-assets");
    expect(call).toBeTruthy();
    expect(JSON.parse(call![1]!.body ?? "{}")).toEqual({ refs: [{ kind: "genre-base", id: "west_fantasy" }] });
    await vi.waitFor(() => {
      expect(screen.getByText("已采用 1 项到本书参考资料")).toBeTruthy();
    });
    unmount();
    render(<AssetLibraryPanel />);
    await vi.waitFor(() => {
      expect(screen.getByText("西方奇幻")).toBeTruthy();
    });
    expect(screen.queryByText("采用到本书")).toBeNull();
  });

  it("导出包拉取导出端点且不报错；kind 切换清展开态（stale expandedId 修复回归）", async () => {
    const user = userEvent.setup();
    render(<AssetLibraryPanel />);
    await vi.waitFor(() => {
      expect(screen.getByText("西方奇幻")).toBeTruthy();
    });
    // 展开资产详情
    await user.click(screen.getByText("西方奇幻"));
    expect(screen.getByText("剑与魔法的世界")).toBeTruthy();
    // 导出链：端点被拉取、无失败提示
    await user.click(screen.getByText("导出包"));
    await vi.waitFor(() => {
      expect(findCall("GET", "/asset-library/genre-base/export")).toBeTruthy();
    });
    expect(screen.queryByText("导出失败")).toBeNull();
    // 修复回归：切库后同 id 异库资产不得沿用展开态
    await user.click(screen.getByText("推进模式"));
    await vi.waitFor(() => {
      expect(screen.getByText("同名异库")).toBeTruthy();
    });
    expect(screen.queryByText("异库正文")).toBeNull();
  });
});
