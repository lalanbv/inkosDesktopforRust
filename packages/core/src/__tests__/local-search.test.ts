import { describe, expect, it } from "vitest";
import {
  LocalSearchIndex,
  splitMarkdownForSearch,
  tokenizeSearchText,
  __takeNormalizedDocumentCount,
} from "../retrieval/local-search.js";

describe("LocalSearchIndex", () => {
  it("retrieves Chinese and English evidence with FTS5 BM25", () => {
    const index = new LocalSearchIndex(":memory:");
    try {
      index.replaceScope("story", [
        {
          id: "mentor-debt",
          scope: "story",
          kind: "hook",
          source: "pending_hooks.md#mentor-debt",
          title: "师债回响 Mentor Debt",
          body: "林月必须核对誓令碎片，继续追查失踪导师留下的债务。",
        },
        {
          id: "guild-route",
          scope: "story",
          kind: "hook",
          source: "pending_hooks.md#guild-route",
          title: "商会路线 Guild Route",
          body: "商会提出一条安全但偏离主线的运输路线。",
        },
      ]);

      expect(index.search("导师 誓令 师债", { scope: "story" })[0]?.id).toBe("mentor-debt");
      expect(index.search("mentor debt oath", { scope: "story" })[0]?.id).toBe("mentor-debt");
    } finally {
      index.close();
    }
  });

  it("replaces stale projection rows instead of retaining old evidence", () => {
    const index = new LocalSearchIndex(":memory:");
    try {
      index.replaceScope("materials", [{
        id: "old",
        scope: "materials",
        kind: "reference",
        source: "old.md",
        title: "旧账",
        body: "0607 赔偿款",
      }]);
      index.replaceScope("materials", [{
        id: "new",
        scope: "materials",
        kind: "reference",
        source: "new.md",
        title: "新账",
        body: "0812 签收单",
      }]);

      expect(index.search("赔偿款", { scope: "materials" })).toEqual([]);
      expect(index.search("签收单", { scope: "materials" })[0]?.id).toBe("new");
    } finally {
      index.close();
    }
  });

  it("keeps identical document ids isolated across scopes", () => {
    const index = new LocalSearchIndex(":memory:");
    try {
      index.replaceScope("story", [{
        id: "shared",
        scope: "story",
        kind: "hook",
        source: "pending_hooks.md#shared",
        title: "失踪导师",
        body: "导师留下誓令碎片。",
      }]);
      index.replaceScope("materials", [{
        id: "shared",
        scope: "materials",
        kind: "reference",
        source: "sample.md#shared",
        title: "赔偿账页",
        body: "账页记录三笔赔偿款。",
      }]);

      expect(index.search("导师", { scope: "story" })[0]?.source).toBe("pending_hooks.md#shared");
      expect(index.search("赔偿", { scope: "materials" })[0]?.source).toBe("sample.md#shared");
    } finally {
      index.close();
    }
  });

  it("segments Markdown without truncating the selected paragraphs", () => {
    const markdown = "# 证据\n\n赔偿款被拆成三笔。\n\n## 人物\n\n司机拒绝签字。";
    const segments = splitMarkdownForSearch(markdown);

    expect(segments.map((segment) => segment.body)).toEqual([
      "赔偿款被拆成三笔。",
      "司机拒绝签字。",
    ]);
    expect(markdown.slice(segments[1]!.charStart, segments[1]!.charEnd)).toBe("司机拒绝签字。");
    expect(tokenizeSearchText("师债 mentor-debt")).toEqual(expect.arrayContaining(["师债", "mentor-debt"]));
  });

  // 未变化的文档不再被分词：内容哈希在分词之前算，只有哈希变了的文档才走
  // normalizeDocument。此前每次 replaceScope 都把全部文档重新分词——热路径
  // （磁盘投影已建）单次检索 = 整本书重分词。产品引擎同形，两腿同改。
  it("tokenizes only changed documents when a scope is replaced", () => {
    const index = new LocalSearchIndex(":memory:");
    const doc = (id: string, body: string) => ({
      id, scope: "story", kind: "hook", source: `${id}.md`, title: id, body,
    });
    try {
      const docs = [doc("d1", "开篇要建立核心冲突。"), doc("d2", "对白要短，避免书面语。"), doc("d3", "伏笔要兑现。")];
      __takeNormalizedDocumentCount();
      index.replaceScope("story", docs);
      expect(__takeNormalizedDocumentCount()).toBe(3);

      // 同一批文档原样再来一次（热路径）：零分词。
      index.replaceScope("story", docs);
      expect(__takeNormalizedDocumentCount()).toBe(0);

      // 只改一篇 + 新增一篇、顺序打乱：只分词这两篇。
      const changed = [doc("d3", "伏笔要兑现。"), doc("d2", "对白要再短一点。"), doc("d1", "开篇要建立核心冲突。"), doc("d4", "节奏张弛有度。")];
      index.replaceScope("story", changed);
      expect(__takeNormalizedDocumentCount()).toBe(2);

      // 元数据变了也算变（哈希覆盖 metadata）。
      index.replaceScope("story", changed.map((entry, position) => (position === 0 ? { ...entry, metadata: { note: "x" } } : entry)));
      expect(__takeNormalizedDocumentCount()).toBe(1);

      // 检索面不受影响：改过的正文可检索到，旧正文检索不到。
      expect(index.search("再短一点", { scope: "story" }).map((hit) => hit.id)).toEqual(["d2"]);
      expect(index.search("避免书面语", { scope: "story" })).toEqual([]);
    } finally {
      index.close();
    }
  });
});
