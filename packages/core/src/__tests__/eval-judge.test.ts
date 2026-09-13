//! R17/385 号：LLM-as-judge 契约单测（离线确定性回放）。
import { describe, expect, it } from "vitest";
import {
  buildJudgePrompt,
  judgeAverage,
  parseJudgeVerdict,
} from "../utils/eval-judge.js";

describe("eval judge contract (R17)", () => {
  it("parses fenced verdicts with clamping", () => {
    const got = parseJudgeVerdict(
      '```json\n{"coherence": 8.26, "tension": 12, "style": -3, "consistency": 7, "verdict": "整体成立，张力偏弱。"}\n```',
    );
    expect(got).toBeDefined();
    expect(got!.coherence).toBe(8.3);
    expect(got!.tension).toBe(10);
    expect(got!.style).toBe(0);
    expect(got!.consistency).toBe(7);
    expect(got!.verdict).toBe("整体成立，张力偏弱。");
    expect(judgeAverage(got!)).toBe(6.3);
  });

  it("returns undefined for garbage and prompt anchors hold", () => {
    expect(parseJudgeVerdict("不是 JSON")).toBeUndefined();
    expect(parseJudgeVerdict('{"verdict": "只有总评"}')).toBeDefined();
    expect(buildJudgePrompt("正文", "zh")).toContain("coherence");
    expect(buildJudgePrompt("正文", "en")).toContain("independent fiction reviewer");
  });
});
