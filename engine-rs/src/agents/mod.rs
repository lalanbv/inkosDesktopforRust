//! agents 域（Phase P3）。
//!
//! 移植自 `packages/core/src/agents/`。自下而上：先纯逻辑解析叶子（无 LLM 调用），
//! 再移植依赖 streaming_client / state / prompts 的 agent 编排。
//!
//! ## 已移植
//! - [`detection_insights`]：检测历史聚合统计
//! - [`settler_parser`]：结算输出 `=== TAG ===` 段提取
//! - [`rules_reader`]：规则读取链（genre 画像 / book_rules / book.json language），
//!   ContinuityAuditor 等编排 agent 的数据入口
//! - [`fanfic_dimensions`]：同人维度配置（模式 → 维度 34-37 激活/严重度/注记）
//! - [`continuity`]：ContinuityAuditor 全量——类型层 + 37 维度注记/激活集 +
//!   四策略结果解析 + [`continuity::audit_chapter`] 编排（[`continuity::AuditorChat`]
//!   trait 注入 LLM 调用，生产实现待 BaseAgent/LLMRouter 移植）
//! - [`fanfic_prompt_sections`] / [`en_prompt_sections`]：同人/英文 prompt 段（writer
//!   system prompt 的 section 依赖）
//! - [`writer_prompts`]：writer system prompt 总装（zh 21 段 / en 19 段）+ 黄金三章纪律
//! - [`settler_prompts`] / [`observer_prompts`]：writer 编排 Phase 2a/2b（Observer 事实
//!   提取 + Settler 状态回写）的 system/user prompt 构造——纯函数，与 [`settler_parser`]
//!   / [`settler_delta_parser`] 输出端配对

pub mod ai_tells;
pub mod architect;
pub mod composer;
pub mod foundation_prompts;
pub mod fanfic_canon_importer;
pub mod foundation_reviewer;
pub mod consolidator;
pub mod chapter_analyzer;
pub mod continuity;
pub mod detection_insights;
pub mod detector;
pub mod en_prompt_sections;
pub mod fanfic_dimensions;
pub mod fanfic_prompt_sections;
pub mod observer_prompts;
pub mod planner;
pub mod radar;
pub mod script_storyboard;
pub mod planner_context;
pub mod planner_prompts;
pub mod post_write_validator;
pub mod reviser;
pub mod rules_reader;
pub mod sensitive_words;
pub mod settler_delta_parser;
pub mod settler_parser;
pub mod settler_prompts;
pub mod short_fiction;
pub mod state_validator;
pub mod style_analyzer;
pub mod writer_parser;
pub mod writer_prompts;
pub mod writer;
pub mod researcher;

// ── 138 号：激活技能指导拼接（TS agents/base.ts appendActivatedSkillGuidance 逐字） ──

/// 把激活技能的指导块拼进 system 消息（无 system 则前置一条）。非作者意图
/// 或输出格式覆盖的声明文案逐字。
pub fn append_activated_skill_guidance(
    messages: &mut Vec<crate::llm::provider::LLMMessage>,
    activations: &[crate::skills::production_bindings::ActivatedSkillGuidance],
) {
    use crate::llm::provider::{LLMMessage, LLMRole};
    if activations.is_empty() {
        return;
    }
    let mut sections: Vec<String> = vec![
        "## Activated professional skills".to_string(),
        "Use this specialist methodology for the current operation. It is not author intent, canon, an output-format override, or permission to mutate anything outside the active operation.".to_string(),
    ];
    for activation in activations {
        sections.push(format!("### {} — {}", activation.skill.id, activation.skill.name));
        let body = activation.skill.body.trim();
        sections.push(if body.is_empty() { activation.skill.description.clone() } else { body.to_string() });
        for resource in &activation.resources {
            let heading = resource
                .heading
                .as_ref()
                .map(|h| format!(" · {h}"))
                .unwrap_or_default();
            sections.push(format!(
                "#### Reference: {}:{}-{}{}",
                resource.path, resource.char_start, resource.char_end, heading
            ));
            sections.push(resource.body.clone());
        }
    }
    let guidance = sections.join("\n\n");
    match messages.iter_mut().find(|m| m.role == LLMRole::System) {
        Some(system) => {
            system.content.push_str("\n\n");
            system.content.push_str(&guidance);
        }
        None => messages.insert(
            0,
            LLMMessage { role: LLMRole::System, content: guidance, tool_calls: None, tool_call_id: None },
        ),
    }
}
