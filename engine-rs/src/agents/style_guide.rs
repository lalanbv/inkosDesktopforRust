//! 文风指南编译（958 号）：TS `packages/core/src/agents/style-guide.ts`
//! `compileStyleGuide` 逐字移植。
//!
//! 漂移来源：上游 2.0 harness 迁移（7aed0d82）把 `POST /style/analyze` 从纯统计
//! `analyzeStyle`（55 号移植形态）换成 LLM 编译文风指南 `{guide}`，studio 文风页
//! 同提交改读 `guide`；Rust 端停在旧形态（返回 StyleProfile 无 guide 字段）=
//! 默认 Rust 后端下文风页「分析结果」恒空。忠实移植+上游后改=漂移 rust→TS，
//! 按 TS 收敛。
//!
//! 语义（TS 逐条）：样本 trim 为空 → 错；必需技能 inkos-long-story-analysis +
//! inkos-imitation-writing 自可用技能集按 id 取**首个**命中（TS
//! `available.skills.find`——builtin 在前），缺任一 →
//! `Style extraction requires unavailable skill: {id}`；激活集 = 外层已激活 ∪
//! 两技能（merge 后写胜），经 AgentRouter::chat 出口注入 system；来源标签 =
//! sourceName trim 非空或「参考文本」/"reference"；响应 trim 为空 →
//! `Style extraction returned an empty guide.`。
//!
//! 备案：①TS `hydrateActivatedSkillGuidance` 的引用资源装载（linked/检索）
//! Rust 路由出口未移植（agent_router 139 号既有备案：resources 恒空、全 body
//! 注入）；②TS `runWorkerAgent` 不传 temperature（走 config/provider 缺省），
//! Rust 路由面必填——沿用同名 agent "style-guide" 既有 0.3（分析型抽取）。

use std::sync::Arc;

use crate::llm::agent_router::AgentRouter;
use crate::llm::provider::{LLMMessage, LLMRole};
use crate::skills::production_bindings::{
    current_operation_skills, merge_activated_skill_guidance, scope_operation_skills,
    ActivatedSkillGuidance,
};
use crate::skills::AgentSkill;
use crate::utils::language::WritingLanguage;

/// TS `compileStyleGuide` 必需技能（声明序=注入序）。
pub const STYLE_GUIDE_SKILL_IDS: [&str; 2] = ["inkos-long-story-analysis", "inkos-imitation-writing"];

/// 编译 system 提示（TS 双语逐字）。
pub const STYLE_GUIDE_SYSTEM_ZH: &str = "把参考文本编译为有证据、可执行的文风指南，只返回 Markdown。";
pub const STYLE_GUIDE_SYSTEM_EN: &str =
    "Compile the reference into an evidence-backed operational style guide. Return Markdown only.";

const STYLE_GUIDE_TEMPERATURE: f64 = 0.3;

/// 编译消息（system + user）；`sample` 由调用方 trim。
pub fn style_guide_messages(
    sample: &str,
    source_name: Option<&str>,
    language: WritingLanguage,
) -> Vec<LLMMessage> {
    let source = source_name.map(str::trim).filter(|name| !name.is_empty());
    let (system, user) = if language == WritingLanguage::En {
        (STYLE_GUIDE_SYSTEM_EN, format!("Source: {}\n\n{sample}", source.unwrap_or("reference")))
    } else {
        (STYLE_GUIDE_SYSTEM_ZH, format!("来源：{}\n\n{sample}", source.unwrap_or("参考文本")))
    };
    vec![
        LLMMessage { role: LLMRole::System, content: system.to_string(), tool_calls: None, tool_call_id: None },
        LLMMessage { role: LLMRole::User, content: user, tool_calls: None, tool_call_id: None },
    ]
}

/// 必需技能解析：按声明序逐个取可用集首个同 id 技能，缺任一即错。
pub fn resolve_style_guide_skills(available: &[AgentSkill]) -> Result<Vec<ActivatedSkillGuidance>, String> {
    STYLE_GUIDE_SKILL_IDS
        .iter()
        .map(|id| {
            available
                .iter()
                .find(|skill| skill.id == *id)
                .map(|skill| ActivatedSkillGuidance { skill: skill.clone(), resources: Vec::new() })
                .ok_or_else(|| format!("Style extraction requires unavailable skill: {id}"))
        })
        .collect()
}

/// TS `compileStyleGuide`：参考文本 → 可执行文风指南（Markdown，已 trim）。
pub async fn compile_style_guide(
    router: &AgentRouter,
    available_skills: &[AgentSkill],
    reference_text: &str,
    source_name: Option<&str>,
    language: WritingLanguage,
) -> Result<String, String> {
    let sample = reference_text.trim();
    if sample.is_empty() {
        return Err("Reference text is required for style extraction.".to_string());
    }
    let skills = resolve_style_guide_skills(available_skills)?;
    let inherited = current_operation_skills();
    let activations = merge_activated_skill_guidance(&[
        inherited.as_deref().map_or(&[][..], Vec::as_slice),
        &skills,
    ]);
    let messages = style_guide_messages(sample, source_name, language);
    let outcome = scope_operation_skills(
        Some(Arc::new(activations)),
        router.chat("style-guide", messages, STYLE_GUIDE_TEMPERATURE, None),
    )
    .await?;
    let guide = outcome.content.trim();
    if guide.is_empty() {
        return Err("Style extraction returned an empty guide.".to_string());
    }
    Ok(guide.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skill(id: &str) -> AgentSkill {
        AgentSkill {
            id: id.to_string(),
            name: format!("{id}-name"),
            description: "d".into(),
            body: format!("{id} 正文"),
            source: crate::skills::SkillSource::Builtin,
            base_dir: None,
            disable_model_invocation: None,
            user_invocable: None,
            rank: None,
        }
    }

    #[test]
    fn messages_mirror_ts_literals_and_source_fallbacks() {
        let zh = style_guide_messages("样本正文", Some("  斗破  "), WritingLanguage::Zh);
        assert_eq!(zh[0].role, LLMRole::System);
        assert_eq!(zh[0].content, "把参考文本编译为有证据、可执行的文风指南，只返回 Markdown。");
        assert_eq!(zh[1].role, LLMRole::User);
        assert_eq!(zh[1].content, "来源：斗破\n\n样本正文");
        // 空白/缺省来源 → 双语兜底标签（TS `sourceName?.trim() || "参考文本"`）。
        assert_eq!(style_guide_messages("x", Some("   "), WritingLanguage::Zh)[1].content, "来源：参考文本\n\nx");
        assert_eq!(style_guide_messages("x", None, WritingLanguage::Zh)[1].content, "来源：参考文本\n\nx");
        let en = style_guide_messages("sample", None, WritingLanguage::En);
        assert_eq!(
            en[0].content,
            "Compile the reference into an evidence-backed operational style guide. Return Markdown only."
        );
        assert_eq!(en[1].content, "Source: reference\n\nsample");
    }

    #[test]
    fn skills_resolve_in_declared_order_first_match_wins() {
        let mut override_skill = skill("inkos-imitation-writing");
        override_skill.body = "项目同名覆盖".into();
        let available = vec![
            skill("inkos-imitation-writing"),
            skill("inkos-long-story-analysis"),
            override_skill,
        ];
        let resolved = resolve_style_guide_skills(&available).unwrap();
        let ids: Vec<&str> = resolved.iter().map(|a| a.skill.id.as_str()).collect();
        assert_eq!(ids, ["inkos-long-story-analysis", "inkos-imitation-writing"]);
        // TS `available.skills.find` 取首个（builtin 在前）——后列同名不覆盖。
        assert_eq!(resolved[1].skill.body, "inkos-imitation-writing 正文");
        assert!(resolved.iter().all(|a| a.resources.is_empty()));
    }

    #[test]
    fn missing_skill_reports_first_absent_id() {
        let err = resolve_style_guide_skills(&[skill("inkos-imitation-writing")]).unwrap_err();
        assert_eq!(err, "Style extraction requires unavailable skill: inkos-long-story-analysis");
        let err = resolve_style_guide_skills(&[skill("inkos-long-story-analysis")]).unwrap_err();
        assert_eq!(err, "Style extraction requires unavailable skill: inkos-imitation-writing");
    }
}
