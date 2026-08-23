import { create } from "zustand";
import type { RecentEntry, RecentsStore } from "./types";

// Same storage convention as the theme preference (`inkos:studio:theme`).
export const RECENTS_STORAGE_KEY = "inkos:studio:recents";

/** 保留的最近访问条数上限：命令面板空查询首屏只展示 5 条，8 条上限兼顾滚动余量。 */
export const RECENTS_LIMIT = 8;

interface RecentsStorageLike {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

function getRecentsStorage(): RecentsStorageLike | null {
  if (typeof window === "undefined") {
    return null;
  }

  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

function isRecentEntry(value: unknown): value is RecentEntry {
  if (typeof value !== "object" || value === null) return false;
  const entry = value as Record<string, unknown>;
  return typeof entry.page === "string" && entry.page.length > 0
    && typeof entry.label === "string";
}

/** 初始化/恢复用：坏数据静默降级为空列表，绝不阻塞启动。 */
export function readStoredRecents(
  storage: Pick<RecentsStorageLike, "getItem"> | null | undefined,
): ReadonlyArray<RecentEntry> {
  try {
    const raw = storage?.getItem(RECENTS_STORAGE_KEY);
    if (!raw) return [];
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed.filter(isRecentEntry).slice(0, RECENTS_LIMIT);
  } catch {
    return [];
  }
}

/** 同一条目的地判键：页面 + 全部定位参数（同一本书的不同章是不同条目）。 */
export function recentIdentity(entry: RecentEntry): string {
  return [
    entry.page,
    entry.bookId ?? "",
    entry.chapterNumber ?? "",
    entry.serviceId ?? "",
    entry.projectId ?? "",
    entry.tab ?? "",
  ].join("|");
}

/** pushRecent 的可测内核：去重置顶 + 截断上限，返回新数组。 */
export function applyRecentPush(
  list: ReadonlyArray<RecentEntry>,
  entry: RecentEntry,
  limit: number = RECENTS_LIMIT,
): ReadonlyArray<RecentEntry> {
  const identity = recentIdentity(entry);
  const rest = list.filter((item) => recentIdentity(item) !== identity);
  return [entry, ...rest].slice(0, limit);
}

function persistRecents(list: ReadonlyArray<RecentEntry>): void {
  try {
    getRecentsStorage()?.setItem(RECENTS_STORAGE_KEY, JSON.stringify(list));
  } catch {
    // Ignore storage failures (e.g. private mode) and keep the in-memory
    // list for this session — same policy as the theme preference.
  }
}

export const useRecentsStore = create<RecentsStore>()((set, get) => ({
  recents: readStoredRecents(getRecentsStorage()),

  pushRecent: (entry) => {
    const next = applyRecentPush(get().recents, entry);
    persistRecents(next);
    set({ recents: next });
  },

  clearRecents: () => {
    persistRecents([]);
    set({ recents: [] });
  },
}));
