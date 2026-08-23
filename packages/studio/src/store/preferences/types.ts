import type { NavSectionId } from "@/lib/nav-sections";

export interface PreferencesStore {
  /**
   * Whether pipeline tool result blocks ("查看操作结果") in chat render
   * expanded by default. Persisted per browser via localStorage.
   */
  toolDetailsDefaultOpen: boolean;

  setToolDetailsDefaultOpen: (open: boolean) => void;

  /**
   * P3-1 双轨开关：活动栏四区布局（ActivityBar + 按区渲染的侧栏）。
   * 默认 false（旧整栏 Sidebar）；P4 走查后删旧轨转正。
   */
  navLayoutV2: boolean;

  setNavLayoutV2: (enabled: boolean) => void;

  /** 活动栏当前选中区（用户手动切换后持久化；null=跟随路由）。 */
  activeNavSection: NavSectionId | null;

  setActiveNavSection: (section: NavSectionId | null) => void;
}
