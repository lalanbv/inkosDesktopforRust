import { describe, it, expect } from "vitest";
import { deriveBreadcrumb } from "./breadcrumb";
import type { HashRoute } from "@/hooks/use-hash-route";

// t 直接回显 key：断言里看到的即所用 i18n key，模板占位符由 breadcrumb 替换。
const t = (key: string) => key;

const opts = (bookTitle?: string) => ({ t: t as never, bookTitle });

function labels(route: HashRoute, bookTitle?: string): string[] {
  return deriveBreadcrumb(route, opts(bookTitle)).map((crumb) => crumb.label);
}

describe("deriveBreadcrumb", () => {
  it("dashboard is a single segment without a route", () => {
    const crumbs = deriveBreadcrumb({ page: "dashboard" }, opts());
    expect(crumbs).toEqual([{ label: "bread.home" }]);
  });

  it("covers every parameterless page as [home, page]", () => {
    const cases: ReadonlyArray<[HashRoute, string]> = [
      [{ page: "chat" }, "bread.chat"],
      [{ page: "book-create" }, "bread.newBook"],
      [{ page: "services" }, "nav.config"],
      [{ page: "project-settings" }, "nav.projectSettings"],
      [{ page: "daemon" }, "daemon.title"],
      [{ page: "logs" }, "logs.title"],
      [{ page: "genres" }, "bread.genres"],
      [{ page: "style" }, "style.title"],
      [{ page: "translation" }, "nav.translation"],
      [{ page: "import" }, "import.title"],
      [{ page: "radar" }, "radar.title"],
      [{ page: "doctor" }, "doctor.title"],
      [{ page: "play", projectId: "p1" }, "bread.play"],
      [{ page: "film", projectId: "p1" }, "bread.film"],
      [{ page: "flow", projectId: "p1" }, "bread.flow"],
      [{ page: "film-author", projectId: "p1" }, "bread.filmAuthor"],
      [{ page: "film-studio", projectId: "p1" }, "bread.filmStudio"],
    ];
    for (const [route, lastKey] of cases) {
      expect(labels(route)).toEqual(["bread.home", lastKey]);
    }
  });

  it("book routes nest under the resolved book title", () => {
    expect(labels({ page: "book", bookId: "b1" }, "山河志")).toEqual([
      "bread.home",
      "山河志",
    ]);
    expect(labels({ page: "book-settings", bookId: "b1" }, "山河志")).toEqual([
      "bread.home",
      "山河志",
      "bread.bookSettings",
    ]);
    expect(labels({ page: "analytics", bookId: "b1" }, "山河志")).toEqual([
      "bread.home",
      "山河志",
      "analytics.title",
    ]);
    expect(labels({ page: "truth", bookId: "b1" }, "山河志")).toEqual([
      "bread.home",
      "山河志",
      "bread.truth",
    ]);
  });

  it("falls back to a generic books label when the title is unknown", () => {
    expect(labels({ page: "book", bookId: "b1" })).toEqual(["bread.home", "bread.books"]);
    expect(labels({ page: "book", bookId: "b1" }, "  ")).toEqual(["bread.home", "bread.books"]);
  });

  it("chapter substitutes the {n} placeholder with the chapter number", () => {
    // 该断言用真实模板形态的 t，验证占位符替换确实发生。
    const templatedOpts = {
      t: ((key: string) => (key === "bread.chapter" ? "第{n}章" : key)) as never,
    };
    const crumbs = deriveBreadcrumb(
      { page: "chapter", bookId: "b1", chapterNumber: 3 },
      templatedOpts,
    );
    expect(crumbs.at(-1)?.label).toBe("第3章");
  });

  it("service-detail keeps the services segment clickable back to the list", () => {
    const crumbs = deriveBreadcrumb({ page: "service-detail", serviceId: "kkaiapi" }, opts());
    expect(crumbs.map((crumb) => crumb.label)).toEqual([
      "bread.home",
      "nav.config",
      "bread.serviceDetail",
    ]);
    expect(crumbs[1].route).toEqual({ page: "services" });
  });

  it("every non-final segment carries a route; the final segment never does", () => {
    const routes: HashRoute[] = [
      { page: "chapter", bookId: "b1", chapterNumber: 2 },
      { page: "service-detail", serviceId: "s1" },
      { page: "book-settings", bookId: "b1" },
      { page: "logs" },
    ];
    for (const route of routes) {
      const crumbs = deriveBreadcrumb(route, opts("书"));
      crumbs.forEach((crumb, index) => {
        const isFinal = index === crumbs.length - 1;
        expect(crumb.route !== undefined).toBe(!isFinal);
      });
      expect(crumbs[0].route).toEqual({ page: "dashboard" });
    }
  });
});
