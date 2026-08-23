import { describe, it, expect, beforeEach, vi } from "vitest";
import {
  useRecentsStore,
  readStoredRecents,
  applyRecentPush,
  recentIdentity,
  RECENTS_STORAGE_KEY,
  RECENTS_LIMIT,
} from "./store";
import type { RecentEntry } from "./types";

function fakeStorage(entries: Record<string, string> = {}) {
  const store = { ...entries };
  return {
    getItem: (key: string) => (key in store ? store[key] : null),
    setItem: (key: string, value: string) => {
      store[key] = value;
    },
    dump: () => store,
  };
}

const entry = (page: RecentEntry["page"], extra: Partial<RecentEntry> = {}): RecentEntry => ({
  page,
  label: page,
  ...extra,
});

describe("readStoredRecents", () => {
  it("returns an empty list when storage is missing or blank", () => {
    expect(readStoredRecents(null)).toEqual([]);
    expect(readStoredRecents(fakeStorage())).toEqual([]);
  });

  it("restores valid entries and drops malformed ones", () => {
    const storage = fakeStorage({
      [RECENTS_STORAGE_KEY]: JSON.stringify([
        entry("book", { bookId: "b1", label: "书名" }),
        { page: "chapter" }, // 缺 label
        "not-an-object",
        42,
      ]),
    });
    expect(readStoredRecents(storage)).toEqual([entry("book", { bookId: "b1", label: "书名" })]);
  });

  it("degrades to an empty list on invalid JSON", () => {
    expect(readStoredRecents(fakeStorage({ [RECENTS_STORAGE_KEY]: "{oops" }))).toEqual([]);
  });
});

describe("applyRecentPush", () => {
  it("puts the newest entry first", () => {
    const next = applyRecentPush([entry("dashboard")], entry("logs"));
    expect(next.map((item) => item.page)).toEqual(["logs", "dashboard"]);
  });

  it("re-visiting an existing entry moves it to the front without duplicating", () => {
    const list = [entry("logs"), entry("dashboard"), entry("daemon")];
    const next = applyRecentPush(list, entry("dashboard"));
    expect(next.map((item) => item.page)).toEqual(["dashboard", "logs", "daemon"]);
  });

  it("caps the list at the limit and drops the oldest overflow", () => {
    const pages = Array.from({ length: RECENTS_LIMIT }, (_, i) => entry(`p${i}` as RecentEntry["page"]));
    const next = applyRecentPush(pages, entry("logs"));
    expect(next).toHaveLength(RECENTS_LIMIT);
    expect(next[0].page).toBe("logs");
    expect(next.at(-1)?.page).toBe(`p${RECENTS_LIMIT - 2}`);
  });

  it("identity distinguishes chapters of the same book", () => {
    const first = entry("chapter", { bookId: "b1", chapterNumber: 1 });
    const second = entry("chapter", { bookId: "b1", chapterNumber: 2 });
    expect(recentIdentity(first)).not.toBe(recentIdentity(second));
    const next = applyRecentPush([first], second);
    expect(next).toHaveLength(2);
  });
});

describe("useRecentsStore", () => {
  beforeEach(() => {
    useRecentsStore.setState({ recents: [] });
  });

  it("pushRecent updates state and persists to the storage key", () => {
    const storage = fakeStorage();
    vi.stubGlobal("window", { localStorage: storage });
    try {
      useRecentsStore.getState().pushRecent(entry("book", { bookId: "b1", label: "书名" }));
      const state = useRecentsStore.getState().recents;
      expect(state.map((item) => item.label)).toEqual(["书名"]);
      expect(JSON.parse(storage.dump()[RECENTS_STORAGE_KEY])).toEqual([
        entry("book", { bookId: "b1", label: "书名" }),
      ]);
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("clearRecents empties state", () => {
    useRecentsStore.setState({ recents: [entry("dashboard")] });
    useRecentsStore.getState().clearRecents();
    expect(useRecentsStore.getState().recents).toEqual([]);
  });
});
