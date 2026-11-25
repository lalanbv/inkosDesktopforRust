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
    "anthropic",
    "openai",
    "google",
    "deepseek",
    "bailian",
    "moonshot",
    "kimicode",
    "zhipu",
    "minimax",
    "xai",
    "siliconcloud",
    "openrouter",
    "aihubmix",
    "novita",
];

fn priority_of(provider_id: &str) -> usize {
    PROVIDER_PRIORITY
        .iter()
        .position(|p| *p == provider_id)
        .unwrap_or(999)
}

/// 按 serviceId + modelId 查模型卡（两层 + 优先级）。
pub fn lookup_model<'a>(
    registry: &'a [InkosEndpoint],
    service_id: &str,
    model_id: &str,
) -> Option<&'a InkosModel> {
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

/// 模型卡标了服务端固定温度的（如 Moonshot kimi-k2.5 / kimi-k2-thinking 系列
/// API 硬要求 `temperature === 1`，其它值直接 400）统一钳到卡上的值——TS
/// `clampTemperatureForModel` 的对应物（TS 挂在 `client.chat` 边界；这里是
/// 传输层 `stream_chat` 漏斗，产品引擎全部模型调用都经过）。
///
/// - 未指定（`None`）仍不指定：用模型默认，不替调用方 invent 一个值；
/// - 卡上没有固定温度：原值返回；
/// - 查找次序与 [`lookup_model`] 相同（已知服务精确查 → 按 provider 优先级
///   全局扫），但**不分配**——每次模型调用都会走到这里。
pub fn clamp_temperature_for_model(
    registry: &[InkosEndpoint],
    service_id: &str,
    model_id: &str,
    requested: Option<f64>,
) -> Option<f64> {
    let requested = requested?;
    fn find<'e>(endpoint: &'e InkosEndpoint, model_id: &str) -> Option<&'e InkosModel> {
        endpoint
            .models
            .iter()
            .find(|model| model.id.eq_ignore_ascii_case(model_id))
    }
    let card = get_endpoint(registry, service_id)
        .and_then(|endpoint| find(endpoint, model_id))
        .or_else(|| {
            registry
                .iter()
                .filter_map(|endpoint| {
                    find(endpoint, model_id).map(|model| (model, endpoint.id.as_str()))
                })
                .min_by_key(|(_, provider)| priority_of(provider))
                .map(|(model, _)| model)
        });
    card.and_then(|model| model.temperature).or(Some(requested))
}

/// 按 serviceId 查 endpoint。
pub fn get_endpoint<'a>(
    registry: &'a [InkosEndpoint],
    service_id: &str,
) -> Option<&'a InkosEndpoint> {
    registry.iter().find(|e| e.id == service_id)
}

/// 某 service 下可用（enabled !== false）的模型列表。
pub fn list_enabled_models<'a>(
    registry: &'a [InkosEndpoint],
    service_id: &str,
) -> Vec<&'a InkosModel> {
    let Some(ep) = get_endpoint(registry, service_id) else {
        return Vec::new();
    };
    ep.models
        .iter()
        .filter(|m| m.enabled != Some(false))
        .collect()
}

/// 模型是否活跃文本模型（enabled + 状态 + text 能力）。
pub fn is_active_text_model(model: &InkosModel) -> bool {
    if model.enabled == Some(false) {
        return false;
    }
    if matches!(
        model.status,
        Some(ModelStatus::Disabled) | Some(ModelStatus::Deprecated) | Some(ModelStatus::NonText)
    ) {
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
pub fn list_active_text_models<'a>(
    registry: &'a [InkosEndpoint],
    service_id: &str,
) -> Vec<&'a InkosModel> {
    let Some(ep) = get_endpoint(registry, service_id) else {
        return Vec::new();
    };
    ep.models
        .iter()
        .filter(|m| is_active_text_model(m))
        .collect()
}

/// 生效模型的上下文窗口（441 号：TS `createLLMClient` 的
/// `modelCard?.contextWindowTokens ?? 128_000` 镜像——内建卡两层查找，
/// miss 回退 128k 缺省；`contextBudgetFromClient` 以此为预算口径）。
pub fn builtin_context_window(service_id: &str, model_id: &str) -> u64 {
    let registry = super::registry::builtin_endpoints();
    lookup_model(&registry, service_id, model_id)
        .map(|m| m.context_window_tokens as u64)
        .unwrap_or(128_000)
}

#[cfg(test)]
mod tests {
    use super::super::providers::{
        ApiProtocol, InkosEndpoint, InkosModel, ModelCapabilities, ModelStatus,
    };
    use super::*;

    fn model(
        id: &str,
        enabled: Option<bool>,
        status: Option<ModelStatus>,
        caps: Option<ModelCapabilities>,
    ) -> InkosModel {
        InkosModel {
            id: id.into(),
            max_output: 8192,
            context_window_tokens: 64000,
            enabled,
            deployment_name: None,
            released_at: None,
            temperature: None,
            status,
            replacement: None,
            capabilities: caps,
        }
    }
    fn ep(id: &str, models: Vec<InkosModel>) -> InkosEndpoint {
        InkosEndpoint {
            id: id.into(),
            label: id.into(),
            group: None,
            api: ApiProtocol::OpenaiCompletions,
            base_url: format!("https://{id}"),
            models_base_url: None,
            check_model: None,
            temperature_range: None,
            default_temperature: None,
            writing_temperature: None,
            temperature_hint: None,
            compat: None,
            transport_defaults: None,
            models,
        }
    }

    fn sample_registry() -> Vec<InkosEndpoint> {
        vec![
            ep(
                "deepseek",
                vec![
                    model("deepseek-chat", Some(true), Some(ModelStatus::Active), None),
                    model(
                        "deepseek-reasoner",
                        Some(true),
                        Some(ModelStatus::Active),
                        None,
                    ),
                ],
            ),
            ep(
                "openai",
                vec![model("gpt-4o", Some(true), Some(ModelStatus::Active), None)],
            ),
            // 同一 modelId 在两个 provider（测优先级）
            ep(
                "openrouter",
                vec![model("gpt-4o", Some(true), Some(ModelStatus::Active), None)],
            ),
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

    /// 441 号：预算窗口口径——内建卡两层查（含 Layer 2 全局扫），
    /// 双 miss 回退 128k 缺省（TS createLLMClient 逐字对齐）。
    #[test]
    fn builtin_context_window_known_card_and_fallback() {
        assert_eq!(
            super::builtin_context_window("deepseek", "deepseek-chat"),
            1_000_000
        );
        // Layer 2：service 未知名但模型 id 全局命中
        assert_eq!(
            super::builtin_context_window("unknown-service", "deepseek-chat"),
            1_000_000
        );
        // 双 miss → 128k 缺省
        assert_eq!(
            super::builtin_context_window("custom", "lm-mock-model"),
            128_000
        );
    }

    #[test]
    fn list_enabled_filters_disabled() {
        let reg = vec![ep(
            "x",
            vec![
                model("a", Some(true), None, None),
                model("b", Some(false), None, None),
            ],
        )];
        let enabled = list_enabled_models(&reg, "x");
        assert_eq!(enabled.len(), 1);
        assert_eq!(enabled[0].id, "a");
    }

    #[test]
    fn is_active_text_filters_status_and_caps() {
        assert!(!is_active_text_model(&model("a", Some(false), None, None))); // disabled
        assert!(!is_active_text_model(&model(
            "a",
            Some(true),
            Some(ModelStatus::Disabled),
            None
        )));
        assert!(!is_active_text_model(&model(
            "a",
            Some(true),
            None,
            Some(ModelCapabilities {
                text: Some(false),
                image_input: None,
                image_output: None,
                tools: None,
                reasoning: None
            })
        )));
        assert!(is_active_text_model(&model(
            "a",
            Some(true),
            Some(ModelStatus::Active),
            None
        )));
        // imageOutput=true 且 text 未设 → 非文本
        assert!(!is_active_text_model(&model(
            "a",
            Some(true),
            None,
            Some(ModelCapabilities {
                text: None,
                image_input: None,
                image_output: Some(true),
                tools: None,
                reasoning: None
            })
        )));
    }

    /// 模型卡固定温度钳制：指定了才钳；未指定仍不指定；卡上无固定温度原值透传。
    #[test]
    fn clamp_temperature_for_model_follows_the_card() {
        let bank = crate::llm::providers_bank::get_all_endpoints();
        assert_eq!(
            clamp_temperature_for_model(bank, "", "kimi-k2.5", Some(0.3)),
            Some(1.0),
            "卡上标了固定温度的模型：其它值一律钳到卡上的值"
        );
        assert_eq!(
            clamp_temperature_for_model(bank, "bailian", "kimi-k2.5", Some(0.3)),
            Some(1.0),
            "已知服务精确查"
        );
        assert_eq!(
            clamp_temperature_for_model(bank, "", "kimi-k2.5", None),
            None,
            "未指定温度不替调用方 invent 一个值"
        );
        assert_eq!(
            clamp_temperature_for_model(bank, "", "KIMI-K2.5", Some(1.0)),
            Some(1.0)
        );
        assert_eq!(
            clamp_temperature_for_model(bank, "", "no-such-model", Some(0.3)),
            Some(0.3),
            "没有卡：原值"
        );
    }

    /// 钳制用的查找与 `lookup_model` 同序（不分配的实现 vs 原实现）：对全量
    /// 模型卡逐个核对，两条路径命中同一张卡。
    #[test]
    fn clamp_lookup_agrees_with_lookup_model_across_the_bank() {
        let bank = crate::llm::providers_bank::get_all_endpoints();
        let mut checked = 0usize;
        for endpoint in bank {
            for model in &endpoint.models {
                let Some(locked) = model.temperature else {
                    continue;
                };
                let via_lookup =
                    lookup_model(bank, "", &model.id).and_then(|card| card.temperature);
                let via_clamp = clamp_temperature_for_model(bank, "", &model.id, Some(0.123));
                assert_eq!(via_lookup, Some(locked), "{}", model.id);
                assert_eq!(via_clamp, Some(locked), "{}", model.id);
                checked += 1;
            }
        }
        assert!(checked >= 10, "全量模型卡上固定温度的条目数不对：{checked}");
    }
}
