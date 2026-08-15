//! 模型探针（live /models + bank 交叉）。
//!
//! 移植自 `packages/core/src/llm/providers/probe.ts`（35 行）与
//! `service-presets.ts` 的 `listModelsForService`（R4 精修版）。

use std::time::Duration;

use serde_json::Value;

use crate::llm::lookup::lookup_model;
use crate::llm::providers_bank::{get_all_endpoints, get_endpoint};
use crate::llm::service_presets::{service_preset, ProviderFamily};
use crate::llm::providers::ApiProtocol;

/// live probe 得到的模型。
#[derive(Debug, Clone, PartialEq)]
pub struct ProbedModel {
    pub id: String,
    pub name: String,
    pub context_window: u32,
}

/// 聚合后的模型信息。对齐 TS `ModelInfo`。
#[derive(Debug, Clone, PartialEq)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub context_window: u32,
    pub max_output: Option<u32>,
}

/// 通用 OpenAI 兼容 `/models` 探针。任何失败一律返回空数组（不抛异常）。
/// 对齐 TS `probeModelsFromUpstream`。
pub async fn probe_models_from_upstream(
    base_url: &str,
    api_key: &str,
    timeout_ms: u64,
) -> Vec<ProbedModel> {
    if base_url.is_empty() {
        return Vec::new();
    }
    let models_url = format!("{}/models", base_url.trim_end_matches('/'));
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_millis(timeout_ms))
        .build()
    {
        Ok(client) => client,
        Err(_) => return Vec::new(),
    };
    let mut request = client.get(&models_url);
    if !api_key.is_empty() {
        request = request.bearer_auth(api_key);
    }
    let response = match request.send().await {
        Ok(response) => response,
        Err(_) => return Vec::new(),
    };
    if !response.status().is_success() {
        return Vec::new();
    }
    let json: Value = match response.json().await {
        Ok(json) => json,
        Err(_) => return Vec::new(),
    };
    let Some(data) = json.get("data").and_then(Value::as_array) else {
        return Vec::new();
    };
    data.iter()
        .filter_map(|m| {
            let id = m.get("id").and_then(Value::as_str)?;
            if id.is_empty() {
                return None;
            }
            Some(ProbedModel {
                id: id.to_string(),
                name: id.to_string(),
                context_window: 0,
            })
        })
        .collect()
}

fn provider_family_of(service: &str) -> ProviderFamily {
    if let Some(endpoint) = get_endpoint(service) {
        return match endpoint.api {
            ApiProtocol::AnthropicMessages => ProviderFamily::Anthropic,
            _ => ProviderFamily::Openai,
        };
    }
    service_preset(service)
        .map(|p| p.provider_family)
        .unwrap_or(ProviderFamily::Openai)
}

/// `listModelsForService`：live `/models` probe → provider bank fallback →
/// legacy knownModels fallback（逐层，去重保持首现序）。
pub async fn list_models_for_service(
    service: &str,
    api_key: Option<&str>,
    live_base_url: Option<&str>,
) -> Vec<ModelInfo> {
    let provider = get_endpoint(service);
    let preset = service_preset(service);
    if provider.is_none() && preset.is_none() {
        return Vec::new();
    }

    let mut by_id: Vec<(String, ModelInfo)> = Vec::new();

    // 1) live /models probe
    let probe_base_url = live_base_url
        .map(|s| s.to_string())
        .or_else(|| {
            provider.map(|p| {
                p.models_base_url
                    .clone()
                    .unwrap_or_else(|| p.base_url.clone())
            })
        })
        .or_else(|| {
            preset
                .map(|p| p.models_base_url.unwrap_or(p.base_url))
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
        });
    let family = preset
        .map(|p| p.provider_family)
        .unwrap_or_else(|| provider_family_of(service));
    let can_probe_without_key = probe_base_url
        .as_deref()
        .map(|url| crate::utils::llm_endpoint_auth::is_api_key_optional_for_endpoint(family_str(family), Some(url)))
        .unwrap_or(false);
    if let Some(base_url) = probe_base_url.as_deref() {
        let has_key = api_key.map(|k| !k.is_empty()).unwrap_or(false);
        if has_key || can_probe_without_key {
            let key = api_key.unwrap_or("");
            let probed = probe_models_from_upstream(base_url, key, 10_000).await;
            if !probed.is_empty() {
                let registry = get_all_endpoints();
                for m in probed {
                    let info = match lookup_model(registry, service, &m.id) {
                        Some(card) => ModelInfo {
                            id: card.id.clone(),
                            name: card.id.clone(),
                            context_window: card.context_window_tokens,
                            max_output: Some(card.max_output),
                        },
                        None => ModelInfo {
                            id: m.id.clone(),
                            name: m.name.clone(),
                            context_window: m.context_window,
                            max_output: None,
                        },
                    };
                    if !by_id.iter().any(|(id, _)| *id == info.id) {
                        by_id.push((info.id.clone(), info));
                    }
                }
            }
        }
    }

    // 2) provider bank fallback / 补充
    if let Some(provider) = provider {
        for m in &provider.models {
            if m.enabled == Some(false) {
                continue;
            }
            if by_id.iter().any(|(id, _)| *id == m.id) {
                continue;
            }
            by_id.push((
                m.id.clone(),
                ModelInfo {
                    id: m.id.clone(),
                    name: m.id.clone(),
                    context_window: m.context_window_tokens,
                    max_output: Some(m.max_output),
                },
            ));
        }
    }

    // 3) legacy knownModels fallback
    if by_id.is_empty() {
        if let Some(known) = preset.map(|p| p.known_models) {
            for id in known {
                by_id.push((
                    (*id).to_string(),
                    ModelInfo {
                        id: (*id).to_string(),
                        name: (*id).to_string(),
                        context_window: 0,
                        max_output: None,
                    },
                ));
            }
        }
    }

    by_id.into_iter().map(|(_, info)| info).collect()
}

fn family_str(family: ProviderFamily) -> &'static str {
    match family {
        ProviderFamily::Openai => "openai",
        ProviderFamily::Anthropic => "anthropic",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn empty_base_url_returns_empty() {
        assert!(probe_models_from_upstream("", "key", 100).await.is_empty());
    }

    #[tokio::test]
    async fn unreachable_host_returns_empty() {
        let out = probe_models_from_upstream("http://127.0.0.1:9", "key", 300).await;
        assert!(out.is_empty(), "网络失败一律空数组");
    }

    #[tokio::test]
    async fn unknown_service_returns_empty() {
        assert!(list_models_for_service("nope-service", None, None).await.is_empty());
    }

    #[tokio::test]
    async fn bank_fallback_without_probe() {
        // deepseek 无 key 探针跳过（公网 hostname）→ bank fallback
        let models = list_models_for_service("deepseek", None, None).await;
        assert!(!models.is_empty());
        assert!(models.iter().any(|m| m.id == "deepseek-v4-flash"));
        // bank 卡带 maxOutput
        let flash = models.iter().find(|m| m.id == "deepseek-v4-flash").unwrap();
        assert_eq!(flash.max_output, Some(393216));
    }
}
