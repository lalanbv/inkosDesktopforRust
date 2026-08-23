import type { HashRoute } from "@/hooks/use-hash-route";

/**
 * 活动栏四区映射（P3-1）：23 种路由变体的 page 级 → 四区纯映射表。
 * 唯一事实源——ActivityBar 图标、SidePanel 按区过滤、路由归属断言都查这里。
 * 标题直接内联 zh/en（与 lib/commands.ts 同款约定，不经 i18n key）。
 */

export type NavSectionId = "create" | "tools" | "manage" | "film";

export interface NavSectionDef {
  id: NavSectionId;
  /** lucide 图标名（渲染层 ActivityBar 映射）。 */
  icon: string;
  titleZh: string;
  titleEn: string;
  /** 活动栏自上而下的排序。 */
  order: number;
  /** 归属本区的路由 page 集合（24 个 page 值全覆盖、无重叠）。 */
  pages: ReadonlyArray<HashRoute["page"]>;
}

export const NAV_SECTIONS: ReadonlyArray<NavSectionDef> = [
  {
    id: "create",
    icon: "feather",
    titleZh: "创作",
    titleEn: "Create",
    order: 1,
    pages: ["dashboard", "chat", "book", "book-settings", "book-create", "chapter", "analytics", "truth"],
  },
  {
    id: "tools",
    icon: "wand",
    titleZh: "工具",
    titleEn: "Tools",
    order: 2,
    pages: ["genres", "style", "translation", "import", "radar", "doctor"],
  },
  {
    id: "manage",
    icon: "settings-2",
    titleZh: "管理",
    titleEn: "Manage",
    order: 3,
    pages: ["services", "service-detail", "project-settings", "daemon", "logs"],
  },
  {
    id: "film",
    icon: "film",
    titleZh: "互动影视",
    titleEn: "Film",
    order: 4,
    pages: ["play", "film", "flow", "film-author", "film-studio"],
  },
];

/** 供测试枚举的 page 全集（与 HashRoute 联合的 page 字面量一一对应）。 */
export const ALL_ROUTE_PAGES: ReadonlyArray<HashRoute["page"]> = [
  "dashboard", "chat", "book", "book-settings", "book-create", "services", "project-settings",
  "service-detail", "chapter", "analytics", "truth", "daemon", "logs", "genres", "style",
  "translation", "import", "radar", "doctor", "play", "film", "flow", "film-author", "film-studio",
];

/** page → 区（纯函数）。未知 page 兜底到创作区，保证活动栏选中态永不落空。 */
export function sectionForPage(page: HashRoute["page"]): NavSectionId {
  return NAV_SECTIONS.find((section) => section.pages.includes(page))?.id ?? "create";
}

/** 路由 → 区（含参数路由，按 page 归属）。 */
export function sectionForRoute(route: HashRoute): NavSectionId {
  return sectionForRoutePage(route.page);
}

/** 接收裸 page 字符串（宽松入口，供路由对象形态不一的调用方）。 */
export function sectionForRoutePage(page: string): NavSectionId {
  return sectionForPage(page as HashRoute["page"]);
}

export function sectionTitle(section: NavSectionDef, lang: "zh" | "en"): string {
  return lang === "en" ? section.titleEn : section.titleZh;
}
