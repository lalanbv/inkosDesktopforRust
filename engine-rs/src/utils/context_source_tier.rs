//! 上下文来源优先级契约（G2/330 号）。
//!
//! TS 真源：`packages/core/src/utils/context-source-tier.ts`；共享向量：
//! `packages/core/src/__tests__/golden/context-priority-vectors.json`
//! （差分测试 `tests/golden_context_priority_diff.rs`）。
//!
//! 分层：本书事实(100) > 本书规划(80) > 本书记忆(60) > 参考资料(40)
//! > 拆书结论(30, 预留 G5) > 写法资产(20, 预留 G4) > 临时/未注册(10)。
//! 核心不变量：参考资料及更低层不得覆盖本书事实与章纲。

use serde::Serialize;

use crate::models::input_governance::ContextSource;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextSourceTier {
    BookFact,
    BookPlanning,
    BookMemory,
    UserReference,
    Deconstruction,
    StyleAsset,
    Ephemeral,
}

impl ContextSourceTier {
    pub fn as_str(self) -> &'static str {
        match self {
            ContextSourceTier::BookFact => "book-fact",
            ContextSourceTier::BookPlanning => "book-planning",
            ContextSourceTier::BookMemory => "book-memory",
            ContextSourceTier::UserReference => "user-reference",
            ContextSourceTier::Deconstruction => "deconstruction",
            ContextSourceTier::StyleAsset => "style-asset",
            ContextSourceTier::Ephemeral => "ephemeral",
        }
    }

    /// 数值越大优先级越高；对齐 RuleStack precedence：100=hard_facts(L1)、80=author_intent(L2)。
    pub fn precedence(self) -> i32 {
        match self {
            ContextSourceTier::BookFact => 100,
            ContextSourceTier::BookPlanning => 80,
            ContextSourceTier::BookMemory => 60,
            ContextSourceTier::UserReference => 40,
            ContextSourceTier::Deconstruction => 30,
            ContextSourceTier::StyleAsset => 20,
            ContextSourceTier::Ephemeral => 10,
        }
    }

    /// 须与 [`crate::utils::context_assembly::is_protected_context_source`] 一致
    /// （golden 差分锁死，单侧漂移即红）。
    pub fn is_protected(self) -> bool {
        matches!(self, ContextSourceTier::BookFact | ContextSourceTier::BookPlanning)
    }

    pub fn label_zh(self) -> &'static str {
        match self {
            ContextSourceTier::BookFact => "本书事实",
            ContextSourceTier::BookPlanning => "本书规划",
            ContextSourceTier::BookMemory => "本书时序记忆",
            ContextSourceTier::UserReference => "参考资料",
            ContextSourceTier::Deconstruction => "拆书结论",
            ContextSourceTier::StyleAsset => "写法资产",
            ContextSourceTier::Ephemeral => "临时/未注册",
        }
    }

    pub fn label_en(self) -> &'static str {
        match self {
            ContextSourceTier::BookFact => "Book facts",
            ContextSourceTier::BookPlanning => "Book planning",
            ContextSourceTier::BookMemory => "Book memory",
            ContextSourceTier::UserReference => "User references",
            ContextSourceTier::Deconstruction => "Deconstruction",
            ContextSourceTier::StyleAsset => "Style assets",
            ContextSourceTier::Ephemeral => "Ephemeral",
        }
    }
}

/// 整文件级来源（story_bible/volume_outline 为 Phase 5 旧名，保留兼容）。
const EXACT_PLANNING_SOURCES: [&str; 6] = [
    "story/author_intent.md",
    "story/current_focus.md",
    "story/audit_drift.md",
    "story/outline/volume_map.md",
    "story/volume_outline.md",
    "runtime/chapter_memo",
];

const EXACT_FACT_SOURCES: [&str; 4] = [
    "story/story_bible.md",
    "story/outline/story_frame.md",
    "story/parent_canon.md",
    "story/fanfic_canon.md",
];

/// ContextPackage 来源 → 层级。总函数：未注册来源一律 ephemeral（排序垫底、
/// 可压缩），绝不臆测其权威性——新来源上线前必须显式注册层级。
pub fn context_source_tier(source: &str) -> ContextSourceTier {
    if EXACT_PLANNING_SOURCES.contains(&source) {
        return ContextSourceTier::BookPlanning;
    }
    if EXACT_FACT_SOURCES.contains(&source) {
        return ContextSourceTier::BookFact;
    }
    if source.starts_with("story/outline/story_frame.md#") {
        return ContextSourceTier::BookFact;
    }
    if source.starts_with("story/outline/volume_map.md#") {
        return ContextSourceTier::BookPlanning;
    }
    // outlineFallback 的旧版文件名（含 #section 锚点）语义不变：仍是章纲/正典段。
    if source.starts_with("story/story_bible.md") {
        return ContextSourceTier::BookFact;
    }
    if source.starts_with("story/volume_outline.md") {
        return ContextSourceTier::BookPlanning;
    }
    if source.starts_with("story/current_state.md") {
        return ContextSourceTier::BookFact;
    }
    if source.starts_with("story/pending_hooks.md#") {
        return ContextSourceTier::BookFact;
    }
    if source.starts_with("runtime/hook_debt#") {
        return ContextSourceTier::BookFact;
    }
    if source.starts_with("story/chapter_summaries.md#") {
        return ContextSourceTier::BookMemory;
    }
    if source.starts_with("story/volume_summaries.md#") {
        return ContextSourceTier::BookMemory;
    }
    if source == "story/chapters#recent_endings" {
        return ContextSourceTier::BookMemory;
    }
    if source.starts_with("reference/") {
        return ContextSourceTier::UserReference;
    }
    if source.starts_with("deconstruction/") {
        return ContextSourceTier::Deconstruction;
    }
    // R5/366 号：反AI规则与经验条目与写法同层（20，写作纪律垫底参考层）。
    if source.starts_with("rules/") || source.starts_with("experience/") {
        return ContextSourceTier::StyleAsset;
    }
    if source.starts_with("style/") {
        return ContextSourceTier::StyleAsset;
    }
    ContextSourceTier::Ephemeral
}

/// 组装固化入口：按层级 precedence 稳定排序（同层保持组装序不变；Rust
/// `sort_by` 为稳定排序，与 TS 的 index 装饰等价）。`compose_governed_chapter`
/// 在合并 story 证据与参考资料后调用。
pub fn enforce_context_priority_order(mut entries: Vec<ContextSource>) -> Vec<ContextSource> {
    entries.sort_by(|left, right| {
        context_source_tier(&right.source)
            .precedence()
            .cmp(&context_source_tier(&left.source).precedence())
    });
    entries
}

/// 机器可读契约（双端 golden 锁形状；键名与 TS 契约对象一致）。
#[derive(Debug, Clone, Serialize)]
pub struct ContextSourcePriorityContract {
    pub version: u32,
    pub rule: &'static str,
    pub layers: Vec<ContextSourceTierDescriptor>,
    #[serde(rename = "overrideRules")]
    pub override_rules: Vec<&'static str>,
    #[serde(rename = "unknownSourceTier")]
    pub unknown_source_tier: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct ContextSourceTierDescriptor {
    pub id: &'static str,
    pub precedence: i32,
    pub protected: bool,
    #[serde(rename = "labelZh")]
    pub label_zh: &'static str,
    #[serde(rename = "labelEn")]
    pub label_en: &'static str,
}

pub const CONTEXT_SOURCE_TIERS: [ContextSourceTier; 7] = [
    ContextSourceTier::BookFact,
    ContextSourceTier::BookPlanning,
    ContextSourceTier::BookMemory,
    ContextSourceTier::UserReference,
    ContextSourceTier::Deconstruction,
    ContextSourceTier::StyleAsset,
    ContextSourceTier::Ephemeral,
];

pub fn context_source_tier_descriptor(tier: ContextSourceTier) -> ContextSourceTierDescriptor {
    ContextSourceTierDescriptor {
        id: tier.as_str(),
        precedence: tier.precedence(),
        protected: tier.is_protected(),
        label_zh: tier.label_zh(),
        label_en: tier.label_en(),
    }
}

pub fn context_source_priority_contract() -> ContextSourcePriorityContract {
    ContextSourcePriorityContract {
        version: 1,
        rule: "higherPrecedenceNumberIsHigherPriority",
        layers: CONTEXT_SOURCE_TIERS
            .iter()
            .copied()
            .map(context_source_tier_descriptor)
            .collect(),
        override_rules: vec![
            // G2 核心断言：参考资料（及更低层）不得覆盖本书事实与章纲段。
            "reference-and-below-cannot-override-fact-or-planning",
            "memory-informs-but-cannot-override-planning",
            "planning-narrows-fact-only-via-explicit-override-edges",
            "unknown-sources-are-ephemeral-and-sort-last",
        ],
        unknown_source_tier: "ephemeral",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_registered_families() {
        assert_eq!(context_source_tier("story/current_state.md#主角"), ContextSourceTier::BookFact);
        assert_eq!(context_source_tier("story/outline/story_frame.md#力量体系"), ContextSourceTier::BookFact);
        assert_eq!(context_source_tier("story/story_bible.md#玉印"), ContextSourceTier::BookFact);
        assert_eq!(context_source_tier("runtime/chapter_memo"), ContextSourceTier::BookPlanning);
        assert_eq!(context_source_tier("story/volume_outline.md#卷一"), ContextSourceTier::BookPlanning);
        assert_eq!(context_source_tier("story/chapter_summaries.md#3"), ContextSourceTier::BookMemory);
        assert_eq!(context_source_tier("reference/mat-01#开场"), ContextSourceTier::UserReference);
        assert_eq!(context_source_tier("story/unknown_new.md"), ContextSourceTier::Ephemeral);
    }

    #[test]
    fn protection_matches_tier_flags() {
        for tier in CONTEXT_SOURCE_TIERS {
            assert_eq!(tier.is_protected(), tier.precedence() >= 80);
        }
    }
}
