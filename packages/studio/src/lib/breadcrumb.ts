import type { HashRoute } from "@/hooks/use-hash-route";
import type { TFunction } from "@/hooks/use-i18n";

/** 面包屑一段：已本地化的文案；非末段携带点击跳转的目标路由。 */
export interface BreadcrumbItem {
  label: string;
  route?: HashRoute;
}

export interface BreadcrumbOptions {
  t: TFunction;
  /** 当前书名（book 系路由）。缺失时降级为通用的「书籍」。 */
  bookTitle?: string;
}

function bookLabel(options: BreadcrumbOptions): string {
  return options.bookTitle?.trim() ? options.bookTitle.trim() : options.t("bread.books");
}

/**
 * 路由 → 面包屑分段（纯函数）。末段恒为当前页（无 route），
 * 其余段按层级携带回跳路由。23 种路由变体全覆盖。
 */
export function deriveBreadcrumb(
  route: HashRoute,
  options: BreadcrumbOptions,
): BreadcrumbItem[] {
  const t = options.t;
  const home = (): BreadcrumbItem => ({ label: t("bread.home"), route: { page: "dashboard" } });
  const book = (bookId: string): BreadcrumbItem =>
    ({ label: bookLabel(options), route: { page: "book", bookId } });

  switch (route.page) {
    case "dashboard":
      return [{ label: t("bread.home") }];
    case "chat":
      return [home(), { label: t("bread.chat") }];
    case "book-create":
      return [home(), { label: t("bread.newBook") }];
    case "book":
      return [home(), book(route.bookId)];
    case "book-settings":
      return [home(), book(route.bookId), { label: t("bread.bookSettings") }];
    case "chapter":
      return [
        home(),
        book(route.bookId),
        { label: t("bread.chapter").replace("{n}", String(route.chapterNumber)) },
      ];
    case "analytics":
      return [home(), book(route.bookId), { label: t("analytics.title") }];
    case "truth":
      return [home(), book(route.bookId), { label: t("bread.truth") }];
    case "services":
      return [home(), { label: t("nav.config") }];
    case "service-detail":
      return [
        home(),
        { label: t("nav.config"), route: { page: "services" } },
        { label: t("bread.serviceDetail") },
      ];
    case "project-settings":
      return [home(), { label: t("nav.projectSettings") }];
    case "daemon":
      return [home(), { label: t("daemon.title") }];
    case "logs":
      return [home(), { label: t("logs.title") }];
    case "genres":
      return [home(), { label: t("bread.genres") }];
    case "style":
      return [home(), { label: t("style.title") }];
    case "translation":
      return [home(), { label: t("nav.translation") }];
    case "import":
      return [home(), { label: t("import.title") }];
    case "radar":
      return [home(), { label: t("radar.title") }];
    case "doctor":
      return [home(), { label: t("doctor.title") }];
    case "play":
      return [home(), { label: t("bread.play") }];
    case "film":
      return [home(), { label: t("bread.film") }];
    case "flow":
      return [home(), { label: t("bread.flow") }];
    case "film-author":
      return [home(), { label: t("bread.filmAuthor") }];
    case "film-studio":
      return [home(), { label: t("bread.filmStudio") }];
  }
}
