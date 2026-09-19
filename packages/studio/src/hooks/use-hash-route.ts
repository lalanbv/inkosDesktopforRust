import { useState, useEffect, useCallback } from "react";

export type HashRoute =
  | { page: "dashboard" }
  | { page: "chat" }
  | { page: "book"; bookId: string }
  | { page: "book-settings"; bookId: string }
  | { page: "book-timeline"; bookId: string }
  | { page: "book-create" }
  | { page: "services" }
  | { page: "onboarding" }
  | { page: "project-settings" }
  | { page: "service-detail"; serviceId: string }
  | { page: "chapter"; bookId: string; chapterNumber: number }
  | { page: "analytics"; bookId: string }
  | { page: "truth"; bookId: string }
  | { page: "daemon" }
  | { page: "logs" }
  | { page: "genres" }
  | { page: "style" }
  | { page: "translation" }
  | { page: "import"; tab?: "chapters" | "canon" | "fanfic" | "spinoff" | "imitation" | "backfill" }
  | { page: "radar" }
  | { page: "doctor" }
  | { page: "play"; projectId: string }
  | { page: "film"; projectId: string }
  | { page: "flow"; projectId: string }
  | { page: "film-author"; projectId: string }
  | { page: "film-studio"; projectId: string };

function parseHash(hash: string): HashRoute {
  const path = hash.replace(/^#\/?/, "");

  if (!path || path === "/") return { page: "dashboard" };
  if (path === "chat") return { page: "chat" };
  if (path === "config" || path === "services") return { page: "services" };
  // R9/370 号：新手创作向导（首跑三步：厂商→Key→能力探测）。
  if (path === "onboarding") return { page: "onboarding" };
  if (path === "settings") return { page: "project-settings" };
  if (path === "import") return { page: "import" };
  // 405 号走查修复：#/radar 直达/刷新此前回落 Dashboard（深链失效）。
  if (path === "radar") return { page: "radar" };
  if (path === "translation") return { page: "translation" };
  const importMatch = path.match(/^import\/(chapters|canon|fanfic|spinoff|imitation|backfill)$/);
  if (importMatch) return { page: "import", tab: importMatch[1] as "chapters" | "canon" | "fanfic" | "spinoff" | "imitation" | "backfill" };
  if (path === "book/new") return { page: "book-create" };

  const serviceMatch = path.match(/^services\/([^/]+)$/);
  if (serviceMatch) return { page: "service-detail", serviceId: decodeURIComponent(serviceMatch[1]) };

  const bookSettingsMatch = path.match(/^book\/([^/]+)\/settings$/);
  if (bookSettingsMatch) return { page: "book-settings", bookId: decodeURIComponent(bookSettingsMatch[1]) };

  // 208 号：章节阅读器深链——此前 state-only（刷新即回首页、URL 不可分享）。
  const chapterMatch = path.match(/^book\/([^/]+)\/chapter\/(\d+)$/);
  if (chapterMatch) {
    return {
      page: "chapter",
      bookId: decodeURIComponent(chapterMatch[1]),
      chapterNumber: Number(chapterMatch[2]),
    };
  }

  const bookTimelineMatch = path.match(/^book\/([^/]+)\/timeline$/);
  if (bookTimelineMatch) return { page: "book-timeline", bookId: decodeURIComponent(bookTimelineMatch[1]) };

  // 460 号：数据分析页深链——此前 state-only（直达/刷新回落 Dashboard，405 号同款）。
  const analyticsMatch = path.match(/^book\/([^/]+)\/analytics$/);
  if (analyticsMatch) return { page: "analytics", bookId: decodeURIComponent(analyticsMatch[1]) };

  // 469 号：daemon/doctor/genres/logs/style/truth 深链补齐——同款批量实例清零。
  if (path === "daemon") return { page: "daemon" };
  if (path === "doctor") return { page: "doctor" };
  if (path === "genres") return { page: "genres" };
  if (path === "logs") return { page: "logs" };
  if (path === "style") return { page: "style" };
  const truthMatch = path.match(/^book\/([^/]+)\/truth$/);
  if (truthMatch) return { page: "truth", bookId: decodeURIComponent(truthMatch[1]) };

  const bookMatch = path.match(/^book\/([^/]+)$/);
  if (bookMatch) return { page: "book", bookId: decodeURIComponent(bookMatch[1]) };

  const playMatch = path.match(/^play\/([^/]+)$/);
  if (playMatch) return { page: "play", projectId: decodeURIComponent(playMatch[1]) };

  const filmMatch = path.match(/^film\/([^/]+)$/);
  if (filmMatch) return { page: "film", projectId: decodeURIComponent(filmMatch[1]) };

  const flowMatch = path.match(/^flow\/([^/]+)$/);
  if (flowMatch) return { page: "flow", projectId: decodeURIComponent(flowMatch[1]) };

  const filmAuthorMatch = path.match(/^film-author\/([^/]+)$/);
  if (filmAuthorMatch) return { page: "film-author", projectId: decodeURIComponent(filmAuthorMatch[1]) };

  const studioFilmMatch = path.match(/^studio\/film\/([^/]+)$/);
  if (studioFilmMatch) return { page: "film-studio", projectId: decodeURIComponent(studioFilmMatch[1]) };

  return { page: "dashboard" };
}

// 461 号：页面路由单一事实表——键对 HashRoute["page"] 穷举（mapped type），
// 新增页面漏配 = TS2322 编译错。根治「新增页面漏 hash 分支」类缺陷
// （analytics 为 208/405 同款第三实例，460 号）。
// - toHash：hash 写入模板（与旧 routeToHash switch 逐字等价）；
// - writable：setRoute 是否写 URL（镜像旧 HASH_PAGES 成员关系——onboarding
//   可产 hash 供深链解析但不写 URL，语义保留；radar 原同列，532 号起升级
//   可写——侧栏切换后 URL 停留旧路由，刷新/复制链接即丢页，与 469 号
//   doctor/genres/logs 深链补齐同款缺陷清零）；
// - sample：round-trip 测试样本（parseHash(toHash(sample)) 必须还原 sample）。
// - null = 纯 state-only 页（不产 hash 亦不解析：doctor/genres/style/truth/daemon/logs）。
interface PageSpecFor<K extends HashRoute["page"]> {
  readonly toHash: (route: Extract<HashRoute, { page: K }>) => string;
  readonly writable: boolean;
  readonly sample: Extract<HashRoute, { page: K }>;
}

const PAGE_SPEC: { readonly [K in HashRoute["page"]]: PageSpecFor<K> | null } = {
  dashboard: { toHash: () => "#/", writable: true, sample: { page: "dashboard" } },
  chat: { toHash: () => "#/chat", writable: true, sample: { page: "chat" } },
  book: { toHash: (r) => `#/book/${encodeURIComponent(r.bookId)}`, writable: true, sample: { page: "book", bookId: "b1" } },
  "book-settings": { toHash: (r) => `#/book/${encodeURIComponent(r.bookId)}/settings`, writable: true, sample: { page: "book-settings", bookId: "b1" } },
  "book-timeline": { toHash: (r) => `#/book/${encodeURIComponent(r.bookId)}/timeline`, writable: true, sample: { page: "book-timeline", bookId: "b1" } },
  analytics: { toHash: (r) => `#/book/${encodeURIComponent(r.bookId)}/analytics`, writable: true, sample: { page: "analytics", bookId: "b1" } },
  "book-create": { toHash: () => "#/book/new", writable: true, sample: { page: "book-create" } },
  chapter: { toHash: (r) => `#/book/${encodeURIComponent(r.bookId)}/chapter/${r.chapterNumber}`, writable: true, sample: { page: "chapter", bookId: "b1", chapterNumber: 2 } },
  services: { toHash: () => "#/services", writable: true, sample: { page: "services" } },
  onboarding: { toHash: () => "#/onboarding", writable: false, sample: { page: "onboarding" } },
  "project-settings": { toHash: () => "#/settings", writable: true, sample: { page: "project-settings" } },
  translation: { toHash: () => "#/translation", writable: true, sample: { page: "translation" } },
  import: { toHash: (r) => (r.tab ? `#/import/${r.tab}` : "#/import"), writable: true, sample: { page: "import" } },
  "service-detail": { toHash: (r) => `#/services/${encodeURIComponent(r.serviceId)}`, writable: true, sample: { page: "service-detail", serviceId: "s1" } },
  play: { toHash: (r) => `#/play/${encodeURIComponent(r.projectId)}`, writable: true, sample: { page: "play", projectId: "p1" } },
  film: { toHash: (r) => `#/film/${encodeURIComponent(r.projectId)}`, writable: true, sample: { page: "film", projectId: "p1" } },
  flow: { toHash: (r) => `#/flow/${encodeURIComponent(r.projectId)}`, writable: true, sample: { page: "flow", projectId: "p1" } },
  "film-author": { toHash: (r) => `#/film-author/${encodeURIComponent(r.projectId)}`, writable: true, sample: { page: "film-author", projectId: "p1" } },
  "film-studio": { toHash: (r) => `#/studio/film/${encodeURIComponent(r.projectId)}`, writable: true, sample: { page: "film-studio", projectId: "p1" } },
  radar: { toHash: () => "#/radar", writable: true, sample: { page: "radar" } },
  doctor: { toHash: () => "#/doctor", writable: true, sample: { page: "doctor" } },
  genres: { toHash: () => "#/genres", writable: true, sample: { page: "genres" } },
  style: { toHash: () => "#/style", writable: true, sample: { page: "style" } },
  truth: { toHash: (r) => `#/book/${encodeURIComponent(r.bookId)}/truth`, writable: true, sample: { page: "truth", bookId: "b1" } },
  daemon: { toHash: () => "#/daemon", writable: true, sample: { page: "daemon" } },
  logs: { toHash: () => "#/logs", writable: true, sample: { page: "logs" } },
};

function routeToHash(route: HashRoute): string {
  // 按 page 查表后宽化：每个 spec 只处理自己的页面类型（查表键即保证）。
  const spec = PAGE_SPEC[route.page] as {
    toHash: (route: HashRoute) => string;
    writable: boolean;
  } | null;
  return spec?.toHash(route) ?? "";
}

export { parseHash, routeToHash, PAGE_SPEC }; // for testing

// 461 号：由 PAGE_SPEC 派生（writable=true 的页面），不再双处手工维护。
const HASH_PAGES = new Set(
  (Object.keys(PAGE_SPEC) as Array<HashRoute["page"]>).filter(
    (page) => PAGE_SPEC[page] !== null && PAGE_SPEC[page]!.writable,
  ),
);

export function useHashRoute() {
  const [route, setRouteState] = useState<HashRoute>(() => parseHash(window.location.hash));

  useEffect(() => {
    const onHashChange = () => setRouteState(parseHash(window.location.hash));
    window.addEventListener("hashchange", onHashChange);
    return () => window.removeEventListener("hashchange", onHashChange);
  }, []);

  const setRoute = useCallback((newRoute: HashRoute) => {
    // 先同步 React state：无论目标页面是否写 URL，保证页面立刻切换。
    // 之前只在非 hash 页面才 setRouteState，hash 页面完全靠 hashchange 事件回调触发。
    // 但当 URL 没有实际变化时（比如从 services → logs → services，中间的 logs
    // 不写 URL，URL 一直停在 #/services），再次赋值同一个 hash 不会触发 hashchange，
    // React state 就永远停留在 logs，表现为"点不动"。
    setRouteState(newRoute);
    if (HASH_PAGES.has(newRoute.page)) {
      const hash = routeToHash(newRoute);
      if (hash && window.location.hash !== hash) {
        window.location.hash = hash;
      }
    }
  }, []);

  const nav = {
    toServices: () => setRoute({ page: "services" }),
    toOnboarding: () => setRoute({ page: "onboarding" }),
    toServiceDetail: (id: string) => setRoute({ page: "service-detail", serviceId: id }),
  };

  return { route, setRoute, nav };
}
