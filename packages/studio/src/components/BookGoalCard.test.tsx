// @vitest-environment jsdom
//! R51/1654 号：书级目标卡交互测试——进度条/预计完成日投影（pace=近 7 日
//! 账本日均）/targetWords/deadline 编辑保存载荷（number|null 语义）。
//! 纯函数语义由 core golden 锁定，此处锁渲染与保存接线。
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { BookGoalCard } from "./BookGoalCard";

const fetchJsonMock = vi.hoisted(() => vi.fn());

vi.mock("../hooks/use-api", () => ({
  fetchJson: (path: string, init?: { method?: string; body?: string }) => fetchJsonMock(path, init),
}));

function atDay(offset: number): string {
  const now = new Date();
  return new Date(now.getFullYear(), now.getMonth(), now.getDate() + offset, 12, 0, 0).toISOString();
}

function seededBook() {
  return { targetWords: 10000, deadline: "2026-12-31" };
}

function seededRows() {
  // 近 3 日各 +1000 → 近 7 日净 3000 → 日均 floor(3000/7)=428。
  return [0, -1, -2].map((offset) => ({
    bookId: "b1", ts: atDay(offset), chapterNum: 1, before: 0, after: 1000,
  }));
}

describe("BookGoalCard（R51/1654 号）", () => {
  beforeEach(() => {
    fetchJsonMock.mockReset();
  });
  afterEach(() => cleanup());

  it("进度条+预计完成日投影（pace=近 7 日日均净增）", async () => {
    fetchJsonMock.mockImplementation((path: string) => {
      if (path === "/books/b1") return Promise.resolve(seededBook());
      if (path === "/writing-log-rows") return Promise.resolve({ rows: seededRows() });
      return Promise.reject(new Error("unexpected " + path));
    });
    render(<BookGoalCard bookId="b1" chapters={[{ number: 1, wordCount: 5000 }]} />);
    // 5000/10000=50%。
    await waitFor(() => {
      expect(document.querySelector('[data-goal-card="progress"]')?.textContent).toContain("50%");
    });
    expect(document.querySelector('[data-goal-card="bar"]')?.getAttribute("style")).toContain("width: 50%");
    // pace=floor(3000/7)=428 字/日 → 剩 5000 → ceil(5000/428)=12 天。
    const finish = document.querySelector('[data-goal-card="expected-finish"]')?.textContent ?? "";
    expect(finish).toMatch(/\d{4}-\d{2}-\d{2}/);
    expect(document.querySelector('[data-goal-card="deadline"]')?.textContent).toContain("2026-12-31");
  });

  it("编辑保存载荷：targetWords=null 清除 + deadline 更新", async () => {
    fetchJsonMock.mockImplementation((path: string, init?: { method?: string; body?: string }) => {
      if (path === "/books/b1" && !init) return Promise.resolve(seededBook());
      if (path === "/writing-log-rows") return Promise.resolve({ rows: [] });
      if (path === "/books/b1" && init?.method === "PUT") return Promise.resolve({ ok: true });
      return Promise.reject(new Error("unexpected " + path));
    });
    render(<BookGoalCard bookId="b1" chapters={[]} />);
    await waitFor(() => {
      expect(document.querySelector('[data-goal-card="target-input"]')).toBeTruthy();
    });
    // 清空目标 + 改截止日 → 保存载荷 targetWords=null, deadline=新值。
    fireEvent.change(document.querySelector('[data-goal-card="target-input"]')!, { target: { value: "" } });
    fireEvent.change(document.querySelector('[data-goal-card="deadline-input"]')!, { target: { value: "2027-01-15" } });
    fireEvent.click(document.querySelector('[data-goal-card="save"]')!);
    await waitFor(() => {
      expect(document.querySelector('[data-goal-card="notice"]')?.textContent).toContain("目标已保存");
    });
    const put = fetchJsonMock.mock.calls.find(([path, init]) => path === "/books/b1" && init?.method === "PUT");
    expect(put).toBeTruthy();
    expect(JSON.parse(put![1]!.body!)).toEqual({ targetWords: null, deadline: "2027-01-15" });
  });

  it("无目标书显示空态（不渲染进度条）", async () => {
    fetchJsonMock.mockImplementation((path: string) => {
      if (path === "/books/b1") return Promise.resolve({});
      if (path === "/writing-log-rows") return Promise.resolve({ rows: [] });
      return Promise.reject(new Error("unexpected " + path));
    });
    render(<BookGoalCard bookId="b1" chapters={[]} />);
    await waitFor(() => {
      expect(screen.getByText("尚未设置全书目标字数。")).toBeTruthy();
    });
    expect(document.querySelector('[data-goal-card="progress"]')).toBeNull();
  });
});
