import { describe, expect, it } from "vitest";
import type { HashRoute } from "@/hooks/use-hash-route";
import {
  ALL_ROUTE_PAGES,
  NAV_SECTIONS,
  sectionForPage,
  sectionForRoute,
  sectionTitle,
} from "./nav-sections";

describe("NAV_SECTIONS coverage", () => {
  it("covers every route page exactly once — no orphans, no duplicates", () => {
    const assigned = NAV_SECTIONS.flatMap((section) => section.pages);
    const counts = new Map<string, number>();
    for (const page of assigned) {
      counts.set(page, (counts.get(page) ?? 0) + 1);
    }
    // 全集每页恰好归属一区
    for (const page of ALL_ROUTE_PAGES) {
      expect(counts.get(page) ?? 0, `${page} 应恰好归属一区`).toBe(1);
    }
    // 反向：映射表里没有全集之外的幽灵页
    expect([...counts.keys()].sort()).toEqual([...ALL_ROUTE_PAGES].sort());
  });

  it("keeps section order stable for the activity bar", () => {
    expect(NAV_SECTIONS.map((section) => section.id)).toEqual(["create", "tools", "manage", "film"]);
  });
});

describe("sectionForRoute", () => {
  it("assigns parameterized creation routes to the create zone", () => {
    expect(sectionForRoute({ page: "book", bookId: "b1" })).toBe("create");
    expect(sectionForRoute({ page: "chapter", bookId: "b1", chapterNumber: 3 })).toBe("create");
    expect(sectionForRoute({ page: "analytics", bookId: "b1" })).toBe("create");
    expect(sectionForRoute({ page: "truth", bookId: "b1" })).toBe("create");
    expect(sectionForRoute({ page: "book-settings", bookId: "b1" })).toBe("create");
    expect(sectionForRoute({ page: "chat" })).toBe("create");
  });

  it("assigns tool, manage and film routes to their zones", () => {
    expect(sectionForRoute({ page: "translation" })).toBe("tools");
    expect(sectionForRoute({ page: "import", tab: "canon" })).toBe("tools");
    expect(sectionForRoute({ page: "genres" })).toBe("tools");
    expect(sectionForRoute({ page: "service-detail", serviceId: "openai" })).toBe("manage");
    expect(sectionForRoute({ page: "daemon" })).toBe("manage");
    expect(sectionForRoute({ page: "logs" })).toBe("manage");
    expect(sectionForRoute({ page: "film-studio", projectId: "p1" })).toBe("film");
    expect(sectionForRoute({ page: "flow", projectId: "p1" })).toBe("film");
    expect(sectionForRoute({ page: "play", projectId: "p1" })).toBe("film");
  });

  it("falls back to the create zone for unknown pages so selection never dangles", () => {
    expect(sectionForPage("no-such-page" as HashRoute["page"])).toBe("create");
  });
});

describe("sectionTitle", () => {
  it("picks the title for the active UI language", () => {
    const create = NAV_SECTIONS[0];
    expect(sectionTitle(create, "zh")).toBe("创作");
    expect(sectionTitle(create, "en")).toBe("Create");
  });
});
