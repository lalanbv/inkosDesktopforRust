//! R17 评测扩容：planner + director agent（复用 R8 fixtures/锚点基建）。
import { describe, expect, it } from "vitest";
import { parseMemo } from "../../utils/chapter-memo-parser.js";
import { parseDirectionCandidates } from "../../models/director.js";
import type { GenreProfile } from "../../models/genre-profile.js";
import { SAMPLE_CHAPTER } from "./eval-fixtures.js";

const baseGp: GenreProfile = {
  name: "玄幻修真",
  id: "xuanhuan",
  language: "zh",
  chapterTypes: ["推进章"],
  fatigueWords: [],
  numericalSystem: false,
  powerScaling: false,
  eraResearch: false,
  pacingRule: "",
  satisfactionTypes: [],
  auditDimensions: [],
};

describe("eval: planner + director (R17 扩容)", () => {
  it("planner memo fixture replays into structured memo", () => {
    const memoMarkdown = [
      "## 本章目标",
      "越界：林动踏出青镇边界。",
      "",
      "## 场景与篇幅预算",
      "废墟边缘 → 边界外，全章约 800 字。",
      "",
      "## 当前任务",
      "林动踏出青镇边界，怀表在他掌心重新走动，巡逻队的犬吠逼近。",
      "",
      "## 读者此刻在等什么",
      "读者在等怀表停摆之谜的第一条线索浮出水面。",
      "",
      "## 该兑现的 / 暂不掀的",
      "兑现迈出边界的决心；暂不掀老陈的知情身份。",
      "",
      "## 日常/过渡承担什么任务",
      "老陈深夜的劝阻并非胆怯，而是补足两人相依为命、彼此隐瞒的关系底色。",
      "",
      "## 关键抉择过三连问",
      "踏线是林动自己选的：真相值得被追，代价是被巡逻队盯上并暴露行踪。",
      "",
      "## 章尾必须发生的改变",
      "怀表指针动了，边界外的世界正式登场，父亲失踪案重新有了第一条活线索。",
      "",
      "## 本章 hook 账",
      "- mentor-debt：怀表复走即为推进，老陈的沉默留作下一章兑现。",
      "",
      "## 不要做",
      "- 不写巡逻队的视角",
    ].join("\n");
    const memo = parseMemo(memoMarkdown, 12, false);
    expect(memo.goal).toContain("越界");
    expect(memo.body).toContain("林动踏出青镇边界");
  });

  it("director candidates fixture replays with dedupe and sort", () => {
    const response = JSON.stringify({
      directions: [
        { id: "d1", title: "怀表遗训", hook: "父亲留下的表会走", genre: "玄幻", synopsis: "以怀表为引的寻父路。", differentiator: "物件驱动", confidence: 0.7 },
        { id: "d2", title: "废墟孤证", hook: "边界上的最后一块碑", genre: "玄幻", synopsis: "碑文指向父亲的去向。", differentiator: "悬疑驱动", confidence: 0.9 },
        { id: "d3", title: "怀表遗训", hook: "重复标题", genre: "玄幻", synopsis: "重复。", differentiator: "", confidence: 0.6 },
      ],
    });
    const got = parseDirectionCandidates(response, ["旧书"]);
    // 379 契约：仅 excludeTitles 剔除，重复标题不去重；confidence 降序稳定。
    expect(got.map((direction) => direction.title)).toEqual([
      "废墟孤证",
      "怀表遗训",
      "怀表遗训",
    ]);
    expect(got[0]!.confidence).toBe(0.9);
  });

  it("genre profile fixture keeps audit dimensions contract", () => {
    expect(baseGp.chapterTypes).toContain("推进章");
  });
});
