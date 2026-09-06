import { useState, useEffect, useMemo, useRef, lazy, Suspense } from "react";
import { useHashRoute } from "./hooks/use-hash-route";
import type { HashRoute } from "./hooks/use-hash-route";
import { Sidebar } from "./components/Sidebar";
import { ActivityBar } from "./components/ActivityBar";
import { SidePanel } from "./components/SidePanel";
import { createNav } from "./lib/nav";
import { sectionForRoute } from "./lib/nav-sections";
import { usePreferencesStore } from "./store/preferences";
import { useTabsStore } from "./store/tabs";
import { TabStrip } from "./components/TabStrip";
import { ContextDock } from "./components/ContextDock";
import { BottomPanel } from "./components/BottomPanel";
import { StatusBar } from "./components/StatusBar";
import { NotificationCenter } from "./components/NotificationCenter";
import { useNotificationsStore } from "./store/notifications";
import { usePerPageVisibility } from "./hooks/use-per-page-visibility";
import { Dashboard } from "./pages/Dashboard";
import { ChatPage } from "./pages/ChatPage";
import { BookDetail } from "./pages/BookDetail";
import { BookTimeline } from "./pages/BookTimeline";
import { ChapterReader } from "./pages/ChapterReader";
import { Analytics } from "./pages/Analytics";
import { ServiceListPage } from "./pages/ServiceListPage";
import { ServiceDetailPage } from "./pages/ServiceDetailPage";
import { ProjectSettings } from "./pages/ProjectSettings";
import { TruthFiles } from "./pages/TruthFiles";
import { DaemonControl } from "./pages/DaemonControl";
import { LogViewer } from "./pages/LogViewer";
import { GenreManager } from "./pages/GenreManager";
import { StyleManager } from "./pages/StyleManager";
import { TranslationManager } from "./pages/TranslationManager";
import { ImportManager } from "./pages/ImportManager";
import { RadarView } from "./pages/RadarView";
import { DoctorView } from "./pages/DoctorView";
import { StoryPlayer } from "./pages/StoryPlayer";
import { StoryGraphTree } from "./pages/StoryGraphTree";
const FlowView = lazy(() => import("./pages/FlowView"));
const FilmWizard = lazy(() => import("./pages/FilmWizard"));
import { LanguageSelector } from "./pages/LanguageSelector";
import { BookSidebar, BookSidebarToggle } from "./components/chat/BookSidebar";
import { useSSE } from "./hooks/use-sse";
import { useSessionEvents } from "./hooks/use-session-events";
import { useTheme, cycleThemeMode } from "./hooks/use-theme";
import { useI18n } from "./hooks/use-i18n";
import { setAppLanguage, tr } from "./lib/app-language";
import { postApi, putApi, useApi } from "./hooks/use-api";
import { Sun, Moon, Monitor, Search } from "lucide-react";
import { AppShellSkeleton } from "./components/AppShellSkeleton";
import { CommandPalette } from "./components/CommandPalette";
import { HotkeyCheatSheet } from "./components/HotkeyCheatSheet";
import { QuickOpenPalette } from "./components/QuickOpenPalette";
import type { QuickOpenSession } from "./lib/quick-open";
import { buildActionCommands, buildNavigationCommands } from "./lib/commands";
import type { CommandContext } from "./lib/commands";
import { useGlobalHotkeys } from "./hooks/use-global-hotkeys";
import type { HotkeyDef } from "./hooks/use-global-hotkeys";
import { deriveBreadcrumb } from "./lib/breadcrumb";
import { deriveHeaderInsetClass, isMacPlatformAgent, isTauriRuntime } from "./lib/titlebar";
import { useRecentsStore } from "./store/recents";
import { useChatStore } from "./store/chat";
import { setProjectChatSessionId } from "./pages/chat-page-state";

export type { HashRoute as Route } from "./hooks/use-hash-route";

const isMacPlatform =
  typeof navigator !== "undefined" && isMacPlatformAgent(navigator.platform);
// P2-1 标题栏融合：仅 macOS Tauri 壳内为交通灯预留顶栏左侧缩进
const isTauriDesktop = isTauriRuntime(typeof window !== "undefined" ? window : undefined);
const headerInsetClass = deriveHeaderInsetClass(isMacPlatform, isTauriDesktop);

/** P2-4 快捷键注册表（单一事实源）：组合 → 命令。速查页（P2-6）由此生成。 */
const HOTKEY_DEFS: ReadonlyArray<HotkeyDef> = [
  { combo: "mod+k", commandId: "app.palette.toggle" },
  { combo: "mod+p", commandId: "app.quickopen.toggle" },
  { combo: "mod+b", commandId: "app.sidepanel.toggle" },
  { combo: "mod+shift+d", commandId: "app.dock.toggle" },
  { combo: "mod+j", commandId: "app.bottom.toggle" },
  { combo: "mod+shift+f", commandId: "app.focus.toggle" },
  { combo: "mod+/", commandId: "app.cheatsheet.toggle" },
  ...Array.from({ length: 9 }, (_, i) => ({
    combo: `mod+${i + 1}`,
    commandId: `app.tab.${i + 1}`,
  })),
  // Esc 链（P4-1）：无弹层且专注模式开启时退出专注；其余 Esc 由各组件自消费
  { combo: "esc", commandId: "app.focus.exit" },
];

/** P2-7：macOS 菜单 id → 命令注册表条目/应用动作 id。 */
const MENU_COMMAND_MAP: Readonly<Record<string, string>> = {
  "menu:new-book": "action.bookCreate",
  "menu:palette": "app.palette.toggle",
  "menu:theme-light": "action.themeLight",
  "menu:theme-dark": "action.themeDark",
  "menu:theme-auto": "action.themeAuto",
  "menu:lang-zh": "action.langZh",
  "menu:lang-en": "action.langEn",
};

/** withGlobalTauri 注入的全局（仅取用到的最小面）。 */
interface TauriGlobalScope {
  __TAURI__?: {
    event?: {
      listen?: <T>(event: string, handler: (event: T) => void) => Promise<() => void>;
    };
  };
}

export function deriveActiveBookId(route: HashRoute): string | undefined {
  if ("bookId" in route) return route.bookId;
  return undefined;
}

export function isBookCreateChatRoute(route: HashRoute): boolean {
  return route.page === "book-create";
}

export function deriveStartupGate(input: {
  readonly ready: boolean;
  readonly projectError: string | null;
}): "ready" | "loading" | "error" {
  if (input.ready) return "ready";
  return input.projectError ? "error" : "loading";
}

export function App() {
  const { route, setRoute } = useHashRoute();
  const sse = useSSE();
  const { theme, mode: themeMode, setThemeMode } = useTheme();
  const { t, lang: currentLang } = useI18n();
  const { data: project, error: projectError, refetch: refetchProject } = useApi<{ language: string; languageExplicit: boolean }>("/project");
  const [showLanguageSelector, setShowLanguageSelector] = useState(false);
  const [ready, setReady] = useState(false);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const pushRecent = useRecentsStore((state) => state.pushRecent);
  const setInput = useChatStore((state) => state.setInput);
  const createDraftSession = useChatStore((state) => state.createDraftSession);
  // 快速打开（P2-5）的会话索引：有标题的会话才可跳转
  const chatSessionsMap = useChatStore((state) => state.sessions);
  const sessionIdsByBook = useChatStore((state) => state.sessionIdsByBook);
  // 书名同源数据：面包屑与「最近访问」标签共用（P3 活动栏重组时收敛为单一 store）。
  const { data: booksData } = useApi<{ books: ReadonlyArray<{ id: string; title: string }> }>("/books");
  // P3-5 状态栏的 daemon 运行点（与 Sidebar 同源）
  const { data: daemonData } = useApi<{ running: boolean }>("/daemon");

  // 快速打开的会话条目：sessionIdsByBook 的 "__null__" 键 = 项目级会话（跳 chat 页）。
  const quickOpenSessions = useMemo<QuickOpenSession[]>(() => {
    const entries: QuickOpenSession[] = [];
    for (const [key, ids] of Object.entries(sessionIdsByBook ?? {})) {
      const bookId = key === "__null__" ? null : key;
      for (const sessionId of ids ?? []) {
        const session = chatSessionsMap[sessionId];
        if (session?.title) {
          entries.push({ sessionId, title: session.title, bookId });
        }
      }
    }
    return entries;
  }, [chatSessionsMap, sessionIdsByBook]);

  const isDark = theme === "dark";

  // 全局语言同步：app-language 是模块级单例，供用不了 hook 的代码（lib 纯函数、
  // store slice）读取。这里在渲染期同步赋值，让子组件在同一次渲染里调用 tr() 时
  // 就读到正确语言（只用 effect 的话，effect 要等本次渲染提交后才执行，本次渲染
  // 里的 tr() 会读到旧语言）。赋值是幂等的模块变量写入，StrictMode 重复渲染无影
  // 响；下面的 effect 在语言加载完成和切换时再设置一次，保证提交后的值也正确。
  setAppLanguage(currentLang);
  useEffect(() => {
    setAppLanguage(currentLang);
  }, [currentLang]);

  useEffect(() => {
    document.documentElement.classList.toggle("dark", isDark);
  }, [isDark]);

  // 全局快捷键分发器（P2-4）：单一 keydown → 归一组合 → 注册表分发。
  // ⌘K 自 P1-5 的临时 keydown 迁入；⌘P 快速打开（P2-5）；⌘/ 速查（P2-6）。
  // macOS 菜单栏（P2-7）经 menu://command 事件汇入同一分发函数。
  const [cheatSheetOpen, setCheatSheetOpen] = useState(false);
  const [quickOpenOpen, setQuickOpenOpen] = useState(false);

  const dispatchCommand = (commandId: string) => {
    const target = commandId.startsWith("menu:")
      ? (MENU_COMMAND_MAP[commandId] ?? null)
      : commandId;
    if (!target) return;
    if (target === "app.palette.toggle") {
      setPaletteOpen((open) => !open);
      return;
    }
    if (target === "app.quickopen.toggle") {
      setPaletteOpen(false);
      setCheatSheetOpen(false);
      setQuickOpenOpen((open) => !open);
      return;
    }
    if (target === "app.cheatsheet.toggle") {
      setPaletteOpen(false);
      setCheatSheetOpen((open) => !open);
      return;
    }
    if (target === "app.sidepanel.toggle") {
      if (navLayoutV2) setSidePanelVisible((visible) => !visible);
      return;
    }
    // Cmd+Shift+D 右侧 dock / Cmd+J 底部面板（P3-4，按页记忆）
    if (target === "app.dock.toggle") {
      dockVisibility.toggle();
      return;
    }
    if (target === "app.bottom.toggle") {
      bottomVisibility.toggle();
      return;
    }
    // P4-1 专注模式：⌘⇧F 切换；Esc 退出——仅当无弹层打开时（弹层的 Esc
    // 由 Base UI 自身消费关层，不应连带退出专注）
    if (target === "app.focus.toggle") {
      setFocusMode(!usePreferencesStore.getState().focusMode);
      return;
    }
    if (target === "app.focus.exit") {
      if (paletteOpen || quickOpenOpen || cheatSheetOpen || showLanguageSelector) return;
      if (!usePreferencesStore.getState().focusMode) return;
      setFocusMode(false);
      return;
    }
    // Cmd+1..9 切标签（P3-3）
    const tabIndexMatch = /^app\.tab\.([1-9])$/.exec(target);
    if (tabIndexMatch) {
      const index = Number(tabIndexMatch[1]) - 1;
      const tab = useTabsStore.getState().tabs[index];
      if (tab) dispatchTabs({ type: "activate", id: tab.id });
      return;
    }
    const entry = [...buildNavigationCommands(), ...buildActionCommands()]
      .find((item) => item.id === target);
    entry?.run(commandCtx);
  };

  useGlobalHotkeys({ defs: HOTKEY_DEFS, runCommand: dispatchCommand });

  // Tauri 菜单事件（P2-7）：Rust on_menu_event → menu://command → 同一分发器。
  // dispatchCommand 经 ref 保持最新；仅在 Tauri 壳内注册（浏览器无 __TAURI__）。
  const dispatchRef = useRef(dispatchCommand);
  dispatchRef.current = dispatchCommand;
  useEffect(() => {
    if (!isTauriDesktop) return;
    const api = (window as TauriGlobalScope).__TAURI__;
    if (typeof api?.event?.listen !== "function") return;
    let unlisten: (() => void) | undefined;
    let disposed = false;
    void api.event.listen<{ payload: unknown }>("menu://command", (event) => {
      const id = typeof event.payload === "string" ? event.payload : "";
      if (id.startsWith("menu:")) dispatchRef.current(id);
    })
      .then((fn) => {
        if (disposed) fn();
        else unlisten = fn;
      })
      .catch(() => {
        // 菜单事件通道不可用时静默降级：快捷键与命令面板不受影响
      });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    if (project) {
      if (!project.languageExplicit) {
        setShowLanguageSelector(true);
      }
      setReady(true);
    }
  }, [project]);

  useSessionEvents(sse, route, setRoute);

  // P4-3 通知桥：关键 SSE 事件 → 通知中心（write:complete=info；错误类=error）。
  // 按 seq 游标去重，专注模式同样入列（铃铛徽标可见，弹层非模态不打断）。
  const lastNotifiedSeqRef = useRef(0);
  const pushNotification = useNotificationsStore((state) => state.pushNotification);
  useEffect(() => {
    const fresh = sse.messages.filter((message) => message.seq > lastNotifiedSeqRef.current);
    if (fresh.length === 0) return;
    lastNotifiedSeqRef.current = fresh[fresh.length - 1].seq;
    for (const message of fresh) {
      if (message.event === "write:complete") {
        const data = message.data as { bookId?: string; chapterNumber?: number } | null;
        pushNotification({
          level: "info",
          title: tr("章节完成", "Chapter complete"),
          detail: data?.chapterNumber !== undefined ? tr(`第 ${data.chapterNumber} 章已写完`, `Chapter ${data.chapterNumber} finished`) : undefined,
        });
      } else if (message.event.endsWith(":error") || message.event === "error") {
        pushNotification({
          level: "error",
          title: tr("任务出错", "Task error"),
          detail: message.event,
        });
      }
    }
  }, [sse.messages, pushNotification]);

  // ── 标签页多任务（P3-3）───────────────────────────────────────────
  // 真相在 tabs store：主区渲染 activeTab.route；hash 仅用于深链同步。
  const tabs = useTabsStore((state) => state.tabs);
  const activeTabId = useTabsStore((state) => state.activeId);
  const dispatchTabs = useTabsStore((state) => state.dispatch);
  const activeTab = tabs.find((tab) => tab.id === activeTabId) ?? null;
  const view = activeTab?.route ?? route;

  /** 路由 → 标签标题（面包屑末段，与最近访问同源）。 */
  const routeTitle = (next: HashRoute): string => {
    const bookTitle =
      "bookId" in next
        ? booksData?.books.find((book) => book.id === next.bookId)?.title
        : undefined;
    return deriveBreadcrumb(next, { t, bookTitle }).at(-1)?.label ?? next.page;
  };

  // 外部 hash 深链同步：仅响应 route 自身的变化（首挂载/地址栏直达/刷新/
  // 分享链接/SSE 系统跳转）→ 激活标签未同步时以预览开标签。
  // 关键：依赖只有 routeKeyJson——标签切换（view 变、route 不变）不得触发
  // 本 effect，否则会把 hash 路由"开回来"吞掉切换；关闭最后一个标签时
  // view 回落 route 且不再重开标签（主区仍显示该页，无标签态）。
  const routeKeyJson = JSON.stringify(route);
  useEffect(() => {
    const store = useTabsStore.getState();
    const active = store.tabs.find((tab) => tab.id === store.activeId);
    if (active && JSON.stringify(active.route) === routeKeyJson) return;
    dispatchTabs({ type: "open", route, title: routeTitle(route), preview: true });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [routeKeyJson, dispatchTabs]);

  // 书名回填（P3-3）：深链开标签时 /books 未到、标题回退「书籍」，
  // 数据到达后按路由重命名书相关标签。
  useEffect(() => {
    if (!booksData?.books) return;
    for (const tab of tabs) {
      const route = tab.route;
      if (!("bookId" in route)) continue;
      const bookId: string = route.bookId;
      const title = booksData.books.find((book) => book.id === bookId)?.title;
      if (title && title !== tab.title) {
        dispatchTabs({ type: "retitle", id: tab.id, title });
      }
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [booksData, dispatchTabs]);

  // P3-1 活动栏布局：双轨开关 + 选中区（用户手动选择持久化，否则跟随路由）
  const navLayoutV2 = usePreferencesStore((state) => state.navLayoutV2);
  const storedActiveNavSection = usePreferencesStore((state) => state.activeNavSection);
  const setActiveNavSection = usePreferencesStore((state) => state.setActiveNavSection);
  const focusMode = usePreferencesStore((state) => state.focusMode);
  const setFocusMode = usePreferencesStore((state) => state.setFocusMode);
  const density = usePreferencesStore((state) => state.density);
  const setDensity = usePreferencesStore((state) => state.setDensity);
  const activeSection = storedActiveNavSection ?? sectionForRoute(view);
  // Cmd+B 面板折叠（P3-2，仅 V2 有意义：活动栏本身即图标条，折叠=隐藏面板）
  const [sidePanelVisible, setSidePanelVisible] = useState(true);
  // P3-4：dock / 底部面板开合按页记忆（dock 沿袭书籍页默认可见，底部默认收起）
  const dockVisibility = usePerPageVisibility("inkos:studio:dock-visibility", view.page, true);
  const bottomVisibility = usePerPageVisibility("inkos:studio:bottom-panel-visibility", view.page, false);

  // 用户导航写入「最近访问」（命令面板空查询首屏）。SSE 系统跳转
  // （useSessionEvents 直用 setRoute）不属于用户意图，不记录。
  // P3-3：导航同时以「预览」语义开标签（单击替换预览标签，双击/固定转常驻）。
  const setRouteTracked = (next: HashRoute) => {
    const bookTitle =
      "bookId" in next
        ? booksData?.books.find((book) => book.id === next.bookId)?.title
        : undefined;
    const nextCrumbs = deriveBreadcrumb(next, { t, bookTitle });
    dispatchTabs({ type: "open", route: next, title: nextCrumbs.at(-1)?.label ?? next.page, preview: true });
    pushRecent({
      page: next.page,
      label: nextCrumbs.at(-1)?.label ?? next.page,
      ...("bookId" in next ? { bookId: next.bookId } : {}),
      ...("chapterNumber" in next ? { chapterNumber: next.chapterNumber } : {}),
      ...("serviceId" in next ? { serviceId: next.serviceId } : {}),
      ...("projectId" in next ? { projectId: next.projectId } : {}),
      ...("tab" in next && next.tab ? { tab: next.tab } : {}),
    });
    setRoute(next);
  };

  // P3-1：统一 Nav 工厂（消除 App/Sidebar 双份接口声明）；全部导航写最近访问
  const nav = createNav(setRouteTracked);

  // 命令面板执行上下文：每次渲染重建，命令闭包不持有过期状态。
  // 创建类动作与 Sidebar 的 launchProjectMode/handleOpenBookCreate 等价
  // （P1 不动 Sidebar 本体，P3 活动栏重组时收敛为单一实现）。
  const commandCtx: CommandContext = {
    setRoute: setRouteTracked,
    setThemeMode,
    setDensity,
    setProjectLanguage: (lang) => {
      void putApi("/project", { language: lang }).then(() => refetchProject());
    },
    refetchProject,
    openBookCreate: () => {
      setInput("");
      nav.toBookCreate();
    },
    createProjectChatDraft: () => {
      const sessionId = createDraftSession(null, "chat");
      setProjectChatSessionId(sessionId);
      setInput("");
      nav.toChat();
    },
    launchProjectMode: (kind, playMode) => {
      const sessionId = createDraftSession(null, kind, playMode);
      setProjectChatSessionId(sessionId);
      setInput("");
      nav.toChat();
    },
  };

  const activeBookId = deriveActiveBookId(view);
  const activePage =
    activeBookId
      ? `book:${activeBookId}`
      : view.page === "service-detail"
        ? "services"
        : view.page;

  const activeBookTitle = activeBookId
    ? booksData?.books.find((book) => book.id === activeBookId)?.title
    : undefined;
  const crumbs = deriveBreadcrumb(view, { t, bookTitle: activeBookTitle });

  const startupGate = deriveStartupGate({ ready, projectError });

  if (startupGate === "error") {
    return (
      <div className="min-h-screen bg-background flex items-center justify-center p-6">
        <div className="max-w-md w-full rounded-2xl border border-destructive/30 bg-destructive/5 p-6 space-y-4">
          <div>
            <h1 className="text-lg font-semibold text-destructive">无法加载项目配置 / Failed to load project config</h1>
            <p className="mt-2 text-sm text-muted-foreground break-all">{projectError}</p>
          </div>
          {/* 项目配置没加载出来，语言未知，所以这屏中英双语并排展示。 */}
          <p className="text-sm text-muted-foreground">
            请检查项目根目录下的 inkos.json 是否存在且为合法 JSON，然后重试。
            <br />
            Check that inkos.json in the project root exists and is valid JSON, then retry.
          </p>
          <button
            type="button"
            onClick={() => refetchProject()}
            className="rounded-lg bg-primary px-4 py-2 text-sm font-medium text-primary-foreground"
          >
            重试 / Retry
          </button>
        </div>
      </div>
    );
  }

  if (startupGate === "loading") {
    return <AppShellSkeleton />;
  }

  return (
    <div className={`h-screen bg-background text-foreground flex overflow-hidden font-sans ${focusMode ? "focus-mode" : ""} ${density === "compact" ? "density-compact" : ""}`.trim()}>
      {/* 首启语言选择：主布局之上的强制 Dialog（P1-3），选完壳直接填内容，无整屏切换 */}
      {showLanguageSelector && (
        <LanguageSelector
          onSelect={async (lang) => {
            await postApi("/project/language", { language: lang });
            setShowLanguageSelector(false);
            refetchProject();
          }}
        />
      )}
      {/* Left Sidebar */}
      {/* P3-1 双轨导航布局：V2 = 活动栏(四区) + 按区渲染的侧面板；V1 = 整栏 Sidebar。
          开关持久化在 preferences(navLayoutV2)，P4 走查后删旧轨。 */}
      {navLayoutV2 ? (
        <>
          <ActivityBar
            nav={nav}
            activeSection={activeSection}
            onSelectSection={setActiveNavSection}
            t={t}
            lang={currentLang}
          />
          <SidePanel nav={nav} activePage={activePage} sse={sse} t={t} zone={activeSection} visible={sidePanelVisible} />
        </>
      ) : (
        <Sidebar nav={nav} activePage={activePage} sse={sse} t={t} />
      )}

      {/* Center Content */}
      <div className="flex-1 flex flex-col min-w-0 bg-background/30 backdrop-blur-sm">
        {/* Header Strip — 三段化（P1-7）：左面包屑 / 中命令面板入口 / 右语言与主题 */}
        {/* P2-1 融合标题栏：header 及三段容器标注拖拽区；Tauri 拖拽脚本只认
            mousedown 目标元素自身的属性，按钮/输入等子元素不带属性即天然可点，
            双击标题栏最大化由系统语义处理。 */}
        <header
          data-tauri-drag-region
          className={`h-14 shrink-0 flex items-center justify-between gap-4 ${headerInsetClass} pr-8 border-b border-border/40`}
        >
          <nav
            aria-label={tr("面包屑", "Breadcrumb")}
            data-testid="breadcrumb"
            data-tauri-drag-region
            className="flex min-w-0 items-center gap-1.5 text-[17px]"
          >
            {crumbs.map((crumb, index) => {
              const target = crumb.route;
              return (
                <span key={`${index}-${crumb.label}`} className="flex min-w-0 items-center gap-1.5">
                  {index > 0 && <span className="text-muted-foreground/50">/</span>}
                  {target ? (
                    <button
                      type="button"
                      onClick={() => setRouteTracked(target)}
                      className="truncate text-muted-foreground transition-colors hover:text-foreground"
                    >
                      {crumb.label}
                    </button>
                  ) : (
                    <span className="truncate font-serif font-medium text-foreground">
                      {crumb.label}
                    </span>
                  )}
                </span>
              );
            })}
          </nav>

          <div data-tauri-drag-region className="flex min-w-0 flex-1 justify-center px-2">
            <button
              type="button"
              data-testid="command-palette-trigger"
              onClick={() => setPaletteOpen(true)}
              className="hidden md:flex w-64 items-center gap-2 rounded-lg border border-border/50 bg-muted/50 px-3 py-1.5 text-sm text-muted-foreground transition-colors hover:bg-secondary/60"
            >
              <Search size={14} className="shrink-0" />
              <span className="flex-1 truncate text-left">{t("cmd.searchPlaceholder")}</span>
              <kbd className="shrink-0 rounded border border-border/60 bg-background px-1.5 py-0.5 text-[10px] font-medium text-muted-foreground">
                {isMacPlatform ? "⌘K" : "Ctrl K"}
              </kbd>
            </button>
          </div>

          <div data-tauri-drag-region className="flex shrink-0 items-center gap-3">
            <div className="flex gap-0.5 bg-muted/50 rounded-lg p-0.5">
              <button
                onClick={async () => {
                  await putApi("/project", { language: "zh" });
                  refetchProject();
                }}
                className={`px-2.5 py-1 text-[16px] font-medium rounded-md ${currentLang === "zh" ? "bg-primary text-primary-foreground" : "text-muted-foreground"}`}
              >
                中
              </button>
              <button
                onClick={async () => {
                  await putApi("/project", { language: "en" });
                  refetchProject();
                }}
                className={`px-2.5 py-1 text-[16px] font-medium rounded-md ${currentLang === "en" ? "bg-primary text-primary-foreground" : "text-muted-foreground"}`}
              >
                EN
              </button>
            </div>

            {/* P2-3 三态循环：light → dark → auto（跟随系统）→ light；图标随模式 */}
            <button
              type="button"
              aria-label={themeMode === "auto" ? tr("主题：跟随系统", "Theme: follow system") : isDark ? tr("主题：深色", "Theme: dark") : tr("主题：浅色", "Theme: light")}
              onClick={() => setThemeMode(cycleThemeMode(themeMode, theme))}
              className="text-muted-foreground hover:text-foreground transition-colors"
            >
              {themeMode === "auto" ? <Monitor size={18} /> : isDark ? <Sun size={18} /> : <Moon size={18} />}
            </button>
          </div>
        </header>

        {/* P3-3 标签条：tabs store 为真相；无标签时整条隐藏 */}
        <TabStrip
          tabs={tabs}
          activeId={activeTabId}
          onActivate={(id) => dispatchTabs({ type: "activate", id })}
          onClose={(id) => dispatchTabs({ type: "close", id })}
          onCloseOthers={(id) => dispatchTabs({ type: "closeOthers", id })}
          onPin={(id, pinned) => dispatchTabs({ type: "pin", id, pinned })}
        />

        {/* Main Content Area */}
        <main className="flex-1 relative overflow-y-auto scroll-smooth">
          {view.page === "dashboard" && (
            <div className="max-w-4xl mx-auto px-6 py-12 md:px-12 lg:py-16 fade-in">
              <Dashboard nav={nav} sse={sse} theme={theme} t={t} />
            </div>
          )}
          {isBookCreateChatRoute(view) && (
            <div className="absolute inset-0 flex min-w-0">
              <ChatPage
                mode="book-create"
                nav={nav}
                theme={theme}
                t={t}
                sse={sse}
              />
            </div>
          )}
          {view.page === "chat" && (
            <div className="absolute inset-0 flex min-w-0">
              <ChatPage
                mode="project-chat"
                nav={nav}
                theme={theme}
                t={t}
                sse={sse}
              />
            </div>
          )}
          {view.page === "book" && (
            <div className="absolute inset-0 flex min-w-0">
              <ChatPage
                activeBookId={view.bookId}
                mode="book"
                nav={nav}
                theme={theme}
                t={t}
                sse={sse}
              />
              {/* P3-4：BookSidebar 泛化为 ContextDock（注册表/宽记忆/按页开合） */}
              <ContextDock
                bookId={view.bookId}
                theme={theme}
                t={t}
                sse={sse}
                visible={dockVisibility.visible}
                onClose={() => dockVisibility.setVisible(false)}
              />
              <BookSidebarToggle bookId={view.bookId} theme={theme} t={t} sse={sse} />
            </div>
          )}
          {view.page === "book-settings" && (
            <div className="max-w-4xl mx-auto px-6 py-12 md:px-12 lg:py-16 fade-in">
              <BookDetail bookId={view.bookId} nav={nav} theme={theme} t={t} sse={sse} />
            </div>
          )}
          {view.page === "book-timeline" && (
            <div className="mx-auto w-full max-w-[1400px] px-4 py-12 sm:px-6 lg:px-10 lg:py-16 2xl:px-12 fade-in">
              <BookTimeline bookId={view.bookId} nav={nav} theme={theme} t={t} />
            </div>
          )}
          {view.page === "chapter" && (
            <div className="mx-auto w-full max-w-[1400px] px-4 py-12 sm:px-6 lg:px-10 lg:py-16 2xl:px-12 fade-in">
              <ChapterReader bookId={view.bookId} chapterNumber={view.chapterNumber} nav={nav} theme={theme} t={t} />
            </div>
          )}
          {view.page === "analytics" && (
            <div className="max-w-4xl mx-auto px-6 py-12 md:px-12 lg:py-16 fade-in">
              <Analytics bookId={view.bookId} nav={nav} theme={theme} t={t} />
            </div>
          )}
          {view.page === "services" && (
            <div className="max-w-4xl mx-auto px-6 py-12 md:px-12 lg:py-16 fade-in">
              <ServiceListPage nav={nav} />
            </div>
          )}
          {view.page === "project-settings" && (
            <div className="max-w-4xl mx-auto px-6 py-12 md:px-12 lg:py-16 fade-in">
              <ProjectSettings nav={nav} theme={theme} t={t} />
            </div>
          )}
          {view.page === "service-detail" && (
            <div className="max-w-4xl mx-auto px-6 py-12 md:px-12 lg:py-16 fade-in">
              <ServiceDetailPage serviceId={view.serviceId} nav={nav} />
            </div>
          )}
          {view.page === "truth" && (
            <div className="max-w-4xl mx-auto px-6 py-12 md:px-12 lg:py-16 fade-in">
              <TruthFiles bookId={view.bookId} nav={nav} theme={theme} t={t} />
            </div>
          )}
          {view.page === "daemon" && (
            <div className="max-w-4xl mx-auto px-6 py-12 md:px-12 lg:py-16 fade-in">
              <DaemonControl nav={nav} theme={theme} t={t} sse={sse} />
            </div>
          )}
          {view.page === "logs" && (
            <div className="max-w-4xl mx-auto px-6 py-12 md:px-12 lg:py-16 fade-in">
              <LogViewer nav={nav} theme={theme} t={t} />
            </div>
          )}
          {view.page === "genres" && (
            <div className="max-w-4xl mx-auto px-6 py-12 md:px-12 lg:py-16 fade-in">
              <GenreManager nav={nav} theme={theme} t={t} />
            </div>
          )}
          {view.page === "style" && (
            <div className="max-w-4xl mx-auto px-6 py-12 md:px-12 lg:py-16 fade-in">
              <StyleManager nav={nav} theme={theme} t={t} />
            </div>
          )}
          {view.page === "translation" && (
            <div className="max-w-6xl mx-auto px-6 py-12 md:px-12 lg:py-16 fade-in">
              <TranslationManager nav={nav} theme={theme} t={t} />
            </div>
          )}
          {view.page === "import" && (
            <div className="max-w-4xl mx-auto px-6 py-12 md:px-12 lg:py-16 fade-in">
              <ImportManager nav={nav} theme={theme} t={t} initialTab={view.tab} />
            </div>
          )}
          {view.page === "radar" && (
            <div className="max-w-4xl mx-auto px-6 py-12 md:px-12 lg:py-16 fade-in">
              <RadarView nav={nav} theme={theme} t={t} />
            </div>
          )}
          {view.page === "doctor" && (
            <div className="max-w-4xl mx-auto px-6 py-12 md:px-12 lg:py-16 fade-in">
              <DoctorView nav={nav} theme={theme} t={t} />
            </div>
          )}
          {view.page === "play" && (
            <div className="max-w-4xl mx-auto px-6 py-12 md:px-12 lg:py-16 fade-in">
              <StoryPlayer projectId={view.projectId} nav={nav} theme={theme} t={t} />
            </div>
          )}
          {view.page === "film" && (
            <div className="max-w-4xl mx-auto px-6 py-12 md:px-12 lg:py-16 fade-in">
              <StoryGraphTree projectId={view.projectId} nav={nav} theme={theme} t={t} />
            </div>
          )}
          {view.page === "film-author" && (
            <div className="absolute inset-0 flex min-w-0">
              <ChatPage
                activeBookId={view.projectId}
                mode="interactive-film-authoring"
                nav={nav}
                theme={theme}
                t={t}
                sse={sse}
              />
            </div>
          )}
          {view.page === "film-studio" && (
            <Suspense fallback={<div className="p-6 text-sm">{tr("加载创作向导…", "Loading creation wizard…")}</div>}>
              <FilmWizard projectId={view.projectId} nav={nav} theme={theme} t={t} sse={sse} />
            </Suspense>
          )}
          {view.page === "flow" && (
            <Suspense fallback={<div className="p-6 text-sm">{tr("加载流程图…", "Loading flow view…")}</div>}>
              <FlowView projectId={view.projectId} nav={nav} theme={theme} t={t} />
            </Suspense>
          )}
        </main>

        {/* P3-4 底部面板（Cmd+J）：任务流/日志，按页记忆开合 */}
        <BottomPanel
          visible={bottomVisibility.visible}
          onClose={() => bottomVisibility.setVisible(false)}
        />

        {/* P3-5 状态栏：书·章·字数 / SSE+daemon 状态点 / 生成中查看 */}
        <StatusBar
          bookTitle={
            view.page === "book" || view.page === "chapter" || view.page === "analytics" || view.page === "truth"
              ? (booksData?.books.find((book) => book.id === activeBookId)?.title ?? activeBookTitle)
              : undefined
          }
          chapter={view.page === "chapter" ? { bookId: view.bookId, number: view.chapterNumber } : undefined}
          sseConnected={sse.connected}
          onReconnect={sse.reconnect}
          daemonRunning={daemonData?.running}
          onOpenBottomPanel={() => bottomVisibility.setVisible(true)}
        />
      </div>

      {/* P4-3 通知中心：右下角铃铛（非模态，专注模式不打断） */}
      <NotificationCenter />

      {/* 全局命令面板（⌘K / Ctrl+K），P1-5/6 */}
      <CommandPalette
        open={paletteOpen}
        onOpenChange={setPaletteOpen}
        ctx={commandCtx}
        lang={currentLang}
      />

      {/* P2-6 快捷键速查（⌘/）：与快捷键注册表同源生成 */}
      <HotkeyCheatSheet
        open={cheatSheetOpen}
        onOpenChange={setCheatSheetOpen}
        defs={HOTKEY_DEFS}
        lang={currentLang}
        isMac={isMacPlatform}
      />

      {/* P2-5 快速打开（⌘P）：书/章节/影游/会话 内容层直达 */}
      <QuickOpenPalette
        open={quickOpenOpen}
        onOpenChange={setQuickOpenOpen}
        books={booksData?.books ?? []}
        sessions={quickOpenSessions}
        onNavigate={setRouteTracked}
      />
    </div>
  );
}
