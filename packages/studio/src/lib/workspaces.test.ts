import { describe, expect, it } from "vitest";
import { resolveWorkspaceDestination, WORKSPACE_DEFS, WORKSPACE_IDS, isWorkspaceId } from "./workspaces";
import type { RecentEntry } from "@/store/recents";

const recent = (overrides: Partial<RecentEntry> & Pick<RecentEntry, "page" | "label">): RecentEntry => ({
  ...overrides,
});

describe("WORKSPACE_DEFS", () => {
  it("keeps the hotkey order stable: index i maps to mod+alt+(i+1)", () => {
    // App.tsx 的 HOTKEY_DEFS 按 WORKSPACE_IDS 序生成 ⌘⌥1..4——顺序即契约
    expect(WORKSPACE_IDS).toEqual(["writing", "review", "analysis", "film"]);
    expect(WORKSPACE_DEFS.map((def) => def.id)).toEqual(WORKSPACE_IDS);
  });

  it("isWorkspaceId rejects unknown ids (dispatchCommand guard depends on it)", () => {
    for (const id of WORKSPACE_IDS) expect(isWorkspaceId(id)).toBe(true);
    expect(isWorkspaceId("writing ")).toBe(false);
    expect(isWorkspaceId("book")).toBe(false);
    expect(isWorkspaceId("")).toBe(false);
  });
});

describe("resolveWorkspaceDestination", () => {
  it("writing lands on the most recent book page", () => {
    const recents = [
      recent({ page: "book", label: "书A", bookId: "a" }),
      recent({ page: "book", label: "书B", bookId: "b" }),
    ];
    expect(resolveWorkspaceDestination("writing", recents)).toEqual({ page: "book", bookId: "a" });
  });

  it("writing falls back to any bookId-carrying record (chapter/analytics/truth)", () => {
    const recents = [
      recent({ page: "dashboard", label: "首页" }),
      recent({ page: "chapter", label: "第 3 章", bookId: "c", chapterNumber: 3 }),
    ];
    expect(resolveWorkspaceDestination("writing", recents)).toEqual({ page: "book", bookId: "c" });
  });

  it("writing returns null with no book traces (caller falls back to dashboard)", () => {
    expect(resolveWorkspaceDestination("writing", [])).toBeNull();
    expect(resolveWorkspaceDestination("writing", [recent({ page: "services", label: "服务" })])).toBeNull();
  });

  it("review lands on the timeline of the most recent book", () => {
    const recents = [
      recent({ page: "book", label: "书A", bookId: "a" }),
      recent({ page: "book", label: "书B", bookId: "b" }),
    ];
    expect(resolveWorkspaceDestination("review", recents)).toEqual({ page: "book-timeline", bookId: "a" });
  });

  it("review returns null without bookId records", () => {
    expect(resolveWorkspaceDestination("review", [])).toBeNull();
  });

  it("analysis is always reachable — radar needs no parameters", () => {
    expect(resolveWorkspaceDestination("analysis", [])).toEqual({ page: "radar" });
  });

  it("film lands on the studio of the most recent project-carrying record", () => {
    const recents = [
      recent({ page: "play", label: "试玩", projectId: "p1" }),
      recent({ page: "film-studio", label: "工作室", projectId: "p2" }),
    ];
    expect(resolveWorkspaceDestination("film", recents)).toEqual({ page: "film-studio", projectId: "p1" });
  });

  it("film returns null without project traces", () => {
    expect(resolveWorkspaceDestination("film", [])).toBeNull();
    expect(resolveWorkspaceDestination("film", [recent({ page: "book", label: "书A", bookId: "a" })])).toBeNull();
  });
});
