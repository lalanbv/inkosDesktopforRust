import { describe, expect, it } from "vitest";
import type { HashRoute } from "@/hooks/use-hash-route";
import {
  EMPTY_TABS,
  findTabByRoute,
  routeKey,
  sanitizeTabsSnapshot,
  tabsReducer,
} from "./tabs-reducer";
import type { TabsSnapshot } from "./tabs-reducer";

function open(state: TabsSnapshot, route: HashRoute, title: string, preview = false): TabsSnapshot {
  return tabsReducer(state, { type: "open", route, title, preview });
}

const dashboard: HashRoute = { page: "dashboard" };
const bookA: HashRoute = { page: "book", bookId: "a" };
const bookB: HashRoute = { page: "book", bookId: "b" };
const chatA: HashRoute = { page: "chapter", bookId: "a", chapterNumber: 2 };

describe("routeKey", () => {
  it("separates books, chapters and import tabs", () => {
    expect(routeKey({ page: "book", bookId: "a" })).not.toBe(routeKey({ page: "book", bookId: "b" }));
    expect(routeKey({ page: "book", bookId: "a" })).not.toBe(routeKey({ page: "chapter", bookId: "a", chapterNumber: 2 }));
    expect(routeKey({ page: "import", tab: "canon" })).not.toBe(routeKey({ page: "import" }));
    expect(routeKey({ page: "import" })).toBe(routeKey({ page: "import" }));
  });
});

describe("open", () => {
  it("appends a sticky tab and activates it", () => {
    const state = open(EMPTY_TABS, bookA, "书 A");
    expect(state.tabs).toHaveLength(1);
    expect(state.tabs[0].preview).toBe(false);
    expect(state.activeId).toBe(state.tabs[0].id);
  });

  it("reuses an existing tab for the same route (dedupe by route key)", () => {
    let state = open(EMPTY_TABS, bookA, "书 A");
    state = open(state, { page: "book", bookId: "a" }, "书 A 再次打开");
    expect(state.tabs).toHaveLength(1);
  });

  it("sticky open promotes an existing preview tab", () => {
    let state = open(EMPTY_TABS, bookA, "书 A", true);
    expect(state.tabs[0].preview).toBe(true);
    state = open(state, { page: "book", bookId: "a" }, "书 A");
    expect(state.tabs).toHaveLength(1);
    expect(state.tabs[0].preview).toBe(false);
  });

  it("preview open replaces the active preview tab in place", () => {
    let state = open(EMPTY_TABS, bookA, "书 A"); // 常驻
    state = open(state, bookB, "书 B", true); // 预览 1
    expect(state.tabs).toHaveLength(2);
    state = open(state, { page: "logs" }, "日志", true); // 预览替换
    expect(state.tabs).toHaveLength(2);
    // 原位替换:第 2 个位置仍是预览标签且保持激活,路由换新(id 随路由键更新)
    expect(state.tabs[1].route).toEqual({ page: "logs" });
    expect(state.tabs[1].preview).toBe(true);
    expect(state.activeId).toBe(state.tabs[1].id);
    expect(findTabByRoute(state, bookB)).toBeUndefined();
  });

  it("preview open reuses a detached preview tab when the active tab is sticky", () => {
    let state = open(EMPTY_TABS, bookA, "书 A"); // 常驻
    state = tabsReducer(state, { type: "activate", id: state.tabs[0].id });
    state = open(state, bookB, "书 B", true); // 预览 1
    state = open(state, { page: "logs" }, "日志", true); // 活活标签是常驻 → 替换游离的预览标签
    expect(state.tabs).toHaveLength(2);
    expect(findTabByRoute(state, bookB)).toBeUndefined();
    expect(findTabByRoute(state, { page: "logs" })).toBeDefined();
  });
});

describe("close / closeOthers", () => {
  it("moves activation to the right neighbor, then left, then none", () => {
    let state = EMPTY_TABS;
    state = open(state, dashboard, "首页");
    state = open(state, bookA, "书 A");
    state = open(state, bookB, "书 B");
    const [home, a, b] = state.tabs;

    // 关中间 → 激活右邻
    state = tabsReducer(state, { type: "close", id: a.id });
    expect(state.activeId).toBe(b.id);
    // 关末位 → 激活左邻
    state = tabsReducer(state, { type: "close", id: b.id });
    expect(state.activeId).toBe(home.id);
    // 关最后一个 → 无激活
    state = tabsReducer(state, { type: "close", id: home.id });
    expect(state.tabs).toHaveLength(0);
    expect(state.activeId).toBeNull();
  });

  it("closeOthers keeps the target plus pinned tabs", () => {
    let state = open(EMPTY_TABS, dashboard, "首页");
    state = open(state, bookA, "书 A");
    state = open(state, bookB, "书 B");
    const [home, a, b] = state.tabs;
    state = tabsReducer(state, { type: "pin", id: home.id, pinned: true });
    state = tabsReducer(state, { type: "closeOthers", id: b.id });
    expect(state.tabs.map((tab) => tab.id)).toEqual([home.id, b.id]);
    expect(state.activeId).toBe(b.id);
    expect(a).toBeDefined();
  });
});

describe("pin", () => {
  it("pins a tab and clears its preview flag", () => {
    let state = open(EMPTY_TABS, bookA, "书 A", true);
    state = tabsReducer(state, { type: "pin", id: state.tabs[0].id, pinned: true });
    expect(state.tabs[0].pinned).toBe(true);
    expect(state.tabs[0].preview).toBe(false);
    expect(state.activeId).toBe(state.tabs[0].id);
  });
});

describe("retitle", () => {
  it("updates the title when it actually changes and stays a no-op otherwise", () => {
    let state = open(EMPTY_TABS, bookA, "书籍");
    state = tabsReducer(state, { type: "retitle", id: state.tabs[0].id, title: "山河志" });
    expect(state.tabs[0].title).toBe("山河志");
    const again = tabsReducer(state, { type: "retitle", id: state.tabs[0].id, title: "山河志" });
    expect(again).toBe(state);
  });
});

describe("activate", () => {
  it("ignores unknown ids instead of dangling the active tab", () => {
    const state = open(EMPTY_TABS, bookA, "书 A");
    const next = tabsReducer(state, { type: "activate", id: "no-such-tab" });
    expect(next).toBe(state);
  });
});

describe("sanitizeTabsSnapshot (hydrate)", () => {
  it("drops malformed entries and duplicate route keys", () => {
    const snapshot = sanitizeTabsSnapshot({
      tabs: [
        { id: "book:a#1", route: { page: "book", bookId: "a" }, title: "书 A", preview: true },
        { id: "book:a#2", route: { page: "book", bookId: "a" }, title: "重复" },
        { id: "broken#3", title: "缺路由" },
        "not-an-object",
        { id: "logs#4", route: { page: "logs" }, title: "日志" },
      ],
      activeId: "dangling",
    });
    expect(snapshot.tabs.map((tab) => tab.id)).toEqual(["book:a#1", "logs#4"]);
    // activeId 悬空 → 回退末位标签
    expect(snapshot.activeId).toBe("logs#4");
  });

  it("keeps a valid activeId and degrades to empty for junk input", () => {
    const snapshot = sanitizeTabsSnapshot({
      tabs: [
        { id: "book:a#1", route: { page: "book", bookId: "a" }, title: "书 A" },
        { id: "logs#2", route: { page: "logs" }, title: "日志" },
      ],
      activeId: "book:a#1",
    });
    expect(snapshot.activeId).toBe("book:a#1");
    expect(sanitizeTabsSnapshot(null)).toEqual(EMPTY_TABS);
    expect(sanitizeTabsSnapshot({ tabs: "x" })).toEqual(EMPTY_TABS);
  });
});

describe("20 tabs fuzz", () => {
  it("keeps consistent invariants under a long open/close/pin sequence", () => {
    let state: TabsSnapshot = EMPTY_TABS;
    for (let i = 0; i < 20; i++) {
      state = open(state, { page: "chapter", bookId: `b${i % 7}`, chapterNumber: i + 1 }, `第 ${i + 1} 章`);
    }
    expect(state.tabs).toHaveLength(20);
    expect(state.activeId).toBe(state.tabs.at(-1)!.id);

    // 每隔一个关闭
    for (let i = 18; i >= 0; i -= 2) {
      state = tabsReducer(state, { type: "close", id: state.tabs[i].id });
    }
    expect(state.tabs).toHaveLength(10);
    // 不变量:activeId 要么空要么指向现存标签
    expect(state.activeId === null || state.tabs.some((tab) => tab.id === state.activeId)).toBe(true);
  });
});
