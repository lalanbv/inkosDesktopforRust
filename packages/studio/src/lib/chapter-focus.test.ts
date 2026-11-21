//! R47/1640 号：章阅读器一次性焦点传递纯函数语义。
//! 取走即清（防重放）+ 书/章不匹配丢弃（深链无此语义，刷新即失焦）。
import { describe, expect, it } from "vitest";
import { setChapterFocus, takeChapterFocus } from "./chapter-focus";

describe("chapter-focus（R47/1640 号）", () => {
  it("匹配书+章时返回查询词且取走即清", () => {
    setChapterFocus("b1", 3, "碎镜之主");
    expect(takeChapterFocus("b1", 3)).toBe("碎镜之主");
    expect(takeChapterFocus("b1", 3)).toBeNull();
  });

  it("书或章不匹配时丢弃且消费 pending（防陈旧焦点重放）", () => {
    setChapterFocus("b1", 3, "碎镜之主");
    expect(takeChapterFocus("b1", 4)).toBeNull();
    expect(takeChapterFocus("b1", 3)).toBeNull();
    setChapterFocus("b2", 1, "灵气");
    expect(takeChapterFocus("b1", 1)).toBeNull();
    // 不匹配消费已清空 pending——后续即便书+章匹配也拿不到（防重放语义）。
    expect(takeChapterFocus("b2", 1)).toBeNull();
  });
});
