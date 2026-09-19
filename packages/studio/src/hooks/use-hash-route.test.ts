import { describe, expect, it } from "vitest";
import { PAGE_SPEC, parseHash, routeToHash } from "./use-hash-route";

describe("hash route", () => {
  describe("parseHash", () => {
    it("parses empty hash as dashboard", () => {
      expect(parseHash("")).toEqual({ page: "dashboard" });
    });

    it("parses #/ as dashboard", () => {
      expect(parseHash("#/")).toEqual({ page: "dashboard" });
    });

    it("parses chat route", () => {
      expect(parseHash("#/chat")).toEqual({ page: "chat" });
    });

    it("parses book route", () => {
      expect(parseHash("#/book/my-novel")).toEqual({ page: "book", bookId: "my-novel" });
    });

    it("parses book settings route", () => {
      expect(parseHash("#/book/my-novel/settings")).toEqual({ page: "book-settings", bookId: "my-novel" });
    });

    it("decodes encoded bookId", () => {
      expect(parseHash("#/book/%E4%B9%9D%E9%BE%99")).toEqual({ page: "book", bookId: "九龙" });
    });

    it("parses book/new as book-create", () => {
      expect(parseHash("#/book/new")).toEqual({ page: "book-create" });
    });

    it("parses config as services (redirect)", () => {
      expect(parseHash("#/config")).toEqual({ page: "services" });
    });

    it("parses services", () => {
      expect(parseHash("#/services")).toEqual({ page: "services" });
    });

    it("parses project settings", () => {
      expect(parseHash("#/settings")).toEqual({ page: "project-settings" });
    });

    it("parses service-detail", () => {
      expect(parseHash("#/services/openai")).toEqual({ page: "service-detail", serviceId: "openai" });
    });

    it("parses import tab routes", () => {
      expect(parseHash("#/import/fanfic")).toEqual({ page: "import", tab: "fanfic" });
    });

    it("parses #/translation", () => {
      expect(parseHash("#/translation")).toEqual({ page: "translation" });
    });

    it("decodes encoded serviceId", () => {
      expect(parseHash("#/services/%E8%87%AA%E5%AE%9A%E4%B9%89")).toEqual({ page: "service-detail", serviceId: "自定义" });
    });

    it("falls back to dashboard for unknown hash", () => {
      expect(parseHash("#/unknown/route")).toEqual({ page: "dashboard" });
    });
  });

  describe("routeToHash", () => {
    it("dashboard -> #/", () => {
      expect(routeToHash({ page: "dashboard" })).toBe("#/");
    });

    it("chat -> #/chat", () => {
      expect(routeToHash({ page: "chat" })).toBe("#/chat");
    });

    it("book -> #/book/{id}", () => {
      expect(routeToHash({ page: "book", bookId: "novel-1" })).toBe("#/book/novel-1");
    });

    it("book-settings -> #/book/{id}/settings", () => {
      expect(routeToHash({ page: "book-settings", bookId: "novel-1" })).toBe("#/book/novel-1/settings");
    });

    it("encodes Chinese bookId", () => {
      const hash = routeToHash({ page: "book", bookId: "九龙城夜行" });
      expect(hash).toContain("#/book/");
      expect(decodeURIComponent(hash)).toContain("九龙城夜行");
    });

    it("book-create -> #/book/new", () => {
      expect(routeToHash({ page: "book-create" })).toBe("#/book/new");
    });

    it("services -> #/services", () => {
      expect(routeToHash({ page: "services" })).toBe("#/services");
    });

    it("project-settings -> #/settings", () => {
      expect(routeToHash({ page: "project-settings" })).toBe("#/settings");
    });

    it("service-detail -> #/services/{id}", () => {
      expect(routeToHash({ page: "service-detail", serviceId: "openai" })).toBe("#/services/openai");
    });

    it("import tab -> #/import/{tab}", () => {
      expect(routeToHash({ page: "import", tab: "chapters" })).toBe("#/import/chapters");
    });

    it("translation -> #/translation", () => {
      expect(routeToHash({ page: "translation" })).toBe("#/translation");
    });

    it("encodes Chinese serviceId", () => {
      const hash = routeToHash({ page: "service-detail", serviceId: "自定义" });
      expect(hash).toContain("#/services/");
      expect(decodeURIComponent(hash)).toContain("自定义");
    });

    it("469 号：daemon/logs 深链补齐后 hash 非空（旧 state-only 语义退役）", () => {
      expect(routeToHash({ page: "daemon" })).toBe("#/daemon");
      expect(routeToHash({ page: "logs" })).toBe("#/logs");
    });
  });
});

describe("play route", () => {
  it("parses #/play/:id", () => {
    expect(parseHash("#/play/my-id")).toEqual({ page: "play", projectId: "my-id" });
  });
  it("round-trips to hash", () => {
    expect(routeToHash({ page: "play", projectId: "my-id" })).toBe("#/play/my-id");
  });
  it("decodes url-encoded ids", () => {
    expect(parseHash("#/play/a%20b")).toEqual({ page: "play", projectId: "a b" });
  });
});

describe("analytics route (460 号)", () => {
  it("parses #/book/:id/analytics", () => {
    expect(parseHash("#/book/b1/analytics")).toEqual({ page: "analytics", bookId: "b1" });
  });
  it("decodes url-encoded book ids", () => {
    expect(parseHash("#/book/%E9%95%9C%E8%8A%B1%E6%B0%B4%E6%9C%88/analytics")).toEqual({
      page: "analytics",
      bookId: "镜花水月",
    });
  });
  it("round-trips to hash", () => {
    expect(routeToHash({ page: "analytics", bookId: "b1" })).toBe("#/book/b1/analytics");
  });
});

describe("PAGE_SPEC 穷举一致性（461 号）", () => {
  it("每个非 null 页面：toHash(sample) → parseHash 往返还原 sample", () => {
    for (const [page, spec] of Object.entries(PAGE_SPEC)) {
      if (!spec) continue;
      // spec 为各页 PageSpecFor<K> 的联合——toHash 参数取交集为 never，
      // 测试侧以 as never 断言（调用方保证 spec 与 sample 同页配对）。
      const hash = spec.toHash(spec.sample as never);
      expect(hash, `页面 ${page} 的 hash 不应为空`).not.toBe("");
      // parseHash 对称性：analytics 类「有写入无解析」缺陷在此被抓住
      expect(parseHash(hash), `页面 ${page} 往返失配`).toEqual(spec.sample);
    }
  });

  it("writable=false 的页面不产可写 URL 语义保持（onboarding 深链解析仍在）", () => {
    // 532 号：radar 升级 writable=true（侧栏切换后 URL 停留旧路由、刷新即丢页，
    // 与 469 号 doctor/genres/logs 深链补齐同款缺陷清零），不可写清单仅余 onboarding。
    for (const page of ["onboarding"] as const) {
      const spec = PAGE_SPEC[page];
      expect(spec).not.toBeNull();
      expect(spec!.writable).toBe(false);
      expect(parseHash(spec!.toHash(spec!.sample as never)).page).toBe(page);
    }
    // radar 反向锁定：必须可写（写入后 hashchange 幂等回落 radar，无回环）。
    expect(PAGE_SPEC.radar!.writable).toBe(true);
  });

  it("所有页面的键集合与 HashRoute 穷举一致（缺一编译失败）", () => {
    const expected = [
      "dashboard", "chat", "book", "book-settings", "book-timeline", "analytics",
      "book-create", "chapter", "services", "onboarding", "project-settings",
      "translation", "import", "service-detail", "play", "film", "flow",
      "film-author", "film-studio", "radar", "doctor", "genres", "style",
      "truth", "daemon", "logs",
    ];
    expect(Object.keys(PAGE_SPEC).sort()).toEqual([...expected].sort());
  });
});
