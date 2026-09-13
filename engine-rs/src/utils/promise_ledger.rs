//! 承诺账本运营化（G10/338 号，Phase B 批次一收尾）。
//!
//! TS 真源：`packages/core/src/utils/promise-ledger.ts`；共享向量：
//! `packages/core/src/__tests__/golden/promise-ledger-vectors.json`
//! （差分测试 `tests/golden_promise_ledger_diff.rs`）。
//!
//! 节奏债告警（core_hook 搁置超 N 章）/ 连续弱钩检测（hookActivity 强度）/
//! 承诺时间线（开启/推进/兑付）。规则语义详见 TS 模块 doc。

use serde::Deserialize;
use serde::Serialize;

pub const DEFAULT_CORE_HOOK_STALLED_THRESHOLD: i64 = 5;
pub const DEFAULT_WEAK_HOOK_MIN_RUN: usize = 3;

/// hookActivity 文本强度。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HookActivityStrength {
    Strong,
    Weak,
    None,
}

impl HookActivityStrength {
    pub fn as_str(self) -> &'static str {
        match self {
            HookActivityStrength::Strong => "strong",
            HookActivityStrength::Weak => "weak",
            HookActivityStrength::None => "none",
        }
    }
}

/// TS `NONE_PATTERNS`：空/无/没有/暂无/none/n-a/破折号。
fn is_none_marker(text: &str) -> bool {
    matches!(
        text.to_lowercase().as_str(),
        "无" | "没有" | "暂无" | "none" | "n/a" | "-" | "—" | "–"
    )
}

/// TS `STRONG_PATTERNS`：钩子记号或动词（大小写不敏感的子串匹配）。
fn has_strong_marker(lower: &str) -> bool {
    ["hook", "h0", "h1", "h2", "h3", "h4", "h5", "h6", "h7", "h8", "h9", "fk", "伏笔", "钩子", "回收", "埋", "推进", "兑现", "resolve", "plant", "advance"]
        .iter()
        .any(|marker| lower.contains(marker))
}

/// TS `ID_TOKEN_PATTERN`：/\\d+/ 的近似（数字记号即视为 hookId token）。
fn has_id_token(text: &str) -> bool {
    text.chars().any(|c| c.is_ascii_digit())
}

pub fn hook_activity_strength(text: &str) -> HookActivityStrength {
    let normalized = text.trim();
    if normalized.is_empty() || is_none_marker(normalized) {
        return HookActivityStrength::None;
    }
    let lower = normalized.to_lowercase();
    if has_strong_marker(&lower) || has_id_token(normalized) {
        return HookActivityStrength::Strong;
    }
    HookActivityStrength::Weak
}

/// TS `RESOLVED_PATTERN`：resolved/closed/done/已回收/已解决。
fn is_fulfilled(status: &str) -> bool {
    matches!(
        status.trim().to_lowercase().as_str(),
        "resolved" | "closed" | "done" | "已回收" | "已解决"
    )
}

/// 从 expectedPayoff 文本提取期望兑现章号（"第10章" / 裸数字）。
pub fn parse_expected_chapter(expected_payoff: &str) -> Option<i64> {
    let trimmed = expected_payoff.trim();
    if trimmed.is_empty() {
        return None;
    }
    let chars: Vec<char> = trimmed.chars().collect();
    if let Some(pos) = trimmed.find("第") {
        let rest: String = chars[pos + "第".chars().count()..].iter().collect();
        let rest = rest.trim_start();
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if let Ok(value) = digits.parse::<i64>() {
            return Some(value);
        }
    }
    // TS `/(?:^|[^\d])(\d{1,4})(?:[^\d]|$)/`：首个被非数字边界包住的 1–4 位数字。
    let bytes = trimmed.as_bytes();
    let n = bytes.len();
    let mut start: Option<usize> = None;
    for (index, byte) in bytes.iter().enumerate() {
        if byte.is_ascii_digit() {
            if start.is_none() {
                start = Some(index);
            }
            continue;
        }
        if let Some(from) = start {
            let length = index - from;
            if (1..=4).contains(&length)
                && (from == 0 || !bytes[from - 1].is_ascii_digit())
                && (index == n || !bytes[index].is_ascii_digit())
            {
                if let Ok(value) = trimmed[from..index].parse::<i64>() {
                    return Some(value);
                }
            }
            start = None;
        }
    }
    if let Some(from) = start {
        let length = n - from;
        if (1..=4).contains(&length) && (from == 0 || !bytes[from - 1].is_ascii_digit()) {
            if let Ok(value) = trimmed[from..].parse::<i64>() {
                return Some(value);
            }
        }
    }
    None
}

/// 判定输入 hook（双端向量子集；TS 直接吃 StoredHook 超集）。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromiseHookInput {
    pub hook_id: String,
    #[serde(default)]
    pub start_chapter: i64,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub last_advanced_chapter: i64,
    #[serde(default)]
    pub expected_payoff: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub core_hook: bool,
    /// R23/396 号：规范类型分类透传（存量无 kind 不出键）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<crate::models::runtime_state::HookKind>,
}

// ── 节奏债告警 ──

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PacingDebtAlert {
    pub hook_id: String,
    pub stalled_chapters: i64,
    pub threshold: i64,
    pub expected_payoff: String,
}

/// 节奏债告警：只盯核心承诺（coreHook）——搁置 ≥ threshold 章即告警（新→旧排序）。
pub fn detect_pacing_debts(
    hooks: &[PromiseHookInput],
    current_chapter: i64,
    threshold: Option<i64>,
) -> Vec<PacingDebtAlert> {
    let threshold = threshold.unwrap_or(DEFAULT_CORE_HOOK_STALLED_THRESHOLD);
    let mut alerts: Vec<PacingDebtAlert> = hooks
        .iter()
        .filter(|hook| hook.core_hook && !is_fulfilled(&hook.status))
        .map(|hook| {
            let anchor = std::cmp::max(hook.last_advanced_chapter.max(0), hook.start_chapter.max(0));
            (hook, std::cmp::max(0, current_chapter - anchor))
        })
        .filter(|(_, stalled)| *stalled >= threshold)
        .map(|(hook, stalled)| PacingDebtAlert {
            hook_id: hook.hook_id.clone(),
            stalled_chapters: stalled,
            threshold,
            expected_payoff: hook.expected_payoff.clone(),
        })
        .collect();
    alerts.sort_by(|a, b| {
        b.stalled_chapters
            .cmp(&a.stalled_chapters)
            .then(a.hook_id.cmp(&b.hook_id))
    });
    alerts
}

// ── 连续弱钩检测 ──

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WeakHookRun {
    pub from_chapter: i64,
    pub to_chapter: i64,
    pub length: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WeakHookSummary {
    pub runs: Vec<WeakHookRun>,
    pub longest_run: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WeakHookSummaryRow {
    pub chapter: i64,
    #[serde(default)]
    pub hook_activity: String,
}

/// 连续弱钩检测：非 strong 的连续章段（章号严格连续，缺章即断段），只返回 ≥ minRun 的段。
pub fn detect_weak_hook_runs(
    summaries: &[WeakHookSummaryRow],
    min_run_length: Option<usize>,
) -> WeakHookSummary {
    let min_run = min_run_length.unwrap_or(DEFAULT_WEAK_HOOK_MIN_RUN) as i64;
    let mut ordered: Vec<&WeakHookSummaryRow> = summaries.iter().collect();
    ordered.sort_by_key(|row| row.chapter);

    let mut runs: Vec<WeakHookRun> = Vec::new();
    let mut run_start: Option<i64> = None;
    let mut run_end: Option<i64> = None;
    {
        let mut close_run = |start: &mut Option<i64>, end: &mut Option<i64>| {
            if let (Some(from), Some(to)) = (*start, *end) {
                let length = to - from + 1;
                if length >= min_run {
                    runs.push(WeakHookRun { from_chapter: from, to_chapter: to, length });
                }
            }
            *start = None;
            *end = None;
        };
        for row in ordered {
            if hook_activity_strength(&row.hook_activity) == HookActivityStrength::Strong {
                close_run(&mut run_start, &mut run_end);
                continue;
            }
            if let Some(end) = run_end {
                if row.chapter != end + 1 {
                    close_run(&mut run_start, &mut run_end);
                }
            }
            if run_start.is_none() {
                run_start = Some(row.chapter);
            }
            run_end = Some(row.chapter);
        }
        close_run(&mut run_start, &mut run_end);
    }

    let longest_run = runs.iter().map(|run| run.length).fold(0, i64::max);
    WeakHookSummary { runs, longest_run }
}

// ── 承诺时间线 ──

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PromiseTimelineEntry {
    pub hook_id: String,
    pub summary: String,
    pub opened_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_advanced_at: Option<i64>,
    pub expected_payoff: String,
    pub state: &'static str,
    /// R23/396 号：规范类型分类透传（存量无 kind 不出键）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<crate::models::runtime_state::HookKind>,
}

const SUMMARY_MAX_CHARS: usize = 40;

/// 承诺时间线：未兑付在前（overdue 置顶），按 startChapter 升序稳定。
pub fn build_promise_timeline(
    hooks: &[PromiseHookInput],
    current_chapter: i64,
) -> Vec<PromiseTimelineEntry> {
    let state_rank = |state: &str| match state {
        "overdue" => 0,
        "open" => 1,
        "advancing" => 2,
        _ => 3,
    };
    let mut entries: Vec<PromiseTimelineEntry> = hooks
        .iter()
        .map(|hook| {
            let last_advanced_at = if hook.last_advanced_chapter > 0 {
                Some(hook.last_advanced_chapter)
            } else {
                None
            };
            let state = if is_fulfilled(&hook.status) {
                "fulfilled"
            } else {
                let expected = parse_expected_chapter(&hook.expected_payoff);
                if expected.is_some_and(|expected| current_chapter >= expected) {
                    "overdue"
                } else if last_advanced_at.is_some() {
                    "advancing"
                } else {
                    "open"
                }
            };
            let summary_source = if !hook.notes.trim().is_empty() {
                hook.notes.trim()
            } else {
                hook.expected_payoff.trim()
            };
            let summary: String = {
                let chars: Vec<char> = summary_source.chars().collect();
                if chars.len() > SUMMARY_MAX_CHARS {
                    let head: String = chars[..SUMMARY_MAX_CHARS - 1].iter().collect();
                    format!("{head}…")
                } else {
                    summary_source.to_string()
                }
            };
            PromiseTimelineEntry {
                hook_id: hook.hook_id.clone(),
                summary,
                opened_at: hook.start_chapter,
                last_advanced_at,
                expected_payoff: hook.expected_payoff.clone(),
                state,
                kind: hook.kind.clone(),
            }
        })
        .collect();
    entries.sort_by(|a, b| {
        state_rank(a.state)
            .cmp(&state_rank(b.state))
            .then(a.opened_at.cmp(&b.opened_at))
            .then(a.hook_id.cmp(&b.hook_id))
    });
    entries
}

/// 机器可读契约（双端 golden 锁形状）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PromiseLedgerContract {
    pub defaults: PromiseLedgerDefaults,
    pub strengths: Vec<&'static str>,
    pub timeline_states: Vec<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PromiseLedgerDefaults {
    pub core_hook_stalled_threshold: i64,
    pub weak_hook_min_run: usize,
}

pub fn promise_ledger_contract() -> PromiseLedgerContract {
    PromiseLedgerContract {
        defaults: PromiseLedgerDefaults {
            core_hook_stalled_threshold: DEFAULT_CORE_HOOK_STALLED_THRESHOLD,
            weak_hook_min_run: DEFAULT_WEAK_HOOK_MIN_RUN,
        },
        strengths: vec!["strong", "weak", "none"],
        timeline_states: vec!["open", "advancing", "fulfilled", "overdue"],
    }
}

// ── R3/360 号：数值紧迫度（WNW urgency 映射）+ 目标章窗 + 置信度 ──

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "export-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "lowercase")]
pub enum UrgencyLevel {
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "export-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "lowercase")]
pub enum UrgencyConfidence {
    Explicit,
    Inferred,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "export-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct PromiseUrgency {
    pub hook_id: String,
    /// 0–100：逾期 100 / 剩 1–3 章 80 / 剩 4–10 章 60 / 剩 >10 或推进中无目标 40 / 开启无目标 20 / 已兑付 0。
    pub urgency: i64,
    pub level: UrgencyLevel,
    /// 目标章来源：expectedPayoff 显式提取 / 书末兜底推断 / 无目标。
    pub confidence: UrgencyConfidence,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_chapter: Option<i64>,
    /// 显式目标 ±2 章缓冲；推断目标 = [当前章, 书末]。None 丢键（对齐 TS）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_window: Option<TargetWindow>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "export-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct TargetWindow {
    pub start: i64,
    pub end: i64,
}

pub const URGENCY_LEVEL_HIGH_THRESHOLD: i64 = 80;
pub const URGENCY_LEVEL_MEDIUM_THRESHOLD: i64 = 40;

/// 单条承诺的数值紧迫度（纯函数，golden 锚定；决策表见 TS `resolvePromiseUrgency`）：
/// - 目标章优先级：expectedPayoff 显式提取（explicit）> targetChapters 书末兜底（inferred）> 无（unknown）；
/// - 分档：remaining ≤0 → 100；1–3 → 80；4–10 → 60；>10 → 40；无目标时
///   推进中 → 40、未推进 → 20；已兑付 → 0；
/// - level：≥80 high / ≥40 medium / 其余 low。
pub fn resolve_promise_urgency(
    hook: &PromiseHookInput,
    current_chapter: i64,
    target_chapters: Option<i64>,
) -> PromiseUrgency {
    if is_fulfilled(&hook.status) {
        return PromiseUrgency {
            hook_id: hook.hook_id.clone(),
            urgency: 0,
            level: UrgencyLevel::Low,
            confidence: UrgencyConfidence::Unknown,
            target_chapter: None,
            target_window: None,
        };
    }

    let explicit = parse_expected_chapter(&hook.expected_payoff);
    let (target, confidence, target_window) = if let Some(explicit) = explicit {
        (
            explicit,
            UrgencyConfidence::Explicit,
            Some(TargetWindow {
                start: (explicit - 2).max(1),
                end: explicit + 2,
            }),
        )
    } else if let Some(book_end) = target_chapters.filter(|value| *value > 0) {
        (
            book_end,
            UrgencyConfidence::Inferred,
            Some(TargetWindow {
                start: current_chapter.max(1),
                end: book_end,
            }),
        )
    } else {
        (0, UrgencyConfidence::Unknown, None)
    };

    let has_target = confidence != UrgencyConfidence::Unknown;
    let urgency = if !has_target {
        if hook.last_advanced_chapter > 0 { 40 } else { 20 }
    } else {
        let remaining = target - current_chapter;
        if remaining <= 0 {
            100
        } else if remaining <= 3 {
            80
        } else if remaining <= 10 {
            60
        } else {
            40
        }
    };

    let level = if urgency >= URGENCY_LEVEL_HIGH_THRESHOLD {
        UrgencyLevel::High
    } else if urgency >= URGENCY_LEVEL_MEDIUM_THRESHOLD {
        UrgencyLevel::Medium
    } else {
        UrgencyLevel::Low
    };

    PromiseUrgency {
        hook_id: hook.hook_id.clone(),
        urgency,
        level,
        confidence,
        target_chapter: if has_target { Some(target) } else { None },
        target_window,
    }
}
