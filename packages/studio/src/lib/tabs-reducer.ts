import type { HashRoute } from "@/hooks/use-hash-route";

/**
 * 标签页多任务（P3-3）reducer 纯函数——P3 最重的测试点。
 * 标签真相在 store（不动 URL），hash 仅同步 activeTab 保深链兼容。
 * 关键语义（VSCode 式预览标签）：
 * - open(preview=true)：复用/替换唯一的预览标签（当前激活是预览 → 原位替换）
 * - open(preview=false) 粘滞打开：同路由已存在（含预览）→ 激活并转正
 * - pin：预览转正（preview=false, pinned=true）
 * - close：激活位移到右邻，无右邻取左邻，关空 → activeId=null
 */

export interface TabState {
  readonly id: string;
  readonly route: HashRoute;
  readonly title: string;
  readonly pinned: boolean;
  readonly preview: boolean;
}

export interface TabsSnapshot {
  readonly tabs: ReadonlyArray<TabState>;
  readonly activeId: string | null;
}

/** 路由 → 标签身份键：同书不同章是不同标签；import 的不同 tab 也算不同标签。 */
export function routeKey(route: HashRoute): string {
  switch (route.page) {
    case "book":
      return `book:${route.bookId}`;
    case "book-settings":
      return `book-settings:${route.bookId}`;
    case "chapter":
      return `chapter:${route.bookId}:${route.chapterNumber}`;
    case "analytics":
      return `analytics:${route.bookId}`;
    case "truth":
      return `truth:${route.bookId}`;
    case "service-detail":
      return `service:${route.serviceId}`;
    case "play":
      return `play:${route.projectId}`;
    case "film":
      return `film:${route.projectId}`;
    case "flow":
      return `flow:${route.projectId}`;
    case "film-author":
      return `film-author:${route.projectId}`;
    case "film-studio":
      return `film-studio:${route.projectId}`;
    case "import":
      return route.tab ? `import:${route.tab}` : "import";
    default:
      return route.page;
  }
}

function nextTabId(route: HashRoute, seed: number): string {
  return `${routeKey(route)}#${seed}`;
}

export type TabsAction =
  | { type: "open"; route: HashRoute; title: string; preview: boolean }
  | { type: "activate"; id: string }
  | { type: "close"; id: string }
  | { type: "closeOthers"; id: string }
  | { type: "pin"; id: string; pinned: boolean }
  | { type: "retitle"; id: string; title: string }
  | { type: "hydrate"; snapshot: TabsSnapshot };

export const EMPTY_TABS: TabsSnapshot = { tabs: [], activeId: null };

/** 当前快照里（若有）匹配该路由的标签。 */
export function findTabByRoute(state: TabsSnapshot, route: HashRoute): TabState | undefined {
  const key = routeKey(route);
  return state.tabs.find((tab) => tab.id.split("#")[0] === key);
}

export function tabsReducer(state: TabsSnapshot, action: TabsAction): TabsSnapshot {
  switch (action.type) {
    case "hydrate":
      return action.snapshot;

    case "activate":
      return state.tabs.some((tab) => tab.id === action.id)
        ? { ...state, activeId: action.id }
        : state;

    case "open": {
      const existing = findTabByRoute(state, action.route);
      if (existing) {
        // 已存在：激活；粘滞打开同时转正（预览→常驻）
        const tabs = action.preview
          ? state.tabs
          : state.tabs.map((tab) => (tab.id === existing.id ? { ...tab, preview: false } : tab));
        return { tabs, activeId: existing.id };
      }
      if (action.preview) {
        // 预览打开：当前激活是预览标签 → 原位替换；否则唯一的预览标签被替换；都没有 → 追加
        const active = state.tabs.find((tab) => tab.id === state.activeId);
        let target: TabState | undefined = active?.preview ? active : state.tabs.find((tab) => tab.preview);
        let tabs: Array<TabState>;
        if (target) {
          tabs = state.tabs.map((tab) =>
            tab.id === target!.id
              ? { id: nextTabId(action.route, Date.now()), route: action.route, title: action.title, pinned: false, preview: true }
              : tab,
          );
          target = tabs.find((tab) => tab.route === action.route);
          return { tabs, activeId: target?.id ?? state.activeId };
        }
        target = { id: nextTabId(action.route, Date.now()), route: action.route, title: action.title, pinned: false, preview: true };
        return { tabs: [...state.tabs, target], activeId: target.id };
      }
      const tab: TabState = { id: nextTabId(action.route, Date.now()), route: action.route, title: action.title, pinned: false, preview: false };
      return { tabs: [...state.tabs, tab], activeId: tab.id };
    }

    case "close": {
      const index = state.tabs.findIndex((tab) => tab.id === action.id);
      if (index === -1) return state;
      const tabs = state.tabs.filter((tab) => tab.id !== action.id);
      let activeId = state.activeId;
      if (state.activeId === action.id) {
        activeId = tabs[index]?.id ?? tabs[index - 1]?.id ?? null;
      }
      return { tabs, activeId };
    }

    case "closeOthers": {
      const keep = state.tabs.filter((tab) => tab.id === action.id || tab.pinned);
      if (keep.length === state.tabs.length) return state;
      return { tabs: keep, activeId: action.id };
    }

    case "pin": {
      const tabs = state.tabs.map((tab) =>
        tab.id === action.id ? { ...tab, pinned: action.pinned, preview: false } : tab,
      );
      return { tabs, activeId: action.id };
    }

    // 书名等异步数据到达后回填标题（深链开标签时标题先回退「书籍」）
    case "retitle": {
      if (!state.tabs.some((tab) => tab.id === action.id && tab.title !== action.title)) return state;
      return {
        ...state,
        tabs: state.tabs.map((tab) => (tab.id === action.id ? { ...tab, title: action.title } : tab)),
      };
    }

    default:
      return state;
  }
}

/** 持久化恢复的合法性清洗：坏条目丢弃；activeId 悬空则回退末位标签。 */
export function sanitizeTabsSnapshot(input: unknown): TabsSnapshot {
  if (typeof input !== "object" || input === null) return EMPTY_TABS;
  const raw = input as { tabs?: unknown; activeId?: unknown };
  if (!Array.isArray(raw.tabs)) return EMPTY_TABS;
  const tabs: TabState[] = [];
  const seen = new Set<string>();
  for (const entry of raw.tabs) {
    if (typeof entry !== "object" || entry === null) continue;
    const candidate = entry as Partial<TabState>;
    if (!candidate.id || !candidate.route || typeof candidate.title !== "string") continue;
    if (typeof candidate.route.page !== "string") continue;
    const key = candidate.id.split("#")[0];
    if (seen.has(key)) continue;
    seen.add(key);
    tabs.push({
      id: candidate.id,
      route: candidate.route as HashRoute,
      title: candidate.title,
      pinned: candidate.pinned === true,
      preview: candidate.preview === true,
    });
  }
  const activeId =
    typeof raw.activeId === "string" && tabs.some((tab) => tab.id === raw.activeId)
      ? raw.activeId
      : (tabs.at(-1)?.id ?? null);
  return { tabs, activeId };
}
