import { describe, expect, it } from "vitest";
import { formatAuditOutcome, summarizeAuditResult } from "./audit-outcome";

describe("audit outcome (961)", () => {
  it("Rust 1.x AuditResult 形态：passed/issues 原样（默认后端行为不变）", () => {
    expect(formatAuditOutcome(summarizeAuditResult({ passed: true, issues: [], summary: "ok" }))).toBe("Audit passed");
    expect(formatAuditOutcome(summarizeAuditResult({ passed: false, issues: [{}, {}], summary: "x" }))).toBe(
      "Audit failed: 2 issues",
    );
  });

  it("TS 2.0 审阅观察形态：assessment=issue 计数，零问题即通过", () => {
    const observations = [
      { code: "continuity", assessment: "issue" },
      { code: "pacing", assessment: "observation" },
      { code: "hook", assessment: "resolved" },
      { code: "canon", assessment: "issue" },
      { code: "lens" },
    ];
    expect(summarizeAuditResult({ summary: "2 处问题", observations })).toEqual({ passed: false, issueCount: 2 });
    expect(formatAuditOutcome(summarizeAuditResult({ summary: "2 处问题", observations }))).toBe("Audit failed: 2 issues");
    // 修复前：observations 形态被读成 passed=undefined/issues=undefined → 恒「Audit failed: 0 issues」。
    expect(formatAuditOutcome(summarizeAuditResult({ summary: "无问题", observations: [] }))).toBe("Audit passed");
    expect(
      formatAuditOutcome(summarizeAuditResult({ summary: "仅观察", observations: [{ assessment: "observation" }] })),
    ).toBe("Audit passed");
  });

  it("畸形回执不抛：null/非对象/空观察元素均安全归一", () => {
    expect(summarizeAuditResult(null)).toEqual({ passed: false, issueCount: 0 });
    expect(summarizeAuditResult("oops")).toEqual({ passed: false, issueCount: 0 });
    expect(summarizeAuditResult({ observations: [null, { assessment: "issue" }] })).toEqual({ passed: false, issueCount: 1 });
  });
});
