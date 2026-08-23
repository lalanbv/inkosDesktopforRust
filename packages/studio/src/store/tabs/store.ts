import { create } from "zustand";
import type { TabsStore } from "./types";
import { EMPTY_TABS, sanitizeTabsSnapshot, tabsReducer } from "@/lib/tabs-reducer";
import type { TabsSnapshot } from "@/lib/tabs-reducer";

export const TABS_STORAGE_KEY = "inkos:studio:tabs";

interface TabsStorageLike {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

function getTabsStorage(): TabsStorageLike | null {
  if (typeof window === "undefined") {
    return null;
  }
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

/** 持久化读取（纯函数，可测）：坏 JSON/坏条目静默降级空集。 */
export function readStoredTabs(storage: Pick<TabsStorageLike, "getItem"> | null | undefined): TabsSnapshot {
  let raw: string | null = null;
  try {
    raw = storage?.getItem(TABS_STORAGE_KEY) ?? null;
  } catch {
    return EMPTY_TABS;
  }
  if (!raw) return EMPTY_TABS;
  try {
    return sanitizeTabsSnapshot(JSON.parse(raw));
  } catch {
    return EMPTY_TABS;
  }
}

function persist(snapshot: TabsSnapshot): void {
  try {
    getTabsStorage()?.setItem(TABS_STORAGE_KEY, JSON.stringify(snapshot));
  } catch {
    // 私密模式等：会话内生效即可
  }
}

export const useTabsStore = create<TabsStore>()((set, get) => ({
  ...readStoredTabs(getTabsStorage()),

  dispatch: (action) => {
    const next = tabsReducer(get(), action);
    if (next === get()) return;
    persist({ tabs: next.tabs, activeId: next.activeId });
    set(next);
  },
}));
