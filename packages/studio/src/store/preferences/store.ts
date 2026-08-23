import { create } from "zustand";
import type { PreferencesStore } from "./types";
import type { NavSectionId } from "@/lib/nav-sections";

// Same storage convention as the theme preference (`inkos:studio:theme`).
export const TOOL_DETAILS_STORAGE_KEY = "inkos:studio:tool-details-default-open";
export const NAV_LAYOUT_V2_STORAGE_KEY = "inkos:studio:nav-layout-v2";
export const ACTIVE_NAV_SECTION_STORAGE_KEY = "inkos:studio:active-nav-section";

interface PreferenceStorageLike {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
  removeItem(key: string): void;
}

function getPreferenceStorage(): PreferenceStorageLike | null {
  if (typeof window === "undefined") {
    return null;
  }

  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

/**
 * Default is `true` (keep today's behavior: result details start expanded).
 * Only an explicitly stored "false" turns the preference off.
 */
export function readStoredToolDetailsDefaultOpen(
  storage: Pick<PreferenceStorageLike, "getItem"> | null | undefined,
): boolean {
  return storage?.getItem(TOOL_DETAILS_STORAGE_KEY) !== "false";
}

/** P3-1 双轨开关：仅显式存 "true" 才启用活动栏布局。 */
export function readStoredNavLayoutV2(
  storage: Pick<PreferenceStorageLike, "getItem"> | null | undefined,
): boolean {
  return storage?.getItem(NAV_LAYOUT_V2_STORAGE_KEY) === "true";
}

const NAV_SECTION_IDS: ReadonlyArray<NavSectionId> = ["create", "tools", "manage", "film"];

/** 持久化的活动栏选中区；非法值/未存 → null（跟随路由）。 */
export function readStoredActiveNavSection(
  storage: Pick<PreferenceStorageLike, "getItem"> | null | undefined,
): NavSectionId | null {
  const stored = storage?.getItem(ACTIVE_NAV_SECTION_STORAGE_KEY);
  return NAV_SECTION_IDS.includes(stored as NavSectionId) ? (stored as NavSectionId) : null;
}

export const usePreferencesStore = create<PreferencesStore>()((set) => ({
  toolDetailsDefaultOpen: readStoredToolDetailsDefaultOpen(getPreferenceStorage()),

  setToolDetailsDefaultOpen: (open: boolean) => {
    try {
      getPreferenceStorage()?.setItem(TOOL_DETAILS_STORAGE_KEY, String(open));
    } catch {
      // Ignore storage failures (e.g. private mode) and keep the in-memory
      // preference for this session — same policy as the theme preference.
    }
    set({ toolDetailsDefaultOpen: open });
  },

  navLayoutV2: readStoredNavLayoutV2(getPreferenceStorage()),

  setNavLayoutV2: (enabled: boolean) => {
    try {
      getPreferenceStorage()?.setItem(NAV_LAYOUT_V2_STORAGE_KEY, String(enabled));
    } catch {
      // 同上：存储失败保持会话内生效
    }
    set({ navLayoutV2: enabled });
  },

  activeNavSection: readStoredActiveNavSection(getPreferenceStorage()),

  setActiveNavSection: (section: NavSectionId | null) => {
    try {
      if (section === null) {
        getPreferenceStorage()?.removeItem(ACTIVE_NAV_SECTION_STORAGE_KEY);
      } else {
        getPreferenceStorage()?.setItem(ACTIVE_NAV_SECTION_STORAGE_KEY, section);
      }
    } catch {
      // 同上
    }
    set({ activeNavSection: section });
  },
}));
