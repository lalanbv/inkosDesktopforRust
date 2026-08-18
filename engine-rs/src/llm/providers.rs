//! provider bank 类型（inkos 自维护的 provider/模型卡定义）。
//!
//! 移植自 `packages/core/src/llm/providers/types.ts`。
//! 这是 lookupModel/getEndpoint 的返回类型——流式客户端构造 piModel 的前置依赖。
//! 实际 provider 数据（各 provider 的 models 数组）+ lookup/index/verify/probe 待后续移植。

use serde::{Deserialize, Serialize};
#[cfg(feature = "export-bindings")]
use ts_rs::TS;

/// API 协议。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(
    feature = "export-bindings",
    ts(export, type = "\"openai-completions\" | \"openai-responses\" | \"anthropic-messages\" | \"google-generative-ai\"")
)]
pub enum ApiProtocol {
    #[serde(rename = "openai-completions")] OpenaiCompletions,
    #[serde(rename = "openai-responses")] OpenaiResponses,
    #[serde(rename = "anthropic-messages")] AnthropicMessages,
    #[serde(rename = "google-generative-ai")] GoogleGenerativeAi,
}

/// UI 分组。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export, type = "\"overseas\" | \"china\" | \"aggregator\" | \"local\" | \"codingPlan\""))]
pub enum EndpointGroup {
    #[serde(rename = "overseas")] Overseas,
    #[serde(rename = "china")] China,
    #[serde(rename = "aggregator")] Aggregator,
    #[serde(rename = "local")] Local,
    #[serde(rename = "codingPlan")] CodingPlan,
}

/// 模型能力。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ModelCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_input: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_output: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<bool>,
}

/// inkos 自维护的模型卡。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct InkosModel {
    pub id: String,
    #[serde(rename = "maxOutput")]
    pub max_output: u32,
    #[serde(rename = "contextWindowTokens")]
    pub context_window_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deployment_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub released_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<ModelStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replacement: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<ModelCapabilities>,
}

/// 模型生命周期状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export, type = "\"active\" | \"deprecated\" | \"disabled\" | \"nonText\""))]
pub enum ModelStatus {
    #[serde(rename = "active")] Active,
    #[serde(rename = "deprecated")] Deprecated,
    #[serde(rename = "disabled")] Disabled,
    #[serde(rename = "nonText")] NonText,
}

/// provider 兼容层标记。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ProviderCompat {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_store: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_system_role: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_developer_role: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requires_assistant_after_tool_result: Option<bool>,
}

/// provider 传输默认值。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
pub struct ProviderTransportDefaults {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_format: Option<TransportApiFormat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
}

/// 传输层 apiFormat（chat/responses，与 ApiProtocol 不同——这是 config 层）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export, type = "\"chat\" | \"responses\""))]
pub enum TransportApiFormat {
    #[serde(rename = "chat")] Chat,
    #[serde(rename = "responses")] Responses,
}

/// provider endpoint（元数据 + models 数组）。lookupModel/getEndpoint 的返回类型。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct InkosEndpoint {
    pub id: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<EndpointGroup>,
    pub api: ApiProtocol,
    pub base_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub models_base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub check_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature_range: Option<(f64, f64)>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub writing_temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature_hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compat: Option<ProviderCompat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transport_defaults: Option<ProviderTransportDefaults>,
    pub models: Vec<InkosModel>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_roundtrip() {
        let ep = InkosEndpoint {
            id: "deepseek".into(),
            label: "DeepSeek".into(),
            group: Some(EndpointGroup::Overseas),
            api: ApiProtocol::OpenaiCompletions,
            base_url: "https://api.deepseek.com".into(),
            models_base_url: None,
            check_model: Some("deepseek-chat".into()),
            temperature_range: Some((0.0, 2.0)),
            default_temperature: Some(1.0),
            writing_temperature: Some(1.5),
            temperature_hint: None,
            compat: None,
            transport_defaults: None,
            models: vec![InkosModel {
                id: "deepseek-chat".into(),
                max_output: 8192,
                context_window_tokens: 64000,
                enabled: Some(true),
                deployment_name: None,
                released_at: None,
                temperature: None,
                status: Some(ModelStatus::Active),
                replacement: None,
                capabilities: Some(ModelCapabilities { text: Some(true), tools: Some(true), image_input: None, image_output: None, reasoning: None }),
            }],
        };
        let json = serde_json::to_string(&ep).unwrap();
        let back: InkosEndpoint = serde_json::from_str(&json).unwrap();
        assert_eq!(ep, back);
        assert!(json.contains(r#""maxOutput":8192"#));
        assert!(json.contains(r#""contextWindowTokens":64000"#));
    }

    #[test]
    fn api_protocol_serializes() {
        assert_eq!(serde_json::to_string(&ApiProtocol::AnthropicMessages).unwrap(), "\"anthropic-messages\"");
    }
}
