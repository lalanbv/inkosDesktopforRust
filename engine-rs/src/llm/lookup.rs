//! provider bank lookup（lookupModel 两层查找 + 优先级 + 模型可用性判定）。
//!
//! 移植自 `packages/core/src/llm/providers/lookup.ts`。
//! registry 作为参数传入（解耦数据源）；实际 ~40 provider 数据待后续逐文件移植。
//!
//! lookupModel 策略（对齐 TS）：
//! - Layer 1：已知 service 精确查（整串比较，不拆斜线）
//! - Layer 2：全局扫所有 provider，按 provider 优先级取首条
//! - 都 miss：None（调用方走保守默认）
//!
//! 不做斜线前缀拆分（对 PPIO/SiliconCloud 原生命名会误匹配）。

use super::providers::{InkosEndpoint, InkosModel, ModelStatus};

/// provider id 优先级（同 modelId 多 provider 命中时按此顺序取首）。白名单外视同 999。
pub const PROVIDER_PRIORITY: &[&str] = &[
    "anthropic", "openai", "google", "deepseek", "bailian", "moonshot", "kimicode",
    "zhipu", "minimax", "xai", "siliconcloud", "openrouter", "aihubmix", "novita",
];

fn priority_of(provider_id: &str) -> usize {
    PROVIDER_PRIORITY.iter().position(|p| *p == provider_id).unwrap_or(999)
}

/// 按 serviceId + modelId 查模型卡（两层 + 优先级）。
pub fn lookup_model<'a>(registry: &'a [InkosEndpoint], service_id: &str, model_id: &str) -> Option<&'a InkosModel> {
    let lower = model_id.to_lowercase();
    // Layer 1：已知 service 精确查
    if let Some(ep) = get_endpoint(registry, service_id) {
        if let Some(hit) = ep.models.iter().find(|m| m.id.to_lowercase() == lower) {
            return Some(hit);
        }
    }
    // Layer 2：全局扫，按 provider 优先级
    let mut matches: Vec<(&InkosModel, &str)> = Vec::new();
    for p in registry {
        if let Some(hit) = p.models.iter().find(|m| m.id.to_lowercase() == lower) {
            matches.push((hit, p.id.as_str()));
        }
    }
    if matches.is_empty() {
        return None;
    }
    matches.sort_by_key(|(_, pid)| priority_of(pid));
    Some(matches.first().unwrap().0)
}

/// 按 serviceId 查 endpoint。
pub fn get_endpoint<'a>(registry: &'a [InkosEndpoint], service_id: &str) -> Option<&'a InkosEndpoint> {
    registry.iter().find(|e| e.id == service_id)
}

/// 某 service 下可用（enabled !== false）的模型列表。
pub fn list_enabled_models<'a>(registry: &'a [InkosEndpoint], service_id: &str) -> Vec<&'a InkosModel> {
    let Some(ep) = get_endpoint(registry, service_id) else { return Vec::new() };
    ep.models.iter().filter(|m| m.enabled != Some(false)).collect()
}

/// 模型是否活跃文本模型（enabled + 状态 + text 能力）。
pub fn is_active_text_model(model: &InkosModel) -> bool {
    if model.enabled == Some(false) {
        return false;
    }
    if matches!(model.status, Some(ModelStatus::Disabled) | Some(ModelStatus::Deprecated) | Some(ModelStatus::NonText)) {
        return false;
    }
    if model.capabilities.as_ref().and_then(|c| c.text) == Some(false) {
        return false;
    }
    // imageOutput=true 且 text 未显式 true → 视为非文本模型
    if model.capabilities.as_ref().and_then(|c| c.image_output) == Some(true)
        && model.capabilities.as_ref().and_then(|c| c.text) != Some(true)
    {
        return false;
    }
    true
}

/// 某 service 下活跃文本模型列表。
pub fn list_active_text_models<'a>(registry: &'a [InkosEndpoint], service_id: &str) -> Vec<&'a InkosModel> {
    let Some(ep) = get_endpoint(registry, service_id) else { return Vec::new() };
    ep.models.iter().filter(|m| is_active_text_model(m)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::providers::{ApiProtocol, InkosEndpoint, InkosModel, ModelCapabilities, ModelStatus};

    fn model(id: &str, enabled: Option<bool>, status: Option<ModelStatus>, caps: Option<ModelCapabilities>) -> InkosModel {
        InkosModel { id: id.into(), max_output: 8192, context_window_tokens: 64000, enabled, deployment_name: None, released_at: None, temperature: None, status, replacement: None, capabilities: caps }
    }
    fn ep(id: &str, models: Vec<InkosModel>) -> InkosEndpoint {
        InkosEndpoint { id: id.into(), label: id.into(), group: None, api: ApiProtocol::OpenaiCompletions, base_url: format!("https://{id}"), models_base_url: None, check_model: None, temperature_range: None, default_temperature: None, writing_temperature: None, temperature_hint: None, compat: None, transport_defaults: None, models }
    }

    fn sample_registry() -> Vec<InkosEndpoint> {
        vec![
            ep("deepseek", vec![model("deepseek-chat", Some(true), Some(ModelStatus::Active), None), model("deepseek-reasoner", Some(true), Some(ModelStatus::Active), None)]),
            ep("openai", vec![model("gpt-4o", Some(true), Some(ModelStatus::Active), None)]),
            // 同一 modelId 在两个 provider（测优先级）
            ep("openrouter", vec![model("gpt-4o", Some(true), Some(ModelStatus::Active), None)]),
        ]
    }

    #[test]
    fn layer1_known_service() {
        let reg = sample_registry();
        let m = lookup_model(&reg, "deepseek", "deepseek-chat").unwrap();
        assert_eq!(m.id, "deepseek-chat");
    }

    #[test]
    fn layer2_global_scan_with_priority() {
        let reg = sample_registry();
        // gpt-4o 同时在 openai 与 openrouter → openai 优先级更高
        let m = lookup_model(&reg, "custom", "gpt-4o").unwrap();
        // 应命中（具体哪个 provider 由优先级，openai 排前）
        assert_eq!(m.id, "gpt-4o");
    }

    #[test]
    fn layer2_priority_order() {
        // openai(1) < openrouter(12)
        let p_openai = priority_of("openai");
        let p_openrouter = priority_of("openrouter");
        assert!(p_openai < p_openrouter);
    }

    #[test]
    fn miss_returns_none() {
        let reg = sample_registry();
        assert!(lookup_model(&reg, "deepseek", "nonexistent").is_none());
    }

    #[test]
    fn case_insensitive_match() {
        let reg = sample_registry();
        assert!(lookup_model(&reg, "deepseek", "DEEPSEEK-CHAT").is_some());
    }

    #[test]
    fn list_enabled_filters_disabled() {
        let reg = vec![ep("x", vec![model("a", Some(true), None, None), model("b", Some(false), None, None)])];
        let enabled = list_enabled_models(&reg, "x");
        assert_eq!(enabled.len(), 1);
        assert_eq!(enabled[0].id, "a");
    }

    #[test]
    fn is_active_text_filters_status_and_caps() {
        assert!(!is_active_text_model(&model("a", Some(false), None, None))); // disabled
        assert!(!is_active_text_model(&model("a", Some(true), Some(ModelStatus::Disabled), None)));
        assert!(!is_active_text_model(&model("a", Some(true), None, Some(ModelCapabilities { text: Some(false), image_input: None, image_output: None, tools: None, reasoning: None }))));
        assert!(is_active_text_model(&model("a", Some(true), Some(ModelStatus::Active), None)));
        // imageOutput=true 且 text 未设 → 非文本
        assert!(!is_active_text_model(&model("a", Some(true), None, Some(ModelCapabilities { text: None, image_input: None, image_output: Some(true), tools: None, reasoning: None }))));
    }
}
