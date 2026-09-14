// @vitest-environment jsdom
//! 452 号：CodexPanel 交互测试——自动选中回填（442 号 SeriesCanon 同款静默
//! 清空防御的镜像回归）+ 保存卡片 PUT 载荷断言。
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { CodexPanel } from "./CodexPanel";

const CODEX_CARDS = [
  { name: "苏檀", kind: "character", summary: "镜宗外门弟子。", facts: ["碎镜贴身收着", "能听见镜中低语"] },
];

const fetchJsonMock = vi.fn(async (path: string, init?: { method?: string; body?: string }) => {
  if (path.includes("/codex") && (!init?.method || init.method === "GET")) {
    return { cards: CODEX_CARDS };
  }
  if (path.includes("/codex") && init?.method === "PUT") {
    return JSON.parse(init.body ?? "{}");
  }
  return {};
});

vi.mock("../hooks/use-api", () => ({
  fetchJson: (path: string, init?: { method?: string; body?: string }) => fetchJsonMock(path, init),
}));

const factsTa = () =>
  document.querySelector("textarea[placeholder^='正典事实']") as HTMLTextAreaElement;
const summaryTa = () =>
  document.querySelector("textarea[placeholder^='卡片摘要']") as HTMLTextAreaElement;

afterEach(() => cleanup());

describe("CodexPanel 交互（452 号）", () => {
  afterEach(() => {
    fetchJsonMock.mockClear();
  });

  it("挂载即自动选中首卡并回填摘要/事实（静默清空防御）", async () => {
    render(<CodexPanel bookId="b1" />);
    await vi.waitFor(() => {
      expect(summaryTa()).toBeTruthy();
    });
    expect(summaryTa().value).toBe("镜宗外门弟子。");
    expect(factsTa().value).toBe("碎镜贴身收着\n能听见镜中低语");
  });

  it("编辑事实后保存卡片——PUT 载荷含新增行且保留原事实", async () => {
    const user = userEvent.setup();
    render(<CodexPanel bookId="b1" />);
    await vi.waitFor(() => {
      expect(factsTa()).toBeTruthy();
    });
    await user.type(factsTa(), "\n镜界反噬时左眼会渗血");
    const save = Array.from(document.querySelectorAll("button")).find(
      (b) => b.textContent?.trim() === "保存卡片",
    ) as HTMLButtonElement;
    await user.click(save);
    await vi.waitFor(() => {
      const putCall = fetchJsonMock.mock.calls.find(
        ([path, init]) => path.includes("/codex") && (init as { method?: string })?.method === "PUT",
      );
      expect(putCall).toBeDefined();
      const payload = JSON.parse((putCall![1] as { body: string }).body);
      expect(payload.cards[0].facts).toContain("镜界反噬时左眼会渗血");
      expect(payload.cards[0].facts).toContain("碎镜贴身收着");
    });
  });
});
