// @vitest-environment jsdom
//! 1683 号（R60 第二批）：章级回收站面板交互测试——批次分组、单条恢复（成功回调
//! 刷章节列表）、整批恢复、清空全部两步确认、404（条目已被清）显示错误并重列。
import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ChapterTrashPanel } from "./ChapterTrashPanel";

const calls: { path: string; init?: { method?: string; body?: string } }[] = [];
let trashEntries: unknown[] = [];

const entry = (batch: string | null, file: string, bytes = 30) => ({
  path: batch ? `chapters/.trash/${batch}/chapters/${file}` : `chapters/.trash/${file}`,
  originalPath: `chapters/${file}`,
  batch,
  bytes,
  modifiedAt: "2026-11-26T01:02:03.000Z",
  trashedAt: batch ? "2026-11-26T01:02:03.000Z" : null,
});

const defaultFetch = async (path: string, init?: { method?: string; body?: string }): Promise<unknown> => {
  calls.push({ path, init });
  if (init?.method === "POST" && path === "/api/v1/books/b1/trash/restore") {
    const body = JSON.parse(init.body ?? "{}");
    if (body.path === "chapters/.trash/0003_收束.md") {
      // 别的窗口已经恢复过这条：404。
      throw new Error("Trash entry not found: chapters/.trash/0003_收束.md");
    }
    // 找回成功的条目从回收站列表移除（重列断言依赖）。
    trashEntries = trashEntries.filter((item) => {
      const item0 = item as { batch: string | null; path: string };
      if (typeof body.batch === "string") return item0.batch !== body.batch;
      return item0.path !== body.path;
    });
    return {
      ok: true,
      restored: [{ path: body.path ?? `chapters/.trash/${body.batch}/chapters/0002_云涌.md`, originalPath: "chapters/0002_云涌.md", chapter: { number: 2, meta: { title: "云涌" } } }],
    };
  }
  if (init?.method === "POST" && path === "/api/v1/books/b1/trash/purge") {
    const body = JSON.parse(init.body ?? "{}");
    if (body.all === true) trashEntries = [];
    else if (typeof body.batch === "string") trashEntries = trashEntries.filter((item) => (item as { batch: string | null }).batch !== body.batch);
    else trashEntries = trashEntries.filter((item) => (item as { path: string }).path !== body.path);
    return { ok: true, removed: 1 };
  }
  if (path === "/api/v1/books/b1/trash") return { entries: trashEntries };
  return {};
};
const fetchJsonMock = vi.fn(defaultFetch);

vi.mock("../hooks/use-api", () => ({
  fetchJson: (path: string, init?: { method?: string; body?: string }) => fetchJsonMock(path, init),
  invalidateApiPaths: vi.fn(),
}));

async function openPanel() {
  render(<ChapterTrashPanel bookId="b1" onRestored={vi.fn()} />);
  await act(async () => {
    (await screen.findByRole("button", { name: /回收站/ })).click();
  });
}

afterEach(() => {
  cleanup();
  calls.length = 0;
  trashEntries = [
    entry("rollback-20261126090301-2", "0002_云涌.md"),
    entry("rollback-20261126090301-2", "index.json", 400),
    entry(null, "0003_收束.md"),
  ];
  fetchJsonMock.mockClear();
  fetchJsonMock.mockImplementation(defaultFetch);
});

trashEntries = [
  entry("rollback-20261126090301-2", "0002_云涌.md"),
  entry("rollback-20261126090301-2", "index.json", 400),
  entry(null, "0003_收束.md"),
];

describe("ChapterTrashPanel", () => {
  it("lists entries grouped by rollback batch with per-batch actions", async () => {
    await openPanel();
    expect(await screen.findByText("chapters/0002_云涌.md")).toBeTruthy();
    expect(screen.getByText("chapters/0003_收束.md")).toBeTruthy();
    // 批次头：来源批次可读 + 该批的整批操作。
    expect(screen.getByText(/rollback-20261126090301-2/)).toBeTruthy();
    expect(screen.getAllByRole("button", { name: /^整批恢复$/ }).length).toBe(1);
    expect(screen.getAllByRole("button", { name: /^整批删除$/ }).length).toBe(1);
  });

  it("restores one entry and tells the book page to refresh", async () => {
    const onRestored = vi.fn();
    cleanup();
    render(<ChapterTrashPanel bookId="b1" onRestored={onRestored} />);
    await act(async () => {
      (await screen.findByRole("button", { name: /回收站/ })).click();
    });
    await act(async () => {
      (await screen.findAllByRole("button", { name: /^恢复$/ }))[0]!.click();
    });
    const restore = calls.find((call) => call.path === "/api/v1/books/b1/trash/restore");
    expect(restore?.init?.method).toBe("POST");
    expect(JSON.parse(restore?.init?.body ?? "{}").path).toBeTruthy();
    expect(onRestored).toHaveBeenCalled();
  });

  it("restores a whole batch with one request", async () => {
    await openPanel();
    await act(async () => {
      (await screen.findByRole("button", { name: /^整批恢复$/ })).click();
    });
    const restore = calls.find((call) => call.path === "/api/v1/books/b1/trash/restore");
    expect(JSON.parse(restore?.init?.body ?? "{}").batch).toBe("rollback-20261126090301-2");
    // 批内两条都没了，平铺的仍在。
    expect(await screen.findByText("chapters/0003_收束.md")).toBeTruthy();
    expect(screen.queryByText("chapters/0002_云涌.md")).toBeNull();
  });

  it("shows an error and reloads when an entry is already gone (404)", async () => {
    await openPanel();
    const row = screen.getByText("chapters/0003_收束.md").closest("div")!;
    const target = within(row as HTMLElement).getByRole("button", { name: "恢复" });
    await act(async () => {
      target.click();
    });
    expect(await screen.findByText(/not found|已被清/)).toBeTruthy();
    // 重列后列表仍在渲染（该条目 mock 里仍会被列出）。
    expect(screen.getByText("chapters/0002_云涌.md")).toBeTruthy();
  });

  it("purges a single entry only after the danger confirm is confirmed", async () => {
    await openPanel();
    const row = screen.getByText("chapters/0003_收束.md").closest("div")!;
    await act(async () => {
      within(row as HTMLElement).getByRole("button", { name: "删除" }).click();
    });
    expect(screen.getByRole("heading", { name: /永久删除/ })).toBeTruthy();
    expect(calls.some((call) => call.path === "/api/v1/books/b1/trash/purge")).toBe(false);
    await act(async () => {
      (await screen.findByRole("button", { name: /永久删除/ })).click();
    });
    const purge = calls.find((call) => call.path === "/api/v1/books/b1/trash/purge");
    expect(JSON.parse(purge?.init?.body ?? "{}").path).toBe("chapters/.trash/0003_收束.md");
    // 重列后该条消失、其余仍在。
    expect(await screen.findByText("chapters/0002_云涌.md")).toBeTruthy();
    expect(screen.queryByText("chapters/0003_收束.md")).toBeNull();
  });

  it("purges everything only after the danger confirm is confirmed, then reloads", async () => {
    const onRestored = vi.fn();
    cleanup();
    render(<ChapterTrashPanel bookId="b1" onRestored={onRestored} />);
    await act(async () => {
      (await screen.findByRole("button", { name: /回收站/ })).click();
    });
    await act(async () => {
      (await screen.findByRole("button", { name: /清空回收站/ })).click();
    });
    expect(screen.getByRole("heading", { name: /永久删除/ })).toBeTruthy();
    expect(calls.some((call) => call.path === "/api/v1/books/b1/trash/purge")).toBe(false);
    await act(async () => {
      (await screen.findByRole("button", { name: /永久删除/ })).click();
    });
    const purge = calls.find((call) => call.path === "/api/v1/books/b1/trash/purge");
    expect(JSON.parse(purge?.init?.body ?? "{}")).toEqual({ all: true });
    expect(await screen.findByText(/回收站是空的/)).toBeTruthy();
    expect(onRestored).toHaveBeenCalled();
  });
});
