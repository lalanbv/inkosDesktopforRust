//! 质量债务账本契约（G3/336 号，Phase B 批次一）。
//!
//! TS 真源：`packages/core/src/models/quality-governance.ts`；共享向量：
//! `packages/core/src/__tests__/golden/quality-verdict-vectors.json`
//! （差分测试 `tests/golden_quality_verdict_diff.rs`）。
//!
//! 八级判定决策表（顺序即优先级）+ 每书治理方案 + 债务状态机，
//! 语义详见 TS 模块 doc。

use serde::Deserialize;
use serde::Serialize;

pub const QUALITY_VERDICTS: [&str; 8] = [
    "accepted",
    "continue-with-warning",
    "local-patch-plan",
    "patchable-obligation-gap",
    "draft-obligation-unmet",
    "defer-and-continue",
    "replan-required",
    "stop-for-replan",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum QualityVerdict {
    Accepted,
    ContinueWithWarning,
    LocalPatchPlan,
    PatchableObligationGap,
    DraftObligationUnmet,
    DeferAndContinue,
    ReplanRequired,
    StopForReplan,
}

impl QualityVerdict {
    pub fn as_str(self) -> &'static str {
        match self {
            QualityVerdict::Accepted => "accepted",
            QualityVerdict::ContinueWithWarning => "continue-with-warning",
            QualityVerdict::LocalPatchPlan => "local-patch-plan",
            QualityVerdict::PatchableObligationGap => "patchable-obligation-gap",
            QualityVerdict::DraftObligationUnmet => "draft-obligation-unmet",
            QualityVerdict::DeferAndContinue => "defer-and-continue",
            QualityVerdict::ReplanRequired => "replan-required",
            QualityVerdict::StopForReplan => "stop-for-replan",
        }
    }

    /// 判定 → 是否记债（债务账本的入账口径）。
    pub fn creates_debt(self) -> bool {
        matches!(
            self,
            QualityVerdict::PatchableObligationGap
                | QualityVerdict::DraftObligationUnmet
                | QualityVerdict::DeferAndContinue
                | QualityVerdict::ReplanRequired
        )
    }

    /// 判定 → 是否继续写下一章（false = 停在已保存章节边界）。
    pub fn continues_pipeline(self) -> bool {
        matches!(
            self,
            QualityVerdict::Accepted
                | QualityVerdict::ContinueWithWarning
                | QualityVerdict::LocalPatchPlan
                | QualityVerdict::PatchableObligationGap
                | QualityVerdict::DeferAndContinue
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GovernancePolicy {
    CompletionFirst,
    QualityFirst,
}

impl GovernancePolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            GovernancePolicy::CompletionFirst => "completion-first",
            GovernancePolicy::QualityFirst => "quality-first",
        }
    }
}

pub const DEFAULT_GOVERNANCE_POLICY: GovernancePolicy = GovernancePolicy::CompletionFirst;
pub const DEFAULT_MAX_CONSECUTIVE_DEBTS: u32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QualityVerdictInput {
    pub passed: bool,
    pub warning_count: u32,
    pub local_count: u32,
    pub structural_count: u32,
    pub unmet_obligations: u32,
    pub consecutive_debts: u32,
    pub policy: GovernancePolicy,
    pub max_consecutive_debts: u32,
}

/// 八级判定决策表（顺序即优先级，golden 向量锁死）。
pub fn resolve_quality_verdict(input: &QualityVerdictInput) -> QualityVerdict {
    if input.passed {
        if input.unmet_obligations > 0 {
            return QualityVerdict::PatchableObligationGap;
        }
        if input.warning_count > 0 {
            return QualityVerdict::ContinueWithWarning;
        }
        return QualityVerdict::Accepted;
    }
    if input.unmet_obligations > 0 {
        return if input.structural_count > 0 {
            QualityVerdict::DraftObligationUnmet
        } else {
            QualityVerdict::PatchableObligationGap
        };
    }
    if input.structural_count == 0 && input.local_count > 0 {
        return QualityVerdict::LocalPatchPlan;
    }
    if input.policy == GovernancePolicy::QualityFirst {
        return QualityVerdict::StopForReplan;
    }
    if input.consecutive_debts + 1 >= input.max_consecutive_debts {
        return QualityVerdict::ReplanRequired;
    }
    QualityVerdict::DeferAndContinue
}

/// 每书治理配置（book 级覆盖 project 级）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GovernanceConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy: Option<GovernancePolicy>,
    #[serde(default, rename = "maxConsecutiveDebts", skip_serializing_if = "Option::is_none")]
    pub max_consecutive_debts: Option<u32>,
}

/// book 级覆盖 project 级覆盖缺省（completion-first / 上限 3）。
pub fn resolve_governance_policy(
    book: Option<&GovernanceConfig>,
    project: Option<&GovernanceConfig>,
) -> (GovernancePolicy, u32) {
    let policy = book
        .and_then(|b| b.policy)
        .or_else(|| project.and_then(|p| p.policy))
        .unwrap_or(DEFAULT_GOVERNANCE_POLICY);
    let max_consecutive_debts = book
        .and_then(|b| b.max_consecutive_debts)
        .or_else(|| project.and_then(|p| p.max_consecutive_debts))
        .unwrap_or(DEFAULT_MAX_CONSECUTIVE_DEBTS);
    (policy, max_consecutive_debts)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DebtStatus {
    Open,
    Deferred,
    Resolved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DebtAction {
    Defer,
    Resolve,
    Reopen,
}

/// 债务状态机：非法转换返回 valid:false（不抛错，跨端简单）。
pub fn transition_debt_status(current: DebtStatus, action: DebtAction) -> (DebtStatus, bool) {
    match action {
        DebtAction::Defer => match current {
            DebtStatus::Open => (DebtStatus::Deferred, true),
            other => (other, false),
        },
        DebtAction::Resolve => match current {
            DebtStatus::Resolved => (DebtStatus::Resolved, false),
            _ => (DebtStatus::Resolved, true),
        },
        DebtAction::Reopen => match current {
            DebtStatus::Open => (DebtStatus::Open, false),
            _ => (DebtStatus::Open, true),
        },
    }
}

/// 机器可读契约（双端 golden 锁形状）。
#[derive(Debug, Clone, Serialize)]
pub struct QualityGovernanceContract {
    pub verdicts: Vec<&'static str>,
    pub policies: Vec<&'static str>,
    #[serde(rename = "debtStatuses")]
    pub debt_statuses: Vec<&'static str>,
    #[serde(rename = "debtActions")]
    pub debt_actions: Vec<&'static str>,
    pub defaults: QualityGovernanceDefaults,
}

#[derive(Debug, Clone, Serialize)]
pub struct QualityGovernanceDefaults {
    pub policy: &'static str,
    #[serde(rename = "maxConsecutiveDebts")]
    pub max_consecutive_debts: u32,
}

pub fn quality_governance_contract() -> QualityGovernanceContract {
    QualityGovernanceContract {
        verdicts: QUALITY_VERDICTS.to_vec(),
        policies: vec![
            GovernancePolicy::CompletionFirst.as_str(),
            GovernancePolicy::QualityFirst.as_str(),
        ],
        debt_statuses: vec!["open", "deferred", "resolved"],
        debt_actions: vec!["defer", "resolve", "reopen"],
        defaults: QualityGovernanceDefaults {
            policy: DEFAULT_GOVERNANCE_POLICY.as_str(),
            max_consecutive_debts: DEFAULT_MAX_CONSECUTIVE_DEBTS,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decision_table_endpoints() {
        let base = QualityVerdictInput {
            passed: true,
            warning_count: 0,
            local_count: 0,
            structural_count: 0,
            unmet_obligations: 0,
            consecutive_debts: 0,
            policy: GovernancePolicy::CompletionFirst,
            max_consecutive_debts: 3,
        };
        assert_eq!(resolve_quality_verdict(&base), QualityVerdict::Accepted);
        assert_eq!(
            resolve_quality_verdict(&QualityVerdictInput { warning_count: 2, ..base }),
            QualityVerdict::ContinueWithWarning
        );
        assert_eq!(
            resolve_quality_verdict(&QualityVerdictInput { passed: false, consecutive_debts: 2, ..base }),
            QualityVerdict::ReplanRequired
        );
    }
}
