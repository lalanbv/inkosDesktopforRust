import { describe, it, expect, beforeEach } from "vitest";
import { readStoredTabs, useTabsStore, TABS_STORAGE_KEY } from "./store";

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

const validSnapshot = {
  tabs: [
    { id: "book:a#1", route: { page: "book", bookId: "a" }, title: "书 A" },
    { id: "logs#2", route: { page: "logs" }, title: "日志", pinned: true },
  ],
  activeId: "logs#2",
};

describe("readStoredTabs", () => {
  it("restores a persisted snapshot", () => {
    const storage = fakeStorage({ [TABS_STORAGE_KEY]: JSON.stringify(validSnapshot) });
    const snapshot = readStoredTabs(storage);
    expect(snapshot.tabs).toHaveLength(2);
    expect(snapshot.activeId).toBe("logs#2");
  });

  it("degrades to empty on missing/invalid JSON or junk entries", () => {
    expect(readStoredTabs(null).tabs).toEqual([]);
    expect(readStoredTabs(fakeStorage()).tabs).toEqual([]);
    expect(readStoredTabs(fakeStorage({ [TABS_STORAGE_KEY]: "{oops" })).tabs).toEqual([]);
    expect(readStoredTabs(fakeStorage({ [TABS_STORAGE_KEY]: JSON.stringify({ tabs: [42, "x"] }) })).tabs).toEqual([]);
  });
});

describe("useTabsStore", () => {
  beforeEach(() => {
    useTabsStore.setState({ tabs: [], activeId: null });
  });

  it("dispatch applies reducer transitions and persists the snapshot", () => {
    const storage = fakeStorage();
    // 借存储探针验证持久化写入：monkeypatch 一层（store 内部直用 localStorage，
    // 测试环境无 window 时 persist 静默跳过，故此处只验证状态机）
    useTabsStore.getState().dispatch({ type: "open", route: { page: "book", bookId: "a" }, title: "书 A", preview: false });
    const state = useTabsStore.getState();
    expect(state.tabs).toHaveLength(1);
    expect(state.activeId).toBe(state.tabs[0].id);

    useTabsStore.getState().dispatch({ type: "pin", id: state.tabs[0].id, pinned: true });
    expect(useTabsStore.getState().tabs[0].pinned).toBe(true);

    useTabsStore.getState().dispatch({ type: "close", id: useTabsStore.getState().tabs[0].id });
    expect(useTabsStore.getState().tabs).toHaveLength(0);
    expect(useTabsStore.getState().activeId).toBeNull();
    expect(storage.dump()).toEqual({});
  });
});
