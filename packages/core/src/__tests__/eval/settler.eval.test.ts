//! R8 settler 行为评测（369 号）。
//!
//! 漂移锚点：TAG 输出模板（RUNTIME_STATE_DELTA JSON/规则 8 章号纪律/
//! TENSION_METRICS 张力评分节/铁律节）；回放：delta 解析（含张力分注入
//! 所需的 chapterSummary）+ legacy 回退解析。
import { describe, expect, it } from "vitest";
import { buildSettlerSystemPrompt } from "../../agents/settler-prompts.js";
import { parseSettlerDeltaOutput } from "../../agents/settler-delta-parser.js";
import { parseSettlementOutput } from "../../agents/settler-parser.js";
import { parseTensionMetrics } from "../../utils/tension-curve.js";
import {
  makeBookFixture,
  SAMPLE_SETTLEMENT_OUTPUT,
} from "./eval-fixtures.js";
import type { GenreProfile } from "../../models/genre-profile.js";
import type { BookConfig } from "../../models/book.js";

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

const baseBook: BookConfig = {
  id: "eval-book",
  title: "评测之书",
  genre: "xuanhuan",
  platform: "qidian",
  chapterWordCount: 800,
  targetChapters: 60,
  status: "active",
  language: "zh",
  createdAt: "2026-09-13T00:00:00.000Z",
  updatedAt: "2026-09-13T00:00:00.000Z",
} as unknown as BookConfig;

describe("eval: settler (R8)", () => {
  it("system prompt keeps TAG discipline and truth-file anchors", async () => {
    await makeBookFixture("settler-drift");
    const prompt = buildSettlerSystemPrompt(baseBook, baseGp, null, "zh");
    expect(prompt).toContain("状态追踪分析师");
    expect(prompt).toContain("=== RUNTIME_STATE_DELTA ===");
    expect(prompt).toContain('"hookOps"');
    expect(prompt).toContain("TENSION_METRICS");
    expect(prompt).toContain("只记录正文中实际发生的事");
    expect(prompt).toContain("chapterSummary.chapter 必须等于当前章节号");
  });

  it("system prompt keeps chapterSummary template and tension metrics section", () => {
    const prompt = buildSettlerSystemPrompt(baseBook, baseGp, null, "zh");
    expect(prompt).toContain('"chapterSummary": {');
    expect(prompt).toContain('"chapterType": "推进章"');
    expect(prompt).toContain("=== TENSION_METRICS ===");
    expect(prompt).toContain("conflictLevel: 7");
    expect(prompt).toContain("revealLevel: 6");
  });

  it("replays the settlement fixture into a structured delta", () => {
    const output = parseSettlerDeltaOutput(SAMPLE_SETTLEMENT_OUTPUT);
    expect(output.runtimeStateDelta.chapter).toBe(12);
    const summary = output.runtimeStateDelta.chapterSummary;
    expect(summary).toBeDefined();
    expect(summary!.title).toBe("越界");
    expect(output.postSettlement).toContain("林动越界");
    // 张力评分节同步解析（R2/358 契约）。
    const metrics = parseTensionMetrics(SAMPLE_SETTLEMENT_OUTPUT);
    expect(metrics).toEqual({ conflictLevel: 7, revealLevel: 6 });
  });

  it("legacy settlement output still parses as fallback", async () => {
    const { bookDir } = await makeBookFixture("settler-legacy");
    void bookDir;
    const legacy = [
      "=== POST_SETTLEMENT ===",
      "旧版结算说明。",
      "=== UPDATED_STATE ===",
      "状态卡正文",
      "=== UPDATED_HOOKS ===",
      "- H01 推进",
      "=== CHAPTER_SUMMARY ===",
      "第 3 章：旧版摘要",
    ].join("\n");
    const settlement = parseSettlementOutput(legacy, baseGp);
    expect(settlement.postSettlement).toContain("旧版结算说明");
    expect(settlement.updatedState).toBe("状态卡正文");
    expect(settlement.chapterSummary).toContain("旧版摘要");
    // legacy 输出无张力节。
    expect(parseTensionMetrics(legacy)).toBeUndefined();
  });
});
