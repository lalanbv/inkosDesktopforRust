import { describe, it, expect } from "vitest";
import {
  buildNavigationCommands,
  buildActionCommands,
  buildRecentCommands,
  buildRecommendedCommands,
  recentToRoute,
  filterCommands,
  groupCommands,
  commandTitle,
} from "./commands";
import type { CommandContext, CommandEntry } from "./commands";
import type { RecentEntry } from "@/store/recents";

function mockCtx(): { ctx: CommandContext; routes: unknown[]; calls: string[] } {
  const routes: unknown[] = [];
  const calls: string[] = [];
  const ctx: CommandContext = {
    setRoute: (route) => routes.push(route),
    setThemeMode: (mode) => calls.push(`theme:${mode}`),
    setProjectLanguage: (lang) => calls.push(`lang:${lang}`),
    refetchProject: () => calls.push("refresh"),
    openBookCreate: () => calls.push("bookCreate"),
    createProjectChatDraft: () => calls.push("draft"),
    launchProjectMode: (kind, playMode) => calls.push(`launch:${kind}:${playMode ?? ""}`),
  };
  return { ctx, routes, calls };
}

const nav = buildNavigationCommands();
const actions = buildActionCommands();

describe("buildNavigationCommands", () => {
  it("covers every parameterless page plus import tabs (18 entries)", () => {
    expect(nav).toHaveLength(18);
    const pages = new Set(nav.map((entry) => entry.id));
    for (const id of [
      "nav.dashboard", "nav.chat", "nav.book-create", "nav.services",
      "nav.project-settings", "nav.daemon", "nav.logs", "nav.genres",
      "nav.style", "nav.translation", "nav.import", "nav.radar", "nav.doctor",
    ]) {
      expect(pages.has(id)).toBe(true);
    }
  });

  it("every entry has a function run and a unique id", () => {
    const ids = new Set(nav.map((entry) => entry.id));
    expect(ids.size).toBe(nav.length);
    for (const entry of nav) expect(typeof entry.run).toBe("function");
  });

  it("running an entry navigates to the matching route", () => {
    const { ctx, routes } = mockCtx();
    nav.find((entry) => entry.id === "nav.dashboard")!.run(ctx);
    nav.find((entry) => entry.id === "nav.import.chapters")!.run(ctx);
    expect(routes).toEqual([
      { page: "dashboard" },
      { page: "import", tab: "chapters" },
    ]);
  });
});

describe("buildActionCommands", () => {
  it("registers at least 18 actions, bringing the total over 30", () => {
    expect(actions).toHaveLength(19);
    expect(nav.length + actions.length).toBeGreaterThanOrEqual(30);
    const ids = new Set(actions.map((entry) => entry.id));
    expect(ids.size).toBe(actions.length);
    for (const entry of actions) expect(typeof entry.run).toBe("function");
  });

  it("create entries route through the expected context handlers", () => {
    const { ctx, calls } = mockCtx();
    actions.find((entry) => entry.id === "action.bookCreate")!.run(ctx);
    actions.find((entry) => entry.id === "action.playGuided")!.run(ctx);
    actions.find((entry) => entry.id === "action.fanfic")!.run(ctx);
    actions.find((entry) => entry.id === "action.themeDark")!.run(ctx);
    actions.find((entry) => entry.id === "action.themeAuto")!.run(ctx);
    actions.find((entry) => entry.id === "action.langEn")!.run(ctx);
    actions.find((entry) => entry.id === "action.projectRefresh")!.run(ctx);
    expect(calls).toEqual([
      "bookCreate",
      "launch:play:guided",
      "draft",
      "theme:dark",
      "theme:auto",
      "lang:en",
      "refresh",
    ]);
  });
});

describe("recent entries", () => {
  it("rebuilds a full route from a stored entry", () => {
    expect(recentToRoute({ page: "chapter", label: "第 3 章", bookId: "b1", chapterNumber: 3 }))
      .toEqual({ page: "chapter", bookId: "b1", chapterNumber: 3 });
    expect(recentToRoute({ page: "film-studio", label: "影游", projectId: "p1" }))
      .toEqual({ page: "film-studio", projectId: "p1" });
  });

  it("drops entries whose route parameters are missing", () => {
    expect(recentToRoute({ page: "book", label: "书" })).toBeNull();
    expect(recentToRoute({ page: "chapter", label: "章", bookId: "b1" })).toBeNull();
  });

  it("buildRecentCommands navigates back on run", () => {
    const recents: RecentEntry[] = [{ page: "logs", label: "日志" }];
    const commands = buildRecentCommands(recents);
    expect(commands).toHaveLength(1);
    expect(commands[0].group).toBe("recent");
    const { ctx, routes } = mockCtx();
    commands[0].run(ctx);
    expect(routes).toEqual([{ page: "logs" }]);
  });
});

describe("buildRecommendedCommands", () => {
  const all: CommandEntry[] = [...nav, ...actions];

  it("recommends create-book, the latest recent (or chat), and settings", () => {
    const recents = buildRecentCommands([{ page: "logs", label: "日志" }]);
    expect(buildRecommendedCommands(all, recents).map((entry) => entry.id)).toEqual([
      "action.bookCreate",
      "recent:logs|||||",
      "nav.project-settings",
    ]);
  });

  it("falls back to chat when there is no history", () => {
    expect(buildRecommendedCommands(all, []).map((entry) => entry.id)).toEqual([
      "action.bookCreate",
      "nav.chat",
      "nav.project-settings",
    ]);
  });
});

describe("filterCommands", () => {
  const list: CommandEntry[] = [...nav, ...actions];

  it("returns the list as-is for a blank query", () => {
    expect(filterCommands(list, "")).toHaveLength(list.length);
    expect(filterCommands(list, "   ")).toHaveLength(list.length);
  });

  it("matches Chinese titles, English titles and keywords case-insensitively", () => {
    expect(filterCommands(list, "设置").map((entry) => entry.id)).toContain("nav.project-settings");
    expect(filterCommands(list, "LOGS").map((entry) => entry.id)).toContain("nav.logs");
    expect(filterCommands(list, "genre").map((entry) => entry.id)).toContain("nav.genres");
    expect(filterCommands(list, "daemon").map((entry) => entry.id)).toContain("nav.daemon");
  });

  it("matches recents by label too", () => {
    const withRecents = [...buildRecentCommands([{ page: "book", label: "山河志", bookId: "b1" }]), ...list];
    expect(filterCommands(withRecents, "山河").map((entry) => entry.id)).toContain("recent:book|b1||||");
  });

  it("returns an empty list when nothing matches", () => {
    expect(filterCommands(list, "zzz-no-match")).toEqual([]);
  });
});

describe("groupCommands", () => {
  it("orders recent before navigation before action and drops empty groups", () => {
    const recent = buildRecentCommands([{ page: "logs", label: "日志" }])[0];
    const grouped = groupCommands([nav[0], recent, actions[0]]);
    expect(grouped.map((group) => group.group)).toEqual(["recent", "navigation", "action"]);
    expect(grouped[0].entries).toEqual([recent]);
  });

  it("keeps registration order inside a group", () => {
    const grouped = groupCommands([nav[2], nav[0], nav[1]]);
    expect(grouped).toHaveLength(1);
    expect(grouped[0].entries.map((entry) => entry.id)).toEqual([
      nav[2].id,
      nav[0].id,
      nav[1].id,
    ]);
  });
});

describe("commandTitle", () => {
  it("picks the title for the active UI language", () => {
    const entry = nav[0];
    expect(commandTitle(entry, "zh")).toBe(entry.titleZh);
    expect(commandTitle(entry, "en")).toBe(entry.titleEn);
  });
});
