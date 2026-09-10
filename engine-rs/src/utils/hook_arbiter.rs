//! Hook 仲裁器（runtime-state delta 的 hook 操作裁决）。
//!
//! 移植自 `packages/core/src/utils/hook-arbiter.ts`（332 行）。
//!
//! [`arbitrate_runtime_state_delta_hooks`] 把 architect/consolidator 产出的原始 delta
//! 裁决为可直接归约的 resolved delta：
//! - 已知 id 的 upsert 直接保留；
//! - 未知候选经 `evaluate_hook_admission`（crate::utils::hook_governance）准入；
//!   - 准入 → 建规范 hook（canonical id，避免冲突）；
//!   - 拒绝（duplicate_family）→ 若有新颖内容则**映射**回匹配的既有 hook（mention/merge），
//!     若是纯重述则降级为 mention；
//!   - 其它拒绝（缺 type/payoff）→ rejected。
//! - 最终 mention/resolve/defer 互斥过滤（upsert 优先于 mention；mention 优先于 resolve/defer）。
//!
//! ## 强类型适配（与 TS 的差异）
//! - `status` 为 `HookStatus`（crate::models::runtime_state）枚举，非字符串。
//! - `payoff_timing` 为 `Option<HookPayoffTiming>` 枚举；调用
//!   `resolve_hook_payoff_timing`（crate::utils::hook_lifecycle）
//!   时，把枚举经 serde 名映射回字符串（与 TS 字符串路径等价，避免有损往返分歧）。

use std::collections::{BTreeSet, HashMap, HashSet};

use crate::models::runtime_state::{
    HookOps, HookPayoffTiming, HookRecord, HookStatus, NewHookCandidate, RuntimeStateDelta,
};
use crate::utils::hook_governance::{evaluate_hook_admission, HookAdmissionReason};
use crate::utils::hook_lifecycle::resolve_hook_payoff_timing;
use crate::utils::story_markdown::normalize_hook_id;

/// 单个候选的仲裁决策（对齐 TS `HookArbiterDecision`）。
#[derive(Debug, Clone, PartialEq)]
pub struct HookArbiterDecision {
    pub action: HookArbiterAction,
    pub reason: String,
    pub hook_id: Option<String>,
    pub candidate: NewHookCandidate,
}

/// 决策动作（对齐 TS `"created" | "mapped" | "mentioned" | "rejected"`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookArbiterAction {
    Created,
    Mapped,
    Mentioned,
    Rejected,
}

/// 仲裁器输入候选（内部用）。比 [`NewHookCandidate`] 多一个 `preferred_hook_id`
/// ——来自 upsert 但 id 未知的候选会携带原 id 作为偏好（对齐 TS `PendingHookCandidate`）。
#[derive(Debug, Clone)]
struct PendingCandidate {
    hook_type: String,
    expected_payoff: String,
    payoff_timing: Option<HookPayoffTiming>,
    notes: String,
    preferred_hook_id: Option<String>,
}

impl PendingCandidate {
    /// 从公共候选构造（无偏好 id）。
    fn from_candidate(c: &NewHookCandidate) -> Self {
        Self {
            hook_type: c.hook_type.clone(),
            expected_payoff: c.expected_payoff.clone(),
            payoff_timing: c.payoff_timing,
            notes: c.notes.clone(),
            preferred_hook_id: None,
        }
    }
}

/// 仲裁 runtime-state delta 的 hook 操作。
///
/// 入参 `hooks` 是当前持久化的全量 hook；`delta` 是本章节增量。返回 resolved delta（
/// `new_hook_candidates` 清空，所有结果汇入 `hook_ops`）+ 每个候选的决策。
///
/// `allow_new_hooks = Some(false)` 时（TS `allowNewHooks === false`，resync 链
/// 「保持稳定 hook id」语义）所有新候选在准入评估前直接拒绝
/// （reason = `new_hooks_disabled`）；已知名单内的 upsert 与 mention/resolve/defer
/// 不受影响。
pub fn arbitrate_runtime_state_delta_hooks(
    hooks: &[HookRecord],
    delta: &RuntimeStateDelta,
    allow_new_hooks: Option<bool>,
) -> (RuntimeStateDelta, Vec<HookArbiterDecision>) {
    let chapter = delta.chapter;
    // 工作副本：仲裁过程中累积的「最新已知 hooks」（用于准入评估的去重/匹配）。
    let mut working_hooks: Vec<HookRecord> = hooks.to_vec();
    let mut known_hook_ids: HashSet<String> = working_hooks.iter().map(|h| h.hook_id.clone()).collect();
    let mut upserts_by_id: HashMap<String, HookRecord> = HashMap::new();
    let mut mentions: BTreeSet<String> = delta.hook_ops.mention.iter().cloned().collect();
    let resolves = unique_strings(&delta.hook_ops.resolve);
    let defers = unique_strings(&delta.hook_ops.defer);
    let mut fallback_candidates: Vec<PendingCandidate> = Vec::new();
    let mut decisions: Vec<HookArbiterDecision> = Vec::new();

    // 1) 已知 id 的 upsert → 直接保留；未知 id → 转 fallback 候选（带偏好 id）。
    for hook in &delta.hook_ops.upsert {
        if known_hook_ids.contains(&hook.hook_id) {
            let normalized = hook.clone();
            upserts_by_id.insert(normalized.hook_id.clone(), normalized.clone());
            replace_working_hook(&mut working_hooks, normalized);
            continue;
        }
        fallback_candidates.push(PendingCandidate {
            hook_type: hook.hook_type.clone(),
            expected_payoff: hook.expected_payoff.clone(),
            payoff_timing: hook.payoff_timing,
            notes: hook.notes.clone(),
            preferred_hook_id: Some(hook.hook_id.clone()),
        });
    }

    // 2) fallback（未知 id 的 upsert）+ newHookCandidates 一起准入评估。
    let all_candidates: Vec<PendingCandidate> = fallback_candidates
        .into_iter()
        .chain(delta.new_hook_candidates.iter().map(PendingCandidate::from_candidate))
        .collect();

    for candidate in all_candidates {
        if allow_new_hooks == Some(false) {
            decisions.push(HookArbiterDecision {
                action: HookArbiterAction::Rejected,
                reason: "new_hooks_disabled".to_string(),
                hook_id: None,
                candidate: candidate_to_public(&candidate),
            });
            continue;
        }
        let active_hooks: Vec<HookRecord> = working_hooks
            .iter()
            .filter(|h| h.status != HookStatus::Resolved)
            .cloned()
            .collect();
        let admission_candidate = crate::utils::hook_governance::HookAdmissionCandidate {
            hook_type: candidate.hook_type.clone(),
            expected_payoff: Some(candidate.expected_payoff.clone()),
            payoff_timing: payoff_timing_as_str(candidate.payoff_timing).map(String::from),
            notes: Some(candidate.notes.clone()),
        };
        let admission = evaluate_hook_admission(&admission_candidate, &active_hooks);

        if !admission.admit {
            if admission.reason == HookAdmissionReason::DuplicateFamily {
                let Some(matched_id) = &admission.matched_hook_id else {
                    decisions.push(HookArbiterDecision {
                        action: HookArbiterAction::Rejected,
                        reason: "duplicate_family_without_match".to_string(),
                        hook_id: None,
                        candidate: candidate_to_public(&candidate),
                    });
                    continue;
                };
                let Some(matched) = working_hooks
                    .iter()
                    .find(|h| &h.hook_id == matched_id)
                    .cloned()
                else {
                    decisions.push(HookArbiterDecision {
                        action: HookArbiterAction::Rejected,
                        reason: "duplicate_family_without_match".to_string(),
                        hook_id: None,
                        candidate: candidate_to_public(&candidate),
                    });
                    continue;
                };

                if is_pure_restatement(&candidate, &matched) {
                    let not_in_upsert = !upserts_by_id.contains_key(&matched.hook_id);
                    let not_in_resolve = !resolves.contains(&matched.hook_id);
                    let not_in_defer = !defers.contains(&matched.hook_id);
                    if not_in_upsert && not_in_resolve && not_in_defer {
                        mentions.insert(matched.hook_id.clone());
                    }
                    decisions.push(HookArbiterDecision {
                        action: HookArbiterAction::Mentioned,
                        reason: "restated_existing_family".to_string(),
                        hook_id: Some(matched.hook_id.clone()),
                        candidate: candidate_to_public(&candidate),
                    });
                    continue;
                }

                // 有新颖内容 → 合并进既有 hook。
                let base = upserts_by_id.get(&matched.hook_id).cloned().unwrap_or(matched);
                let mapped = merge_candidate_into_existing_hook(&base, &candidate, chapter);
                upserts_by_id.insert(mapped.hook_id.clone(), mapped.clone());
                mentions.remove(&mapped.hook_id);
                replace_working_hook(&mut working_hooks, mapped);
                decisions.push(HookArbiterDecision {
                    action: HookArbiterAction::Mapped,
                    reason: "duplicate_family_with_novelty".to_string(),
                    hook_id: Some(matched_id.clone()),
                    candidate: candidate_to_public(&candidate),
                });
                continue;
            }

            // 其它拒绝（缺 type / 缺 payoff signal）。
            decisions.push(HookArbiterDecision {
                action: HookArbiterAction::Rejected,
                reason: admission_reason_str(admission.reason).to_string(),
                hook_id: None,
                candidate: candidate_to_public(&candidate),
            });
            continue;
        }

        // 准入 → 建规范 hook。
        let existing_ids: HashSet<String> = working_hooks
            .iter()
            .map(|h| h.hook_id.clone())
            .chain(upserts_by_id.keys().cloned())
            .collect();
        let created = create_canonical_hook(&candidate, chapter, &existing_ids);
        upserts_by_id.insert(created.hook_id.clone(), created.clone());
        working_hooks.push(created.clone());
        known_hook_ids.insert(created.hook_id.clone());
        decisions.push(HookArbiterDecision {
            action: HookArbiterAction::Created,
            reason: "admit".to_string(),
            hook_id: Some(created.hook_id),
            candidate: candidate_to_public(&candidate),
        });
    }

    // 3) 汇总 resolved delta。mention 与 upsert/resolve/defer 互斥；upsert 按 sortHooks 排序。
    let mut upserts: Vec<HookRecord> = upserts_by_id.into_values().collect();
    upserts.sort_by(sort_hooks);
    let mention: Vec<String> = mentions
        .into_iter()
        .filter(|id| !upserts.iter().any(|h| &h.hook_id == id))
        .filter(|id| !resolves.contains(id))
        .filter(|id| !defers.contains(id))
        .collect();

    let resolved_delta = RuntimeStateDelta {
        chapter,
        current_state_patch: delta.current_state_patch.clone(),
        hook_ops: HookOps {
            upsert: upserts,
            mention,
            resolve: resolves,
            defer: defers,
        },
        new_hook_candidates: Vec::new(),
        chapter_summary: delta.chapter_summary.clone(),
        subplot_ops: delta.subplot_ops.clone(),
        emotional_arc_ops: delta.emotional_arc_ops.clone(),
        character_matrix_ops: delta.character_matrix_ops.clone(),
        notes: delta.notes.clone(),
    };

    (resolved_delta, decisions)
}

/// 把仲裁内部候选转回公共 [`NewHookCandidate`]（决策里携带，供调用方观测）。
fn candidate_to_public(c: &PendingCandidate) -> NewHookCandidate {
    NewHookCandidate {
        hook_type: c.hook_type.clone(),
        expected_payoff: c.expected_payoff.clone(),
        payoff_timing: c.payoff_timing,
        notes: c.notes.clone(),
    }
}

/// 合并候选进既有 hook（对齐 TS `mergeCandidateIntoExistingHook`）。
fn merge_candidate_into_existing_hook(
    existing: &HookRecord,
    candidate: &PendingCandidate,
    chapter: u32,
) -> HookRecord {
    let expected_payoff = prefer_richer_text(&existing.expected_payoff, &candidate.expected_payoff);
    let notes = prefer_richer_text(&existing.notes, &candidate.notes);
    let payoff_timing = resolve_hook_payoff_timing(
        payoff_timing_as_str(candidate.payoff_timing.or(existing.payoff_timing)),
        Some(&expected_payoff),
        Some(&notes),
    );
    HookRecord {
        hook_id: existing.hook_id.clone(),
        start_chapter: existing.start_chapter,
        hook_type: prefer_richer_text(&existing.hook_type, &candidate.hook_type),
        status: if existing.status == HookStatus::Resolved {
            HookStatus::Resolved
        } else {
            HookStatus::Progressing
        },
        status_raw: String::new(),
        last_advanced_chapter: existing.last_advanced_chapter.max(chapter),
        expected_payoff,
        payoff_timing: Some(payoff_timing),
        notes,
        depends_on: existing.depends_on.clone(),
        pays_off_in_arc: existing.pays_off_in_arc.clone(),
        core_hook: existing.core_hook,
        half_life_chapters: existing.half_life_chapters,
        advanced_count: existing.advanced_count,
        promoted: existing.promoted,
    }
}

/// 建规范 hook（对齐 TS `createCanonicalHook`）。
fn create_canonical_hook(candidate: &PendingCandidate, chapter: u32, existing_ids: &HashSet<String>) -> HookRecord {
    let payoff_timing = resolve_hook_payoff_timing(
        payoff_timing_as_str(candidate.payoff_timing),
        Some(candidate.expected_payoff.trim()),
        Some(candidate.notes.trim()),
    );
    HookRecord {
        hook_id: build_canonical_hook_id(candidate, existing_ids),
        start_chapter: chapter,
        hook_type: candidate.hook_type.trim().to_string(),
        status: HookStatus::Open,
        status_raw: String::new(),
        last_advanced_chapter: chapter,
        expected_payoff: candidate.expected_payoff.trim().to_string(),
        payoff_timing: Some(payoff_timing),
        notes: candidate.notes.trim().to_string(),
        depends_on: None,
        pays_off_in_arc: None,
        core_hook: None,
        half_life_chapters: None,
        advanced_count: None,
        promoted: None,
    }
}

/// 生成规范 hook id（对齐 TS `buildCanonicalHookId`）：偏好 id 可用则用，否则 slugify + 去重后缀。
fn build_canonical_hook_id(candidate: &PendingCandidate, existing_ids: &HashSet<String>) -> String {
    if let Some(preferred) = &candidate.preferred_hook_id {
        let normalized = normalize_hook_id(Some(preferred));
        if !normalized.is_empty() && !existing_ids.contains(&normalized) {
            return normalized;
        }
    }

    let joined = format!("{} {} {}", candidate.hook_type, candidate.expected_payoff, candidate.notes);
    let base = slugify_hook_stem(&joined);
    let mut next = base.clone();
    let mut suffix = 2;
    while existing_ids.contains(&next) {
        next = format!("{base}-{suffix}");
        suffix += 1;
    }
    next
}

/// slugify hook 词干（对齐 TS `slugifyHookStem`）：
/// - 规范化（小写 + 非 [a-z0-9 CJK] → 空格）；
/// - 英文词 ≥3 字符、非停用词，取前 5；
/// - 中文片段 2-6 字符，取前 3；
/// - 用 `-` 连接，截断到 64，去尾 dash；空则 "hook"。
fn slugify_hook_stem(value: &str) -> String {
    let normalized = normalize_text(value);
    let english_terms: Vec<&str> = english_term_re()
        .find_iter(&normalized)
        .map(|m| m.as_str())
        .filter(|t| !stop_words_contains(t))
        .take(5)
        .collect();
    let chinese_terms: Vec<&str> = chinese_term_re().find_iter(&normalized).map(|m| m.as_str()).take(3).collect();

    let mut stem = String::new();
    for (i, t) in english_terms.iter().chain(chinese_terms.iter()).enumerate() {
        if i > 0 {
            stem.push('-');
        }
        stem.push_str(t);
    }
    if stem.len() > 64 {
        stem.truncate(64);
    }
    let stem = trailing_dash_re().replace_all(&stem, "").to_string();
    if stem.is_empty() {
        "hook".to_string()
    } else {
        stem
    }
}

/// 纯重述判定（对齐 TS `isPureRestatement`）：
/// 候选文本与既有文本的英文词/中文 bigram 几乎全部重叠（无英文新词且中文新 bigram < 2）。
fn is_pure_restatement(candidate: &PendingCandidate, existing: &HookRecord) -> bool {
    let candidate_text = normalize_text(&format!(
        "{} {} {}",
        candidate.hook_type, candidate.expected_payoff, candidate.notes
    ));
    let existing_text = normalize_text(&format!(
        "{} {} {}",
        existing.hook_type, existing.expected_payoff, existing.notes
    ));

    if candidate_text.is_empty() {
        return true;
    }
    if candidate_text == existing_text {
        return true;
    }

    let candidate_terms = extract_terms(&candidate_text);
    let existing_terms = extract_terms(&existing_text);
    let novel_terms = candidate_terms.difference(&existing_terms).count();

    let candidate_chinese = extract_chinese_bigrams(&candidate_text);
    let existing_chinese = extract_chinese_bigrams(&existing_text);
    let novel_chinese = candidate_chinese.difference(&existing_chinese).count();

    novel_terms == 0 && novel_chinese < 2
}

/// 就地替换工作副本中的同 id hook（不存在则追加）。对齐 TS `replaceWorkingHook`。
fn replace_working_hook(working_hooks: &mut Vec<HookRecord>, hook: HookRecord) {
    if let Some(slot) = working_hooks.iter_mut().find(|h| h.hook_id == hook.hook_id) {
        *slot = hook;
    } else {
        working_hooks.push(hook);
    }
}

/// hook 排序：startChapter 升序 → lastAdvanced 升序 → hookId 字典序。对齐 TS `sortHooks`。
fn sort_hooks(left: &HookRecord, right: &HookRecord) -> std::cmp::Ordering {
    left.start_chapter
        .cmp(&right.start_chapter)
        .then_with(|| left.last_advanced_chapter.cmp(&right.last_advanced_chapter))
        .then_with(|| left.hook_id.cmp(&right.hook_id))
}

/// trim + 去空（对齐 TS `uniqueStrings`）。
fn unique_strings(values: &[String]) -> Vec<String> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<String> = Vec::new();
    for v in values {
        let trimmed = v.trim();
        if trimmed.is_empty() {
            continue;
        }
        if seen.insert(trimmed.to_string()) {
            out.push(trimmed.to_string());
        }
    }
    out
}

/// 取更「丰富」的文本（对齐 TS `preferRicherText`）：trim 后非空优先，等长取原，否则取更长者。
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
    if right.chars().count() > left.chars().count() {
        right.to_string()
    } else {
        left.to_string()
    }
}

/// 文本规范化（对齐 TS `normalizeText`）：trim + 小写 + 非 [a-z0-9 CJK] → 空格 + 压缩空白。
fn normalize_text(value: &str) -> String {
    let lower: String = value.trim().to_lowercase();
    let cleaned: String = non_keepable_re().replace_all(&lower, " ").to_string();
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 提取词项集合（对齐 TS `extractTerms`）：英文词 ≥4 且非停用词 + 中文片段（2-6 字符 regex 匹配）。
fn extract_terms(value: &str) -> HashSet<String> {
    let mut set: HashSet<String> = HashSet::new();
    for term in value.split_whitespace() {
        let t = term.trim();
        if t.chars().count() >= 4 && !stop_words_contains(t) {
            set.insert(t.to_string());
        }
    }
    for m in chinese_term_re().find_iter(value) {
        set.insert(m.as_str().to_string());
    }
    set
}

/// 提取中文 bigram 集合（对齐 TS `extractChineseBigrams`）：每个 ≥2 字符的中文连续段，取所有相邻 2-gram。
fn extract_chinese_bigrams(value: &str) -> HashSet<String> {
    let mut terms: HashSet<String> = HashSet::new();
    for segment in chinese_run_re().find_iter(value) {
        let seg: Vec<char> = segment.as_str().chars().collect();
        if seg.len() < 2 {
            continue;
        }
        for window in seg.windows(2) {
            let bigram: String = window.iter().collect();
            terms.insert(bigram);
        }
    }
    terms
}

/// 把枚举 timing 映射为 serde 名字符串（与 TS 字符串路径等价）。
fn payoff_timing_as_str(timing: Option<HookPayoffTiming>) -> Option<&'static str> {
    timing.map(|t| match t {
        HookPayoffTiming::Immediate => "immediate",
        HookPayoffTiming::NearTerm => "near-term",
        HookPayoffTiming::MidArc => "mid-arc",
        HookPayoffTiming::SlowBurn => "slow-burn",
        HookPayoffTiming::Endgame => "endgame",
    })
}

/// 枚举 reason → TS 字符串名（决策 reason 字段沿用 TS 串，便于前端/日志观测）。
fn admission_reason_str(reason: HookAdmissionReason) -> &'static str {
    match reason {
        HookAdmissionReason::Admit => "admit",
        HookAdmissionReason::MissingType => "missing_type",
        HookAdmissionReason::MissingPayoffSignal => "missing_payoff_signal",
        HookAdmissionReason::DuplicateFamily => "duplicate_family",
    }
}

/// 英文停用词（对齐 TS `STOP_WORDS`）。slugify 与 extractTerms 都用它过滤无信息量的高频词。
const STOP_WORDS: &[&str] = &[
    "that",
    "this",
    "with",
    "from",
    "into",
    "still",
    "just",
    "have",
    "will",
    "reveal",
    "about",
    "already",
    "question",
    "chapter",
];

fn stop_words_contains(t: &str) -> bool {
    STOP_WORDS.contains(&t)
}
// --- 编译一次的正则（OnceLock）-------------------------------------------------

use regex::Regex;
use std::sync::OnceLock;

fn non_keepable_re() -> &'static Regex {
    // TS normalizeText: /[^a-z0-9一-鿿]+/g（小写化后）。一-鿿 = CJK 基本区。
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"[^a-z0-9\x{4e00}-\x{9fff}]+").expect("non-keepable regex"))
}

fn english_term_re() -> &'static Regex {
    // TS slugifyHookStem: /[a-z0-9]{3,}/g
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"[a-z0-9]{3,}").expect("english term regex"))
}

fn chinese_term_re() -> &'static Regex {
    // TS slugifyHookStem & extractTerms: /[一-鿿]{2,6}/g
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"[\x{4e00}-\x{9fff}]{2,6}").expect("chinese term regex"))
}

fn chinese_run_re() -> &'static Regex {
    // TS extractChineseBigrams: /[一-鿿]+/g
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"[\x{4e00}-\x{9fff}]+").expect("chinese run regex"))
}

fn trailing_dash_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"-+$").expect("trailing dash regex"))
}

// OnceLock 函数句柄已缓存编译后的 Regex；各 normalize/extract 函数直接调用上述
// 访问器（每次仅一次原子查表），无需额外静态包装。

#[cfg(test)]
mod tests {
    use super::*;

    fn hook(id: &str, hook_type: &str, expected: &str) -> HookRecord {
        HookRecord {
            hook_id: id.to_string(),
            start_chapter: 1,
            hook_type: hook_type.to_string(),
            status: HookStatus::Open,
            status_raw: String::new(),
            last_advanced_chapter: 1,
            expected_payoff: expected.to_string(),
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

    fn candidate(hook_type: &str, expected: &str, notes: &str) -> NewHookCandidate {
        NewHookCandidate {
            hook_type: hook_type.to_string(),
            expected_payoff: expected.to_string(),
            payoff_timing: None,
            notes: notes.to_string(),
        }
    }

    fn delta(chapter: u32, candidates: Vec<NewHookCandidate>) -> RuntimeStateDelta {
        RuntimeStateDelta {
            chapter,
            current_state_patch: None,
            hook_ops: HookOps::default(),
            new_hook_candidates: candidates,
            chapter_summary: None,
            subplot_ops: Vec::new(),
            emotional_arc_ops: Vec::new(),
            character_matrix_ops: Vec::new(),
            notes: Vec::new(),
        }
    }

    #[test]
    fn maps_duplicate_family_candidate_with_novelty_onto_existing() {
        // 对齐 TS 第 1 个测试：候选与既有同族但有新内容 → 映射回既有 id，lastAdvanced=chapter。
        let existing = HookRecord {
            hook_id: "anonymous-source-scope".to_string(),
            start_chapter: 3,
            hook_type: "source-risk".to_string(),
            status: HookStatus::Open,
            status_raw: String::new(),
            last_advanced_chapter: 8,
            expected_payoff: "Reveal how much the anonymous source already knew about the route."
                .to_string(),
            notes: "The source knowledge question remains unresolved.".to_string(),
            payoff_timing: None,
            depends_on: None,
            pays_off_in_arc: None,
            core_hook: None,
            half_life_chapters: None,
            advanced_count: None,
            promoted: None,
        };
        let d = delta(
            12,
            vec![candidate(
                "source-risk",
                "Reveal how much the anonymous source already knew about the route and address.",
                "This chapter adds the address angle to the anonymous source question.",
            )],
        );
        let (resolved, _decisions) = arbitrate_runtime_state_delta_hooks(&[existing], &d, None);
        assert_eq!(resolved.hook_ops.upsert.len(), 1);
        assert_eq!(resolved.hook_ops.upsert[0].hook_id, "anonymous-source-scope");
        assert_eq!(resolved.hook_ops.upsert[0].last_advanced_chapter, 12);
        assert!(resolved.new_hook_candidates.is_empty());
    }

    #[test]
    fn downgrades_pure_restatement_to_mention() {
        // 对齐 TS 第 2 个测试：候选与既有完全相同 → 降级为 mention。
        let existing = HookRecord {
            hook_id: "mentor-debt".to_string(),
            start_chapter: 1,
            hook_type: "relationship".to_string(),
            status: HookStatus::Open,
            status_raw: String::new(),
            last_advanced_chapter: 1,
            expected_payoff: "Reveal the real mentor debt.".to_string(),
            notes: "The mentor debt is still unresolved.".to_string(),
            payoff_timing: None,
            depends_on: None,
            pays_off_in_arc: None,
            core_hook: None,
            half_life_chapters: None,
            advanced_count: None,
            promoted: None,
        };
        let d = delta(
            12,
            vec![candidate(
                "relationship",
                "Reveal the real mentor debt.",
                "The mentor debt is still unresolved.",
            )],
        );
        let (resolved, _decisions) = arbitrate_runtime_state_delta_hooks(&[existing], &d, None);
        assert!(resolved.hook_ops.upsert.is_empty());
        assert!(resolved.hook_ops.mention.contains(&"mentor-debt".to_string()));
        assert!(resolved.new_hook_candidates.is_empty());
    }

    #[test]
    fn creates_canonical_hook_for_genuinely_new_candidate() {
        // 对齐 TS 第 3 个测试：全新候选 → 建 canonical hook（start=lastAdvanced=chapter，open）。
        let existing = hook("mentor-debt", "relationship", "Reveal the real mentor debt.");
        let d = delta(
            15,
            vec![candidate(
                "artifact",
                "Reveal why the seal answers only at midnight.",
                "A fresh unresolved rule around the seal appears in this chapter.",
            )],
        );
        let (resolved, _decisions) = arbitrate_runtime_state_delta_hooks(&[existing], &d, None);
        assert_eq!(resolved.hook_ops.upsert.len(), 1);
        let created = &resolved.hook_ops.upsert[0];
        assert_eq!(created.start_chapter, 15);
        assert_eq!(created.last_advanced_chapter, 15);
        assert_eq!(created.hook_type, "artifact");
        assert_eq!(created.status, HookStatus::Open);
        assert_ne!(created.hook_id, "mentor-debt");
        assert!(resolved.new_hook_candidates.is_empty());
    }

    #[test]
    fn rejects_candidate_missing_type() {
        let d = delta(5, vec![candidate("", "Some payoff signal here", "notes")]);
        let (resolved, decisions) = arbitrate_runtime_state_delta_hooks(&[], &d, None);
        assert!(resolved.hook_ops.upsert.is_empty());
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].action, HookArbiterAction::Rejected);
        assert_eq!(decisions[0].reason, "missing_type");
    }

    #[test]
    fn rejects_candidate_missing_payoff_signal() {
        // 有 type 但 expectedPayoff 和 notes 都为空 → missing_payoff_signal。
        let c = NewHookCandidate {
            hook_type: "mystery".to_string(),
            expected_payoff: String::new(),
            payoff_timing: None,
            notes: String::new(),
        };
        let d = delta(5, vec![c]);
        let (resolved, decisions) = arbitrate_runtime_state_delta_hooks(&[], &d, None);
        assert!(resolved.hook_ops.upsert.is_empty());
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].action, HookArbiterAction::Rejected);
        assert_eq!(decisions[0].reason, "missing_payoff_signal");
    }

    #[test]
    fn mention_is_filtered_out_when_same_id_in_resolve() {
        // delta 同时 mention + resolve 同一 id → resolve 优先，mention 被过滤。
        let existing = hook("h1", "mystery", "Reveal the secret");
        let d = RuntimeStateDelta {
            chapter: 10,
            current_state_patch: None,
            hook_ops: HookOps {
                upsert: Vec::new(),
                mention: vec!["h1".to_string()],
                resolve: vec!["h1".to_string()],
                defer: Vec::new(),
            },
            new_hook_candidates: Vec::new(),
            chapter_summary: None,
            subplot_ops: Vec::new(),
            emotional_arc_ops: Vec::new(),
            character_matrix_ops: Vec::new(),
            notes: Vec::new(),
        };
        let (resolved, _) = arbitrate_runtime_state_delta_hooks(&[existing], &d, None);
        assert!(resolved.hook_ops.mention.is_empty());
        assert!(resolved.hook_ops.resolve.contains(&"h1".to_string()));
    }

    #[test]
    fn allow_new_hooks_false_rejects_all_new_candidates() {
        // 对齐 TS allowNewHooks === false：新候选在准入评估前直接拒绝。
        // 既不影响已知名单内 upsert，也不影响 mention/resolve/defer。
        let existing = hook("h1", "mystery", "Reveal the old secret");
        let upsert = HookRecord {
            hook_id: "h1".to_string(),
            start_chapter: 1,
            hook_type: "mystery".to_string(),
            status: HookStatus::Progressing,
            status_raw: String::new(),
            last_advanced_chapter: 9,
            expected_payoff: "Reveal the updated secret".to_string(),
            payoff_timing: None,
            notes: "advanced".to_string(),
            depends_on: None,
            pays_off_in_arc: None,
            core_hook: None,
            half_life_chapters: None,
            advanced_count: None,
            promoted: None,
        };
        let d = RuntimeStateDelta {
            chapter: 9,
            current_state_patch: None,
            hook_ops: HookOps {
                upsert: vec![upsert],
                mention: Vec::new(),
                resolve: Vec::new(),
                defer: Vec::new(),
            },
            new_hook_candidates: vec![candidate(
                "artifact",
                "Reveal why the seal answers only at midnight.",
                "A fresh unresolved rule around the seal appears in this chapter.",
            )],
            chapter_summary: None,
            subplot_ops: Vec::new(),
            emotional_arc_ops: Vec::new(),
            character_matrix_ops: Vec::new(),
            notes: Vec::new(),
        };
        let (resolved, decisions) =
            arbitrate_runtime_state_delta_hooks(&[existing], &d, Some(false));
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].action, HookArbiterAction::Rejected);
        assert_eq!(decisions[0].reason, "new_hooks_disabled");
        // 已知 id 的 upsert 保留，未被禁用面波及。
        assert_eq!(resolved.hook_ops.upsert.len(), 1);
        assert_eq!(resolved.hook_ops.upsert[0].hook_id, "h1");
    }

    #[test]
    fn known_id_upsert_is_kept_directly() {
        // delta upsert 一个已知 id → 直接保留，不经准入。
        let existing = hook("h1", "mystery", "Reveal the old secret");
        let upsert = HookRecord {
            hook_id: "h1".to_string(),
            start_chapter: 1,
            hook_type: "mystery".to_string(),
            status: HookStatus::Progressing,
            status_raw: String::new(),
            last_advanced_chapter: 9,
            expected_payoff: "Reveal the updated secret".to_string(),
            payoff_timing: None,
            notes: "advanced".to_string(),
            depends_on: None,
            pays_off_in_arc: None,
            core_hook: None,
            half_life_chapters: None,
            advanced_count: None,
            promoted: None,
        };
        let d = RuntimeStateDelta {
            chapter: 9,
            current_state_patch: None,
            hook_ops: HookOps {
                upsert: vec![upsert.clone()],
                mention: Vec::new(),
                resolve: Vec::new(),
                defer: Vec::new(),
            },
            new_hook_candidates: Vec::new(),
            chapter_summary: None,
            subplot_ops: Vec::new(),
            emotional_arc_ops: Vec::new(),
            character_matrix_ops: Vec::new(),
            notes: Vec::new(),
        };
        let (resolved, _) = arbitrate_runtime_state_delta_hooks(&[existing], &d, None);
        assert_eq!(resolved.hook_ops.upsert.len(), 1);
        assert_eq!(resolved.hook_ops.upsert[0].expected_payoff, "Reveal the updated secret");
    }

    #[test]
    fn canonical_id_appends_suffix_on_collision() {
        // build_canonical_hook_id：preferred_id 与既有冲突时回退到 slug；slug 也冲突则 -2/-3 后缀。
        let candidate = PendingCandidate {
            hook_type: "artifact".to_string(),
            expected_payoff: "Reveal why the seal answers only at midnight".to_string(),
            payoff_timing: None,
            notes: "fresh rule".to_string(),
            preferred_hook_id: None,
        };
        let existing: HashSet<String> = HashSet::new();
        let first = build_canonical_hook_id(&candidate, &existing);
        // 第二次：first 已占用 → 应得 first-2。
        let mut occupied = HashSet::new();
        occupied.insert(first.clone());
        let second = build_canonical_hook_id(&candidate, &occupied);
        assert!(
            second.starts_with(&first) && second.len() > first.len(),
            "冲突时应加后缀：first={first} second={second}"
        );
        assert!(second.ends_with("-2"), "首次后缀应为 -2（got {second}）");
    }
}
