// @vitest-environment jsdom
//! 452 号：SeriesCanonPanel 交互测试——442 号自动回填修复的回归锁定 +
//! 保存条目 PUT 载荷断言。
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { SeriesCanonPanel } from "./SeriesCanonPanel";

const CANON_ENTRIES = [
  {
    name: "镜灵",
    aliases: ["镜中少女"],
    kind: "character",
    summary: "苏檀在镜界的引路人。",
    facts: ["镜灵不能离开镜面超过十步"],
    relationships: [],
  },
];

const fetchJsonMock = vi.fn(async (path: string, init?: { method?: string; body?: string }) => {
  if (path.endsWith("/series-id") && (!init?.method || init.method === "GET")) {
    return { seriesId: "s1" };
  }
  if (path.includes("/canon") && init?.method === "PUT") {
    return JSON.parse(init.body ?? "{}");
  }
  if (path.includes("/canon")) {
    return { entries: CANON_ENTRIES };
  }
  return {};
});

vi.mock("../hooks/use-api", () => ({
  fetchJson: (path: string, init?: { method?: string; body?: string }) => fetchJsonMock(path, init),
}));

const aliasesInput = () =>
  document.querySelector("input[placeholder^='别名']") as HTMLInputElement;
const summaryTa = () =>
  document.querySelector("textarea[placeholder^='条目摘要']") as HTMLTextAreaElement;
const factsTa = () =>
  document.querySelector("textarea[placeholder^='跨书正典事实']") as HTMLTextAreaElement;

afterEach(() => {
  fetchJsonMock.mockClear();
});

describe("SeriesCanonPanel 交互（442 号回归）", () => {
  it("绑定加载后自动选中首条目并回填编辑字段（静默清空防御）", async () => {
    render(<SeriesCanonPanel bookId="b1" />);
    await vi.waitFor(() => {
      expect(aliasesInput()).toBeTruthy();
    });
    expect(aliasesInput().value).toBe("镜中少女");
    expect(summaryTa().value).toBe("苏檀在镜界的引路人。");
    expect(factsTa().value).toBe("镜灵不能离开镜面超过十步");
  });

  it("编辑摘要后保存条目——PUT 载荷含新摘要且保留事实", async () => {
    const user = userEvent.setup();
    render(<SeriesCanonPanel bookId="b1" />);
    await vi.waitFor(() => {
      expect(summaryTa()).toBeTruthy();
    });
    await user.clear(summaryTa());
    await user.type(summaryTa(), "镜界引路人，身份成谜。");
    const save = Array.from(document.querySelectorAll("button")).find(
      (b) => b.textContent?.trim() === "保存条目",
    ) as HTMLButtonElement;
    await user.click(save);
    await vi.waitFor(() => {
      const putCall = fetchJsonMock.mock.calls.find(
        ([path, init]) => path.includes("/canon") && (init as { method?: string })?.method === "PUT",
      );
      expect(putCall).toBeDefined();
      const payload = JSON.parse((putCall![1] as { body: string }).body);
      expect(payload.entries[0].summary).toBe("镜界引路人，身份成谜。");
      expect(payload.entries[0].facts).toContain("镜灵不能离开镜面超过十步");
      expect(payload.entries[0].aliases).toContain("镜中少女");
    });
  });
});
