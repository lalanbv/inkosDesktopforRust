import { z } from "zod";

/**
 * 质量债务账本契约（G3/336 号，Phase B 批次一；ANWA A3/A4 八级判定 + 双治理方案采纳）。
 *
 * 既有审查循环是"分数+修复"二元（85 分阈值 + 1 轮上限）：局部问题 ≠ 全局失败，
 * 批量场景需要一个**分级判定 + 债务账本 + 每书治理方案**的第三层：
 *
 * - **八级判定** `resolveQualityVerdict`：确定性决策表（golden 向量锁死，双端差分）——
 *   从审查结果 + 义务缺口 + 治理方案推导八级之一；
 * - **每书治理方案** `resolveGovernancePolicy`：completion-first（默认，局部问题降级为
 *   债务继续）/ quality-first（停在已保存章节边界等待显式恢复）；book 级覆盖 project 级；
 * - **债务记录** `QualityDebt` + 状态机（open → deferred → resolved，可 reopen）。
 *
 * 双端：`engine-rs/src/models/quality_governance.rs` 1:1 镜像；共享向量
 * `src/__tests__/golden/quality-verdict-vectors.json`（TS 断言
 * `golden-quality-verdict.test.ts`；Rust 差分 `tests/golden_quality_verdict_diff.rs`）。
 */

// ── 八级判定 ──

export const QUALITY_VERDICTS = [
  "accepted",
  "continue-with-warning",
  "local-patch-plan",
  "patchable-obligation-gap",
  "draft-obligation-unmet",
  "defer-and-continue",
  "replan-required",
  "stop-for-replan",
] as const;

export type QualityVerdict = (typeof QUALITY_VERDICTS)[number];

export const QualityVerdictSchema = z.enum(QUALITY_VERDICTS);

/** 每书治理方案（A4：执行器不写死分支，治理决定结构化）。 */
export const GOVERNANCE_POLICIES = ["completion-first", "quality-first"] as const;
export const GovernancePolicySchema = z.enum(GOVERNANCE_POLICIES);
export type GovernancePolicy = z.infer<typeof GovernancePolicySchema>;

export const DEFAULT_GOVERNANCE_POLICY: GovernancePolicy = "completion-first";
export const DEFAULT_MAX_CONSECUTIVE_DEBTS = 3;

/** 判定输入（从 AuditResult / 审查循环产物推导，见 counting helper）。 */
export interface QualityVerdictInput {
  /** 审查整体通过（分数 ≥ 阈值且无 critical）。 */
  readonly passed: boolean;
  readonly warningCount: number;
  /** repairScope=local 的未解问题数。 */
  readonly localCount: number;
  /** repairScope=structural 的未解问题数。 */
  readonly structuralCount: number;
  /** 未兑义务数（义务类 critical 问题；伏笔/承诺账本口径）。 */
  readonly unmetObligations: number;
  /** 本判定之前已连续记债的章数。 */
  readonly consecutiveDebts: number;
  readonly policy: GovernancePolicy;
  readonly maxConsecutiveDebts: number;
}

export const QualityVerdictInputSchema = z.object({
  passed: z.boolean(),
  warningCount: z.number().int().nonnegative(),
  localCount: z.number().int().nonnegative(),
  structuralCount: z.number().int().nonnegative(),
  unmetObligations: z.number().int().nonnegative(),
  consecutiveDebts: z.number().int().nonnegative(),
  policy: GovernancePolicySchema,
  maxConsecutiveDebts: z.number().int().min(1),
});

/**
 * 八级判定决策表（顺序即优先级，golden 向量锁死）：
 * 1. 过线但有未兑义务            → patchable-obligation-gap（记债 + 局部补）
 * 2. 过线但有 warning            → continue-with-warning
 * 3. 过线                        → accepted
 * 4. 未过线 + 义务未兑 + 结构级  → draft-obligation-unmet（重写）
 * 5. 未过线 + 义务未兑           → patchable-obligation-gap
 * 6. 未过线 + 纯局部问题         → local-patch-plan（进入修复循环）
 * 7. 未过线 + quality-first      → stop-for-replan（停在章节边界）
 * 8. 未过线 + 连续债务将达上限   → replan-required（提示重规划）
 * 9. 其余                        → defer-and-continue（记债继续）
 */
export function resolveQualityVerdict(input: QualityVerdictInput): QualityVerdict {
  const {
    passed,
    warningCount,
    localCount,
    structuralCount,
    unmetObligations,
    consecutiveDebts,
    policy,
    maxConsecutiveDebts,
  } = QualityVerdictInputSchema.parse(input);

  if (passed) {
    if (unmetObligations > 0) return "patchable-obligation-gap";
    if (warningCount > 0) return "continue-with-warning";
    return "accepted";
  }

  if (unmetObligations > 0) {
    return structuralCount > 0 ? "draft-obligation-unmet" : "patchable-obligation-gap";
  }
  if (structuralCount === 0 && localCount > 0) return "local-patch-plan";
  if (policy === "quality-first") return "stop-for-replan";
  if (consecutiveDebts + 1 >= maxConsecutiveDebts) return "replan-required";
  return "defer-and-continue";
}

// ── 每书治理方案解析 ──

export const GovernanceConfigSchema = z.object({
  policy: GovernancePolicySchema.optional(),
  maxConsecutiveDebts: z.number().int().min(1).max(20).optional(),
});
export type GovernanceConfig = z.infer<typeof GovernanceConfigSchema>;

/** book 级覆盖 project 级覆盖缺省（completion-first / 上限 3）。 */
export function resolveGovernancePolicy(
  book?: GovernanceConfig,
  project?: GovernanceConfig,
): { policy: GovernancePolicy; maxConsecutiveDebts: number } {
  const policy = book?.policy ?? project?.policy ?? DEFAULT_GOVERNANCE_POLICY;
  const maxConsecutiveDebts =
    book?.maxConsecutiveDebts ?? project?.maxConsecutiveDebts ?? DEFAULT_MAX_CONSECUTIVE_DEBTS;
  return { policy, maxConsecutiveDebts };
}

// ── 债务记录与状态机 ──

export const DEBT_STATUSES = ["open", "deferred", "resolved"] as const;
export const DebtStatusSchema = z.enum(DEBT_STATUSES);
export type DebtStatus = (typeof DEBT_STATUSES)[number];

export const DEBT_ACTIONS = ["defer", "resolve", "reopen"] as const;
export const DebtActionSchema = z.enum(DEBT_ACTIONS);
export type DebtAction = (typeof DEBT_ACTIONS)[number];

export interface QualityDebt {
  readonly id: string;
  readonly bookId: string;
  /** 产生债务的章节号。 */
  readonly chapter: number;
  /** 问题类别（对齐 AuditIssue.category）。 */
  readonly issueCategory: string;
  readonly severity: "critical" | "warning" | "info";
  readonly status: DebtStatus;
  readonly createdAt: string;
  /** 跟进备注（降级理由 / 解决方式）。 */
  readonly followUpNote?: string;
}

/**
 * 债务状态机：open →(defer)→ deferred →(resolve)→ resolved；
 * reopen 可从 deferred/resolved 回 open。非法转换返回 valid:false（不抛错，跨端简单）。
 */
export function transitionDebtStatus(
  current: DebtStatus,
  action: DebtAction,
): { status: DebtStatus; valid: boolean } {
  switch (action) {
    case "defer":
      return current === "open" ? { status: "deferred", valid: true } : { status: current, valid: false };
    case "resolve":
      return current === "resolved"
        ? { status: current, valid: false }
        : { status: "resolved", valid: true };
    case "reopen":
      return current === "open" ? { status: current, valid: false } : { status: "open", valid: true };
  }
}

/** 判定 → 是否记债（债务账本的入账口径）。 */
export function verdictCreatesDebt(verdict: QualityVerdict): boolean {
  return (
    verdict === "patchable-obligation-gap"
    || verdict === "draft-obligation-unmet"
    || verdict === "defer-and-continue"
    || verdict === "replan-required"
  );
}

/** 判定 → 是否继续写下一章（false = 停在已保存章节边界）。 */
export function verdictContinuesPipeline(verdict: QualityVerdict): boolean {
  return (
    verdict === "accepted"
    || verdict === "continue-with-warning"
    || verdict === "local-patch-plan"
    || verdict === "patchable-obligation-gap"
    || verdict === "defer-and-continue"
  );
}
