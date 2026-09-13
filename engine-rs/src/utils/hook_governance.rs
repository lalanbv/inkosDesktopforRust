//! 伏笔治理（陈旧债收集 + 准入判定 + 处置分类）。
//!
//! 移植自 `packages/core/src/utils/hook-governance.ts`（200 行，纯函数）。
//! 依赖已移植的 [`describe_hook_lifecycle`] + [`HookRecord`] + [`RuntimeStateDelta`]。

use crate::models::runtime_state::{HookPayoffTiming, HookRecord, RuntimeStateDelta};
use crate::utils::hook_lifecycle::describe_hook_lifecycle;
use regex::Regex;
use std::sync::OnceLock;

/// 处置结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookDisposition {
    None,
    Mention,
    Advance,
    Resolve,
    Defer,
}

/// 准入候选（与 TS HookAdmissionCandidate 对齐；payoff_timing 为原始字符串）。
#[derive(Debug, Clone, Default)]
pub struct HookAdmissionCandidate {
    pub hook_type: String,
    pub expected_payoff: Option<String>,
    pub payoff_timing: Option<String>,
    pub notes: Option<String>,
}

/// 准入原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookAdmissionReason {
    Admit,
    MissingType,
    MissingPayoffSignal,
    DuplicateFamily,
}

/// 准入决策。
#[derive(Debug, Clone)]
pub struct HookAdmissionDecision {
    pub admit: bool,
    pub reason: HookAdmissionReason,
    pub matched_hook_id: Option<String>,
}

/// 收集陈旧/逾期伏笔债（按 lastAdvanced/startChapter/hookId 升序）。
pub fn collect_stale_hook_debt(
    hooks: &[HookRecord],
    chapter_number: u32,
    target_chapters: Option<u32>,
    stale_after_chapters: Option<u32>,
) -> Vec<HookRecord> {
    let mut filtered: Vec<HookRecord> = hooks
        .iter()
        .filter(|h| !matches!(h.status, HookStatus_::Resolved | HookStatus_::Deferred))
        .filter(|h| h.start_chapter <= chapter_number)
        .filter(|h| {
            if let Some(stale) = stale_after_chapters {
                return h.last_advanced_chapter <= chapter_number.saturating_sub(stale);
            }
            let lifecycle = describe_hook_lifecycle(
                hook_payoff_timing_str(h.payoff_timing),
                Some(&h.expected_payoff),
                Some(&h.notes),
                h.start_chapter,
                h.last_advanced_chapter,
                &hook_status_str(h.status),
                chapter_number,
                target_chapters,
            );
            lifecycle.stale || lifecycle.overdue
        })
        .cloned()
        .collect();

    filtered.sort_by(|a, b| {
        a.last_advanced_chapter
            .cmp(&b.last_advanced_chapter)
            .then_with(|| a.start_chapter.cmp(&b.start_chapter))
            .then_with(|| a.hook_id.cmp(&b.hook_id))
    });
    filtered
}

/// 评估新伏笔准入：缺失类型/缺失回收信号/与活跃伏笔同族重复则拒绝。
pub fn evaluate_hook_admission(
    candidate: &HookAdmissionCandidate,
    active_hooks: &[HookRecord],
) -> HookAdmissionDecision {
    let candidate_type = normalize_text(&candidate.hook_type);
    if candidate_type.is_empty() {
        return HookAdmissionDecision { admit: false, reason: HookAdmissionReason::MissingType, matched_hook_id: None };
    }

    let mut parts: Vec<&str> = Vec::new();
    for v in [candidate.expected_payoff.as_deref(), candidate.notes.as_deref()].into_iter().flatten() {
        if !v.trim().is_empty() {
            parts.push(v);
        }
    }
    let payoff_signal = parts.join(" ");
    let payoff_signal = payoff_signal.trim();
    if payoff_signal.is_empty() {
        return HookAdmissionDecision { admit: false, reason: HookAdmissionReason::MissingPayoffSignal, matched_hook_id: None };
    }

    let candidate_blob = normalize_text(&[
        candidate.hook_type.as_str(),
        candidate.expected_payoff.as_deref().unwrap_or(""),
        candidate.payoff_timing.as_deref().unwrap_or(""),
        candidate.notes.as_deref().unwrap_or(""),
    ].join(" "));
    let candidate_terms = extract_terms(&candidate_blob);
    let candidate_bigrams = extract_chinese_bigrams(&candidate_blob);

    for hook in active_hooks {
        let active_blob = normalize_text(&[
            hook.hook_type.as_str(),
            hook.expected_payoff.as_str(),
            hook_payoff_timing_str(hook.payoff_timing).unwrap_or(""),
            hook.notes.as_str(),
        ].join(" "));

        if candidate_blob == active_blob {
            return HookAdmissionDecision {
                admit: false,
                reason: HookAdmissionReason::DuplicateFamily,
                matched_hook_id: Some(hook.hook_id.clone()),
            };
        }

        if candidate_type != normalize_text(&hook.hook_type) {
            continue;
        }

        let active_terms = extract_terms(&active_blob);
        let overlap = candidate_terms.iter().filter(|t| active_terms.contains(*t)).count();
        let active_bigrams = extract_chinese_bigrams(&active_blob);
        let chinese_overlap = candidate_bigrams.iter().filter(|t| active_bigrams.contains(*t)).count();
        if overlap >= 2 || chinese_overlap >= 3 {
            return HookAdmissionDecision {
                admit: false,
                reason: HookAdmissionReason::DuplicateFamily,
                matched_hook_id: Some(hook.hook_id.clone()),
            };
        }
    }

    HookAdmissionDecision { admit: true, reason: HookAdmissionReason::Admit, matched_hook_id: None }
}

/// 按 delta.hookOps 分类伏笔处置（defer > resolve > advance > mention > none）。
pub fn classify_hook_disposition(hook_id: &str, delta: &RuntimeStateDelta) -> HookDisposition {
    if delta.hook_ops.defer.iter().any(|h| h == hook_id) {
        return HookDisposition::Defer;
    }
    if delta.hook_ops.resolve.iter().any(|h| h == hook_id) {
        return HookDisposition::Resolve;
    }
    if delta
        .hook_ops
        .upsert
        .iter()
        .any(|h| h.hook_id == hook_id && h.last_advanced_chapter == delta.chapter)
    {
        return HookDisposition::Advance;
    }
    if delta.hook_ops.mention.iter().any(|h| h == hook_id) {
        return HookDisposition::Mention;
    }
    HookDisposition::None
}

// ---- 内部辅助 ----

use crate::models::runtime_state::HookStatus as HookStatus_;

fn normalize_text_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"[^a-z0-9\u{4e00}-\u{9fff}]+").unwrap())
}
fn cjk_term_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"[\u{4e00}-\u{9fff}]{2,6}").unwrap())
}
fn cjk_segment_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"[\u{4e00}-\u{9fff}]+").unwrap())
}

fn normalize_text(value: &str) -> String {
    let lower = value.trim().to_lowercase();
    let cleaned = normalize_text_re().replace_all(&lower, " ");
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn extract_terms(value: &str) -> std::collections::BTreeSet<String> {
    let mut set = std::collections::BTreeSet::new();
    for term in value.split(' ') {
        let term = term.trim();
        if term.len() >= 4 && !is_stop_word(term) {
            set.insert(term.to_string());
        }
    }
    for m in cjk_term_re().find_iter(value) {
        set.insert(m.as_str().to_string());
    }
    set
}

fn extract_chinese_bigrams(value: &str) -> std::collections::BTreeSet<String> {
    let mut set = std::collections::BTreeSet::new();
    for m in cjk_segment_re().find_iter(value) {
        let chars: Vec<char> = m.as_str().chars().collect();
        if chars.len() < 2 {
            continue;
        }
        for w in chars.windows(2) {
            set.insert(format!("{}{}", w[0], w[1]));
        }
    }
    set
}

fn is_stop_word(t: &str) -> bool {
    matches!(t, "that" | "this" | "with" | "from" | "into" | "still" | "just" | "have" | "will" | "reveal")
}

fn hook_status_str(s: HookStatus_) -> String {
    match s {
        HookStatus_::Open => "open",
        HookStatus_::Progressing => "progressing",
        HookStatus_::Deferred => "deferred",
        HookStatus_::Resolved => "resolved",
    }
    .into()
}

fn hook_payoff_timing_str(t: Option<HookPayoffTiming>) -> Option<&'static str> {
    t.map(|x| match x {
        HookPayoffTiming::Immediate => "immediate",
        HookPayoffTiming::NearTerm => "near-term",
        HookPayoffTiming::MidArc => "mid-arc",
        HookPayoffTiming::SlowBurn => "slow-burn",
        HookPayoffTiming::Endgame => "endgame",
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::runtime_state::{HookOps, HookStatus};

    fn hook(id: &str, start: u32, last: u32, status: HookStatus) -> HookRecord {
        HookRecord {
            kind: None,
            hook_id: id.into(),
            start_chapter: start,
            hook_type: "plot".into(),
            status,
            status_raw: String::new(),
            last_advanced_chapter: last,
            expected_payoff: String::new(),
            payoff_timing: None,
            notes: String::new(),
            depends_on: None,
            pays_off_in_arc: None,
            core_hook: None,
            half_life_chapters: None,
            advanced_count: None,
            promoted: None,
        }
    }

    #[test]
    fn stale_debt_filters_resolved_deferred() {
        let hooks = vec![
            hook("a", 1, 1, HookStatus::Open),
            hook("b", 1, 1, HookStatus::Resolved),
            hook("c", 1, 1, HookStatus::Deferred),
        ];
        let debt = collect_stale_hook_debt(&hooks, 5, None, None);
        let ids: Vec<_> = debt.iter().map(|h| h.hook_id.as_str()).collect();
        assert_eq!(ids, vec!["a"]); // immediate 默认 timing，chapter 5 → overdue
    }

    #[test]
    fn stale_after_chapters_override() {
        let hooks = vec![hook("a", 1, 4, HookStatus::Open)];
        // staleAfter=2，chapter 5 → lastAdvanced(4) <= 5-2=3？否 → 不陈旧
        let debt = collect_stale_hook_debt(&hooks, 5, None, Some(2));
        assert!(debt.is_empty());
        // staleAfter=1 → lastAdvanced(4) <= 4 → 陈旧
        let debt = collect_stale_hook_debt(&hooks, 5, None, Some(1));
        assert_eq!(debt.len(), 1);
    }

    #[test]
    fn admission_rejects_missing_type() {
        let cand = HookAdmissionCandidate { hook_type: "  ".into(), expected_payoff: Some("soon".into()), notes: None, payoff_timing: None };
        let d = evaluate_hook_admission(&cand, &[]);
        assert!(!d.admit);
        assert_eq!(d.reason, HookAdmissionReason::MissingType);
    }

    #[test]
    fn admission_rejects_missing_payoff_signal() {
        let cand = HookAdmissionCandidate { hook_type: "plot".into(), expected_payoff: None, notes: Some("   ".into()), payoff_timing: None };
        let d = evaluate_hook_admission(&cand, &[]);
        assert_eq!(d.reason, HookAdmissionReason::MissingPayoffSignal);
    }

    #[test]
    fn admission_admits_when_no_active() {
        let cand = HookAdmissionCandidate { hook_type: "plot".into(), expected_payoff: Some("终局摊牌".into()), notes: None, payoff_timing: None };
        let d = evaluate_hook_admission(&cand, &[]);
        assert!(d.admit);
        assert_eq!(d.reason, HookAdmissionReason::Admit);
    }

    #[test]
    fn admission_rejects_duplicate_family_exact_blob() {
        let active = vec![HookRecord {
            hook_type: "plot".into(),
            expected_payoff: "终局摊牌".into(),
            ..hook("h1", 1, 1, HookStatus::Open)
        }];
        let cand = HookAdmissionCandidate { hook_type: "plot".into(), expected_payoff: Some("终局摊牌".into()), notes: None, payoff_timing: None };
        let d = evaluate_hook_admission(&cand, &active);
        assert!(!d.admit);
        assert_eq!(d.reason, HookAdmissionReason::DuplicateFamily);
        assert_eq!(d.matched_hook_id.as_deref(), Some("h1"));
    }

    #[test]
    fn classify_disposition_priority() {
        let mut delta = RuntimeStateDelta {
            chapter: 5,
            hook_ops: HookOps { defer: vec!["h".into()], ..Default::default() },
            ..Default::default()
        };
        assert_eq!(classify_hook_disposition("h", &delta), HookDisposition::Defer);

        delta.hook_ops.defer.clear();
        delta.hook_ops.resolve.push("h".into());
        assert_eq!(classify_hook_disposition("h", &delta), HookDisposition::Resolve);

        delta.hook_ops.resolve.clear();
        delta.hook_ops.mention.push("h".into());
        assert_eq!(classify_hook_disposition("h", &delta), HookDisposition::Mention);

        delta.hook_ops.mention.clear();
        assert_eq!(classify_hook_disposition("h", &delta), HookDisposition::None);
    }

    #[test]
    fn classify_advance_requires_chapter_match() {
        let mut delta = RuntimeStateDelta { chapter: 5, ..Default::default() };
        let advancing = HookRecord { hook_id: "h".into(), last_advanced_chapter: 5, ..hook("h", 1, 5, HookStatus::Open) };
        delta.hook_ops.upsert.push(advancing);
        assert_eq!(classify_hook_disposition("h", &delta), HookDisposition::Advance);
        // lastAdvancedChapter !== chapter → none
        delta.hook_ops.upsert[0].last_advanced_chapter = 3;
        assert_eq!(classify_hook_disposition("h", &delta), HookDisposition::None);
    }

    #[test]
    fn normalize_text_collapses_punctuation() {
        assert_eq!(normalize_text("Hello, WORLD! 你好"), "hello world 你好");
        assert_eq!(normalize_text("  "), "");
    }

    #[test]
    fn extract_chinese_bigrams_sliding() {
        let b = extract_chinese_bigrams("主角复仇");
        assert!(b.contains("主角"));
        assert!(b.contains("角复"));
        assert!(b.contains("复仇"));
    }
}
