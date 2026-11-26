// @vitest-environment jsdom
//! 1683 号（R60 第二批）：书级回收站面板交互测试——列表渲染、空态、恢复（成功
//! 刷新书架 + 409 目标占用显示错误不假清空）、彻底删除两步确认（确认前不发请求）。
import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { TrashPanel } from "./TrashPanel";

const calls: { path: string; init?: { method?: string; body?: string } }[] = [];
let booksInTrash: unknown[] = [];
let restoreResult: { ok?: boolean; error?: { code: string; message: string; conflicts?: string[] } } = { ok: true };

const defaultFetch = async (path: string, init?: { method?: string; body?: string }): Promise<unknown> => {
  calls.push({ path, init });
  if (init?.method === "POST" && path === "/api/v1/trash/books/restore") {
    if (restoreResult.error) throw new Error(restoreResult.error.message);
    // 找回成功：条目离开回收站（重列断言依赖）。
    const restoredEntry = JSON.parse(init.body ?? "{}").entry;
    booksInTrash = booksInTrash.filter((item) => (item as { entry: string }).entry !== restoredEntry);
    return { ok: true, entry: restoredEntry, bookId: "b1" };
  }
  if (init?.method === "POST" && path === "/api/v1/trash/books/purge") {
    booksInTrash = booksInTrash.filter((entry) => (entry as { entry: string }).entry !== JSON.parse(init.body ?? "{}").entry);
    return { ok: true, entry: JSON.parse(init.body ?? "{}").entry };
  }
  if (path === "/api/v1/trash/books") return { entries: booksInTrash };
  return {};
};
const fetchJsonMock = vi.fn(defaultFetch);
const invalidateApiPathsMock = vi.fn();

vi.mock("../hooks/use-api", () => ({
  fetchJson: (path: string, init?: { method?: string; body?: string }) => fetchJsonMock(path, init),
  invalidateApiPaths: (paths: ReadonlyArray<string>) => invalidateApiPathsMock(paths),
}));

const entry = (name: string, title: string) => ({
  entry: name,
  bookId: name.split("--")[0],
  trashedAt: "2026-11-26T01:02:03.000Z",
  bytes: 2048,
  fileCount: 12,
  title,
});

async function openPanel() {
  render(<TrashPanel />);
  await act(async () => {
    (await screen.findByRole("button", { name: /查看回收站/ })).click();
  });
}

afterEach(() => {
  cleanup();
  calls.length = 0;
  booksInTrash = [entry("b1--20261126090301", "被删的书")];
  restoreResult = { ok: true };
  fetchJsonMock.mockClear();
  fetchJsonMock.mockImplementation(defaultFetch);
  invalidateApiPathsMock.mockClear();
});

booksInTrash = [entry("b1--20261126090301", "被删的书")];

describe("TrashPanel", () => {
  it("lists deleted books with title, time and size once expanded", async () => {
    await openPanel();
    expect(await screen.findByText("被删的书")).toBeTruthy();
    expect(screen.getByText(/12/)).toBeTruthy();
    expect(screen.getByText(/2026/)).toBeTruthy();
    // 列表来自 GET /api/v1/trash/books。
    expect(calls.some((call) => call.path === "/api/v1/trash/books")).toBe(true);
  });

  it("shows an empty hint when the trash has no entries", async () => {
    booksInTrash = [];
    await openPanel();
    expect(await screen.findByText(/回收站是空的/)).toBeTruthy();
  });

  it("restores a book and refreshes the library list", async () => {
    await openPanel();
    await act(async () => {
      (await screen.findByRole("button", { name: /恢复/ })).click();
    });
    const restore = calls.find((call) => call.path === "/api/v1/trash/books/restore");
    expect(restore?.init?.method).toBe("POST");
    expect(JSON.parse(restore?.init?.body ?? "{}").entry).toBe("b1--20261126090301");
    // 书架列表失效重取。
    expect(invalidateApiPathsMock).toHaveBeenCalledWith(["/api/v1/books"]);
    // 面板重列：恢复后回收站为空。
    expect(await screen.findByText(/回收站是空的/)).toBeTruthy();
  });

  it("shows the conflict error when the book directory is already taken, and keeps the entry listed", async () => {
    restoreResult = { error: { code: "TRASH_TARGET_EXISTS", message: 'Book "b1" already exists.', conflicts: ["works/b1"] } };
    await openPanel();
    await act(async () => {
      (await screen.findByRole("button", { name: /恢复/ })).click();
    });
    expect(await screen.findByText(/already exists/)).toBeTruthy();
    // 条目还在列表里（没有假刷新成空态）。
    expect(screen.getByText("被删的书")).toBeTruthy();
  });

  it("purges only after the danger confirm dialog is confirmed", async () => {
    await openPanel();
    await act(async () => {
      (await screen.findByRole("button", { name: /彻底删除/ })).click();
    });
    // 确认弹层已开、purge 尚未发出。
    expect(screen.getByRole("heading", { name: /永久删除/ })).toBeTruthy();
    expect(calls.some((call) => call.path === "/api/v1/trash/books/purge")).toBe(false);
    await act(async () => {
      (await screen.findByRole("button", { name: /永久删除/ })).click();
    });
    const purge = calls.find((call) => call.path === "/api/v1/trash/books/purge");
    expect(purge?.init?.method).toBe("POST");
    expect(JSON.parse(purge?.init?.body ?? "{}").entry).toBe("b1--20261126090301");
    // 重列后为空。
    expect(await screen.findByText(/回收站是空的/)).toBeTruthy();
  });

  it("does not purge when the confirm dialog is cancelled", async () => {
    await openPanel();
    await act(async () => {
      (await screen.findByRole("button", { name: /彻底删除/ })).click();
    });
    await act(async () => {
      (await screen.findByRole("button", { name: /取消/ })).click();
    });
    expect(calls.some((call) => call.path === "/api/v1/trash/books/purge")).toBe(false);
    expect(screen.getByText("被删的书")).toBeTruthy();
  });
});
