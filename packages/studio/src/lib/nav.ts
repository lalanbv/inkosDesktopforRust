import type { HashRoute } from "@/hooks/use-hash-route";

/**
 * 统一 Nav 接口（P3-1）。
 * 此前 App.tsx（25 方法）与 Sidebar.tsx（16 方法子集）各自声明同名接口，
 * 新增导航入口（P1-5 命令面板、P2-7 菜单栏）时要多处同步——现收敛为单一导出。
 */

export type ImportTab = "chapters" | "canon" | "fanfic" | "spinoff" | "imitation" | "backfill";

export interface Nav {
  toDashboard(): void;
  toChat(): void;
  toBook(bookId: string): void;
  toBookSettings(bookId: string): void;
  toBookTimeline(bookId: string): void;
  toBookCreate(): void;
  toChapter(bookId: string, chapterNumber: number): void;
  toAnalytics(bookId: string): void;
  toServices(): void;
  toOnboarding(): void;
  toProjectSettings(): void;
  toServiceDetail(id: string): void;
  toTruth(bookId: string): void;
  toDaemon(): void;
  toLogs(): void;
  toGenres(): void;
  toStyle(): void;
  toTranslation(): void;
  toImport(tab?: ImportTab): void;
  toRadar(): void;
  toDoctor(): void;
  toPlay(projectId: string): void;
  toFilm(projectId: string): void;
  toFlow(projectId: string): void;
  toFilmAuthor(projectId: string): void;
  toFilmStudio(projectId: string): void;
}

/** Nav 工厂：navigate 通常是 App 的 setRouteTracked（写最近访问）。 */
export function createNav(navigate: (route: HashRoute) => void): Nav {
  return {
    toDashboard: () => navigate({ page: "dashboard" }),
    toChat: () => navigate({ page: "chat" }),
    toBook: (bookId) => navigate({ page: "book", bookId }),
    toBookSettings: (bookId) => navigate({ page: "book-settings", bookId }),
    toBookTimeline: (bookId) => navigate({ page: "book-timeline", bookId }),
    toBookCreate: () => navigate({ page: "book-create" }),
    toChapter: (bookId, chapterNumber) => navigate({ page: "chapter", bookId, chapterNumber }),
    toAnalytics: (bookId) => navigate({ page: "analytics", bookId }),
    toServices: () => navigate({ page: "services" }),
    toOnboarding: () => navigate({ page: "onboarding" }),
    toProjectSettings: () => navigate({ page: "project-settings" }),
    toServiceDetail: (id) => navigate({ page: "service-detail", serviceId: id }),
    toTruth: (bookId) => navigate({ page: "truth", bookId }),
    toDaemon: () => navigate({ page: "daemon" }),
    toLogs: () => navigate({ page: "logs" }),
    toGenres: () => navigate({ page: "genres" }),
    toStyle: () => navigate({ page: "style" }),
    toTranslation: () => navigate({ page: "translation" }),
    toImport: (tab) => navigate({ page: "import", ...(tab ? { tab } : {}) }),
    toRadar: () => navigate({ page: "radar" }),
    toDoctor: () => navigate({ page: "doctor" }),
    toPlay: (projectId) => navigate({ page: "play", projectId }),
    toFilm: (projectId) => navigate({ page: "film", projectId }),
    toFlow: (projectId) => navigate({ page: "flow", projectId }),
    toFilmAuthor: (projectId) => navigate({ page: "film-author", projectId }),
    toFilmStudio: (projectId) => navigate({ page: "film-studio", projectId }),
  };
}
