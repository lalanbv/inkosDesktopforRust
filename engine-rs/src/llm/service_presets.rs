//! 服务预设（SERVICE_PRESETS 数据表 + 纯查询函数）。
//!
//! 移植自 `packages/core/src/llm/service-presets.ts`。当前移植：
//! - [`ServicePreset`] 类型 + [`SERVICE_PRESETS`] 静态表（14 服务）
//! - [`guess_service_from_base_url`]：按 hostname 反查 service id
//! - [`clamp_temperature`] / [`get_writing_temperature`] / [`resolve_service_models_base_url`]
//!
//! ## 待移植（依赖未移植模块）
//! resolveServicePreset（合并 endpoint bank + legacy，需 providers/）/ listModelsForService
//! （HTTP /models probe，需 reqwest）/ resolveServiceModel（pi-ai Model 构造）。
//! 当前纯函数仅用 legacy SERVICE_PRESETS 查询（endpoint 合并待 providers 域移植后补）。

use url::Url;

#[derive(Debug, Clone, Copy)]
pub struct ServicePreset {
    pub key: &'static str,
    pub provider_family: ProviderFamily,
    pub api: &'static str,
    pub base_url: &'static str,
    pub label: &'static str,
    pub temperature_range: Option<(f64, f64)>,
    pub default_temperature: Option<f64>,
    pub writing_temperature: Option<f64>,
    pub temperature_hint: Option<&'static str>,
    pub known_models: &'static [&'static str],
    pub pi_provider: Option<&'static str>,
    pub models_base_url: Option<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderFamily {
    Openai,
    Anthropic,
}

/// 14 服务预设（逐字移植 TS SERVICE_PRESETS）。
pub const SERVICE_PRESETS: &[ServicePreset] = &[
    ServicePreset { key: "openai", provider_family: ProviderFamily::Openai, api: "openai-responses", base_url: "https://api.openai.com/v1", label: "OpenAI", temperature_range: Some((0.0, 2.0)), default_temperature: Some(1.0), writing_temperature: Some(1.0), temperature_hint: None, known_models: &[], pi_provider: None, models_base_url: None },
    ServicePreset { key: "anthropic", provider_family: ProviderFamily::Anthropic, api: "anthropic-messages", base_url: "https://api.anthropic.com", label: "Anthropic", temperature_range: Some((0.0, 1.0)), default_temperature: Some(1.0), writing_temperature: Some(1.0), temperature_hint: Some("不要同时改 temperature 和 top_p"), known_models: &[], pi_provider: None, models_base_url: None },
    ServicePreset { key: "deepseek", provider_family: ProviderFamily::Openai, api: "openai-completions", base_url: "https://api.deepseek.com", label: "DeepSeek", temperature_range: Some((0.0, 2.0)), default_temperature: Some(1.0), writing_temperature: Some(1.5), temperature_hint: Some("创意写作推荐 1.5"), known_models: &[], pi_provider: None, models_base_url: None },
    ServicePreset { key: "moonshot", provider_family: ProviderFamily::Openai, api: "openai-completions", base_url: "https://api.moonshot.cn/v1", label: "Moonshot (Kimi)", temperature_range: Some((0.0, 1.0)), default_temperature: Some(0.3), writing_temperature: Some(1.0), temperature_hint: Some("kimi-k2.5 推荐 temperature=1.0"), known_models: &[], pi_provider: None, models_base_url: None },
    ServicePreset { key: "minimax", provider_family: ProviderFamily::Openai, api: "openai-completions", base_url: "https://api.minimaxi.com/v1", label: "MiniMax", temperature_range: Some((0.0, 2.0)), default_temperature: Some(0.9), writing_temperature: Some(0.9), temperature_hint: None, known_models: &["MiniMax-M3", "MiniMax-M2.7", "MiniMax-M2.7-highspeed", "MiniMax-M2.5", "MiniMax-M2.5-highspeed", "MiniMax-M2.1", "MiniMax-M2.1-highspeed", "MiniMax-M2"], pi_provider: None, models_base_url: None },
    ServicePreset { key: "bailian", provider_family: ProviderFamily::Anthropic, api: "anthropic-messages", base_url: "https://dashscope.aliyuncs.com/apps/anthropic", label: "百炼 (通义千问)", temperature_range: Some((0.0, 2.0)), default_temperature: Some(0.7), writing_temperature: Some(1.0), temperature_hint: None, known_models: &[], pi_provider: Some("anthropic"), models_base_url: None },
    ServicePreset { key: "zhipu", provider_family: ProviderFamily::Openai, api: "openai-completions", base_url: "https://open.bigmodel.cn/api/paas/v4", label: "智谱 GLM", temperature_range: Some((0.0, 1.0)), default_temperature: Some(0.95), writing_temperature: Some(0.95), temperature_hint: None, known_models: &[], pi_provider: Some("zai"), models_base_url: None },
    ServicePreset { key: "siliconflow", provider_family: ProviderFamily::Openai, api: "openai-completions", base_url: "https://api.siliconflow.cn/v1", label: "硅基流动", temperature_range: None, default_temperature: None, writing_temperature: None, temperature_hint: None, known_models: &[], pi_provider: None, models_base_url: None },
    ServicePreset { key: "ppio", provider_family: ProviderFamily::Openai, api: "openai-completions", base_url: "https://api.ppinfra.com/v3/openai", label: "PPIO", temperature_range: None, default_temperature: None, writing_temperature: None, temperature_hint: None, known_models: &[], pi_provider: None, models_base_url: None },
    ServicePreset { key: "openrouter", provider_family: ProviderFamily::Openai, api: "openai-responses", base_url: "https://openrouter.ai/api/v1", label: "OpenRouter", temperature_range: None, default_temperature: None, writing_temperature: None, temperature_hint: None, known_models: &[], pi_provider: Some("openrouter"), models_base_url: None },
    ServicePreset { key: "kkaiapi", provider_family: ProviderFamily::Openai, api: "openai-completions", base_url: "https://api.kkaiapi.com/v1", label: "kkaiapi", temperature_range: None, default_temperature: None, writing_temperature: None, temperature_hint: None, known_models: &[], pi_provider: None, models_base_url: Some("https://api.kkaiapi.com/v1") },
    ServicePreset { key: "ollama", provider_family: ProviderFamily::Openai, api: "openai-completions", base_url: "http://localhost:11434/v1", label: "Ollama (本地)", temperature_range: None, default_temperature: None, writing_temperature: None, temperature_hint: None, known_models: &[], pi_provider: None, models_base_url: None },
    ServicePreset { key: "lmstudio", provider_family: ProviderFamily::Openai, api: "openai-completions", base_url: "http://localhost:1234/v1", label: "LM Studio (本地)", temperature_range: None, default_temperature: None, writing_temperature: None, temperature_hint: None, known_models: &[], pi_provider: None, models_base_url: Some("http://localhost:1234/v1") },
    ServicePreset { key: "custom", provider_family: ProviderFamily::Openai, api: "openai-completions", base_url: "", label: "自定义端点", temperature_range: None, default_temperature: None, writing_temperature: None, temperature_hint: None, known_models: &[], pi_provider: None, models_base_url: None },
];

/// legacy 预设查询（不合并 endpoint bank，后者待 providers 域移植）。
pub fn service_preset(service: &str) -> Option<&'static ServicePreset> {
    SERVICE_PRESETS.iter().find(|p| p.key == service)
}

const DEFAULT_TEMPERATURE_RANGE: (f64, f64) = (0.0, 2.0);

/// 把 temperature 钳制到服务区间。
pub fn clamp_temperature(service: &str, temperature: f64) -> f64 {
    let (min, max) = service_preset(service)
        .and_then(|p| p.temperature_range)
        .unwrap_or(DEFAULT_TEMPERATURE_RANGE);
    temperature.max(min).min(max)
}

/// 写作温度：writingTemperature ?? defaultTemperature ?? 1.0。
pub fn get_writing_temperature(service: &str) -> f64 {
    let preset = service_preset(service);
    preset
        .and_then(|p| p.writing_temperature)
        .or_else(|| preset.and_then(|p| p.default_temperature))
        .unwrap_or(1.0)
}

/// modelsBaseUrl ?? baseUrl（用于 /models probe）。
pub fn resolve_service_models_base_url(service: &str) -> Option<&'static str> {
    let preset = service_preset(service)?;
    preset.models_base_url.or(Some(preset.base_url)).filter(|s| !s.is_empty())
}

/// 按 baseUrl hostname 反查 service id（custom/空 baseUrl 跳过）。
pub fn guess_service_from_base_url(base_url: &str) -> &'static str {
    for preset in SERVICE_PRESETS {
        if preset.key == "custom" || preset.base_url.is_empty() {
            continue;
        }
        if let Ok(url) = Url::parse(preset.base_url) {
            if let Some(host) = url.host_str() {
                if base_url.contains(host) {
                    return preset.key;
                }
            }
        }
    }
    "custom"
}

/// service → pi provider 映射（piProvider ?? providerFamily；google 特例）。
pub fn service_to_pi_provider(service: &str) -> Option<&'static str> {
    if service == "google" {
        return Some("google");
    }
    let preset = service_preset(service)?;
    if service == "custom" {
        return None;
    }
    Some(preset.pi_provider.unwrap_or(match preset.provider_family {
        ProviderFamily::Openai => "openai",
        ProviderFamily::Anthropic => "anthropic",
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preset_lookup() {
        assert_eq!(service_preset("deepseek").unwrap().label, "DeepSeek");
        assert!(service_preset("nonexistent").is_none());
    }

    #[test]
    fn clamp_to_range() {
        // deepseek range [0,2]
        assert_eq!(clamp_temperature("deepseek", 5.0), 2.0);
        assert_eq!(clamp_temperature("deepseek", -1.0), 0.0);
        assert_eq!(clamp_temperature("deepseek", 1.5), 1.5);
        // 无 range 的服务用默认 [0,2]
        assert_eq!(clamp_temperature("siliconflow", 5.0), 2.0);
    }

    #[test]
    fn writing_temperature_fallbacks() {
        // deepseek writingTemperature=1.5
        assert_eq!(get_writing_temperature("deepseek"), 1.5);
        // siliconflow 无 writing/default → 1.0
        assert_eq!(get_writing_temperature("siliconflow"), 1.0);
        // 未知服务 → 1.0
        assert_eq!(get_writing_temperature("unknown"), 1.0);
    }

    #[test]
    fn guess_by_hostname() {
        assert_eq!(guess_service_from_base_url("https://api.deepseek.com/v1"), "deepseek");
        assert_eq!(guess_service_from_base_url("https://api.moonshot.cn/v1/chat"), "moonshot");
        assert_eq!(guess_service_from_base_url("http://localhost:11434/v1"), "ollama");
        assert_eq!(guess_service_from_base_url("https://unknown.example.com"), "custom");
    }

    #[test]
    fn models_base_url_resolution() {
        assert_eq!(resolve_service_models_base_url("kkaiapi"), Some("https://api.kkaiapi.com/v1"));
        assert_eq!(resolve_service_models_base_url("deepseek"), Some("https://api.deepseek.com"));
        assert_eq!(resolve_service_models_base_url("custom"), None); // 空 baseUrl
    }

    #[test]
    fn pi_provider_mapping() {
        assert_eq!(service_to_pi_provider("openai"), Some("openai"));
        assert_eq!(service_to_pi_provider("zhipu"), Some("zai"));
        assert_eq!(service_to_pi_provider("bailian"), Some("anthropic"));
        assert_eq!(service_to_pi_provider("google"), Some("google"));
        assert_eq!(service_to_pi_provider("custom"), None);
    }
}
