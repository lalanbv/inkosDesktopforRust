//! 生产模式技能绑定（TS `skills/production-bindings.ts` 逐字）。
//!
//! 生产模式（long/short/play/script/storyboard/interactive-film/translation）
//! 与内置技能包的绑定表：worker agent 按能力取绑定的技能指导
//! （`ActivatedSkillGuidance`），不可用的技能静默跳过。

use crate::skills::AgentSkill;

/// 生产能力。对齐 TS `ProductionSkillCapability`（绑定表的键族）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProductionSkillCapability {
    LongWriting,
    LongReview,
    ShortWriting,
    Play,
    Script,
    Storyboard,
    InteractiveFilm,
    Translation,
}

/// TS `PRODUCTION_SKILL_IDS` 绑定表逐字。
pub fn production_skill_ids(capability: ProductionSkillCapability) -> &'static [&'static str] {
    match capability {
        ProductionSkillCapability::LongWriting => &["inkos-long-writing"],
        ProductionSkillCapability::LongReview => &["inkos-long-writing", "inkos-story-review"],
        ProductionSkillCapability::ShortWriting => &["inkos-short-writing"],
        ProductionSkillCapability::Play => &["inkos-play-world"],
        ProductionSkillCapability::Script => &["inkos-script-writing"],
        ProductionSkillCapability::Storyboard => &["inkos-storyboard"],
        ProductionSkillCapability::InteractiveFilm => &["inkos-interactive-film"],
        ProductionSkillCapability::Translation => &["inkos-translation"],
    }
}

/// TS `NON_LONG_PRODUCTION_CAPABILITIES`。
pub const NON_LONG_PRODUCTION_CAPABILITIES: &[ProductionSkillCapability] = &[
    ProductionSkillCapability::ShortWriting,
    ProductionSkillCapability::Play,
    ProductionSkillCapability::Script,
    ProductionSkillCapability::Storyboard,
    ProductionSkillCapability::InteractiveFilm,
    ProductionSkillCapability::Translation,
];

/// 激活的技能参考资源（TS `ActivatedSkillResource`）。
#[derive(Debug, Clone, PartialEq)]
pub struct ActivatedSkillResource {
    pub path: String,
    pub heading: Option<String>,
    pub body: String,
    pub char_start: usize,
    pub char_end: usize,
}

/// 激活的技能指导（TS `ActivatedSkillGuidance`：技能 + 已加载的参考资源）。
#[derive(Debug, Clone, PartialEq)]
pub struct ActivatedSkillGuidance {
    pub skill: AgentSkill,
    pub resources: Vec<ActivatedSkillResource>,
}

/// TS `resolveProductionSkillActivations`：绑定表 ∩ 可用技能（不可用静默
/// 跳过），resources 恒空（引用加载由 use-skill 工具面按需做）。
pub fn resolve_production_skill_activations(
    available_skills: &[AgentSkill],
    capability: ProductionSkillCapability,
) -> Vec<ActivatedSkillGuidance> {
    let by_id = |id: &str| available_skills.iter().find(|skill| skill.id == id);
    production_skill_ids(capability)
        .iter()
        .filter_map(|id| by_id(id).map(|skill| ActivatedSkillGuidance { skill: skill.clone(), resources: Vec::new() }))
        .collect()
}

/// TS `mergeActivatedSkillGuidance`：按技能 id 去重合并（后写胜）。
pub fn merge_activated_skill_guidance(
    groups: &[&[ActivatedSkillGuidance]],
) -> Vec<ActivatedSkillGuidance> {
    let mut merged = std::collections::HashMap::<&str, ActivatedSkillGuidance>::new();
    let mut order: Vec<&str> = Vec::new();
    for group in groups {
        for activation in *group {
            let id = activation.skill.id.as_str();
            if !merged.contains_key(id) {
                order.push(id);
            }
            merged.insert(id, activation.clone());
        }
    }
    order.into_iter().filter_map(|id| merged.remove(id)).collect()
}

/// TS `activatedSkillIds`。
pub fn activated_skill_ids(activations: &[ActivatedSkillGuidance]) -> Vec<String> {
    activations.iter().map(|a| a.skill.id.clone()).collect()
}

/// TS agent-session `workerSkills` 闭包逐字：architect/writer → longWriting；
/// auditor/reviser → longReview；其它面空。
pub fn worker_skills_for_agent(
    available_skills: &[AgentSkill],
    agent: &str,
) -> Vec<ActivatedSkillGuidance> {
    match agent {
        "architect" | "writer" => {
            resolve_production_skill_activations(available_skills, ProductionSkillCapability::LongWriting)
        }
        "auditor" | "reviser" => {
            resolve_production_skill_activations(available_skills, ProductionSkillCapability::LongReview)
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skill(id: &str) -> AgentSkill {
        AgentSkill {
            id: id.to_string(),
            name: id.to_string(),
            description: "d".into(),
            body: "正文".into(),
            source: crate::skills::SkillSource::Builtin,
            base_dir: None,
        }
    }

    #[test]
    fn production_skill_ids_table_matches_ts() {
        assert_eq!(production_skill_ids(ProductionSkillCapability::LongWriting), ["inkos-long-writing"]);
        assert_eq!(
            production_skill_ids(ProductionSkillCapability::LongReview),
            ["inkos-long-writing", "inkos-story-review"]
        );
        assert_eq!(production_skill_ids(ProductionSkillCapability::Play), ["inkos-play-world"]);
        assert_eq!(production_skill_ids(ProductionSkillCapability::Translation), ["inkos-translation"]);
        assert_eq!(NON_LONG_PRODUCTION_CAPABILITIES.len(), 6);
    }

    #[test]
    fn activations_intersect_available_skills() {
        let available = vec![skill("inkos-long-writing"), skill("inkos-short-writing")];
        // longReview 绑定两条但只可用一条 → 静默跳过缺失项。
        let long_review = resolve_production_skill_activations(&available, ProductionSkillCapability::LongReview);
        assert_eq!(activated_skill_ids(&long_review), ["inkos-long-writing"]);
        // 全缺失 → 空。
        let play = resolve_production_skill_activations(&available, ProductionSkillCapability::Play);
        assert!(play.is_empty());
        assert!(play.iter().all(|a| a.resources.is_empty()), "resources 恒空");
    }

    #[test]
    fn merge_dedupes_by_skill_id_last_write_wins() {
        let a = vec![ActivatedSkillGuidance { skill: skill("x"), resources: vec![] }];
        let mut b_variant = skill("x");
        b_variant.body = "更新版".into();
        let b = vec![ActivatedSkillGuidance { skill: b_variant, resources: vec![] }];
        let c = vec![ActivatedSkillGuidance { skill: skill("y"), resources: vec![] }];
        let merged = merge_activated_skill_guidance(&[&a, &b, &c]);
        assert_eq!(activated_skill_ids(&merged), ["x", "y"]);
        assert_eq!(merged[0].skill.body, "更新版", "后写胜");
    }


    #[test]
    fn append_guidance_targets_system_message() {
        use crate::agents::append_activated_skill_guidance;
        use crate::llm::provider::{LLMMessage, LLMRole};
        let mut skill_body = skill("inkos-long-writing");
        skill_body.body = "长篇方法正文".into();
        let activation = ActivatedSkillGuidance {
            skill: skill_body,
            resources: vec![ActivatedSkillResource {
                path: "references/craft.md".into(),
                heading: Some("节奏".into()),
                body: "参考正文".into(),
                char_start: 12,
                char_end: 88,
            }],
        };
        // 无 system → 前置一条。
        let mut messages = vec![LLMMessage { role: LLMRole::User, content: "写一章".into(), tool_calls: None, tool_call_id: None }];
        append_activated_skill_guidance(&mut messages, std::slice::from_ref(&activation));
        assert_eq!(messages[0].role, LLMRole::System);
        assert!(messages[0].content.contains("## Activated professional skills"));
        assert!(messages[0].content.contains("### inkos-long-writing — inkos-long-writing"));
        assert!(messages[0].content.contains("长篇方法正文"));
        assert!(messages[0].content.contains("#### Reference: references/craft.md:12-88 · 节奏"));
        assert!(messages[0].content.contains("参考正文"));
        // 有 system → 追加不替换；空激活零改动。
        let mut with_system = vec![
            LLMMessage { role: LLMRole::System, content: "你是写手".into(), tool_calls: None, tool_call_id: None },
            LLMMessage { role: LLMRole::User, content: "继续".into(), tool_calls: None, tool_call_id: None },
        ];
        append_activated_skill_guidance(&mut with_system, &[]);
        assert_eq!(with_system[0].content, "你是写手");
        append_activated_skill_guidance(&mut with_system, &[activation]);
        assert!(with_system[0].content.starts_with("你是写手\n\n## Activated professional skills"));
        assert_eq!(with_system[1].content, "继续");
    }

    #[test]
    fn worker_skills_closure_binding() {
        let available = vec![
            skill("inkos-long-writing"),
            skill("inkos-story-review"),
            skill("inkos-play-world"),
        ];
        assert_eq!(
            activated_skill_ids(&worker_skills_for_agent(&available, "architect")),
            ["inkos-long-writing"]
        );
        assert_eq!(
            activated_skill_ids(&worker_skills_for_agent(&available, "writer")),
            ["inkos-long-writing"]
        );
        assert_eq!(
            activated_skill_ids(&worker_skills_for_agent(&available, "reviser")),
            ["inkos-long-writing", "inkos-story-review"]
        );
        assert_eq!(
            activated_skill_ids(&worker_skills_for_agent(&available, "auditor")),
            ["inkos-long-writing", "inkos-story-review"]
        );
        assert!(worker_skills_for_agent(&available, "exporter").is_empty());
        assert!(worker_skills_for_agent(&[], "writer").is_empty(), "无可用技能 → 空");
    }
}
