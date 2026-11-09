/**
 * 961 号：章节审计回执双形态归一。
 *
 * - Rust 引擎（默认后端）：`POST /books/:id/audit/:chapter` 返回 1.x `AuditResult`
 *   `{ passed, issues, summary, … }`。
 * - TS 2.0 回退端（上游 fb8c8e3e 起）：返回审阅观察 `{ summary, observations }`，
 *   无 passed/issues——上游自己的审计按钮仍读 passed/issues，回退端每次审计都显示
 *   「Audit failed: 0 issues」。
 *
 * 2.0 形态以 `assessment === "issue"` 的观察计问题数（与上游翻译回执「review
 * issue(s)」同口径），零问题即通过。
 */
export interface AuditOutcome {
  readonly passed: boolean;
  readonly issueCount: number;
}

export function summarizeAuditResult(raw: unknown): AuditOutcome {
  const result = (raw && typeof raw === "object" ? raw : {}) as {
    readonly passed?: unknown;
    readonly issues?: unknown;
    readonly observations?: unknown;
  };
  if (Array.isArray(result.observations)) {
    const issueCount = result.observations.filter(
      (observation) => (observation as { assessment?: unknown } | null)?.assessment === "issue",
    ).length;
    return { passed: issueCount === 0, issueCount };
  }
  return {
    passed: result.passed === true,
    issueCount: Array.isArray(result.issues) ? result.issues.length : 0,
  };
}

export function formatAuditOutcome(outcome: AuditOutcome): string {
  return outcome.passed ? "Audit passed" : `Audit failed: ${outcome.issueCount} issues`;
}
