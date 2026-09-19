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


tokio::task_local! {
    /// 当前生产操作的激活技能（139 号：TS runner `operationContext.activatedSkills`
    /// 的 task-local 对应物——sub_agent 工具面 set（merge worker 绑定与会话
    /// 激活），AgentRouter::chat 出口读取注入 system 消息）。
    /// 530 号：写作链直呼路径（run_draft / ops 连写）同样经
    /// [`writing_chain_activations`] + [`scope_operation_skills`] set。
    pub static OPERATION_SKILLS: Option<std::sync::Arc<Vec<ActivatedSkillGuidance>>>;
}

/// 读取当前操作激活技能（无 set → None——TS storage.getStore() 语义）。
pub fn current_operation_skills() -> Option<std::sync::Arc<Vec<ActivatedSkillGuidance>>> {
    OPERATION_SKILLS.try_with(|skills| skills.clone()).ok().flatten()
}

/// 写作链 craft 激活解析（530 号）：longWriting 绑定 ∩ 可用技能；
/// 交集为空 → None（保持无注入，等价 TS 空数组直通）。
pub fn writing_chain_activations(
    available_skills: &[AgentSkill],
) -> Option<std::sync::Arc<Vec<ActivatedSkillGuidance>>> {
    let activations =
        resolve_production_skill_activations(available_skills, ProductionSkillCapability::LongWriting);
    if activations.is_empty() {
        None
    } else {
        Some(std::sync::Arc::new(activations))
    }
}

/// 以激活技能 task-local 作用域包住写作链 future（装配点：server 路由层，
/// 与 TS「server 解析 → PipelineConfig.activatedSkills → agentCtxFor 透传」
/// 对称；None → scope 空置，行为与未接线一致）。
pub async fn scope_operation_skills<T, F>(
    activations: Option<std::sync::Arc<Vec<ActivatedSkillGuidance>>>,
    fut: F,
) -> T
where
    F: std::future::Future<Output = T>,
{
    OPERATION_SKILLS.scope(activations, fut).await
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

    /// 530 号：写作链激活解析——有交集 Some、无交集 None。
    #[test]
    fn writing_chain_activations_intersect() {
        let available = vec![skill("inkos-long-writing")];
        let Some(activations) = writing_chain_activations(&available) else {
            panic!("有交集应返回 Some");
        };
        assert_eq!(activated_skill_ids(&activations), ["inkos-long-writing"]);
        // 交集为空（builtin 缺失/无技能）→ None = 保持无注入。
        assert!(writing_chain_activations(&[skill("inkos-translation")]).is_none());
        assert!(writing_chain_activations(&[]).is_none());
    }

    /// 530 号：scope 内可见、scope 外不可见（task-local 语义）。
    #[tokio::test]
    async fn scope_operation_skills_visibility() {
        let activations = writing_chain_activations(&[skill("inkos-long-writing")]).unwrap();
        let outside = current_operation_skills();
        assert!(outside.is_none(), "scope 外无 set → None");
        let inside = scope_operation_skills(Some(activations), async {
            current_operation_skills().map(|a| activated_skill_ids(&a))
        })
        .await;
        assert_eq!(inside, Some(vec!["inkos-long-writing".to_string()]));
        // None scope（空交集）→ 仍 None。
        let empty = scope_operation_skills(None, async { current_operation_skills() }).await;
        assert!(empty.is_none());
    }
}
