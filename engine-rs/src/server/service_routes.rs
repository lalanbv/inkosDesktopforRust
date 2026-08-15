//! services / cover 域端点（server.ts L3705-L4177 / L3854-L3953）。
//!
//! - services 列表/配置/密钥/模型发现（11 端点）
//! - cover 封面配置/密钥（4 端点）
//!
//! 错误形态注意：本域 400 多为**平铺** `{error: 文案}` 或 `{ok:false, error}`
//! （非 ApiError 结构）；`/test` 为两步验证 B12 形状 `{probe, chat}`。

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use axum::body::Bytes;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde_json::{json, Map, Value};

use crate::llm::cover_providers::{
    cover_secret_key, normalize_cover_base_url, resolve_cover_provider_preset,
    COVER_PROVIDER_PRESETS,
};
use crate::llm::probe::{list_models_for_service, probe_models_from_upstream};
use crate::llm::providers_bank::{get_all_endpoints, get_endpoint};
use crate::llm::secrets::{load_secrets, save_secrets};
use crate::llm::service_presets::{
    guess_service_from_base_url, resolve_service_preset, resolve_service_provider_family,
    ProviderFamily,
};
use crate::server::books_routes::BooksRuntime;
use crate::server::project_config_routes::{load_raw_config, save_raw_config};
use crate::utils::llm_endpoint_auth::is_api_key_optional_for_endpoint;
use crate::utils::llm_env::{global_env_path, read_env_config_values};

// ── 服务配置条目（server.ts ServiceConfigEntry + 归一/合并） ────────

/// 服务配置条目（serde camelCase 对齐 TS ServiceConfigEntry 序列化形态）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ServiceConfigEntry {
    pub service: String,
    pub name: Option<String>,
    pub base_url: Option<String>,
    pub temperature: Option<f64>,
    pub max_tokens: Option<f64>,
    pub api_format: Option<String>,
    pub stream: Option<bool>,
}

impl ServiceConfigEntry {
    fn to_json(&self) -> Value {
        let mut map = Map::new();
        map.insert("service".into(), json!(self.service));
        if let Some(name) = &self.name {
            map.insert("name".into(), json!(name));
        }
        if let Some(base_url) = &self.base_url {
            if !base_url.is_empty() {
                map.insert("baseUrl".into(), json!(base_url));
            }
        }
        if let Some(temperature) = self.temperature {
            map.insert("temperature".into(), json!(temperature));
        }
        if let Some(max_tokens) = self.max_tokens {
            map.insert("maxTokens".into(), json!(max_tokens));
        }
        if let Some(api_format) = &self.api_format {
            map.insert("apiFormat".into(), json!(api_format));
        }
        if let Some(stream) = self.stream {
            map.insert("stream".into(), json!(stream));
        }
        Value::Object(map)
    }

    fn from_json(entry: &Value) -> Self {
        let str_field = |key: &str| entry.get(key).and_then(Value::as_str).map(|s| s.to_string());
        ServiceConfigEntry {
            service: str_field("service").filter(|s| !s.is_empty()).unwrap_or_else(|| "custom".into()),
            name: str_field("name").filter(|s| !s.is_empty()),
            base_url: str_field("baseUrl").filter(|s| !s.is_empty()),
            temperature: entry.get("temperature").and_then(Value::as_f64),
            max_tokens: entry.get("maxTokens").and_then(Value::as_f64),
            api_format: str_field("apiFormat").filter(|v| v == "chat" || v == "responses"),
            stream: entry.get("stream").and_then(Value::as_bool),
        }
    }
}

/// `custom:Name` / `custom`。对齐 TS `isCustomServiceId`。
fn is_custom_service_id(service_id: &str) -> bool {
    service_id == "custom" || service_id.starts_with("custom:")
}

/// 条目存储键：custom → `custom:{name ?? "Custom"}`。对齐 TS `serviceConfigKey`。
fn service_config_key(entry: &ServiceConfigEntry) -> String {
    if entry.service == "custom" {
        format!("custom:{}", entry.name.as_deref().unwrap_or("Custom"))
    } else {
        entry.service.clone()
    }
}

/// 对象形态键归一（`custom:Name` → name 解码；`custom` → name 字段）。
/// 对齐 TS `normalizeServiceEntry`。
fn normalize_service_entry(service_id: &str, value: &Value) -> ServiceConfigEntry {
    let str_field = |key: &str| value.get(key).and_then(Value::as_str).map(|s| s.to_string());
    let optional_base = || str_field("baseUrl").filter(|s| !s.is_empty());
    let temperature = value.get("temperature").and_then(Value::as_f64);
    let max_tokens = value.get("maxTokens").and_then(Value::as_f64);
    let api_format = str_field("apiFormat").filter(|v| v == "chat" || v == "responses");
    let stream = value.get("stream").and_then(Value::as_bool);

    if let Some(encoded) = service_id.strip_prefix("custom:") {
        let name = percent_decode(encoded).unwrap_or_else(|| encoded.to_string());
        return ServiceConfigEntry {
            service: "custom".into(),
            name: Some(name),
            base_url: optional_base(),
            temperature,
            max_tokens,
            api_format,
            stream,
        };
    }
    if service_id == "custom" {
        return ServiceConfigEntry {
            service: "custom".into(),
            name: str_field("name").filter(|s| !s.is_empty()),
            base_url: optional_base(),
            temperature,
            max_tokens,
            api_format,
            stream,
        };
    }
    ServiceConfigEntry {
        service: service_id.to_string(),
        name: None,
        base_url: optional_base(),
        temperature,
        max_tokens,
        api_format,
        stream,
    }
}

fn percent_decode(value: &str) -> Option<String> {
    let decoded = percent_encoding::percent_decode_str(value).decode_utf8().ok()?;
    Some(decoded.into_owned())
}

/// `normalizeServiceConfig`：数组形态 / 对象形态（服务 id 为键）/ 其他 → 空。
pub fn normalize_service_config(raw: Option<&Value>) -> Vec<ServiceConfigEntry> {
    match raw {
        Some(Value::Array(items)) => items
            .iter()
            .filter(|entry| entry.is_object())
            .map(ServiceConfigEntry::from_json)
            .collect(),
        Some(Value::Object(map)) => map
            .iter()
            .filter(|(_, value)| value.is_object())
            .map(|(service_id, value)| normalize_service_entry(service_id, value))
            .collect(),
        _ => Vec::new(),
    }
}

/// `mergeServiceConfig`：updates 按 key 覆盖 existing（保持 existing 首现序）。
fn merge_service_config(
    existing: Vec<ServiceConfigEntry>,
    updates: Vec<ServiceConfigEntry>,
) -> Vec<ServiceConfigEntry> {
    let mut merged: Vec<ServiceConfigEntry> = existing;
    for update in updates {
        let key = service_config_key(&update);
        match merged.iter().position(|e| service_config_key(e) == key) {
            Some(index) => merged[index] = update,
            None => merged.push(update),
        }
    }
    merged
}

/// `normalizeConfigSource`：仅 "studio" 保留，其余回 "env"。
fn normalize_config_source(value: Option<&Value>) -> &'static str {
    if value == Some(&json!("studio")) {
        "studio"
    } else {
        "env"
    }
}

/// `syncTopLevelLlmMirror`：选中服务的 family/baseUrl/model 等镜像到 llm 顶层
/// （54 号备案的功能性缺口，本轮补齐）。
pub fn sync_top_level_llm_mirror(llm: &mut Map<String, Value>) {
    let Some(selected_service) = llm.get("service").and_then(Value::as_str).map(|s| s.to_string()) else {
        return;
    };
    let services = normalize_service_config(llm.get("services"));
    let selected_entry = services
        .iter()
        .find(|entry| service_config_key(entry) == selected_service)
        .cloned()
        .or_else(|| {
            (!is_custom_service_id(&selected_service)).then(|| ServiceConfigEntry {
                service: selected_service.clone(),
                ..Default::default()
            })
        });
    let Some(entry) = selected_entry else {
        return;
    };

    let preset = resolve_service_preset(&entry.service);
    let family = resolve_service_provider_family(&entry.service).unwrap_or(ProviderFamily::Openai);
    llm.insert(
        "provider".into(),
        json!(match family {
            ProviderFamily::Openai => "openai",
            ProviderFamily::Anthropic => "anthropic",
        }),
    );
    let base_url = entry
        .base_url
        .clone()
        .or_else(|| preset.map(|p| p.base_url))
        .unwrap_or_default();
    llm.insert("baseUrl".into(), json!(base_url));

    let default_model = llm
        .get("defaultModel")
        .and_then(Value::as_str)
        .map(|m| m.trim().to_string())
        .unwrap_or_default();
    if !default_model.is_empty() {
        llm.insert("model".into(), json!(default_model));
    }
    if let Some(temperature) = entry.temperature {
        llm.insert("temperature".into(), json!(temperature));
    }
    if let Some(api_format) = &entry.api_format {
        llm.insert("apiFormat".into(), json!(api_format));
    }
    if let Some(stream) = entry.stream {
        llm.insert("stream".into(), json!(stream));
    }
}

/// `resolveConfiguredServiceBaseUrl`：inline → 预设 → custom 的 config baseUrl。
async fn resolve_configured_service_base_url(
    root: &Path,
    service_id: &str,
    inline_base_url: Option<&str>,
) -> Option<String> {
    if let Some(inline) = inline_base_url {
        let trimmed = inline.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    if !is_custom_service_id(service_id) {
        return resolve_service_preset(service_id).map(|p| p.base_url);
    }
    let config = load_raw_config(root).await?;
    let llm = config.get("llm")?;
    let services = normalize_service_config(llm.get("services"));
    services
        .iter()
        .find(|entry| service_config_key(entry) == service_id)
        .and_then(|entry| entry.base_url.clone())
}

// ── 杂项小件 ───────────────────────────────────────────────────

/// `compareServiceListItems`：kkaiapi/openrouter/newapi/siliconcloud 优先置前
/// （其余保持原序——稳定排序）。
fn service_list_priority(service: &str) -> i32 {
    match service {
        "kkaiapi" => 0,
        "openrouter" => 1,
        "newapi" => 2,
        "siliconcloud" => 3,
        _ => 999,
    }
}

/// `/^[\x21-\x7E]+$/`：可放进 Authorization header 的非空白 ASCII。
fn is_header_safe_api_key(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|b| (0x21..=0x7E).contains(&b))
}

const NON_TEXT_MODEL_ID_PARTS: [&str; 8] = [
    "image", "embedding", "embed", "rerank", "tts", "speech", "audio", "moderation",
];

fn is_text_chat_model_id(model_id: &str) -> bool {
    let normalized = model_id.trim().to_lowercase();
    !normalized.is_empty()
        && !NON_TEXT_MODEL_ID_PARTS.iter().any(|part| normalized.contains(part))
}

/// 模型列表缓存（10 分钟；`?refresh=1` 绕过）。对齐 server.ts `modelListCache`。
type ModelListCacheEntry = (Vec<Value>, Instant);
type ModelListCache = Mutex<HashMap<String, ModelListCacheEntry>>;

fn model_list_cache() -> &'static ModelListCache {
    static CACHE: OnceLock<ModelListCache> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 语言取值：默认 zh（对齐 currentProjectLanguage 的 normalize 语义——缺省/非法回 zh）。
async fn project_language(root: &Path) -> &'static str {
    let raw = load_raw_config(root).await;
    match raw.as_ref().and_then(|c| c.get("language")).and_then(Value::as_str) {
        Some("en") => "en",
        _ => "zh",
    }
}

fn llm_map_mut(config: &mut Value) -> &mut Map<String, Value> {
    if !config.is_object() {
        *config = json!({});
    }
    if !config.get("llm").is_some_and(Value::is_object) {
        config
            .as_object_mut()
            .expect("上方已保证对象")
            .insert("llm".into(), json!({}));
    }
    config
        .get_mut("llm")
        .and_then(Value::as_object_mut)
        .expect("上方已保证 llm 为对象")
}

fn llm_of(config: &Value) -> Map<String, Value> {
    config
        .get("llm")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default()
}

// ── GET /api/v1/services ───────────────────────────────────────

pub async fn list_services(State(runtime): State<BooksRuntime>) -> impl IntoResponse {
    let root = runtime.state.project_root();
    let secrets = load_secrets(root).unwrap_or_default();
    let mut services: Vec<(i32, Value)> = Vec::new();

    let config = load_raw_config(root).await;
    let configured = config
        .as_ref()
        .map(|c| normalize_service_config(c.get("llm").and_then(|l| l.get("services"))))
        .unwrap_or_default();
    let configured_bank: std::collections::HashSet<String> = configured
        .iter()
        .filter(|s| s.service != "custom")
        .map(|s| s.service.clone())
        .collect();

    for ep in get_all_endpoints().iter().filter(|ep| ep.id != "custom") {
        let family = resolve_service_provider_family(&ep.id).unwrap_or(ProviderFamily::Openai);
        let preset_base = resolve_service_preset(&ep.id)
            .map(|p| p.base_url)
            .unwrap_or_else(|| ep.base_url.clone());
        let family_str = match family {
            ProviderFamily::Openai => "openai",
            ProviderFamily::Anthropic => "anthropic",
        };
        let api_key_optional = is_api_key_optional_for_endpoint(family_str, Some(&preset_base));
        let connected = secrets.services.get(&ep.id).map(|s| !s.api_key.is_empty()).unwrap_or(false)
            || (api_key_optional && configured_bank.contains(&ep.id));
        services.push((
            service_list_priority(&ep.id),
            json!({
                "service": ep.id,
                "label": ep.label,
                "group": ep.group,
                "apiKeyOptional": api_key_optional,
                "connected": connected,
            }),
        ));
    }

    // custom 服务追加（inkos.json 里的 custom 条目）
    for svc in &configured {
        if svc.service == "custom" {
            let secret_key = service_config_key(svc);
            let api_key_optional = is_api_key_optional_for_endpoint(
                "openai",
                svc.base_url.as_deref(),
            );
            let connected = secrets
                .services
                .get(&secret_key)
                .map(|s| !s.api_key.is_empty())
                .unwrap_or(false)
                || api_key_optional;
            services.push((
                999,
                json!({
                    "service": secret_key,
                    "label": svc.name.clone().unwrap_or_else(|| "Custom".into()),
                    "apiKeyOptional": api_key_optional,
                    "connected": connected,
                }),
            ));
        }
    }

    // 稳定排序：优先级组在前（同优先级保持插入序）
    services.sort_by_key(|(priority, _)| *priority);
    let list: Vec<Value> = services.into_iter().map(|(_, item)| item).collect();
    (StatusCode::OK, Json(json!({ "services": list })))
}

// ── GET /api/v1/services/config ────────────────────────────────

pub async fn get_services_config(State(runtime): State<BooksRuntime>) -> impl IntoResponse {
    let root = runtime.state.project_root();
    let config = load_raw_config(root).await.unwrap_or(json!({}));
    let llm = llm_of(&config);
    let services: Vec<Value> = normalize_service_config(llm.get("services"))
        .iter()
        .map(|e| e.to_json())
        .collect();
    let env_config = read_env_config_status(root).await;
    (
        StatusCode::OK,
        Json(json!({
            "services": services,
            "service": llm.get("service").and_then(Value::as_str),
            "defaultModel": llm.get("defaultModel"),
            "configSource": "studio",
            "storedConfigSource": normalize_config_source(llm.get("configSource")),
            "envConfig": env_config,
        })),
    )
}

/// `readEnvConfigStatus`：project / global 双层摘要。
async fn read_env_config_status(root: &Path) -> Value {
    let project = read_env_config_values(&root.join(".env")).await;
    let global = match global_env_path() {
        Some(path) => read_env_config_values(&path).await,
        None => crate::utils::llm_env::EnvConfigValues::default(),
    };
    let to_summary = |v: &crate::utils::llm_env::EnvConfigValues| {
        json!({
            "detected": v.detected,
            "provider": v.provider,
            "service": v.service,
            "baseUrl": v.base_url,
            "model": v.model,
            "hasApiKey": v.has_api_key,
        })
    };
    let effective_source: Value = if project.detected {
        json!("project")
    } else if global.detected {
        json!("global")
    } else {
        Value::Null
    };
    json!({
        "project": to_summary(&project),
        "global": to_summary(&global),
        "effectiveSource": effective_source,
        "runtimeUsesEnv": false,
    })
}

// ── POST /api/v1/services/config/import-env ────────────────────

pub async fn import_env_config(State(runtime): State<BooksRuntime>) -> impl IntoResponse {
    let root = runtime.state.project_root();
    // project 优先 global（readEffectiveEnvConfigValues）
    let project = read_env_config_values(&root.join(".env")).await;
    let (source, env) = if project.detected {
        ("project", project)
    } else {
        match global_env_path() {
            Some(path) => {
                let global = read_env_config_values(&path).await;
                if global.detected {
                    ("global", global)
                } else {
                    ("", crate::utils::llm_env::EnvConfigValues::default())
                }
            }
            None => ("", crate::utils::llm_env::EnvConfigValues::default()),
        }
    };
    let language = project_language(root).await;
    let no_env_message = match language {
        "en" => "No importable LLM environment variable configuration was detected, or INKOS_LLM_API_KEY is missing.",
        _ => "未检测到可导入的 LLM 环境变量配置，或缺少 INKOS_LLM_API_KEY。",
    };
    let api_key = match env.api_key.as_deref() {
        Some(key) if !key.is_empty() => key.to_string(),
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": no_env_message })),
            )
        }
    };

    let mut config = load_raw_config(root).await.unwrap_or(json!({}));
    let mut existing = Vec::new();
    if let Some(llm) = config.get("llm") {
        existing = normalize_service_config(llm.get("services"));
    }
    let explicit_service = env.service.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let guessed = env.base_url.as_deref().map(guess_service_from_base_url);
    let service = explicit_service
        .or(guessed)
        .unwrap_or("custom")
        .to_string();

    let entry = if service == "custom" {
        ServiceConfigEntry {
            service: "custom".into(),
            name: Some("Env LLM".into()),
            base_url: env.base_url.clone().filter(|s| !s.is_empty()),
            ..Default::default()
        }
    } else {
        ServiceConfigEntry {
            service: service.clone(),
            ..Default::default()
        }
    };
    let service_key = service_config_key(&entry);

    let llm = llm_map_mut(&mut config);
    let merged = merge_service_config(existing, vec![entry]);
    llm.insert(
        "services".into(),
        json!(merged.iter().map(|e| e.to_json()).collect::<Vec<_>>()),
    );
    llm.insert("service".into(), json!(service_key));
    llm.insert("configSource".into(), json!("studio"));
    if let Some(model) = &env.model {
        llm.insert("defaultModel".into(), json!(model));
    }
    sync_top_level_llm_mirror(llm);

    let mut secrets = load_secrets(root).unwrap_or_default();
    secrets.services.insert(service_key.clone(), crate::llm::secrets::ServiceSecret { api_key });
    let _ = save_secrets(root, &secrets);
    if !save_raw_config(root, &config).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "failed to save inkos.json" })),
        );
    }

    (
        StatusCode::OK,
        Json(json!({
            "ok": true,
            "source": source,
            "service": service_key,
            "defaultModel": env.model,
        })),
    )
}

// ── PUT /api/v1/services/config ────────────────────────────────

pub async fn put_services_config(
    State(runtime): State<BooksRuntime>,
    body: Bytes,
) -> impl IntoResponse {
    let root = runtime.state.project_root();
    let payload: Value = match serde_json::from_slice(&body) {
        Ok(payload) => payload,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "invalid JSON body" })),
            )
        }
    };
    let mut config = load_raw_config(root).await.unwrap_or(json!({}));
    let language = project_language(root).await;

    let llm = llm_map_mut(&mut config);
    if let Some(incoming) = payload.get("services") {
        let existing = normalize_service_config(llm.get("services"));
        let updates = normalize_service_config(Some(incoming));
        let merged = merge_service_config(existing, updates);
        llm.insert(
            "services".into(),
            json!(merged.iter().map(|e| e.to_json()).collect::<Vec<_>>()),
        );
    }
    if let Some(default_model) = payload.get("defaultModel") {
        llm.insert("defaultModel".into(), default_model.clone());
    }
    if payload.get("configSource") == Some(&json!("env")) {
        let message = match language {
            "en" => "The Studio runtime does not support switching to env; env only acts as an override layer in the CLI/daemon/deployment runtimes.",
            _ => "Studio 运行时不支持切换到 env；env 只在 CLI/daemon/部署运行时作为覆盖层使用。",
        };
        // TS 在此分支直接 return（services/defaultModel 的内存修改不落盘）
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": message })));
    }
    if let Some(config_source) = payload.get("configSource") {
        llm.insert("configSource".into(), json!(normalize_config_source(Some(config_source))));
    }
    if let Some(service) = payload.get("service") {
        llm.insert("service".into(), service.clone());
    }
    sync_top_level_llm_mirror(llm);
    if !save_raw_config(root, &config).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "failed to save inkos.json" })),
        );
    }
    (StatusCode::OK, Json(json!({ "ok": true })))
}

// ── DELETE /api/v1/services/:service ───────────────────────────

pub async fn delete_service(
    State(runtime): State<BooksRuntime>,
    AxumPath(service): AxumPath<String>,
) -> impl IntoResponse {
    let root = runtime.state.project_root();
    let mut config = load_raw_config(root).await.unwrap_or(json!({}));
    let llm = llm_map_mut(&mut config);
    let existing = normalize_service_config(llm.get("services"));
    let next: Vec<ServiceConfigEntry> = existing
        .into_iter()
        .filter(|entry| service_config_key(entry) != service)
        .collect();
    llm.insert(
        "services".into(),
        json!(next.iter().map(|e| e.to_json()).collect::<Vec<_>>()),
    );
    if llm.get("service").and_then(Value::as_str) == Some(service.as_str()) {
        llm.remove("service");
        llm.remove("defaultModel");
    }
    let _ = save_raw_config(root, &config).await;

    let mut secrets = load_secrets(root).unwrap_or_default();
    secrets.services.remove(&service);
    let _ = save_secrets(root, &secrets);
    model_list_cache().lock().unwrap().clear();
    (StatusCode::OK, Json(json!({ "ok": true, "service": service })))
}

// ── POST /api/v1/services/:service/test ────────────────────────

pub async fn test_service(
    State(runtime): State<BooksRuntime>,
    AxumPath(service): AxumPath<String>,
    body: Bytes,
) -> impl IntoResponse {
    let root = runtime.state.project_root();
    let payload: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let api_key = payload
        .get("apiKey")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let inline_base_url = payload.get("baseUrl").and_then(Value::as_str);
    let preferred_api_format = payload
        .get("apiFormat")
        .and_then(Value::as_str)
        .unwrap_or("chat");
    let preferred_stream = payload.get("stream").and_then(Value::as_bool).unwrap_or(false);
    let language = project_language(root).await;

    let Some(resolved_base_url) =
        resolve_configured_service_base_url(root, &service, inline_base_url).await
    else {
        let message = match language {
            "en" => format!("Unknown service: {service}"),
            _ => format!("未知服务商: {service}"),
        };
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "ok": false, "error": message })),
        );
    };

    let base_service = if is_custom_service_id(&service) { "custom" } else { service.as_str() };
    let family = resolve_service_provider_family(base_service).unwrap_or(ProviderFamily::Openai);
    let family_str = match family {
        ProviderFamily::Openai => "openai",
        ProviderFamily::Anthropic => "anthropic",
    };
    let api_key_optional =
        is_api_key_optional_for_endpoint(family_str, Some(&resolved_base_url));
    if api_key.trim().is_empty() && !api_key_optional {
        let message = match language {
            "en" => "API Key must not be empty",
            _ => "API Key 不能为空",
        };
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "ok": false, "error": message })),
        );
    }

    // probe 主路径：live /models → 非空即成功（chatCompletion 深链探测暂缓，见偏差备案）
    let probed = probe_models_from_upstream(&resolved_base_url, api_key.trim(), 10_000).await;
    let endpoint = get_endpoint(base_service);
    let preset = resolve_service_preset(base_service);
    let discovered: Vec<Value> = probed
        .iter()
        .map(|m| json!({ "id": m.id, "name": m.name }))
        .collect();
    let connection_failed = match language {
        "en" => "Connection failed",
        _ => "连接失败",
    };

    if !discovered.is_empty() {
        let selected = probed
            .iter()
            .find(|m| is_text_chat_model_id(&m.id))
            .or_else(|| probed.first())
            .map(|m| m.id.clone());
        let Some(selected_model) = selected.filter(|m| is_text_chat_model_id(m)) else {
            let message = match language {
                "en" => "The model list is reachable, but no model usable for text chat was found.",
                _ => "模型列表可访问，但没有发现可用于文本对话的模型。",
            };
            let probe = json!({ "ok": false, "models": discovered.len(), "error": message });
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "ok": false, "error": message, "probe": probe, "chat": null })),
            );
        };
        let models: Vec<String> = probed.iter().map(|m| m.id.clone()).collect();
        let probe = json!({ "ok": true, "models": models.len() });
        return (
            StatusCode::OK,
            Json(json!({
                "ok": true,
                "modelCount": models.len(),
                "models": models,
                "selectedModel": selected_model,
                "detected": {
                    "apiFormat": preferred_api_format,
                    "stream": preferred_stream,
                    "baseUrl": resolved_base_url,
                    "modelsSource": "api",
                },
                "probe": probe,
                "chat": null,
            })),
        );
    }

    // live 列表不可达：aggregator 组信任静态 bank（shouldTrustStaticModels...）
    let trust_static = endpoint
        .map(|ep| ep.group == Some(crate::llm::providers::EndpointGroup::Aggregator))
        .unwrap_or(false);
    if trust_static {
        let models: Vec<Value> = endpoint
            .map(|ep| {
                ep.models
                    .iter()
                    .filter(|m| m.enabled != Some(false))
                    .filter(|m| is_text_chat_model_id(&m.id))
                    .map(|m| json!({ "id": m.id, "name": m.id }))
                    .collect()
            })
            .unwrap_or_default();
        if models.is_empty() && preset.is_some() {
            // legacy knownModels fallback（fallbackTextModelsForEndpoint 第二层）
        }
        let selected = endpoint
            .and_then(|ep| ep.check_model.clone())
            .filter(|check| models.iter().any(|m| m["id"] == json!(check)))
            .or_else(|| models.first().and_then(|m| m["id"].as_str().map(|s| s.to_string())));
        if let Some(selected_model) = selected {
            let probe = json!({ "ok": true, "models": models.len() });
            return (
                StatusCode::OK,
                Json(json!({
                    "ok": true,
                    "modelCount": models.len(),
                    "models": models,
                    "selectedModel": selected_model,
                    "detected": {
                        "apiFormat": preferred_api_format,
                        "stream": preferred_stream,
                        "baseUrl": resolved_base_url,
                        "modelsSource": "fallback",
                    },
                    "probe": probe,
                    "chat": null,
                })),
            );
        }
    }

    let message = match language {
        "en" => "Could not determine a model automatically. Fill in an available model first, or provide a service endpoint that supports /models.",
        _ => "无法自动确定模型，请先填写可用模型或提供支持 /models 的服务端点。",
    };
    let probe = json!({ "ok": false, "models": 0, "error": connection_failed });
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "ok": false, "error": message, "probe": probe, "chat": null })),
    )
}

// ── PUT / GET /api/v1/services/:service/secret ─────────────────

pub async fn put_service_secret(
    State(runtime): State<BooksRuntime>,
    AxumPath(service): AxumPath<String>,
    body: Bytes,
) -> impl IntoResponse {
    let root = runtime.state.project_root();
    let payload: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let language = project_language(root).await;
    let api_key = payload.get("apiKey").and_then(Value::as_str).unwrap_or("");
    let trimmed = api_key.trim();

    let mut secrets = load_secrets(root).unwrap_or_default();
    if !trimmed.is_empty() {
        if !is_header_safe_api_key(trimmed) {
            let message = match language {
                "en" => "API Key may only contain non-whitespace ASCII characters that fit in an HTTP Authorization header; do not paste connection failure hints or diagnostic text.",
                _ => "API Key 只能包含可放进 HTTP Authorization header 的非空白 ASCII 字符；请不要粘贴连接失败提示或诊断文本。",
            };
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "ok": false, "error": message })),
            );
        }
        secrets.services.insert(service, crate::llm::secrets::ServiceSecret { api_key: trimmed.to_string() });
    } else {
        secrets.services.remove(&service);
    }
    if save_secrets(root, &secrets).is_err() {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "ok": false, "error": "failed to save secrets" })),
        );
    }
    (StatusCode::OK, Json(json!({ "ok": true })))
}

pub async fn get_service_secret(
    State(runtime): State<BooksRuntime>,
    AxumPath(service): AxumPath<String>,
) -> impl IntoResponse {
    let root = runtime.state.project_root();
    let secrets = load_secrets(root).unwrap_or_default();
    let api_key = secrets
        .services
        .get(&service)
        .map(|s| s.api_key.clone())
        .unwrap_or_default();
    (StatusCode::OK, Json(json!({ "apiKey": api_key })))
}

// ── GET /api/v1/services/models ────────────────────────────────

pub async fn list_services_models(State(runtime): State<BooksRuntime>) -> impl IntoResponse {
    let root = runtime.state.project_root();
    let secrets = load_secrets(root).unwrap_or_default();
    let groups: Vec<Value> = get_all_endpoints()
        .iter()
        .filter(|ep| ep.id != "custom")
        .filter(|ep| secrets.services.get(&ep.id).map(|s| !s.api_key.is_empty()).unwrap_or(false))
        .map(|ep| {
            let models: Vec<Value> = ep
                .models
                .iter()
                .filter(|m| m.enabled != Some(false))
                .filter(|m| is_text_chat_model_id(&m.id))
                .map(|m| {
                    let mut item = json!({ "id": m.id, "name": m.id });
                    let obj = item.as_object_mut().unwrap();
                    obj.insert("maxOutput".into(), json!(m.max_output));
                    if m.context_window_tokens > 0 {
                        obj.insert("contextWindow".into(), json!(m.context_window_tokens));
                    }
                    item
                })
                .collect();
            json!({ "service": ep.id, "label": ep.label, "models": models })
        })
        .collect();
    (StatusCode::OK, Json(json!({ "groups": groups })))
}

// ── GET /api/v1/services/models/custom ─────────────────────────

pub async fn list_custom_services_models(
    State(runtime): State<BooksRuntime>,
) -> impl IntoResponse {
    let root = runtime.state.project_root();
    let secrets = load_secrets(root).unwrap_or_default();
    let config = load_raw_config(root).await;
    let customs: Vec<(String, String, String)> = normalize_service_config(
        config
            .as_ref()
            .and_then(|c| c.get("llm"))
            .and_then(|l| l.get("services")),
    )
    .iter()
    .filter(|s| s.service == "custom")
    .map(|s| {
        (
            format!("custom:{}", s.name.clone().unwrap_or_else(|| "Custom".into())),
            s.base_url.clone().unwrap_or_default(),
            s.name.clone().unwrap_or_else(|| "Custom".into()),
        )
    })
    .filter(|(id, base_url, _)| {
        !base_url.is_empty()
            && secrets.services.get(id).map(|s| !s.api_key.is_empty()).unwrap_or(false)
    })
    .collect();

    let mut groups = Vec::with_capacity(customs.len());
    for (id, base_url, label) in customs {
        let api_key = secrets.services.get(&id).map(|s| s.api_key.clone()).unwrap_or_default();
        let probed = probe_models_from_upstream(&base_url, &api_key, 10_000).await;
        let models: Vec<Value> = probed
            .iter()
            .filter(|m| is_text_chat_model_id(&m.id))
            .map(|m| json!({ "id": m.id, "name": m.name }))
            .collect();
        groups.push(json!({ "service": id, "label": label, "models": models }));
    }
    (StatusCode::OK, Json(json!({ "groups": groups })))
}

// ── GET /api/v1/services/:service/models ───────────────────────

pub async fn list_service_models(
    State(runtime): State<BooksRuntime>,
    AxumPath(service): AxumPath<String>,
    Query(query): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let root = runtime.state.project_root();
    let refresh = query.get("refresh").map(|v| v == "1").unwrap_or(false);
    let secrets = load_secrets(root).unwrap_or_default();
    let api_key = query
        .get("apiKey")
        .cloned()
        .filter(|k| !k.is_empty())
        .or_else(|| secrets.services.get(&service).map(|s| s.api_key.clone()))
        .unwrap_or_default();

    let resolved_base_url =
        resolve_configured_service_base_url(root, &service, None).await;
    let base_service = if is_custom_service_id(&service) { "custom" } else { service.as_str() };
    let family = resolve_service_provider_family(base_service).unwrap_or(ProviderFamily::Openai);
    let family_str = match family {
        ProviderFamily::Openai => "openai",
        ProviderFamily::Anthropic => "anthropic",
    };
    let api_key_optional = resolved_base_url
        .as_deref()
        .map(|url| is_api_key_optional_for_endpoint(family_str, Some(url)))
        .unwrap_or(false);

    // No key = no models（本地端点除外）
    if api_key.is_empty() && !api_key_optional {
        return (StatusCode::OK, Json(json!({ "models": [] })));
    }

    let cache_key = format!(
        "{service}::{}::{}",
        resolved_base_url.as_deref().unwrap_or(""),
        if api_key.len() >= 8 { &api_key[api_key.len() - 8..] } else { "" }
    );
    if !refresh {
        if let Ok(cache) = model_list_cache().lock() {
            if let Some((models, at)) = cache.get(&cache_key) {
                if at.elapsed() < Duration::from_secs(10 * 60) {
                    return (StatusCode::OK, Json(json!({ "models": models })));
                }
            }
        }
    }

    let live_base_url = if is_custom_service_id(&service) {
        resolved_base_url.as_deref()
    } else {
        None
    };
    let enriched =
        list_models_for_service(base_service, Some(&api_key), live_base_url).await;
    let models: Vec<Value> = enriched
        .iter()
        .filter(|m| is_text_chat_model_id(&m.id))
        .map(|m| {
            let mut item = json!({ "id": m.id, "name": m.name });
            let obj = item.as_object_mut().unwrap();
            if let Some(max_output) = m.max_output {
                obj.insert("maxOutput".into(), json!(max_output));
            }
            if m.context_window > 0 {
                obj.insert("contextWindow".into(), json!(m.context_window));
            }
            item
        })
        .collect();
    if let Ok(mut cache) = model_list_cache().lock() {
        cache.insert(cache_key, (models.clone(), Instant::now()));
    }
    (StatusCode::OK, Json(json!({ "models": models })))
}

// ── GET / PUT /api/v1/cover/config ─────────────────────────────

pub async fn get_cover_config(State(runtime): State<BooksRuntime>) -> impl IntoResponse {
    let root = runtime.state.project_root();
    let config = load_raw_config(root).await.unwrap_or(json!({}));
    let llm = llm_of(&config);
    let cover = normalize_cover_config_value(llm.get("cover"));
    let secrets = load_secrets(root).unwrap_or_default();
    let key_for = |service: &str| -> bool {
        secrets
            .services
            .get(&cover_secret_key(service))
            .map(|s| !s.api_key.is_empty())
            .unwrap_or(false)
            || secrets
                .services
                .get(service)
                .map(|s| !s.api_key.is_empty())
                .unwrap_or(false)
    };
    let env_base = std::env::var("INKOS_COVER_BASE_URL").ok()
        .or_else(|| std::env::var("INKOS_COVER_ENDPOINT").ok());
    let env_key = std::env::var("INKOS_COVER_API_KEY").ok().filter(|k| !k.is_empty());
    let env_configured =
        env_base.is_some() && (env_key.is_some() || key_for("kkaiapi"));
    let configured = cover
        .as_ref()
        .and_then(|c| c.get("service"))
        .and_then(Value::as_str)
        .map(&key_for)
        .unwrap_or(false)
        || env_configured;

    let providers: Vec<Value> = COVER_PROVIDER_PRESETS
        .iter()
        .map(|provider| {
            json!({
                "service": provider.service,
                "label": provider.label,
                "baseUrl": provider.base_url,
                "defaultModel": provider.default_model,
                "models": provider.models,
                "connected": key_for(provider.service),
            })
        })
        .collect();
    let field = |key: &str| cover.as_ref().and_then(|c| c.get(key)).cloned();
    (
        StatusCode::OK,
        Json(json!({
            "service": field("service"),
            "model": field("model"),
            "baseUrl": field("baseUrl"),
            "configured": configured,
            "providers": providers,
        })),
    )
}

/// `normalizeCoverConfig`：cover 对象 → preset 校验 + model 白名单 + baseUrl。
fn normalize_cover_config_value(raw: Option<&Value>) -> Option<Map<String, Value>> {
    let record = raw?.as_object()?;
    let service = record.get("service").and_then(Value::as_str)?;
    let preset = resolve_cover_provider_preset(Some(service))?;
    let requested_model = record
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    let model = if !requested_model.is_empty() && preset.models.contains(&requested_model) {
        requested_model.to_string()
    } else {
        preset.default_model.to_string()
    };
    let base_url = normalize_cover_base_url(
        record.get("baseUrl").and_then(Value::as_str),
    );
    let mut out = Map::new();
    out.insert("service".into(), json!(preset.service));
    out.insert("model".into(), json!(model));
    if let Some(base_url) = base_url {
        out.insert("baseUrl".into(), json!(base_url));
    }
    Some(out)
}

pub async fn put_cover_config(
    State(runtime): State<BooksRuntime>,
    body: Bytes,
) -> impl IntoResponse {
    let root = runtime.state.project_root();
    let payload: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let language = project_language(root).await;
    let body_service = payload.get("service").and_then(Value::as_str);
    let Some(preset) = resolve_cover_provider_preset(body_service) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Unsupported cover service" })),
        );
    };
    let body_model = payload.get("model").and_then(Value::as_str);
    let model = match body_model {
        Some(model) if preset.models.contains(&model) => model.to_string(),
        _ => preset.default_model.to_string(),
    };
    let requested_base_url = payload
        .get("baseUrl")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    let base_url = normalize_cover_base_url(Some(requested_base_url));
    if !requested_base_url.is_empty() && base_url.is_none() {
        let message = match language {
            "en" => "Cover Base URL must be a valid HTTP(S) URL without credentials, query parameters, or fragments.",
            _ => "封面 Base URL 必须是有效的 HTTP(S) 地址，且不能包含账号、查询参数或锚点。",
        };
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": message })));
    }

    let mut config = load_raw_config(root).await.unwrap_or(json!({}));
    let mut cover = Map::new();
    cover.insert("service".into(), json!(preset.service));
    cover.insert("model".into(), json!(model));
    if let Some(base_url) = base_url.clone() {
        cover.insert("baseUrl".into(), json!(base_url));
    }
    let llm = llm_map_mut(&mut config);
    llm.insert("cover".into(), Value::Object(cover));
    if !save_raw_config(root, &config).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "failed to save inkos.json" })),
        );
    }
    (
        StatusCode::OK,
        Json(json!({
            "ok": true,
            "service": preset.service,
            "model": model,
            "baseUrl": base_url,
        })),
    )
}

// ── GET / PUT /api/v1/cover/secret/:service ────────────────────

pub async fn get_cover_secret(
    State(runtime): State<BooksRuntime>,
    AxumPath(service): AxumPath<String>,
) -> impl IntoResponse {
    let root = runtime.state.project_root();
    if resolve_cover_provider_preset(Some(&service)).is_none() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Unsupported cover service" })),
        );
    }
    let secrets = load_secrets(root).unwrap_or_default();
    let api_key = secrets
        .services
        .get(&cover_secret_key(&service))
        .map(|s| s.api_key.clone())
        .unwrap_or_default();
    (StatusCode::OK, Json(json!({ "apiKey": api_key })))
}

pub async fn put_cover_secret(
    State(runtime): State<BooksRuntime>,
    AxumPath(service): AxumPath<String>,
    body: Bytes,
) -> impl IntoResponse {
    let root = runtime.state.project_root();
    if resolve_cover_provider_preset(Some(&service)).is_none() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Unsupported cover service" })),
        );
    }
    let language = project_language(root).await;
    let payload: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let api_key = payload.get("apiKey").and_then(Value::as_str).unwrap_or("");
    let trimmed = api_key.trim();
    if !trimmed.is_empty() && !is_header_safe_api_key(trimmed) {
        let message = match language {
            "en" => "API Key contains characters that cannot go into an HTTP Authorization header. Paste only the raw key.",
            _ => "API Key 包含不能放入 HTTP Authorization header 的字符，请只粘贴原始密钥。",
        };
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": message })));
    }
    let mut secrets = load_secrets(root).unwrap_or_default();
    let key = cover_secret_key(&service);
    if !trimmed.is_empty() {
        secrets
            .services
            .insert(key, crate::llm::secrets::ServiceSecret { api_key: trimmed.to_string() });
    } else {
        secrets.services.remove(&key);
    }
    if save_secrets(root, &secrets).is_err() {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "failed to save secrets" })),
        );
    }
    (StatusCode::OK, Json(json!({ "ok": true, "service": service })))
}

// ── resolveEffectiveLLMConfig（studio-project 主路径，63 号补齐 56 号偏差） ──

/// 动态模型清单服务（静态 bank 必然滞后，用户配置的模型 id 直接透传；
/// effective-llm-config.ts `SERVICES_WITH_DYNAMIC_MODELS`）。
const SERVICES_WITH_DYNAMIC_MODELS: [&str; 7] = [
    "ollama", "lmstudio", "openrouter", "newapi", "kkaiapi", "ppio", "siliconcloud",
];

fn service_allows_unlisted_models(service: &str) -> bool {
    SERVICES_WITH_DYNAMIC_MODELS.contains(&service)
}

fn model_belongs_to_service(service: &str, model: &str) -> bool {
    if service_allows_unlisted_models(service) {
        return true;
    }
    match get_endpoint(service) {
        Some(endpoint) => endpoint
            .models
            .iter()
            .any(|known| known.id.eq_ignore_ascii_case(model)),
        None => true,
    }
}

/// `resolveServiceModel`：defaultModel/currentModel 中属于该服务者优先，
/// 否则 checkModel → bank 第一可用；custom → defaultModel||currentModel||noop。
fn resolve_service_model(
    entry: Option<&ServiceConfigEntry>,
    current_model: Option<&str>,
    default_model: Option<&str>,
) -> String {
    let noop = || "noop-model".to_string();
    let Some(entry) = entry else {
        return default_model.or(current_model).map(str::to_string).unwrap_or_else(noop);
    };
    if entry.service == "custom" {
        return default_model.or(current_model).map(str::to_string).unwrap_or_else(noop);
    }
    let endpoint = get_endpoint(&entry.service);
    for candidate in [default_model, current_model].into_iter().flatten() {
        if model_belongs_to_service(&entry.service, candidate) {
            return candidate.to_string();
        }
    }
    endpoint
        .and_then(|ep| ep.check_model.clone())
        .or_else(|| {
            endpoint.and_then(|ep| {
                ep.models
                    .iter()
                    .find(|m| m.enabled != Some(false))
                    .map(|m| m.id.clone())
            })
        })
        .or_else(|| default_model.map(str::to_string))
        .or_else(|| current_model.map(str::to_string))
        .unwrap_or_else(noop)
}

/// `applyServiceEntry`：服务条目镜像到 llm 顶层（provider/baseUrl/传输默认）。
fn apply_service_entry(llm: &mut Map<String, Value>, entry: &ServiceConfigEntry) {
    let endpoint = get_endpoint(&entry.service);
    let transport = endpoint.and_then(|ep| ep.transport_defaults.as_ref());
    llm.insert("service".into(), json!(entry.service));
    let family = if entry.service == "custom" {
        "custom"
    } else {
        match resolve_service_provider_family(&entry.service).unwrap_or(ProviderFamily::Openai) {
            ProviderFamily::Openai => "openai",
            ProviderFamily::Anthropic => "anthropic",
        }
    };
    llm.insert("provider".into(), json!(family));
    let base_url = entry
        .base_url
        .clone()
        .or_else(|| resolve_service_preset(&entry.service).map(|p| p.base_url))
        .unwrap_or_default();
    llm.insert("baseUrl".into(), json!(base_url));

    if let Some(temperature) = entry.temperature {
        llm.insert("temperature".into(), json!(temperature));
    }
    if let Some(api_format) = &entry.api_format {
        llm.insert("apiFormat".into(), json!(api_format));
    } else {
        let api_format = transport
            .and_then(|t| t.api_format)
            .map(|f| match f {
                crate::llm::providers::TransportApiFormat::Chat => "chat",
                crate::llm::providers::TransportApiFormat::Responses => "responses",
            })
            .map(str::to_string)
            .unwrap_or_else(|| {
                let api = resolve_service_preset(&entry.service)
                    .map(|p| p.api)
                    .unwrap_or_default();
                if api.starts_with("openai-responses") { "responses".into() } else { "chat".into() }
            });
        llm.insert("apiFormat".into(), json!(api_format));
    }
    if let Some(stream) = entry.stream {
        llm.insert("stream".into(), json!(stream));
    } else if let Some(stream) = transport.and_then(|t| t.stream) {
        llm.insert("stream".into(), json!(stream));
    }
}

/// studio 消费者的有效 LLM 配置（resolveEffectiveLLMConfig 的 studio-project
/// 分支：services 选择 + 镜像 + secrets key + noop 默认填充）。
/// 输入为 raw inkos.json 的 llm 对象，返回合并后的 llm（浅拷贝修改）。
pub async fn resolve_effective_llm_studio(
    root: &Path,
    raw_llm: &Map<String, Value>,
) -> Map<String, Value> {
    let mut llm = raw_llm.clone();
    let services = normalize_service_config(llm.get("services"));
    llm.insert("configSource".into(), json!("studio"));

    // selectServiceEntry：configuredService ?? synthesize ?? services[0]
    let configured = llm.get("service").and_then(Value::as_str).filter(|s| !s.is_empty());
    let selected: Option<ServiceConfigEntry> = configured
        .and_then(|configured_service| {
            services
                .iter()
                .find(|entry| entry.service == configured_service || service_config_key(entry) == configured_service)
                .cloned()
                .or_else(|| synthesize_service_entry(configured_service))
        })
        .or_else(|| services.first().cloned());

    if let Some(entry) = &selected {
        apply_service_entry(&mut llm, entry);
    }

    let ignore_top_level_model = !services.is_empty();
    let current_model = llm.get("model").and_then(Value::as_str);
    let default_model = llm.get("defaultModel").and_then(Value::as_str);
    let model = resolve_service_model(
        selected.as_ref(),
        if ignore_top_level_model { None } else { current_model },
        default_model,
    );
    llm.insert("model".into(), json!(model));

    let service_key = selected
        .as_ref()
        .map(service_config_key)
        .or_else(|| llm.get("service").and_then(Value::as_str).map(str::to_string));
    let secrets = load_secrets(root).unwrap_or_default();
    let api_key = service_key
        .and_then(|key| secrets.services.get(&key).map(|s| s.api_key.clone()))
        .unwrap_or_default();
    llm.insert("apiKey".into(), json!(api_key));

    // requireApiKey=false → noop 默认填充（fillNoopLLMDefaults）
    if llm.get("provider").and_then(Value::as_str).map(str::is_empty).unwrap_or(true) {
        llm.insert("provider".into(), json!("openai"));
    }
    if llm.get("baseUrl").and_then(Value::as_str).map(str::is_empty).unwrap_or(true) {
        llm.insert("baseUrl".into(), json!("https://example.invalid/v1"));
    }
    if llm.get("model").and_then(Value::as_str).map(str::is_empty).unwrap_or(true) {
        llm.insert("model".into(), json!("noop-model"));
    }
    llm
}

fn synthesize_service_entry(service: &str) -> Option<ServiceConfigEntry> {
    if service.is_empty() {
        return None;
    }
    if let Some(name) = service.strip_prefix("custom:") {
        return Some(ServiceConfigEntry {
            service: "custom".into(),
            name: Some(if name.is_empty() { "Custom".into() } else { name.to_string() }),
            ..Default::default()
        });
    }
    if service == "custom" || get_endpoint(service).is_some() || resolve_service_preset(service).is_some() {
        return Some(ServiceConfigEntry { service: service.to_string(), ..Default::default() });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_safe_api_key_rules() {
        assert!(is_header_safe_api_key("sk-abc123XYZ"));
        assert!(!is_header_safe_api_key(""));
        assert!(!is_header_safe_api_key("has space"));
        assert!(!is_header_safe_api_key("中文key"));
        assert!(!is_header_safe_api_key("line\nbreak"));
    }

    #[test]
    fn text_model_filter() {
        assert!(is_text_chat_model_id("deepseek-v4-flash"));
        assert!(!is_text_chat_model_id("text-embedding-3"));
        assert!(!is_text_chat_model_id("gpt-image-2"));
        assert!(!is_text_chat_model_id("  "));
    }

    #[test]
    fn service_config_key_and_custom_detection() {
        let entry = ServiceConfigEntry { service: "custom".into(), name: Some("Env LLM".into()), ..Default::default() };
        assert_eq!(service_config_key(&entry), "custom:Env LLM");
        assert!(is_custom_service_id("custom"));
        assert!(is_custom_service_id("custom:Env LLM"));
        assert!(!is_custom_service_id("deepseek"));
    }

    #[test]
    fn normalize_service_config_both_shapes() {
        let array_form = json!([{ "service": "deepseek", "temperature": 0.7 }]);
        let entries = normalize_service_config(Some(&array_form));
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].service, "deepseek");
        assert_eq!(entries[0].temperature, Some(0.7));

        let object_form = json!({ "custom:My LLM": { "baseUrl": "https://x/v1" }, "deepseek": { "stream": true } });
        let entries = normalize_service_config(Some(&object_form));
        assert_eq!(entries.len(), 2);
        let my = entries.iter().find(|e| e.name.as_deref() == Some("My LLM")).unwrap();
        assert_eq!(my.service, "custom");
        assert_eq!(my.base_url.as_deref(), Some("https://x/v1"));
    }

    #[test]
    fn merge_updates_by_key_keeps_order() {
        let existing = vec![
            ServiceConfigEntry { service: "deepseek".into(), ..Default::default() },
            ServiceConfigEntry { service: "custom".into(), name: Some("A".into()), ..Default::default() },
        ];
        let updates = vec![
            ServiceConfigEntry { service: "custom".into(), name: Some("B".into()), base_url: Some("https://b/v1".into()), ..Default::default() },
            ServiceConfigEntry { service: "moonshot".into(), ..Default::default() },
        ];
        let merged = merge_service_config(existing, updates);
        // 键按 custom:{name}——不同名 custom 是不同条目（TS 同款）
        assert_eq!(merged.len(), 4);
        assert_eq!(merged[1].name.as_deref(), Some("A"), "custom:A 键不同于 custom:B，共存");
        assert_eq!(merged[2].name.as_deref(), Some("B"));
        assert_eq!(merged[3].service, "moonshot", "新条目追加尾部");

        // 同键覆盖：custom:B 再更新 baseUrl
        let again = vec![ServiceConfigEntry {
            service: "custom".into(),
            name: Some("B".into()),
            base_url: Some("https://new/v1".into()),
            ..Default::default()
        }];
        let merged = merge_service_config(merged, again);
        assert_eq!(merged.len(), 4);
        assert_eq!(merged[2].base_url.as_deref(), Some("https://new/v1"));
    }

    #[test]
    fn sync_top_level_mirror_writes_provider_baseurl_model() {
        let mut llm = Map::new();
        llm.insert("service".into(), json!("deepseek"));
        llm.insert("defaultModel".into(), json!("deepseek-v4-pro"));
        llm.insert(
            "services".into(),
            json!([{ "service": "deepseek", "temperature": 1.2 }]),
        );
        sync_top_level_llm_mirror(&mut llm);
        assert_eq!(llm.get("provider").unwrap(), "openai");
        assert_eq!(llm.get("baseUrl").unwrap(), "https://api.deepseek.com");
        assert_eq!(llm.get("model").unwrap(), "deepseek-v4-pro");
        assert_eq!(llm.get("temperature").unwrap(), &json!(1.2));
    }

    #[test]
    fn sync_top_level_mirror_without_service_returns_early() {
        let mut llm = Map::new();
        llm.insert("model".into(), json!("keep-me"));
        sync_top_level_llm_mirror(&mut llm);
        assert_eq!(llm.get("model").unwrap(), "keep-me");
        assert!(llm.get("provider").is_none());
    }

    #[tokio::test]
    async fn effective_llm_studio_selects_service_and_fills_noop() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("inkos.json"),
            r#"{
  "llm": {
    "service": "custom:My LLM",
    "defaultModel": "my-model",
    "services": [{ "service": "custom", "name": "My LLM", "baseUrl": "https://my.llm/v1" }]
  }
}"#,
        )
        .unwrap();
        std::fs::create_dir_all(root.join(".inkos")).unwrap();
        std::fs::write(
            root.join(".inkos").join("secrets.json"),
            r#"{ "services": { "custom:My LLM": { "apiKey": "sk-123" } } }"#,
        )
        .unwrap();

        let raw_llm: Map<String, Value> =
            serde_json::from_str(r#"{ "service": "custom:My LLM", "defaultModel": "my-model", "services": [{ "service": "custom", "name": "My LLM", "baseUrl": "https://my.llm/v1" }] }"#)
                .unwrap();
        let effective = resolve_effective_llm_studio(root, &raw_llm).await;
        assert_eq!(effective.get("baseUrl").unwrap(), "https://my.llm/v1");
        assert_eq!(effective.get("model").unwrap(), "my-model");
        assert_eq!(effective.get("apiKey").unwrap(), "sk-123");
        assert_eq!(effective.get("configSource").unwrap(), "studio");
    }

    #[tokio::test]
    async fn effective_llm_studio_bank_check_model_fallback() {
        let dir = tempfile::tempdir().unwrap();
        // 无 services、无 service 字段 → 全 noop 填充
        let effective = resolve_effective_llm_studio(
            dir.path(),
            &serde_json::from_str::<Map<String, Value>>(r#"{}"#).unwrap(),
        )
        .await;
        assert_eq!(effective.get("provider").unwrap(), "openai");
        assert_eq!(effective.get("baseUrl").unwrap(), "https://example.invalid/v1");
        assert_eq!(effective.get("model").unwrap(), "noop-model");
    }
}
