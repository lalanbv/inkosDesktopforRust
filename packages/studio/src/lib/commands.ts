import type { HashRoute } from "@/hooks/use-hash-route";
import type { ThemeMode } from "@/hooks/use-theme";
import { recentIdentity } from "@/store/recents";
import type { RecentEntry } from "@/store/recents";

export type CommandGroup = "recent" | "navigation" | "action";
// 219 号：ja 面板文案缺失时由调用侧回退（PaletteLang 与 UI Lang 解耦，
// 本表 zh/en 二态保持；ja 渐进补齐后并入）。
export type PaletteLang = "zh" | "en";

/** 可启动的创作会话类型（= ChatSessionKind；内联声明避免 lib 层依赖 core 运行时）。 */
export type LaunchKind =
  | "chat"
  | "book-create"
  | "book"
  | "short"
  | "play"
  | "script"
  | "storyboard"
  | "interactive-film"
  | "edit"
  | "interactive-film-authoring";

/**
 * 命令执行上下文：App 每次渲染重建并传入，命令闭包不持有过期状态
 * （151 号 P1-5 风险缓解：run 引用过期闭包）。
 */
export interface CommandContext {
  setRoute: (route: HashRoute) => void;
  /** P2-3 三态主题：light / dark / auto（跟随系统）。 */
  setThemeMode: (mode: ThemeMode) => void;
  /** P4-2 密度档：舒适/紧凑。 */
  setDensity: (density: "comfortable" | "compact") => void;
  setProjectLanguage: (lang: "zh" | "en") => void;
  refetchProject: () => void;
  /** 新建长篇小说（进入书籍创建对话流）。 */
  openBookCreate: () => void;
  /** 新建一个项目级草稿会话（同人 / 续写 / 番外 / 仿写 / 翻译）。 */
  createProjectChatDraft: () => void;
  launchProjectMode: (kind: LaunchKind, playMode?: "guided" | "open") => void;
}

export interface CommandEntry {
  /** 稳定 id：`nav.dashboard` / `action.theme.toggle` / `recent:<identity>`。 */
  id: string;
  group: CommandGroup;
  titleZh: string;
  titleEn: string;
  /** 过滤用别名（中英双语混合，小写英文便于 includes）。 */
  keywords?: ReadonlyArray<string>;
  /** 展示用快捷键徽标（P2 快捷键注册表复用；仅登记真实存在的组合）。 */
  hotkey?: string;
  /** lucide 图标名（渲染层 CommandPalette 映射，缺失则不显示图标）。 */
  icon?: string;
  run: (ctx: CommandContext) => void;
}

/** 导航层：全部无参数页面直达（23 种路由变体的页面级去重）。 */
export function buildNavigationCommands(): CommandEntry[] {
  const nav = (
    id: string,
    titleZh: string,
    titleEn: string,
    icon: string,
    route: HashRoute,
    keywords: ReadonlyArray<string> = [],
  ): CommandEntry => ({
    id,
    group: "navigation",
    titleZh,
    titleEn,
    icon,
    keywords,
    run: (ctx) => ctx.setRoute(route),
  });

  return [
    nav("nav.dashboard", "首页", "Home", "house", { page: "dashboard" }, ["home", "主页"]),
    nav("nav.chat", "对话", "Chat", "message-square", { page: "chat" }, ["chat", "会话", "session"]),
    nav("nav.book-create", "新建书籍", "New Book", "book-plus", { page: "book-create" }, ["create", "new", "创建", "new book"]),
    nav("nav.services", "模型配置", "Model Config", "settings", { page: "services" }, ["config", "services", "服务", "provider"]),
    nav("nav.project-settings", "项目设置", "Project Settings", "settings", { page: "project-settings" }, ["settings", "配置"]),
    nav("nav.daemon", "守护进程", "Daemon", "zap", { page: "daemon" }, ["daemon", "后台", "background"]),
    nav("nav.logs", "日志", "Logs", "terminal", { page: "logs" }, ["log", "日志"]),
    nav("nav.genres", "题材管理", "Genre Manager", "boxes", { page: "genres" }, ["genre", "题材"]),
    nav("nav.style", "文风分析", "Style Analyzer", "wand", { page: "style" }, ["style", "文风"]),
    nav("nav.translation", "翻译译介", "Translation", "languages", { page: "translation" }, ["translate", "翻译"]),
    nav("nav.import", "导入工具", "Import Tools", "file-input", { page: "import" }, ["import", "导入"]),
    nav("nav.import.chapters", "导入：导入章节", "Import: Chapters", "file-input", { page: "import", tab: "chapters" }, ["章节"]),
    nav("nav.import.canon", "导入：导入母本", "Import: Canon", "file-input", { page: "import", tab: "canon" }, ["母本"]),
    nav("nav.import.fanfic", "导入：同人创作", "Import: Fanfic", "feather", { page: "import", tab: "fanfic" }, ["同人"]),
    nav("nav.import.spinoff", "导入：番外创作", "Import: Side-story", "book-copy", { page: "import", tab: "spinoff" }, ["番外"]),
    nav("nav.import.imitation", "导入：仿写创作", "Import: Imitation", "wand", { page: "import", tab: "imitation" }, ["仿写"]),
    nav("nav.import.backfill", "导入：系列书回填", "Import: Series Backfill", "book-copy", { page: "import", tab: "backfill" }, ["系列", "回填", "backfill"]),
    nav("nav.radar", "市场雷达", "Market Radar", "trending-up", { page: "radar" }, ["radar", "市场", "market"]),
    nav("nav.doctor", "环境诊断", "Doctor", "stethoscope", { page: "doctor" }, ["doctor", "诊断", "health"]),
  ];
}

/** 动作层：创建类 + 主题/语言 + 数据类。 */
export function buildActionCommands(): CommandEntry[] {
  const action = (
    id: string,
    titleZh: string,
    titleEn: string,
    icon: string,
    run: (ctx: CommandContext) => void,
    keywords: ReadonlyArray<string> = [],
  ): CommandEntry => ({ id, group: "action", titleZh, titleEn, icon, keywords, run });

  return [
    action("action.bookCreate", "新建长篇小说", "New Novel", "plus",
      (ctx) => ctx.openBookCreate(), ["novel", "长篇", "创建"]),
    action("action.short", "新建短篇小说", "New Short Story", "scroll",
      (ctx) => ctx.launchProjectMode("short"), ["short", "短篇"]),
    action("action.script", "新建剧本", "New Script", "clapperboard",
      (ctx) => ctx.launchProjectMode("script"), ["script", "剧本"]),
    action("action.storyboard", "新建分镜", "New Storyboard", "rows",
      (ctx) => ctx.launchProjectMode("storyboard"), ["storyboard", "分镜"]),
    action("action.interactiveFilm", "新建互动影游", "New Interactive Film", "film",
      (ctx) => ctx.launchProjectMode("interactive-film"), ["film", "影游", "interactive"]),
    action("action.playGuided", "新建分支互动", "New Branching Play", "git-branch",
      (ctx) => ctx.launchProjectMode("play", "guided"), ["play", "分支", "branching"]),
    action("action.playOpen", "新建开放世界", "New Open World", "gamepad",
      (ctx) => ctx.launchProjectMode("play", "open"), ["play", "开放世界", "open world"]),
    action("action.fanfic", "同人创作会话", "Fanfic Session", "feather",
      (ctx) => ctx.createProjectChatDraft(), ["fanfic", "同人"]),
    action("action.continuation", "续写创作会话", "Continuation Session", "file-input",
      (ctx) => ctx.createProjectChatDraft(), ["continuation", "续写"]),
    action("action.spinoff", "番外创作会话", "Side-story Session", "book-copy",
      (ctx) => ctx.createProjectChatDraft(), ["spinoff", "番外"]),
    action("action.imitation", "仿写创作会话", "Imitation Session", "wand",
      (ctx) => ctx.createProjectChatDraft(), ["imitation", "仿写"]),
    action("action.translationSession", "翻译译介会话", "Translation Session", "languages",
      (ctx) => ctx.createProjectChatDraft(), ["translation", "翻译"]),
    action("action.themeLight", "切换到浅色主题", "Switch to Light Theme", "sun",
      (ctx) => ctx.setThemeMode("light"), ["theme", "light", "浅色"]),
    action("action.themeDark", "切换到深色主题", "Switch to Dark Theme", "moon",
      (ctx) => ctx.setThemeMode("dark"), ["theme", "dark", "深色"]),
    action("action.themeAuto", "主题跟随系统", "Follow System Theme", "monitor",
      (ctx) => ctx.setThemeMode("auto"), ["theme", "auto", "跟随系统", "system"]),
    action("action.densityComfortable", "界面密度：舒适", "Density: Comfortable", "rows",
      (ctx) => ctx.setDensity("comfortable"), ["density", "密度", "舒适", "comfortable"]),
    action("action.densityCompact", "界面密度：紧凑", "Density: Compact", "rows",
      (ctx) => ctx.setDensity("compact"), ["density", "密度", "紧凑", "compact"]),
    action("action.langZh", "界面语言：中文", "UI Language: Chinese", "globe",
      (ctx) => ctx.setProjectLanguage("zh"), ["language", "语言", "chinese"]),
    action("action.langEn", "界面语言：English", "UI Language: English", "globe",
      (ctx) => ctx.setProjectLanguage("en"), ["language", "语言", "english"]),
    action("action.projectRefresh", "刷新项目数据", "Refresh Project Data", "refresh",
      (ctx) => ctx.refetchProject(), ["refresh", "刷新", "reload"]),
    action("action.sessionNew", "新建项目会话", "New Project Session", "plus",
      (ctx) => ctx.createProjectChatDraft(), ["session", "会话", "new"]),
  ];
}

/** 最近访问记录 → 可执行命令（路由参数不全的坏记录静默丢弃）。 */
export function recentToRoute(entry: RecentEntry): HashRoute | null {
  switch (entry.page) {
    case "dashboard":
      return { page: "dashboard" };
    case "chat":
      return { page: "chat" };
    case "book-create":
      return { page: "book-create" };
    case "onboarding":
      return { page: "onboarding" };
    case "book":
      return entry.bookId ? { page: "book", bookId: entry.bookId } : null;
    case "book-settings":
      return entry.bookId ? { page: "book-settings", bookId: entry.bookId } : null;
    case "book-timeline":
      return entry.bookId ? { page: "book-timeline", bookId: entry.bookId } : null;
    case "chapter":
      return entry.bookId && entry.chapterNumber !== undefined
        ? { page: "chapter", bookId: entry.bookId, chapterNumber: entry.chapterNumber }
        : null;
    case "analytics":
      return entry.bookId ? { page: "analytics", bookId: entry.bookId } : null;
    case "truth":
      return entry.bookId ? { page: "truth", bookId: entry.bookId } : null;
    case "services":
      return { page: "services" };
    case "project-settings":
      return { page: "project-settings" };
    case "service-detail":
      return entry.serviceId ? { page: "service-detail", serviceId: entry.serviceId } : null;
    case "daemon":
      return { page: "daemon" };
    case "logs":
      return { page: "logs" };
    case "genres":
      return { page: "genres" };
    case "style":
      return { page: "style" };
    case "translation":
      return { page: "translation" };
    case "import":
      return { page: "import", ...(entry.tab ? { tab: entry.tab as "chapters" | "canon" | "fanfic" | "spinoff" | "imitation" } : {}) };
    case "radar":
      return { page: "radar" };
    case "doctor":
      return { page: "doctor" };
    case "play":
      return entry.projectId ? { page: "play", projectId: entry.projectId } : null;
    case "film":
      return entry.projectId ? { page: "film", projectId: entry.projectId } : null;
    case "flow":
      return entry.projectId ? { page: "flow", projectId: entry.projectId } : null;
    case "film-author":
      return entry.projectId ? { page: "film-author", projectId: entry.projectId } : null;
    case "film-studio":
      return entry.projectId ? { page: "film-studio", projectId: entry.projectId } : null;
  }
}

export function buildRecentCommands(
  recents: ReadonlyArray<RecentEntry>,
): CommandEntry[] {
  return recents.flatMap((entry) => {
    const route = recentToRoute(entry);
    if (!route) return [];
    return [{
      id: `recent:${recentIdentity(entry)}`,
      group: "recent" as const,
      titleZh: entry.label,
      titleEn: entry.label,
      icon: "history",
      run: (ctx: CommandContext) => ctx.setRoute(route),
    }];
  });
}

/** 空查询首屏的推荐组：新建小说 / 继续上次（无记录则对话页）/ 打开设置。 */
export function buildRecommendedCommands(
  all: ReadonlyArray<CommandEntry>,
  recentCommands: ReadonlyArray<CommandEntry>,
): CommandEntry[] {
  const byId = (id: string) => all.find((entry) => entry.id === id);
  return [
    byId("action.bookCreate"),
    recentCommands[0] ?? byId("nav.chat"),
    byId("nav.project-settings"),
  ].filter((entry): entry is CommandEntry => Boolean(entry));
}

/**
 * 过滤：title/keywords 不区分大小写 includes，中英文标题都参与匹配
 * （中文用户输英文品牌词、英文用户输拼音别名都能命中）。空查询原样返回。
 */
export function filterCommands(
  list: ReadonlyArray<CommandEntry>,
  query: string,
): CommandEntry[] {
  const q = query.trim().toLowerCase();
  if (!q) return [...list];
  return list.filter((entry) => {
    const haystack = [entry.titleZh, entry.titleEn, ...(entry.keywords ?? [])]
      .join("\n")
      .toLowerCase();
    return haystack.includes(q);
  });
}

const GROUP_ORDER: Record<CommandGroup, number> = { recent: 0, navigation: 1, action: 2 };

/** 分组排序：recent < navigation < action，组内保持注册顺序，空组剔除。 */
export function groupCommands(
  list: ReadonlyArray<CommandEntry>,
): Array<{ group: CommandGroup; entries: CommandEntry[] }> {
  const buckets = new Map<CommandGroup, CommandEntry[]>();
  for (const entry of list) {
    const bucket = buckets.get(entry.group);
    if (bucket) {
      bucket.push(entry);
    } else {
      buckets.set(entry.group, [entry]);
    }
  }
  return [...buckets.entries()]
    .sort(([a], [b]) => GROUP_ORDER[a] - GROUP_ORDER[b])
    .map(([group, entries]) => ({ group, entries }));
}

/** 展示文案按界面语言取值。 */
export function commandTitle(entry: CommandEntry, lang: PaletteLang): string {
  return lang === "en" ? entry.titleEn : entry.titleZh;
}
