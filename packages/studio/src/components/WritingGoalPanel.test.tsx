// @vitest-environment jsdom
//! R51/1653 号：写作目标面板交互测试——账本行面 → 月历三档着色/streak/
//! 日目标编辑（localStorage 持久）。纯函数语义由 core golden 锁定，此处
//! 锁渲染接线（dayKey 前端本地计算、月首偏移、三档 class）。真实时钟 +
//! 相对日期（今天/昨/前日），零 fake timers（waitFor 与假时钟死锁）。
import { localDayKey } from "@actalk/inkos-core/utils/writing-goal";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { WritingGoalPanel } from "./WritingGoalPanel";

const fetchJsonMock = vi.hoisted(() => vi.fn());

vi.mock("../hooks/use-api", () => ({
  fetchJson: (path: string) => fetchJsonMock(path),
}));

function shiftDays(offset: number): string {
  const now = new Date();
  return localDayKey(new Date(now.getFullYear(), now.getMonth(), now.getDate() + offset));
}

function seedRows(): Array<{ bookId: string; ts: string; chapterNum: number; before: number; after: number }> {
  const at = (offset: number, before: number, after: number) => {
    const now = new Date();
    const local = new Date(now.getFullYear(), now.getMonth(), now.getDate() + offset, 12, 0, 0);
    return { bookId: "b1", ts: local.toISOString(), chapterNum: 1, before, after };
  };
  return [
    at(0, 100, 700),   // 今日 +600（达成 500 → 计入）
    at(-1, 400, 1000), // 昨日 +600（达成）
    at(-2, 0, 100),    // 前日 +100（未达）
  ];
}

describe("WritingGoalPanel（R51/1653 号）", () => {
  beforeEach(() => {
    globalThis.localStorage?.clear();
    fetchJsonMock.mockReset();
  });
  afterEach(() => {
    cleanup();
  });

  it("行面 → 月历三档着色 + streak（今日达成计入）", async () => {
    fetchJsonMock.mockResolvedValue({ rows: seedRows() });
    render(<WritingGoalPanel />);
    // streak：今日+昨日两日达成 → 2（data-goal 锚避免与月历日格文本撞）。
    await waitFor(() => {
      expect(document.querySelector('[data-goal="streak"]')?.textContent).toBe("2");
    });
    const metCell = document.querySelector(`[data-goal-cell="${shiftDays(0)}"]`);
    expect(metCell?.className).toContain("bg-primary");
    expect(metCell?.className).not.toContain("bg-primary/40");
    const unmetCell = document.querySelector(`[data-goal-cell="${shiftDays(-2)}"]`);
    expect(unmetCell?.className).toContain("bg-primary/40");
    // 无记录日（更早两天）muted 档。
    const emptyCell = document.querySelector(`[data-goal-cell="${shiftDays(-4)}"]`);
    expect(emptyCell?.className).toContain("bg-muted");
  });

  it("日目标编辑落 localStorage 并即时重算着色档与 streak", async () => {
    fetchJsonMock.mockResolvedValue({ rows: seedRows() });
    render(<WritingGoalPanel />);
    await waitFor(() => {
      expect(document.querySelector(`[data-goal-cell="${shiftDays(-2)}"]`)).toBeTruthy();
    });
    const input = screen.getByDisplayValue("500");
    fireEvent.change(input, { target: { value: "100" } });
    // 目标降为 100：前日（+100）从「未达」升「达成」档；streak 三日全达成 → 3。
    await waitFor(() => {
      const cell = document.querySelector(`[data-goal-cell="${shiftDays(-2)}"]`);
      expect(cell?.className).toContain("bg-primary");
      expect(cell?.className).not.toContain("bg-primary/40");
    });
    expect(globalThis.localStorage?.getItem("inkos.dailyGoalWords")).toBe("100");
    expect(document.querySelector('[data-goal="streak"]')?.textContent).toBe("3");
  });

  it("行面失败 → 面板降级空账本（不炸卡）", async () => {
    fetchJsonMock.mockRejectedValue(new Error("unreachable"));
    render(<WritingGoalPanel />);
    await waitFor(() => {
      expect(document.querySelector('[data-goal-panel="ready"]')).toBeTruthy();
    });
    // 全月无记录 → 全 muted（零崩溃、网格仍在）。
    const anyCell = document.querySelector('[data-goal-cell]');
    expect(anyCell?.className).toContain("bg-muted");
  });
});
