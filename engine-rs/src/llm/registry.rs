//! providers bank 内建注册表（功能性子集）。
//!
//! 提供 [`builtin_endpoints`]——`lookup_model`/`get_endpoint` 的数据源。
//! 当前含 deepseek（全量）+ openai（关键模型子集）+ anthropic（关键子集），
//! 让 lookup 端到端可用。完整 ~40 provider 数据 bank（每个 provider 的全量 models 数组）
//! 待后续逐文件移植（纯数据录入，机械工作）。
//!
//! 数据源：`packages/core/src/llm/providers/endpoints/*.ts`。

use super::providers::{
    ApiProtocol, EndpointGroup, InkosEndpoint, InkosModel, ModelCapabilities,
    ProviderCompat,
};

fn m(id: &'static str, max_output: u32, ctx: u32) -> InkosModel {
    InkosModel {
        id: String::from(id),
        max_output,
        context_window_tokens: ctx,
        enabled: None,
        deployment_name: None,
        released_at: None,
        temperature: None,
        status: None,
        replacement: None,
        capabilities: None,
    }
}

/// 内建 endpoint 注册表（功能性子集）。
pub fn builtin_endpoints() -> Vec<InkosEndpoint> {
    vec![
        // DeepSeek（全量 4 模型）
        InkosEndpoint {
            id: "deepseek".into(),
            label: "DeepSeek".into(),
            group: Some(EndpointGroup::China),
            api: ApiProtocol::OpenaiCompletions,
            base_url: "https://api.deepseek.com".into(),
            models_base_url: None,
            check_model: Some("deepseek-v4-flash".into()),
            temperature_range: Some((0.0, 2.0)),
            default_temperature: Some(1.0),
            writing_temperature: Some(1.5),
            temperature_hint: Some("创意写作推荐 1.5".into()),
            compat: Some(ProviderCompat { supports_store: None, supports_system_role: None, supports_developer_role: None, requires_assistant_after_tool_result: Some(true) }),
            transport_defaults: None,
            models: vec![
                InkosModel { enabled: Some(true), ..m("deepseek-v4-flash", 393216, 1_000_000) },
                InkosModel { enabled: Some(true), ..m("deepseek-v4-pro", 393216, 1_000_000) },
                m("deepseek-chat", 393216, 1_000_000),
                m("deepseek-reasoner", 393216, 1_000_000),
            ],
        },
        // OpenAI（关键模型子集；完整 40+ 待补）
        InkosEndpoint {
            id: "openai".into(),
            label: "OpenAI".into(),
            group: Some(EndpointGroup::Overseas),
            api: ApiProtocol::OpenaiResponses,
            base_url: "https://api.openai.com/v1".into(),
            models_base_url: None,
            check_model: Some("gpt-4o-mini".into()),
            temperature_range: Some((0.0, 2.0)),
            default_temperature: Some(1.0),
            writing_temperature: Some(1.0),
            temperature_hint: None,
            compat: None,
            transport_defaults: None,
            models: vec![
                InkosModel { enabled: Some(true), ..m("gpt-5.4", 128000, 1_050_000) },
                InkosModel { enabled: Some(true), ..m("gpt-5.4-mini", 128000, 400_000) },
                InkosModel { enabled: Some(true), ..m("gpt-4o", 4096, 128_000) },
                InkosModel { enabled: Some(true), ..m("gpt-4o-mini", 16384, 128_000) },
            ],
        },
        // Anthropic（关键模型子集）
        InkosEndpoint {
            id: "anthropic".into(),
            label: "Anthropic".into(),
            group: Some(EndpointGroup::Overseas),
            api: ApiProtocol::AnthropicMessages,
            base_url: "https://api.anthropic.com".into(),
            models_base_url: None,
            check_model: Some("claude-opus-4-8".into()),
            temperature_range: Some((0.0, 1.0)),
            default_temperature: Some(1.0),
            writing_temperature: Some(1.0),
            temperature_hint: Some("不要同时改 temperature 和 top_p".into()),
            compat: None,
            transport_defaults: None,
            models: vec![
                InkosModel {
                    enabled: Some(true),
                    capabilities: Some(ModelCapabilities { text: Some(true), tools: Some(true), image_input: Some(true), image_output: None, reasoning: Some(true) }),
                    ..m("claude-opus-4-8", 32000, 200_000)
                },
                InkosModel {
                    enabled: Some(true),
                    capabilities: Some(ModelCapabilities { text: Some(true), tools: Some(true), image_input: Some(true), image_output: None, reasoning: None }),
                    ..m("claude-sonnet-5", 64000, 200_000)
                },
            ],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::lookup::{get_endpoint, is_active_text_model, list_active_text_models, lookup_model};

    #[test]
    fn registry_has_three_providers() {
        let reg = builtin_endpoints();
        assert!(reg.len() >= 3);
        assert!(get_endpoint(&reg, "deepseek").is_some());
        assert!(get_endpoint(&reg, "openai").is_some());
        assert!(get_endpoint(&reg, "anthropic").is_some());
    }

    #[test]
    fn lookup_real_model() {
        let reg = builtin_endpoints();
        let m = lookup_model(&reg, "deepseek", "deepseek-v4-flash").unwrap();
        assert_eq!(m.max_output, 393216);
        assert_eq!(m.context_window_tokens, 1_000_000);
        assert!(m.enabled == Some(true));
    }

    #[test]
    fn lookup_openai_model() {
        let reg = builtin_endpoints();
        assert!(lookup_model(&reg, "openai", "gpt-4o").is_some());
        assert!(lookup_model(&reg, "openai", "nonexistent").is_none());
    }

    #[test]
    fn list_active_text_models_works() {
        let reg = builtin_endpoints();
        let active = list_active_text_models(&reg, "anthropic");
        assert!(active.len() >= 2);
        assert!(active.iter().all(|m| is_active_text_model(m)));
    }

    #[test]
    fn deepseek_compat_requires_assistant_bridge() {
        let reg = builtin_endpoints();
        let ep = get_endpoint(&reg, "deepseek").unwrap();
        assert_eq!(ep.compat.as_ref().and_then(|c| c.requires_assistant_after_tool_result), Some(true));
    }
}
