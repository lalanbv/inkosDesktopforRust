//! 运行时状态归约（应用 RuntimeStateDelta → 产出下一快照）。
//!
//! 移植自 `packages/core/src/state/state-reducer.ts`（274 行）。
//! 依赖链已全数就绪：
//! [`evaluate_hook_admission`]（hook-governance）+ [`resolve_hook_payoff_timing`]（hook-lifecycle）
//! + [`validate_runtime_state`]（state/validator）+ [`RuntimeStateDelta`] 等模型。

use crate::models::runtime_state::{
    ChapterSummariesState, ChapterSummaryRow, CurrentStateFact, CurrentStatePatch, CurrentStateState,
    HookPayoffTiming, HookRecord, HookStatus, HooksState, RuntimeStateDelta, StateManifest,
};
use crate::state::validator::{validate_runtime_state, ValidationInput};
use crate::utils::hook_governance::{
    evaluate_hook_admission, HookAdmissionCandidate, HookAdmissionReason,
};
use crate::utils::hook_lifecycle::resolve_hook_payoff_timing;
use thiserror::Error;

/// 运行时状态快照（四部分）。
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[cfg_attr(feature = "export-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
pub struct RuntimeStateSnapshot {
    pub manifest: StateManifest,
    pub current_state: CurrentStateState,
    pub hooks: HooksState,
    pub chapter_summaries: ChapterSummariesState,
}

#[derive(Debug, Error, PartialEq)]
pub enum StateReducerError {
    #[error("delta chapter {0} goes backwards")]
    BackwardsChapter(u32),
    #[error("chapter summary {0} does not match delta chapter {1}")]
    SummaryChapterMismatch(u32, u32),
    #[error("duplicate summary row for chapter {0}")]
    DuplicateSummaryRow(u32),
    #[error("duplicate active hook family: {0} overlaps {1}")]
    DuplicateHookFamily(String, String),
    #[error("validation failed: {0}")]
    ValidationFailed(String),
}

/// 应用增量到快照，返回新快照（不可变——输入不被修改）。
pub fn apply_runtime_state_delta(
    snapshot: &RuntimeStateSnapshot,
    delta: &RuntimeStateDelta,
    allow_reapply: Option<bool>,
) -> Result<RuntimeStateSnapshot, StateReducerError> {
    let allow_reapply = allow_reapply.unwrap_or(false);

    // 倒退章节守卫
    let goes_backwards = if allow_reapply {
        delta.chapter < snapshot.manifest.last_applied_chapter
    } else {
        delta.chapter <= snapshot.manifest.last_applied_chapter
    };
    if goes_backwards {
        return Err(StateReducerError::BackwardsChapter(delta.chapter));
    }

    // chapterSummary 章节一致性
    if let Some(summary) = &delta.chapter_summary {
        if summary.chapter != delta.chapter {
            return Err(StateReducerError::SummaryChapterMismatch(summary.chapter, delta.chapter));
        }
        let already = snapshot.chapter_summaries.rows.iter().any(|r| r.chapter == summary.chapter);
        if already && !allow_reapply {
            return Err(StateReducerError::DuplicateSummaryRow(summary.chapter));
        }
    }

    let hooks = apply_hook_ops(&snapshot.hooks, delta)?;
    let current_state = apply_current_state_patch(&snapshot.current_state, &snapshot.manifest.language, delta);
    let chapter_summaries = apply_summary_delta(&snapshot.chapter_summaries, delta, allow_reapply);

    let next = RuntimeStateSnapshot {
        manifest: StateManifest {
            last_applied_chapter: delta.chapter,
            ..snapshot.manifest.clone()
        },
        current_state,
        hooks,
        chapter_summaries,
    };

    // 最终校验（序列化四部分 → validate_runtime_state）
    let issues = validate_runtime_state(&ValidationInput {
        manifest: serde_json::to_value(&next.manifest).unwrap_or(serde_json::Value::Null),
        current_state: serde_json::to_value(&next.current_state).unwrap_or(serde_json::Value::Null),
        hooks: serde_json::to_value(&next.hooks).unwrap_or(serde_json::Value::Null),
        chapter_summaries: serde_json::to_value(&next.chapter_summaries).unwrap_or(serde_json::Value::Null),
    });
    if !issues.is_empty() {
        let msg = issues
            .iter()
            .map(|i| format!("{}: {}", i.code, i.message))
            .collect::<Vec<_>>()
            .join("; ");
        return Err(StateReducerError::ValidationFailed(msg));
    }

    Ok(next)
}

fn apply_hook_ops(hooks_state: &HooksState, delta: &RuntimeStateDelta) -> Result<HooksState, StateReducerError> {
    // TS 用 Map(hookId → record)；这里用 Vec + 按 id 定位。最终排序保证输出稳定。
    let mut hooks: Vec<HookRecord> = hooks_state.hooks.to_vec();

    for incoming in &delta.hook_ops.upsert {
        if let Some(idx) = hooks.iter().position(|h| h.hook_id == incoming.hook_id) {
            let merged = merge_hook_record(&hooks[idx], incoming);
            hooks[idx] = merged;
            continue;
        }

        // 新 hook：准入评估
        let active: Vec<HookRecord> = hooks
            .iter()
            .filter(|h| h.status != HookStatus::Resolved)
            .cloned()
            .collect();
        let admission = evaluate_hook_admission(
            &HookAdmissionCandidate {
                hook_type: incoming.hook_type.clone(),
                expected_payoff: Some(incoming.expected_payoff.clone()),
                notes: Some(incoming.notes.clone()),
                payoff_timing: None,
            },
            &active,
        );

        if !admission.admit && admission.reason == HookAdmissionReason::DuplicateFamily {
            let matched_id = admission.matched_hook_id.clone();
            if let Some(mid) = &matched_id {
                if let Some(idx) = hooks.iter().position(|h| &h.hook_id == mid) {
                    let merged = merge_hook_record(&hooks[idx], incoming);
                    hooks[idx] = merged;
                    continue;
                }
            }
            return Err(StateReducerError::DuplicateHookFamily(
                incoming.hook_id.clone(),
                matched_id.unwrap_or_default(),
            ));
        }

        hooks.push(incoming.clone());
    }

    for hook_id in &delta.hook_ops.resolve {
        if let Some(idx) = hooks.iter().position(|h| &h.hook_id == hook_id) {
            let advanced = hooks[idx].last_advanced_chapter.max(delta.chapter);
            hooks[idx].status = HookStatus::Resolved;
            hooks[idx].last_advanced_chapter = advanced;
        }
        // 不存在则优雅跳过（TS 同行为）
    }

    for hook_id in &delta.hook_ops.defer {
        if let Some(idx) = hooks.iter().position(|h| &h.hook_id == hook_id) {
            let advanced = hooks[idx].last_advanced_chapter.max(delta.chapter);
            hooks[idx].status = HookStatus::Deferred;
            hooks[idx].last_advanced_chapter = advanced;
        }
    }

    hooks.sort_by(|a, b| {
        a.start_chapter
            .cmp(&b.start_chapter)
            .then_with(|| a.last_advanced_chapter.cmp(&b.last_advanced_chapter))
            .then_with(|| a.hook_id.cmp(&b.hook_id))
    });

    Ok(HooksState { hooks })
}

fn merge_hook_record(existing: &HookRecord, incoming: &HookRecord) -> HookRecord {
    let expected_payoff = prefer_richer_text(&existing.expected_payoff, &incoming.expected_payoff);
    let notes = prefer_richer_text(&existing.notes, &incoming.notes);
    let advanced = existing.last_advanced_chapter.max(incoming.last_advanced_chapter);
    let progressed = advanced > existing.last_advanced_chapter;

    let merged_timing = incoming.payoff_timing.or(existing.payoff_timing);
    let resolved_timing = resolve_hook_payoff_timing(
        payoff_timing_to_str(merged_timing),
        Some(&expected_payoff),
        Some(&notes),
    );

    HookRecord {
        kind: None,
        hook_id: existing.hook_id.clone(),
        start_chapter: existing.start_chapter.min(incoming.start_chapter),
        hook_type: prefer_richer_text(&existing.hook_type, &incoming.hook_type),
        status: merge_hook_status(existing.status, incoming.status, progressed),
        status_raw: String::new(),
        last_advanced_chapter: advanced,
        expected_payoff,
        payoff_timing: Some(resolved_timing),
        notes,
        // 其余字段保留 existing（对齐 TS ...existing 展开）
        depends_on: existing.depends_on.clone(),
        pays_off_in_arc: existing.pays_off_in_arc.clone(),
        core_hook: existing.core_hook,
        half_life_chapters: existing.half_life_chapters,
        advanced_count: existing.advanced_count,
        promoted: existing.promoted,
    }
}

fn merge_hook_status(existing: HookStatus, incoming: HookStatus, progressed: bool) -> HookStatus {
    if existing == HookStatus::Resolved || incoming == HookStatus::Resolved {
        return HookStatus::Resolved;
    }
    if progressed || existing == HookStatus::Progressing || incoming == HookStatus::Progressing {
        return HookStatus::Progressing;
    }
    existing
}

fn prefer_richer_text(primary: &str, fallback: &str) -> String {
    let left = primary.trim();
    let right = fallback.trim();
    if left.is_empty() {
        return right.to_string();
    }
    if right.is_empty() {
        return left.to_string();
    }
    if left == right {
        return left.to_string();
    }
    if right.encode_utf16().count() > left.encode_utf16().count() {
        right.to_string()
    } else {
        left.to_string()
    }
}

fn payoff_timing_to_str(t: Option<HookPayoffTiming>) -> Option<&'static str> {
    t.map(|x| match x {
        HookPayoffTiming::Immediate => "immediate",
        HookPayoffTiming::NearTerm => "near-term",
        HookPayoffTiming::MidArc => "mid-arc",
        HookPayoffTiming::SlowBurn => "slow-burn",
        HookPayoffTiming::Endgame => "endgame",
    })
}

fn apply_current_state_patch(
    current_state: &CurrentStateState,
    language: &str,
    delta: &RuntimeStateDelta,
) -> CurrentStateState {
    let mut next_facts: Vec<CurrentStateFact> = current_state.facts.to_vec();

    let patch = match &delta.current_state_patch {
        None => {
            return CurrentStateState { chapter: delta.chapter, facts: next_facts };
        }
        Some(p) => p,
    };

    // 标签别名表（语言决定首选标签 + 顺序）
    let en = language == "en";
    let entries: [(&CurrentStatePatchField, &[&str]); 6] = [
        (&CurrentStatePatchField::CurrentLocation, if en { &["Current Location", "当前位置"] } else { &["当前位置", "Current Location"] }),
        (&CurrentStatePatchField::ProtagonistState, if en { &["Protagonist State", "主角状态"] } else { &["主角状态", "Protagonist State"] }),
        (&CurrentStatePatchField::CurrentGoal, if en { &["Current Goal", "当前目标"] } else { &["当前目标", "Current Goal"] }),
        (&CurrentStatePatchField::CurrentConstraint, if en { &["Current Constraint", "当前限制"] } else { &["当前限制", "Current Constraint"] }),
        (&CurrentStatePatchField::CurrentAlliances, if en { &["Current Alliances", "Current Relationships", "当前敌我"] } else { &["当前敌我", "Current Alliances", "Current Relationships"] }),
        (&CurrentStatePatchField::CurrentConflict, if en { &["Current Conflict", "当前冲突"] } else { &["当前冲突", "Current Conflict"] }),
    ];

    for (field, aliases) in entries {
        let value = match field_value(patch, field) {
            Some(v) => v,
            None => continue,
        };
        // 倒序删除匹配别名的旧 fact
        let mut index = next_facts.len() as i64 - 1;
        while index >= 0 {
            let i = index as usize;
            let predicate = next_facts[i].predicate.to_lowercase();
            if aliases.iter().any(|a| a.to_lowercase() == predicate) {
                next_facts.remove(i);
            }
            index -= 1;
        }
        next_facts.push(CurrentStateFact {
            subject: "protagonist".into(),
            predicate: aliases[0].to_string(),
            object: value,
            valid_from_chapter: delta.chapter,
            valid_until_chapter: None,
            source_chapter: delta.chapter,
        });
    }

    next_facts.sort_by(|a, b| {
        a.predicate
            .cmp(&b.predicate)
            .then_with(|| a.object.cmp(&b.object))
    });

    CurrentStateState { chapter: delta.chapter, facts: next_facts }
}

fn apply_summary_delta(
    state: &ChapterSummariesState,
    delta: &RuntimeStateDelta,
    allow_reapply: bool,
) -> ChapterSummariesState {
    let mut rows: Vec<ChapterSummaryRow> = state.rows.to_vec();
    match &delta.chapter_summary {
        None => {
            rows.sort_by(|a, b| a.chapter.cmp(&b.chapter));
            ChapterSummariesState { rows }
        }
        Some(summary) => {
            if allow_reapply {
                rows.retain(|r| r.chapter != summary.chapter);
            }
            rows.push(summary.clone());
            rows.sort_by(|a, b| a.chapter.cmp(&b.chapter));
            ChapterSummariesState { rows }
        }
    }
}

// ---- CurrentStatePatch 字段访问辅助 ----

enum CurrentStatePatchField {
    CurrentLocation,
    ProtagonistState,
    CurrentGoal,
    CurrentConstraint,
    CurrentAlliances,
    CurrentConflict,
}

fn field_value(p: &CurrentStatePatch, f: &CurrentStatePatchField) -> Option<String> {
    match f {
        CurrentStatePatchField::CurrentLocation => p.current_location.clone(),
        CurrentStatePatchField::ProtagonistState => p.protagonist_state.clone(),
        CurrentStatePatchField::CurrentGoal => p.current_goal.clone(),
        CurrentStatePatchField::CurrentConstraint => p.current_constraint.clone(),
        CurrentStatePatchField::CurrentAlliances => p.current_alliances.clone(),
        CurrentStatePatchField::CurrentConflict => p.current_conflict.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::runtime_state::{HookOps, HookStatus};

    fn manifest(last: u32) -> StateManifest {
        StateManifest {
            schema_version: 2,
            language: "zh".into(),
            last_applied_chapter: last,
            projection_version: 1,
            migration_warnings: vec![],
        }
    }

    fn empty_snapshot(last: u32) -> RuntimeStateSnapshot {
        RuntimeStateSnapshot {
            manifest: manifest(last),
            current_state: CurrentStateState { chapter: last, facts: vec![] },
            hooks: HooksState { hooks: vec![] },
            chapter_summaries: ChapterSummariesState { rows: vec![] },
        }
    }

    fn hook(id: &str, start: u32, last_adv: u32) -> HookRecord {
        HookRecord {
            kind: None,
            hook_id: id.into(),
            start_chapter: start,
            hook_type: "plot".into(),
            status: HookStatus::Open,
            status_raw: String::new(),
            last_advanced_chapter: last_adv,
            expected_payoff: "soon".into(),
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
    fn rejects_backwards_chapter() {
        let snap = empty_snapshot(5);
        let delta = RuntimeStateDelta { chapter: 4, ..Default::default() };
        assert_eq!(apply_runtime_state_delta(&snap, &delta, None), Err(StateReducerError::BackwardsChapter(4)));
    }

    #[test]
    fn rejects_equal_chapter_without_reapply() {
        let snap = empty_snapshot(5);
        let delta = RuntimeStateDelta { chapter: 5, ..Default::default() };
        assert_eq!(apply_runtime_state_delta(&snap, &delta, None), Err(StateReducerError::BackwardsChapter(5)));
    }

    #[test]
    fn allow_reapply_accepts_equal_chapter() {
        let snap = empty_snapshot(5);
        let delta = RuntimeStateDelta { chapter: 5, ..Default::default() };
        assert!(apply_runtime_state_delta(&snap, &delta, Some(true)).is_ok());
    }

    #[test]
    fn rejects_summary_chapter_mismatch() {
        let snap = empty_snapshot(5);
        let summary = ChapterSummaryRow { chapter: 7, title: "t".into(), ..Default::default() };
        let delta = RuntimeStateDelta { chapter: 6, chapter_summary: Some(summary), ..Default::default() };
        assert_eq!(
            apply_runtime_state_delta(&snap, &delta, None),
            Err(StateReducerError::SummaryChapterMismatch(7, 6))
        );
    }

    #[test]
    fn rejects_duplicate_summary_row() {
        let mut snap = empty_snapshot(5);
        let row = ChapterSummaryRow { chapter: 6, title: "old".into(), ..Default::default() };
        snap.chapter_summaries.rows.push(row);
        let summary = ChapterSummaryRow { chapter: 6, title: "new".into(), ..Default::default() };
        let delta = RuntimeStateDelta { chapter: 6, chapter_summary: Some(summary), ..Default::default() };
        assert_eq!(
            apply_runtime_state_delta(&snap, &delta, None),
            Err(StateReducerError::DuplicateSummaryRow(6))
        );
        // reapply 允许覆盖
        assert!(apply_runtime_state_delta(&snap, &delta, Some(true)).is_ok());
    }

    #[test]
    fn upsert_new_hook_admitted() {
        let snap = empty_snapshot(5);
        let new_hook = hook("h1", 6, 0);
        let delta = RuntimeStateDelta {
            chapter: 6,
            hook_ops: HookOps { upsert: vec![new_hook], ..Default::default() },
            ..Default::default()
        };
        let next = apply_runtime_state_delta(&snap, &delta, None).unwrap();
        assert_eq!(next.hooks.hooks.len(), 1);
        assert_eq!(next.hooks.hooks[0].hook_id, "h1");
        assert_eq!(next.manifest.last_applied_chapter, 6);
    }

    #[test]
    fn upsert_merges_existing_hook_by_id() {
        let mut snap = empty_snapshot(5);
        snap.hooks.hooks.push(hook("h1", 1, 3));
        let mut advancing = hook("h1", 1, 6);
        advancing.expected_payoff = "主角终局摊牌场景".into(); // 比现有 "soon" 更长 → richer
        let delta = RuntimeStateDelta {
            chapter: 6,
            hook_ops: HookOps { upsert: vec![advancing], ..Default::default() },
            ..Default::default()
        };
        let next = apply_runtime_state_delta(&snap, &delta, None).unwrap();
        assert_eq!(next.hooks.hooks.len(), 1);
        assert_eq!(next.hooks.hooks[0].last_advanced_chapter, 6); // advanced
        assert_eq!(next.hooks.hooks[0].status, HookStatus::Progressing); // progressed → progressing
        assert_eq!(next.hooks.hooks[0].expected_payoff, "主角终局摊牌场景");
    }

    #[test]
    fn resolve_sets_status_resolved() {
        let mut snap = empty_snapshot(5);
        snap.hooks.hooks.push(hook("h1", 1, 3));
        let delta = RuntimeStateDelta {
            chapter: 6,
            hook_ops: HookOps { resolve: vec!["h1".into()], ..Default::default() },
            ..Default::default()
        };
        let next = apply_runtime_state_delta(&snap, &delta, None).unwrap();
        assert_eq!(next.hooks.hooks[0].status, HookStatus::Resolved);
        assert_eq!(next.hooks.hooks[0].last_advanced_chapter, 6);
    }

    #[test]
    fn defer_sets_status_deferred() {
        let mut snap = empty_snapshot(5);
        snap.hooks.hooks.push(hook("h1", 1, 3));
        let delta = RuntimeStateDelta {
            chapter: 6,
            hook_ops: HookOps { defer: vec!["h1".into()], ..Default::default() },
            ..Default::default()
        };
        let next = apply_runtime_state_delta(&snap, &delta, None).unwrap();
        assert_eq!(next.hooks.hooks[0].status, HookStatus::Deferred);
    }

    #[test]
    fn current_state_patch_replaces_by_alias() {
        let mut snap = empty_snapshot(5);
        snap.current_state.facts.push(CurrentStateFact {
            subject: "protagonist".into(),
            predicate: "当前位置".into(),
            object: "旧位置".into(),
            valid_from_chapter: 3,
            valid_until_chapter: None,
            source_chapter: 3,
        });
        let delta = RuntimeStateDelta {
            chapter: 6,
            current_state_patch: Some(CurrentStatePatch {
                current_location: Some("新位置".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let next = apply_runtime_state_delta(&snap, &delta, None).unwrap();
        let locs: Vec<_> = next.current_state.facts.iter().filter(|f| f.predicate == "当前位置").collect();
        assert_eq!(locs.len(), 1);
        assert_eq!(locs[0].object, "新位置");
        assert_eq!(locs[0].valid_from_chapter, 6);
    }

    #[test]
    fn prefer_richer_text_picks_longer() {
        assert_eq!(prefer_richer_text("ab", "abcd"), "abcd");
        assert_eq!(prefer_richer_text("abcd", "ab"), "abcd");
        assert_eq!(prefer_richer_text("", "x"), "x");
        assert_eq!(prefer_richer_text("x", ""), "x");
        assert_eq!(prefer_richer_text("same", "same"), "same");
    }

    #[test]
    fn merge_hook_status_logic() {
        use HookStatus::*;
        assert_eq!(merge_hook_status(Open, Resolved, false), Resolved);
        assert_eq!(merge_hook_status(Resolved, Open, false), Resolved);
        assert_eq!(merge_hook_status(Open, Open, true), Progressing);
        assert_eq!(merge_hook_status(Open, Progressing, false), Progressing);
        assert_eq!(merge_hook_status(Open, Open, false), Open);
    }

    #[test]
    fn duplicate_family_new_upsert_merged_into_existing() {
        // 已有同族 hook；新 upsert 不同 id 但同 type+payoff → 准入判为 duplicate_family
        let mut snap = empty_snapshot(5);
        let mut existing = hook("h1", 1, 3);
        existing.expected_payoff = "终局摊牌".into();
        snap.hooks.hooks.push(existing);
        let mut dup = hook("h2", 2, 0); // 不同 id
        dup.expected_payoff = "终局摊牌".into();
        let delta = RuntimeStateDelta {
            chapter: 6,
            hook_ops: HookOps { upsert: vec![dup], ..Default::default() },
            ..Default::default()
        };
        let next = apply_runtime_state_delta(&snap, &delta, None).unwrap();
        // 应合并进 h1，不新增 h2
        let ids: Vec<_> = next.hooks.hooks.iter().map(|h| h.hook_id.as_str()).collect();
        assert!(ids.contains(&"h1"));
        assert!(!ids.contains(&"h2"));
    }
}
