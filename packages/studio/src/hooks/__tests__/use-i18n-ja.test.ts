import { describe, expect, it } from "vitest";
import { resolveString } from "@/hooks/use-i18n";

/**
 * 219 号：ja 界面语言基础设施——补充表命中 / en 回退 / zh-en 原行为不变。
 * 全量 ja 翻译与「界面语言 vs 创作语言」解耦见 219 号缺口分析（方案分级）。
 */
describe("resolveString ja fallback chain", () => {
  it("returns the ja override when the key has a japanese entry", () => {
    expect(resolveString("nav.books", "ja")).toBe("作品");
    expect(resolveString("common.save", "ja")).toBe("保存");
    expect(resolveString("dash.writeNext", "ja")).toBe("次の章を書く");
  });

  it("falls back to the english string for keys without a ja entry", () => {
    expect(resolveString("nav.agentOnline", "ja")).toBe("Agent Online");
    expect(resolveString("book.antiDetect", "ja")).toBe("Anti-Detect");
  });

  it("keeps the original zh/en behaviour untouched", () => {
    expect(resolveString("nav.books", "zh")).toBe("书籍");
    expect(resolveString("nav.books", "en")).toBe("Books");
    expect(resolveString("chapter.stateDegraded", "zh")).toBe("状态降级");
    expect(resolveString("chapter.stateDegraded", "en")).toBe("State Degraded");
  });
});
