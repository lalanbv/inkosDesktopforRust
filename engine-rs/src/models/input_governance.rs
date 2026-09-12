//! 输入治理类型。
//!
//! 移植自 `packages/core/src/models/input-governance.ts`（124 行，全量）。
//! 纯类型层：ChapterMemo / ChapterIntent / ContextSource / ContextPackage /
//! RuleStack 族 / ChapterTrace。zod 的 min/max 约束由构造方（解析器/编排层）保证，
//! 类型层只承载形状（与 ChapterMemo 同策略）。

use serde::{Deserialize, Serialize};
#[cfg(feature = "export-bindings")]
use ts_rs::TS;

/// R1 读者体验合同（357 号）——memo「读者体验合同」节的结构化提取产物。
///
/// 字段契约对齐 TS `ReaderExperienceSchema`：七个叙事字段各 ≤200 UTF-16 码元
/// （解析器截断后入型，避免 LLM 偶发超长触发重试风暴）；titleCandidates 为
/// 章名候选（≤3 个，各 ≤60 码元），服务"目标 + 追读钩子"。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ReaderExperience {
    pub previous_handoff: String,
    pub reader_question: String,
    pub promise_payoff: String,
    pub protagonist_want: String,
    pub protagonist_obstacle: String,
    pub scene_turn: String,
    pub ending_net_change: String,
    pub title_candidates: Vec<String>,
}

/// 章节规划备忘录（LLM planner 输出经 [`crate::utils::chapter_memo_parser::parse_memo`] 解析后产物）。
///
/// 字段契约对齐 TS `ChapterMemoSchema`：
/// - `chapter`：`z.number().int().min(1)` → u32（≥1 由解析器入参保证）
/// - `goal`：`z.string().min(1).max(50)` → 显示用短目标（≤50 UTF-16 码元）
/// - `is_golden_opening`：`z.boolean().default(false)`
/// - `body`：`z.string().min(1)` → 完整 memo 正文（含完整目标）
/// - `thread_refs`：`z.array(z.string()).default([])` → 关联线索 ID（去重保序）
/// - `reader_experience`：`ReaderExperience.optional()` → R1 合同，存量 memo 无此节时为 `None`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ChapterMemo {
    pub chapter: u32,
    pub goal: String,
    pub is_golden_opening: bool,
    pub body: String,
    pub thread_refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reader_experience: Option<ReaderExperience>,
}

/// 章节意图。对齐 TS `ChapterIntentSchema`（planner 产物，writer/auditor 消费）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase", default)]
pub struct ChapterIntent {
    pub chapter: u32,
    pub goal: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outline_node: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arc_context: Option<String>,
    pub must_keep: Vec<String>,
    pub must_avoid: Vec<String>,
    pub style_emphasis: Vec<String>,
}

impl Default for ChapterIntent {
    fn default() -> Self {
        ChapterIntent {
            chapter: 1,
            goal: String::new(),
            outline_node: None,
            arc_context: None,
            must_keep: vec![],
            must_avoid: vec![],
            style_emphasis: vec![],
        }
    }
}

/// 单条入选上下文来源。对齐 TS `ContextSourceSchema`（source/reason 非空由构造方保证）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ContextSource {
    pub source: String,
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub excerpt: Option<String>,
}

/// 上下文包。对齐 TS `ContextPackageSchema`——governed-context / ContinuityAuditor 的输入。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase", default)]
pub struct ContextPackage {
    pub chapter: u32,
    pub selected_context: Vec<ContextSource>,
}

impl Default for ContextPackage {
    fn default() -> Self {
        ContextPackage {
            chapter: 1,
            selected_context: vec![],
        }
    }
}

/// 规则层作用域。对齐 TS `RuleLayerScopeSchema = z.enum(["global","book","arc","local"])`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export, type = "\"global\" | \"book\" | \"arc\" | \"local\""))]
pub enum RuleLayerScope {
    #[serde(rename = "global")]
    Global,
    #[serde(rename = "book")]
    Book,
    #[serde(rename = "arc")]
    Arc,
    #[serde(rename = "local")]
    Local,
}

/// 规则层。对齐 TS `RuleLayerSchema`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct RuleLayer {
    pub id: String,
    pub name: String,
    pub precedence: i32,
    pub scope: RuleLayerScope,
}

/// 覆盖边。对齐 TS `OverrideEdgeSchema`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct OverrideEdge {
    pub from: String,
    pub to: String,
    pub allowed: bool,
    pub scope: String,
}

/// 生效中的覆盖。对齐 TS `ActiveOverrideSchema`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ActiveOverride {
    pub from: String,
    pub to: String,
    pub target: String,
    pub reason: String,
}

/// 规则栈分节。对齐 TS `RuleStackSectionsSchema`。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase", default)]
pub struct RuleStackSections {
    pub hard: Vec<String>,
    pub soft: Vec<String>,
    pub diagnostic: Vec<String>,
}

/// 规则栈。对齐 TS `RuleStackSchema`（layers ≥1 由构造方保证）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase", default)]
pub struct RuleStack {
    pub layers: Vec<RuleLayer>,
    pub sections: RuleStackSections,
    pub override_edges: Vec<OverrideEdge>,
    pub active_overrides: Vec<ActiveOverride>,
}

/// token 预算统计。对齐 TS `ChapterTraceSchema.tokenBudget`。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase", default)]
pub struct TraceTokenBudget {
    pub protected_tokens: u64,
    pub compressible_tokens: u64,
    pub total_selected_tokens: u64,
}

/// 上下文分层。对齐 TS `ChapterTraceSchema.contextTiers`。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase", default)]
pub struct TraceContextTiers {
    pub protected_sources: Vec<String>,
    pub compressible_sources: Vec<String>,
}

/// 压缩结果。对齐 TS `ChapterTraceSchema.compression`（可选）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase", default)]
pub struct TraceCompression {
    pub compiled_source: String,
    pub protected_sources: Vec<String>,
    pub compressed_sources: Vec<String>,
    pub protected_tokens: u64,
    pub compressible_tokens: u64,
    pub budget_tokens: u64,
}

/// 章节追踪。对齐 TS `ChapterTraceSchema`（pipeline 追踪产物）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase", default)]
pub struct ChapterTrace {
    pub chapter: u32,
    pub planner_inputs: Vec<String>,
    pub composer_inputs: Vec<String>,
    pub selected_sources: Vec<String>,
    pub prompt_packs: Vec<String>,
    pub context_tiers: TraceContextTiers,
    pub token_budget: TraceTokenBudget,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compression: Option<TraceCompression>,
    pub notes: Vec<String>,
}

impl Default for ChapterTrace {
    fn default() -> Self {
        ChapterTrace {
            chapter: 1,
            planner_inputs: vec![],
            composer_inputs: vec![],
            selected_sources: vec![],
            prompt_packs: vec![],
            context_tiers: TraceContextTiers::default(),
            token_budget: TraceTokenBudget::default(),
            compression: None,
            notes: vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_package_roundtrips_camel_case() {
        let pkg = ContextPackage {
            chapter: 3,
            selected_context: vec![ContextSource {
                source: "story/pending_hooks.md#h1".into(),
                reason: "伏笔".into(),
                excerpt: Some("摘录".into()),
            }],
        };
        let json = serde_json::to_value(&pkg).unwrap();
        assert_eq!(json["chapter"], 3);
        assert_eq!(json["selectedContext"][0]["source"], "story/pending_hooks.md#h1");
        assert_eq!(json["selectedContext"][0]["excerpt"], "摘录");
        let back: ContextPackage = serde_json::from_value(json).unwrap();
        assert_eq!(back, pkg);
    }

    #[test]
    fn rule_stack_defaults_and_scopes() {
        let json = serde_json::json!({
            "layers": [{ "id": "book-rules", "name": "书规则", "precedence": 10, "scope": "book" }],
            "sections": { "hard": ["硬规则"] },
            "overrideEdges": [],
            "activeOverrides": []
        });
        let stack: RuleStack = serde_json::from_value(json).unwrap();
        assert_eq!(stack.layers.len(), 1);
        assert_eq!(stack.layers[0].scope, RuleLayerScope::Book);
        assert_eq!(stack.sections.hard, vec!["硬规则".to_string()]);
        assert!(stack.sections.soft.is_empty());
    }

    #[test]
    fn chapter_trace_defaults_fill_nested() {
        let trace: ChapterTrace = serde_json::from_str("{}").unwrap();
        assert!(trace.prompt_packs.is_empty());
        assert_eq!(trace.token_budget.total_selected_tokens, 0);
        assert!(trace.compression.is_none());
        assert_eq!(trace.chapter, 1);
    }
}
