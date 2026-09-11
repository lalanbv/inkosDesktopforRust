//! G3/336 号：质量债务账本契约 golden 断言（Phase B 批次一）。
//!
//! 唯一事实源 = `golden/quality-verdict-vectors.json`；Rust 侧
//! `engine-rs/tests/golden_quality_verdict_diff.rs` 读同一文件差分。
//! 四组断言：八级判定决策表、每书治理解析、债务状态机、契约形状；
//! 另验证判定 → 记债/继续管线的派生口径。
import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
  DEFAULT_GOVERNANCE_POLICY,
  DEFAULT_MAX_CONSECUTIVE_DEBTS,
  GOVERNANCE_POLICIES,
  QUALITY_VERDICTS,
  DEBT_ACTIONS,
  DEBT_STATUSES,
  resolveGovernancePolicy,
  resolveQualityVerdict,
  transitionDebtStatus,
  verdictContinuesPipeline,
  verdictCreatesDebt,
  type GovernanceConfig,
  type QualityVerdictInput,
} from "../models/quality-governance.js";

const here = dirname(fileURLToPath(import.meta.url));
const vectors = JSON.parse(
  readFileSync(resolve(here, "golden/quality-verdict-vectors.json"), "utf-8"),
) as {
  verdict: Array<{ name: string; input: QualityVerdictInput; expected: string }>;
  policy: Array<{ name: string; input: { book: GovernanceConfig | null; project: GovernanceConfig | null }; expected: { policy: string; maxConsecutiveDebts: number } }>;
  transition: Array<{ name: string; current: string; action: string; expected: string; valid: boolean }>;
  contract: unknown;
};

describe("quality verdict contract (G3)", () => {
  it("resolves every verdict per shared decision-table vectors", () => {
    for (const vector of vectors.verdict) {
      expect(resolveQualityVerdict(vector.input), vector.name).toBe(vector.expected);
    }
    // 决策表每条规则至少被两个向量覆盖。
    const covered = new Set(vectors.verdict.map((v) => v.expected));
    for (const verdict of QUALITY_VERDICTS) {
      expect(covered.has(verdict), `verdict ${verdict} uncovered`).toBe(true);
    }
  });

  it("derives ledger/pipeline semantics from verdicts", () => {
    // 记债口径：四种降级/义务判定入账；accepted/warning/patch/stop 不入账。
    expect(verdictCreatesDebt("patchable-obligation-gap")).toBe(true);
    expect(verdictCreatesDebt("draft-obligation-unmet")).toBe(true);
    expect(verdictCreatesDebt("defer-and-continue")).toBe(true);
    expect(verdictCreatesDebt("replan-required")).toBe(true);
    expect(verdictCreatesDebt("accepted")).toBe(false);
    expect(verdictCreatesDebt("continue-with-warning")).toBe(false);
    expect(verdictCreatesDebt("local-patch-plan")).toBe(false);
    expect(verdictCreatesDebt("stop-for-replan")).toBe(false);
    // 管线继续口径：stop/replan/unmet 停在章节边界。
    expect(verdictContinuesPipeline("stop-for-replan")).toBe(false);
    expect(verdictContinuesPipeline("replan-required")).toBe(false);
    expect(verdictContinuesPipeline("draft-obligation-unmet")).toBe(false);
    expect(verdictContinuesPipeline("defer-and-continue")).toBe(true);
  });

  it("resolves governance policy with book-over-project precedence", () => {
    for (const vector of vectors.policy) {
      const got = resolveGovernancePolicy(vector.input.book ?? undefined, vector.input.project ?? undefined);
      expect(got, vector.name).toEqual(vector.expected);
    }
    expect(DEFAULT_GOVERNANCE_POLICY).toBe("completion-first");
    expect(DEFAULT_MAX_CONSECUTIVE_DEBTS).toBe(3);
  });

  it("validates every debt state-machine transition", () => {
    for (const vector of vectors.transition) {
      const got = transitionDebtStatus(vector.current as never, vector.action as never);
      expect(got, vector.name).toEqual({ status: vector.expected, valid: vector.valid });
    }
    expect(DEBT_STATUSES).toHaveLength(3);
    expect(DEBT_ACTIONS).toHaveLength(3);
  });

  it("freezes the machine-readable contract shape", () => {
    expect(vectors.contract).toEqual({
      verdicts: [...QUALITY_VERDICTS],
      policies: [...GOVERNANCE_POLICIES],
      debtStatuses: [...DEBT_STATUSES],
      debtActions: [...DEBT_ACTIONS],
      defaults: { policy: DEFAULT_GOVERNANCE_POLICY, maxConsecutiveDebts: DEFAULT_MAX_CONSECUTIVE_DEBTS },
    });
  });
});
