//! 治理上下文装配（规则栈 + 追踪链构建 + 保护源判定）。
//!
//! 移植自 `packages/core/src/utils/context-assembly.ts`（148 行）。
//! [`build_governed_rule_stack`] 产出 writer/continuity/reviser prompt 共用的
//! 每章规则栈（activeOverrides 由 planner intent 动态派生）；[`build_governed_trace`]
//! 产出治理追踪链（分层来源 + token 预算）；[`is_protected_context_source`]
//! 判定不可压缩的保护源。

use crate::llm::provider::estimate_text_tokens;
use crate::models::input_governance::{
    ActiveOverride, ChapterTrace, ContextPackage, ContextSource, OverrideEdge, RuleLayer,
    RuleLayerScope, RuleStack, RuleStackSections, TraceContextTiers, TraceTokenBudget,
};

/// override reason 截断上限（UTF-16 码元）。对齐 TS `MAX_OVERRIDE_REASON_CHARS`。
const MAX_OVERRIDE_REASON_CHARS: usize = 80;

fn truncate_for_override_reason(value: &str) -> String {
    let collapsed = whitespace_re().replace_all(value, " ").trim().to_string();
    if collapsed.chars().count() > MAX_OVERRIDE_REASON_CHARS {
        // TS slice(0, 79) + "…"：按 UTF-16 码元取 79 个（BMP 与 chars 等价）。
        let head: String = collapsed.chars().take(MAX_OVERRIDE_REASON_CHARS - 1).collect();
        format!("{head}…")
    } else {
        collapsed
    }
}

/// 构建每章规则栈。mustAvoid/styleEmphasis 派生 L4→L3 的逐章覆盖，
/// 使「Governed Control Stack」块反映本章实际生效的门控。
pub fn build_governed_rule_stack(
    must_avoid: &[String],
    style_emphasis: &[String],
    chapter_number: u32,
) -> RuleStack {
    let mut active_overrides: Vec<ActiveOverride> = Vec::new();

    // L4 → L3：逐章禁忌收窄规划层（来源 = rules-reader prohibitions +
    // current_focus avoid 段，经 planner.collect_must_avoid 汇聚）。
    for item in must_avoid {
        active_overrides.push(ActiveOverride {
            from: "L4".to_string(),
            to: "L3".to_string(),
            target: format!("chapter:{chapter_number}/mustAvoid"),
            reason: truncate_for_override_reason(item),
        });
    }

    // L4 → L3：planner 风格强调同样是逐章覆盖（POV 收紧/角色冲突聚焦等）。
    for item in style_emphasis {
        active_overrides.push(ActiveOverride {
            from: "L4".to_string(),
            to: "L3".to_string(),
            target: format!("chapter:{chapter_number}/styleEmphasis"),
            reason: truncate_for_override_reason(item),
        });
    }

    RuleStack {
        layers: vec![
            RuleLayer { id: "L1".to_string(), name: "hard_facts".to_string(), precedence: 100, scope: RuleLayerScope::Global },
            RuleLayer { id: "L2".to_string(), name: "author_intent".to_string(), precedence: 80, scope: RuleLayerScope::Book },
            RuleLayer { id: "L3".to_string(), name: "planning".to_string(), precedence: 60, scope: RuleLayerScope::Arc },
            RuleLayer { id: "L4".to_string(), name: "current_task".to_string(), precedence: 70, scope: RuleLayerScope::Local },
        ],
        sections: RuleStackSections {
            // Phase 5 权威源名（旧名 story_bible/volume_outline 已废弃）。
            hard: vec![
                "story_frame".to_string(),
                "current_state".to_string(),
                "book_rules".to_string(),
                "roles".to_string(),
            ],
            soft: vec![
                "author_intent".to_string(),
                "current_focus".to_string(),
                "volume_map".to_string(),
            ],
            diagnostic: vec![
                "anti_ai_checks".to_string(),
                "continuity_audit".to_string(),
                "style_regression_checks".to_string(),
            ],
        },
        override_edges: vec![
            OverrideEdge { from: "L4".to_string(), to: "L3".to_string(), allowed: true, scope: "current_chapter".to_string() },
            OverrideEdge { from: "L4".to_string(), to: "L2".to_string(), allowed: false, scope: "current_chapter".to_string() },
            OverrideEdge { from: "L4".to_string(), to: "L1".to_string(), allowed: false, scope: "current_chapter".to_string() },
        ],
        active_overrides,
    }
}

/// 治理追踪链构建入参。
pub struct GovernedTraceParams<'a> {
    pub chapter_number: u32,
    pub planner_inputs: &'a [String],
    pub composer_inputs: &'a [String],
    pub context_package: &'a ContextPackage,
    pub notes: &'a [String],
    pub prompt_packs: Option<&'a [String]>,
    pub compression: Option<crate::models::input_governance::TraceCompression>,
}

/// 构建治理追踪链：选定来源、保护/可压分层、token 预算。
pub fn build_governed_trace(params: &GovernedTraceParams<'_>) -> ChapterTrace {
    let protected_entries: Vec<_> = params
        .context_package
        .selected_context
        .iter()
        .filter(|entry| is_protected_context_source(&entry.source))
        .collect();
    let compressible_entries: Vec<_> = params
        .context_package
        .selected_context
        .iter()
        .filter(|entry| !is_protected_context_source(&entry.source))
        .collect();
    let protected_tokens: u64 = protected_entries
        .iter()
        .map(|entry| u64::from(estimate_context_source_tokens(entry)))
        .sum();
    let compressible_tokens: u64 = compressible_entries
        .iter()
        .map(|entry| u64::from(estimate_context_source_tokens(entry)))
        .sum();

    ChapterTrace {
        chapter: params.chapter_number,
        planner_inputs: params.planner_inputs.to_vec(),
        composer_inputs: params.composer_inputs.to_vec(),
        selected_sources: params
            .context_package
            .selected_context
            .iter()
            .map(|entry| entry.source.clone())
            .collect(),
        prompt_packs: params.prompt_packs.map(|p| p.to_vec()).unwrap_or_default(),
        context_tiers: TraceContextTiers {
            protected_sources: protected_entries
                .iter()
                .map(|entry| entry.source.clone())
                .collect(),
            compressible_sources: compressible_entries
                .iter()
                .map(|entry| entry.source.clone())
                .collect(),
        },
        token_budget: TraceTokenBudget {
            protected_tokens,
            compressible_tokens,
            total_selected_tokens: protected_tokens + compressible_tokens,
        },
        compression: params.compression.clone(),
        notes: params.notes.to_vec(),
    }
}

/// 保护源（不可压缩）：memo / 焦点 / 作者意图 / 漂移指引 / 大纲 / 正典 /
/// 硬状态 / 活跃 hook 证据 / hook 债简报。旧版文件名章纲段
/// （story_bible#/volume_outline#，G2/330 号）与新版 outline 段同等保护。
pub fn is_protected_context_source(source: &str) -> bool {
    source == "runtime/chapter_memo"
        || source == "story/current_focus.md"
        || source == "story/author_intent.md"
        || source == "story/audit_drift.md"
        || source == "story/outline/story_frame.md"
        || source.starts_with("story/outline/story_frame.md#")
        || source == "story/story_bible.md"
        || source.starts_with("story/story_bible.md#")
        || source == "story/outline/volume_map.md"
        || source.starts_with("story/outline/volume_map.md#")
        || source == "story/volume_outline.md"
        || source.starts_with("story/volume_outline.md#")
        || source == "story/parent_canon.md"
        || source == "story/fanfic_canon.md"
        || source.starts_with("story/current_state.md")
        || source.starts_with("story/pending_hooks.md#")
        || source.starts_with("runtime/hook_debt#")
}

fn estimate_context_source_tokens(entry: &ContextSource) -> u32 {
    // TS filter(Boolean)：excerpt 为 null/空串均剔除。
    let parts: Vec<&str> = [&entry.source, &entry.reason, entry.excerpt.as_deref().unwrap_or("")]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect();
    estimate_text_tokens(&parts.join("\n"))
}

fn whitespace_re() -> &'static regex::Regex {
    use std::sync::OnceLock;
    static R: OnceLock<regex::Regex> = OnceLock::new();
    R.get_or_init(|| regex::Regex::new(r"\s+").unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::input_governance::ContextSource;

    #[test]
    fn rule_stack_derives_overrides_from_intent() {
        let stack = build_governed_rule_stack(
            &["禁止降智".into(), "不要圣母".into()],
            &["POV 收紧".into()],
            7,
        );
        assert_eq!(stack.layers.len(), 4);
        assert_eq!(stack.active_overrides.len(), 3);
        assert_eq!(stack.active_overrides[0].target, "chapter:7/mustAvoid");
        assert_eq!(stack.active_overrides[0].reason, "禁止降智");
        assert_eq!(stack.active_overrides[2].target, "chapter:7/styleEmphasis");
        assert!(stack.override_edges[0].allowed);
        assert!(!stack.override_edges[1].allowed);
    }

    #[test]
    fn override_reason_truncates_to_80_chars() {
        let long = "长".repeat(100);
        let out = truncate_for_override_reason(&long);
        assert_eq!(out.chars().count(), MAX_OVERRIDE_REASON_CHARS);
        assert!(out.ends_with('…'));
        // 空白折叠。
        assert_eq!(truncate_for_override_reason("a  b\n c"), "a b c");
    }

    #[test]
    fn protected_source_matrix() {
        assert!(is_protected_context_source("runtime/chapter_memo"));
        assert!(is_protected_context_source("story/outline/volume_map.md#卷一"));
        assert!(is_protected_context_source("story/current_state.md#主角"));
        assert!(is_protected_context_source("runtime/hook_debt#H01"));
        assert!(is_protected_context_source("story/pending_hooks.md#H01"));
        // G2/330 号：旧版文件名章纲段（含锚点）与新版同等保护。
        assert!(is_protected_context_source("story/story_bible.md#玉印"));
        assert!(is_protected_context_source("story/volume_outline.md#chapter-4"));
        assert!(!is_protected_context_source("story/chapter_summaries.md#3"));
        assert!(!is_protected_context_source("story/volume_summaries.md#arc"));
        assert!(!is_protected_context_source("story/chapters#recent_endings"));
        assert!(!is_protected_context_source("reference/mat-01#开场"));
    }

    #[test]
    fn trace_partitions_and_sums_tokens() {
        let package = ContextPackage {
            chapter: 4,
            selected_context: vec![
                ContextSource {
                    source: "runtime/chapter_memo".into(),
                    reason: "memo".into(),
                    excerpt: Some("goal=推进主线".into()),
                },
                ContextSource {
                    source: "story/chapter_summaries.md#3".into(),
                    reason: "episodic".into(),
                    excerpt: Some("第3章摘要内容".into()),
                },
            ],
        };
        let trace = build_governed_trace(&GovernedTraceParams {
            chapter_number: 4,
            planner_inputs: &["story/author_intent.md".to_string()],
            composer_inputs: &["story/runtime/chapter-0004.intent.md".to_string()],
            context_package: &package,
            notes: &["note-a".to_string()],
            prompt_packs: None,
            compression: None,
        });
        assert_eq!(trace.context_tiers.protected_sources, vec!["runtime/chapter_memo"]);
        assert_eq!(trace.context_tiers.compressible_sources.len(), 1);
        assert_eq!(
            trace.token_budget.total_selected_tokens,
            trace.token_budget.protected_tokens + trace.token_budget.compressible_tokens
        );
        assert_eq!(trace.notes, vec!["note-a"]);
        assert!(trace.compression.is_none());
    }
}
