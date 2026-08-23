import { create } from "zustand";
import type { PreferencesStore } from "./types";
import type { NavSectionId } from "@/lib/nav-sections";

// Same storage convention as the theme preference (`inkos:studio:theme`).
export const TOOL_DETAILS_STORAGE_KEY = "inkos:studio:tool-details-default-open";
export const NAV_LAYOUT_V2_STORAGE_KEY = "inkos:studio:nav-layout-v2";
export const ACTIVE_NAV_SECTION_STORAGE_KEY = "inkos:studio:active-nav-section";
export const FOCUS_MODE_STORAGE_KEY = "inkos:studio:focus-mode";
export const DENSITY_STORAGE_KEY = "inkos:studio:density";

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

/** P4-1 专注模式：仅显式存 "true" 才恢复（避免意外会话残留）。 */
export function readStoredFocusMode(
  storage: Pick<PreferenceStorageLike, "getItem"> | null | undefined,
): boolean {
  return storage?.getItem(FOCUS_MODE_STORAGE_KEY) === "true";
}

/** P4-2 密度档：仅显式 "compact" 为紧凑，其余(含缺失)为舒适默认。 */
export function readStoredDensity(
  storage: Pick<PreferenceStorageLike, "getItem"> | null | undefined,
): "comfortable" | "compact" {
  return storage?.getItem(DENSITY_STORAGE_KEY) === "compact" ? "compact" : "comfortable";
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

  focusMode: readStoredFocusMode(getPreferenceStorage()),

  setFocusMode: (enabled: boolean) => {
    try {
      getPreferenceStorage()?.setItem(FOCUS_MODE_STORAGE_KEY, String(enabled));
    } catch {
      // 同上
    }
    set({ focusMode: enabled });
  },

  density: readStoredDensity(getPreferenceStorage()),

  setDensity: (density: "comfortable" | "compact") => {
    try {
      getPreferenceStorage()?.setItem(DENSITY_STORAGE_KEY, density);
    } catch {
      // 同上
    }
    set({ density });
  },
}));
