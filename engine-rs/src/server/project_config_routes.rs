//! project 配置域端点（54 号）：inkos.json 轻量键值读写面。
//!
//! 契约来源 `packages/studio/src/api/server.ts`：
//! - `GET/PUT /project/input-governance-mode`（L4348/L4353）：mode ∈
//!   legacy|v2（缺省/非法值读侧归 v2；写侧非法 400）
//! - `GET/PUT /project/detection`（L4367/L4372）：DetectionConfigSchema
//!   校验 + zod default 填充（7 字段标准化后落盘）；null 删键
//! - `GET/PUT /project/model-overrides`（L5733/L5738）：对象透传
//! - `GET/PUT /project/default-model`（L5750/L5765）：读侧 defaultModel →
//!   model 链式回退；写侧必填 defaultModel + 可选 service
//! - `GET/PUT /project/research-search`（L5787/L5792）：Schema 校验 + 填充
//!   （整体缺省 `{enabled:false, provider:"tavily"}`）
//! - `GET/PUT /project/chapter-review-mode`（L5803/L5808）：writing.reviewMode
//! - `GET/PUT /project/notify`（L5875/L5880）：数组透传
//! - `POST /project/language`（L5553）：language 字段透传（不校验值）
//!
//! 错误形状：多数端点读/写失败走 onError → 500
//! `{"error":{"code":"INTERNAL_ERROR",...}}`；language POST 为平铺
//! `{"error": String(e)}`（端点自带 catch）。
//!
//! ## 暂缓件
//! `PUT /default-model` 的 `syncTopLevelLlmMirror`（顶层 llm 镜像：
//! provider/baseUrl/model 同步）依赖 40+ provider 预设表
//! （core/llm/providers/endpoints/*），随 LLM provider 配置域整体移植。

use axum::body::Bytes;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde_json::{json, Map, Value};

use crate::server::books_routes::BooksRuntime;

type ApiError = (StatusCode, Json<Value>);

fn internal_error() -> ApiError {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "error": { "code": "INTERNAL_ERROR", "message": "Unexpected server error." } })),
    )
}

fn ok_json(payload: Value) -> ApiError {
    (StatusCode::OK, Json(payload))
}

/// 读 inkos.json（raw JSON，保未知字段）。失败 → onError 500 语义。
pub(crate) async fn load_raw_config(root: &std::path::Path) -> Option<Value> {
    let raw = tokio::fs::read_to_string(root.join("inkos.json")).await.ok()?;
    serde_json::from_str(&raw).ok()
}

/// 写 inkos.json（2 空格缩进无尾换行，对齐 `JSON.stringify(raw, null, 2)`）。
pub(crate) async fn save_raw_config(root: &std::path::Path, raw: &Value) -> bool {
    let serialized = serde_json::to_string_pretty(raw).unwrap_or_default();
    tokio::fs::write(root.join("inkos.json"), serialized).await.is_ok()
}

/// 简化 URL 校验（zod `z.string().url()`）：scheme 字母开头 + `://` + 非空余部。
fn is_valid_url(s: &str) -> bool {
    let Some(idx) = s.find("://") else { return false };
    let scheme = &s[..idx];
    let mut chars = scheme.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.')
        && !s[idx + 3..].is_empty()
}

// ── GET/PUT /api/v1/project（56 号：全量配置读写主路径） ──────────

/// GET /project：ProjectConfig 语义提取（name/language + llm 五字段，schema
/// 默认值填充）+ languageExplicit（raw 中显式设置且非空串）。
///
/// 解析/schema 校验失败 → ApiError 500 `PROJECT_CONFIG_INVALID`
/// （server.ts L4181）。env 层与 services 预设合并暂缓（偏差备案：返回
/// raw 项目配置而非 env 合并有效值）。
///
/// 读侧三态与 Node `readProjectConfig`/`resolveEffectiveLLMConfig` 逐字对齐
/// （165 号修复：真实遗留项目 inkos.json 无 `llm` 键时 Node 走
/// `config.llm ?? {}` + noop 填充返回 200，原 Rust 实现误把 zod 缺字段
/// 报错面复刻到了读取闸门 → 桌面切 Rust 后端后启动即 500）：
/// - 文件缺失 → "inkos.json not found in {root}.…"（Node 指引文案）；
/// - JSON 非法 → "inkos.json in {root} is not valid JSON.…"；
/// - 合法 JSON 非对象 / `llm` 缺失或非对象 → 空对象（TS `?? {}` + 展开语义），
///   后续 noop 填充兜底。
pub async fn get_project(State(runtime): State<BooksRuntime>) -> impl IntoResponse {
    let root = runtime.state.project_root();
    let invalid = |detail: String| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": {
                    "code": "PROJECT_CONFIG_INVALID",
                    "message": format!("Failed to load inkos.json: {detail}"),
                }
            })),
        )
    };
    let raw_text = match tokio::fs::read_to_string(root.join("inkos.json")).await {
        Ok(text) => text,
        Err(_) => {
            return invalid(format!(
                "inkos.json not found in {}.\nMake sure you are inside an InkOS project directory (cd into the project created by 'inkos init').",
                root.display()
            ));
        }
    };
    let raw: Value = match serde_json::from_str(&raw_text) {
        Ok(value) => value,
        Err(_) => {
            return invalid(format!(
                "inkos.json in {} is not valid JSON. Check the file for syntax errors.",
                root.display()
            ));
        }
    };
    // TS `{ ...((config.llm ?? {})) }`：根非对象（展开得 {}）/llm 缺失或非对象
    // → 空对象，noop 填充兜底——zod 缺字段报错只发生在 Node schema 终验，
    // 而读取层 llm 永远先被填充成合法对象，不应在此处拦截。
    let obj: Map<String, Value> = raw.as_object().cloned().unwrap_or_default();
    // ProjectConfigSchema 校验（响应所需字段面）。
    let Some(name) = obj.get("name").and_then(Value::as_str).filter(|n| !n.is_empty()) else {
        return invalid("name must contain at least 1 character(s)".to_string());
    };
    let language = match obj.get("language") {
        None => "zh",
        Some(Value::String(s)) if s == "zh" || s == "en" => s.as_str(),
        Some(_) => return invalid("Invalid enum value. Expected 'zh' | 'en'".to_string()),
    };
    let llm: Map<String, Value> = obj
        .get("llm")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    // 63 号补齐（56 号偏差备案）：resolveEffectiveLLMConfig 的 studio-project
    // 主路径——services 选择 + 镜像 + secrets key + noop 默认填充后取有效值。
    let effective = crate::server::service_routes::resolve_effective_llm_studio(
        runtime.state.project_root(),
        &llm,
    )
    .await;
    let get_str = |key: &str| effective.get(key).and_then(Value::as_str);
    let Some(model) = get_str("model").filter(|m| !m.is_empty()) else {
        return invalid("model must contain at least 1 character(s)".to_string());
    };
    let Some(base_url) = get_str("baseUrl").filter(|u| is_valid_url(u)) else {
        return invalid("Invalid url".to_string());
    };
    let provider = get_str("provider");
    let Some(provider) =
        provider.filter(|p| matches!(*p, "anthropic" | "openai" | "custom"))
    else {
        return invalid(
            "Invalid enum value. Expected 'anthropic' | 'openai' | 'custom'".to_string(),
        );
    };
    let stream = effective.get("stream").and_then(Value::as_bool).unwrap_or(true);
    let temperature = effective.get("temperature").and_then(Value::as_f64).unwrap_or(0.7);
    // TS `"language" in raw && raw.language !== ""`。
    let language_explicit = obj
        .get("language")
        .is_some_and(|v| v != &Value::Null && v.as_str() != Some(""));

    (
        StatusCode::OK,
        Json(json!({
            "name": name,
            "language": language,
            "languageExplicit": language_explicit,
            "model": model,
            "provider": provider,
            "baseUrl": base_url,
            "stream": stream,
            "temperature": temperature,
        })),
    )
}

/// PUT /project：合并更新（server.ts L4324）。temperature/stream 走
/// `existing.llm.x = v` 直写——llm 缺失且给出任一字段 → TypeError → 500
/// 平铺（TS 怪癖）；language 仅 zh/en 精确命中；值透传不校验。
pub async fn put_project(
    State(runtime): State<BooksRuntime>,
    body: Bytes,
) -> impl IntoResponse {
    let flat_internal = |message: String| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": message })),
        )
    };
    let Ok(parsed) = serde_json::from_slice::<Value>(&body) else {
        return flat_internal("Unexpected token".to_string());
    };
    let root = runtime.state.project_root();
    let Some(mut raw) = load_raw_config(root).await else {
        return flat_internal("inkos.json read failed".to_string());
    };
    let has_temperature = parsed.get("temperature").is_some();
    let has_stream = parsed.get("stream").is_some();
    if has_temperature || has_stream {
        // TS existing.llm.temperature = ...：llm 缺失即解引用失败。
        let Some(llm) = raw.get_mut("llm").and_then(Value::as_object_mut) else {
            return flat_internal(
                "Cannot read properties of undefined (reading 'temperature')".to_string(),
            );
        };
        if has_temperature {
            llm.insert("temperature".to_string(), parsed["temperature"].clone());
        }
        if has_stream {
            llm.insert("stream".to_string(), parsed["stream"].clone());
        }
    }
    if parsed.get("language") == Some(&json!("zh")) || parsed.get("language") == Some(&json!("en")) {
        raw.as_object_mut().unwrap().insert("language".to_string(), parsed["language"].clone());
    }
    if !save_raw_config(root, &raw).await {
        return flat_internal("inkos.json write failed".to_string());
    }
    ok_json(json!({ "ok": true }))
}

// ── GET/PUT /api/v1/project/input-governance-mode ────────────────

pub async fn get_input_governance_mode(State(runtime): State<BooksRuntime>) -> impl IntoResponse {
    let Some(raw) = load_raw_config(runtime.state.project_root()).await else {
        return internal_error();
    };
    let mode = if raw.get("inputGovernanceMode") == Some(&json!("legacy")) { "legacy" } else { "v2" };
    ok_json(json!({ "mode": mode }))
}

pub async fn put_input_governance_mode(
    State(runtime): State<BooksRuntime>,
    body: Bytes,
) -> impl IntoResponse {
    let Ok(parsed) = serde_json::from_slice::<Value>(&body) else {
        return internal_error();
    };
    let mode = parsed.get("mode");
    let mode = match mode {
        Some(Value::String(s)) if s == "legacy" || s == "v2" => s.as_str(),
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "mode must be legacy or v2" })),
            )
        }
    };
    let root = runtime.state.project_root();
    let Some(mut raw) = load_raw_config(root).await else {
        return internal_error();
    };
    raw.as_object_mut().unwrap().insert("inputGovernanceMode".to_string(), json!(mode));
    if !save_raw_config(root, &raw).await {
        return internal_error();
    }
    ok_json(json!({ "ok": true, "mode": mode }))
}

// ── GET/PUT /api/v1/project/detection ────────────────────────────

/// DetectionConfigSchema 校验 + zod default 填充（未知键剥离，7 字段标准化）。
/// 错误串对齐 `issues.map(i => i.message).join("; ")` 的拼接形态（文案为
/// 英文近似，偏差备案）。
fn parse_detection_config(value: &Value) -> Result<Value, String> {
    let Some(obj) = value.as_object() else {
        return Err("Expected object, received other".to_string());
    };
    let mut errors: Vec<String> = Vec::new();
    let provider = match obj.get("provider") {
        None => "custom".to_string(),
        Some(Value::String(s)) if s == "gptzero" || s == "originality" || s == "custom" => s.clone(),
        Some(_) => {
            errors.push("Invalid enum value. Expected 'gptzero' | 'originality' | 'custom'".to_string());
            "custom".to_string()
        }
    };
    let api_url = match obj.get("apiUrl") {
        Some(Value::String(s)) if is_valid_url(s) => s.clone(),
        Some(Value::String(_)) => {
            errors.push("Invalid url".to_string());
            String::new()
        }
        _ => {
            errors.push("Invalid input: expected string, received missing".to_string());
            String::new()
        }
    };
    let api_key_env = match obj.get("apiKeyEnv") {
        Some(Value::String(s)) if !s.is_empty() => s.clone(),
        Some(Value::String(_)) => {
            errors.push("String must contain at least 1 character(s)".to_string());
            String::new()
        }
        _ => {
            errors.push("Invalid input: expected string, received missing".to_string());
            String::new()
        }
    };
    let threshold = match obj.get("threshold") {
        None => 0.5,
        Some(Value::Number(n)) => {
            let value = n.as_f64().unwrap_or(f64::NAN);
            if value.is_finite() && (0.0..=1.0).contains(&value) {
                value
            } else {
                errors.push("Number must be less than or equal to 1".to_string());
                0.5
            }
        }
        Some(_) => {
            errors.push("Invalid input: expected number, received other".to_string());
            0.5
        }
    };
    let enabled = match obj.get("enabled") {
        None => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => {
            errors.push("Invalid input: expected boolean, received other".to_string());
            false
        }
    };
    let auto_rewrite = match obj.get("autoRewrite") {
        None => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => {
            errors.push("Invalid input: expected boolean, received other".to_string());
            false
        }
    };
    let max_retries = match obj.get("maxRetries") {
        None => 3,
        Some(Value::Number(n)) => {
            let value = n.as_f64().unwrap_or(f64::NAN);
            if value.fract() == 0.0 && (1.0..=10.0).contains(&value) {
                value as u32
            } else {
                errors.push("Number must be less than or equal to 10".to_string());
                3
            }
        }
        Some(_) => {
            errors.push("Invalid input: expected number, received other".to_string());
            3
        }
    };
    if !errors.is_empty() {
        return Err(errors.join("; "));
    }
    Ok(json!({
        "provider": provider,
        "apiUrl": api_url,
        "apiKeyEnv": api_key_env,
        "threshold": threshold,
        "enabled": enabled,
        "autoRewrite": auto_rewrite,
        "maxRetries": max_retries,
    }))
}

pub async fn get_detection(State(runtime): State<BooksRuntime>) -> impl IntoResponse {
    let Some(raw) = load_raw_config(runtime.state.project_root()).await else {
        return internal_error();
    };
    ok_json(json!({ "detection": raw.get("detection").cloned().unwrap_or(Value::Null) }))
}

pub async fn put_detection(
    State(runtime): State<BooksRuntime>,
    body: Bytes,
) -> impl IntoResponse {
    let Ok(parsed) = serde_json::from_slice::<Value>(&body) else {
        return internal_error();
    };
    let root = runtime.state.project_root();
    let Some(mut raw) = load_raw_config(root).await else {
        return internal_error();
    };
    let detection = match parsed.get("detection") {
        Some(Value::Null) | None => {
            // TS detection === null → 删键（undefined 经 zod 校验失败 400）。
            if parsed.get("detection").is_none() {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": "Invalid input: expected object, received missing" })),
                );
            }
            raw.as_object_mut().unwrap().remove("detection");
            Value::Null
        }
        Some(value) => match parse_detection_config(value) {
            Ok(normalized) => {
                raw.as_object_mut().unwrap().insert("detection".to_string(), normalized.clone());
                normalized
            }
            Err(errors) => {
                return (StatusCode::BAD_REQUEST, Json(json!({ "error": errors })));
            }
        },
    };
    if !save_raw_config(root, &raw).await {
        return internal_error();
    }
    ok_json(json!({ "ok": true, "detection": detection }))
}

// ── GET/PUT /api/v1/project/model-overrides ──────────────────────

pub async fn get_model_overrides(State(runtime): State<BooksRuntime>) -> impl IntoResponse {
    let Some(raw) = load_raw_config(runtime.state.project_root()).await else {
        return internal_error();
    };
    let overrides = raw.get("modelOverrides").cloned().unwrap_or_else(|| json!({}));
    ok_json(json!({ "overrides": overrides }))
}

pub async fn put_model_overrides(
    State(runtime): State<BooksRuntime>,
    body: Bytes,
) -> impl IntoResponse {
    let Ok(parsed) = serde_json::from_slice::<Value>(&body) else {
        return internal_error();
    };
    let root = runtime.state.project_root();
    let Some(mut raw) = load_raw_config(root).await else {
        return internal_error();
    };
    // TS raw.modelOverrides = overrides：undefined 赋值在 stringify 时删键。
    match parsed.get("overrides") {
        Some(overrides) => {
            raw.as_object_mut().unwrap().insert("modelOverrides".to_string(), overrides.clone());
        }
        None => {
            raw.as_object_mut().unwrap().remove("modelOverrides");
        }
    }
    if !save_raw_config(root, &raw).await {
        return internal_error();
    }
    ok_json(json!({ "ok": true }))
}

// ── GET/PUT /api/v1/project/default-model ────────────────────────

pub async fn get_default_model(State(runtime): State<BooksRuntime>) -> impl IntoResponse {
    let Some(raw) = load_raw_config(runtime.state.project_root()).await else {
        return internal_error();
    };
    // llm 须为对象且非数组；否则视作 {}。
    let empty = Map::new();
    let llm = raw
        .get("llm")
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    let service = llm.get("service").and_then(Value::as_str);
    let default_model = llm
        .get("defaultModel")
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
        .or_else(|| {
            llm.get("model").and_then(Value::as_str).filter(|v| !v.trim().is_empty())
        });
    ok_json(json!({
        "service": service,
        "defaultModel": default_model,
    }))
}

pub async fn put_default_model(
    State(runtime): State<BooksRuntime>,
    body: Bytes,
) -> impl IntoResponse {
    let Ok(parsed) = serde_json::from_slice::<Value>(&body) else {
        return internal_error();
    };
    let default_model = parsed.get("defaultModel").and_then(Value::as_str).map(str::trim).unwrap_or("");
    if default_model.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "defaultModel is required" })));
    }
    let root = runtime.state.project_root();
    let Some(mut raw) = load_raw_config(root).await else {
        return internal_error();
    };
    // llm 非对象（或缺失）→ 重建 {}。
    if !raw.get("llm").is_some_and(|v| v.is_object()) {
        raw.as_object_mut().unwrap().insert("llm".to_string(), json!({}));
    }
    let service;
    {
        let llm = raw.get_mut("llm").and_then(Value::as_object_mut).unwrap();
        llm.insert("defaultModel".to_string(), json!(default_model));
        service = parsed
            .get("service")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| {
                llm.insert("service".to_string(), json!(s));
                s.to_string()
            });
        // 63 号补齐：syncTopLevelLlmMirror（54 号偏差备案——顶层
        // llm.model/baseUrl/provider 镜像随选中服务与 defaultModel 更新）。
        crate::server::service_routes::sync_top_level_llm_mirror(llm);
    }
    let service_value = service
        .clone()
        .or_else(|| llm_service(&raw));
    if !save_raw_config(root, &raw).await {
        return internal_error();
    }
    ok_json(json!({ "ok": true, "service": service_value, "defaultModel": default_model }))
}

/// `raw.llm.service`（字符串才有值）。
fn llm_service(raw: &Value) -> Option<String> {
    raw.get("llm")
        .and_then(|llm| llm.get("service"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

// ── GET/PUT /api/v1/project/research-search ──────────────────────

/// ResearchSearchConfigSchema.parse + 填充（整体缺省
/// `{enabled:false, provider:"tavily"}`；可选 baseUrl/apiKey/apiKeyEnv）。
fn parse_research_search(value: Option<&Value>) -> Result<Value, String> {
    let Some(obj) = value.filter(|v| !v.is_null()).and_then(Value::as_object) else {
        return Ok(json!({ "enabled": false, "provider": "tavily" }));
    };
    let enabled = match obj.get("enabled") {
        None => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err("Invalid input: expected boolean, received other".to_string()),
    };
    let provider = match obj.get("provider") {
        None => "tavily".to_string(),
        Some(Value::String(s)) if s == "tavily" || s == "custom" => s.clone(),
        Some(_) => {
            return Err("Invalid enum value. Expected 'tavily' | 'custom'".to_string())
        }
    };
    let mut out = json!({ "enabled": enabled, "provider": provider });
    if let Some(base_url) = obj.get("baseUrl") {
        match base_url {
            Value::String(s) if is_valid_url(s) => {
                out.as_object_mut().unwrap().insert("baseUrl".to_string(), json!(s));
            }
            Value::String(_) => return Err("Invalid url".to_string()),
            _ => return Err("Invalid input: expected string, received other".to_string()),
        }
    }
    for key in ["apiKey", "apiKeyEnv"] {
        if let Some(value) = obj.get(key) {
            match value {
                Value::String(s) => {
                    out.as_object_mut().unwrap().insert(key.to_string(), json!(s));
                }
                _ => return Err("Invalid input: expected string, received other".to_string()),
            }
        }
    }
    Ok(out)
}

pub async fn get_research_search(State(runtime): State<BooksRuntime>) -> impl IntoResponse {
    let Some(raw) = load_raw_config(runtime.state.project_root()).await else {
        return internal_error();
    };
    match parse_research_search(raw.get("researchSearch")) {
        Ok(research_search) => ok_json(json!({ "researchSearch": research_search })),
        Err(_) => internal_error(),
    }
}

pub async fn put_research_search(
    State(runtime): State<BooksRuntime>,
    body: Bytes,
) -> impl IntoResponse {
    let Ok(parsed) = serde_json::from_slice::<Value>(&body) else {
        return internal_error();
    };
    // TS Schema.parse(body.researchSearch ?? {})：解析失败 → zod 抛 → 500。
    let research_search = match parse_research_search(parsed.get("researchSearch")) {
        Ok(value) => value,
        Err(_) => return internal_error(),
    };
    let root = runtime.state.project_root();
    let Some(mut raw) = load_raw_config(root).await else {
        return internal_error();
    };
    raw.as_object_mut()
        .unwrap()
        .insert("researchSearch".to_string(), research_search.clone());
    if !save_raw_config(root, &raw).await {
        return internal_error();
    }
    ok_json(json!({ "ok": true, "researchSearch": research_search }))
}

// ── GET/PUT /api/v1/project/chapter-review-mode ──────────────────

pub async fn get_chapter_review_mode(State(runtime): State<BooksRuntime>) -> impl IntoResponse {
    let Some(raw) = load_raw_config(runtime.state.project_root()).await else {
        return internal_error();
    };
    let mode = if raw.get("writing").and_then(|w| w.get("reviewMode")) == Some(&json!("manual")) {
        "manual"
    } else {
        "auto"
    };
    ok_json(json!({ "mode": mode }))
}

pub async fn put_chapter_review_mode(
    State(runtime): State<BooksRuntime>,
    body: Bytes,
) -> impl IntoResponse {
    let Ok(parsed) = serde_json::from_slice::<Value>(&body) else {
        return internal_error();
    };
    // TS normalizeChapterReviewMode：仅 "manual" 精确命中，其余归 auto。
    let next = if parsed.get("mode") == Some(&json!("manual")) { "manual" } else { "auto" };
    let root = runtime.state.project_root();
    let Some(mut raw) = load_raw_config(root).await else {
        return internal_error();
    };
    let mut writing = raw
        .get("writing")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    writing.insert("reviewMode".to_string(), json!(next));
    raw.as_object_mut().unwrap().insert("writing".to_string(), Value::Object(writing));
    if !save_raw_config(root, &raw).await {
        return internal_error();
    }
    ok_json(json!({ "ok": true, "mode": next }))
}

// ── GET/PUT /api/v1/project/notify ───────────────────────────────

pub async fn get_notify(State(runtime): State<BooksRuntime>) -> impl IntoResponse {
    let Some(raw) = load_raw_config(runtime.state.project_root()).await else {
        return internal_error();
    };
    let channels = raw.get("notify").cloned().unwrap_or_else(|| json!([]));
    ok_json(json!({ "channels": channels }))
}

pub async fn put_notify(
    State(runtime): State<BooksRuntime>,
    body: Bytes,
) -> impl IntoResponse {
    let Ok(parsed) = serde_json::from_slice::<Value>(&body) else {
        return internal_error();
    };
    let root = runtime.state.project_root();
    let Some(mut raw) = load_raw_config(root).await else {
        return internal_error();
    };
    match parsed.get("channels") {
        Some(channels) => {
            raw.as_object_mut().unwrap().insert("notify".to_string(), channels.clone());
        }
        None => {
            raw.as_object_mut().unwrap().remove("notify");
        }
    }
    if !save_raw_config(root, &raw).await {
        return internal_error();
    }
    ok_json(json!({ "ok": true }))
}

// ── POST /api/v1/project/language ────────────────────────────────

pub async fn post_language(
    State(runtime): State<BooksRuntime>,
    body: Bytes,
) -> impl IntoResponse {
    // TS 自带 catch → 500 平铺 error（区别于 onError 形状）。
    let flat_internal = |message: String| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": message })),
        )
    };
    let Ok(parsed) = serde_json::from_slice::<Value>(&body) else {
        return flat_internal("Unexpected token".to_string());
    };
    // TS 不校验 language 值：任意 JSON 值透传；undefined 赋值 → stringify 删键。
    let language = parsed.get("language").cloned();
    let root = runtime.state.project_root();
    let Some(mut raw) = load_raw_config(root).await else {
        return flat_internal("inkos.json read failed".to_string());
    };
    let mut response = json!({ "ok": true });
    match language {
        Some(value) => {
            raw.as_object_mut().unwrap().insert("language".to_string(), value.clone());
            response["language"] = value;
        }
        None => {
            raw.as_object_mut().unwrap().remove("language");
        }
    }
    let serialized = serde_json::to_string_pretty(&raw).unwrap_or_default();
    if tokio::fs::write(root.join("inkos.json"), serialized).await.is_err() {
        return flat_internal("inkos.json write failed".to_string());
    }
    ok_json(response)
}

#[cfg(test)]
mod get_project_tests {
    //! 165 号回归：GET /project 读侧三态与 Node `resolveEffectiveLLMConfig`
    //! 逐字对齐——真实遗留项目 inkos.json 无 `llm` 键曾在此 500（桌面切 Rust
    //! 后端后的启动阻断），Node 同形状走 `?? {}` + noop 填充返回 200。

    use super::*;
    use crate::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use crate::server::sse::BroadcastHub;
    use crate::state::manager::StateManager;
    use axum::body::Body;
    use axum::http::Request;
    use axum::http::StatusCode;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tower::ServiceExt;

    async fn run(root: &std::path::Path) -> (StatusCode, Value) {
        // 经完整路由跑（与真实挂载一致），避免直接调 handler 的提取器差异。
        let state = crate::server::AppState { version: "test".to_string() };
        let hub = Arc::new(BroadcastHub::new());
        let sm = Arc::new(StateManager::new(root.to_path_buf()));
        let runner: crate::server::WriteNextRunner = Arc::new(|_, _, _, _| Box::pin(async { Err("unused".to_string()) }));
        let default = LlmEndpointConfig {
            base_url: "http://127.0.0.1:9".to_string(),
            api_key: String::new(),
            model: "default".to_string(),
            max_tokens: 8192,
            extra_headers: HashMap::new(),
        };
        let books = BooksRuntime {
            hub: hub.clone(),
            state: sm,
            router: Arc::new(AgentRouter::new(default, HashMap::new())),
            builtin_genres_dir: std::path::PathBuf::from("assets/genres"),
            revision_gate: crate::pipeline::merged_audit::RevisionGate::parse(None),
        };
        let audit = crate::server::audit_route::AuditRuntime {
            hub,
            state: books.state.clone(),
            router: books.router.clone(),
            builtin_genres_dir: books.builtin_genres_dir.clone(),
        };
        let runtime = crate::server::WriteNextRuntime {
            hub: books.hub.clone(),
            state: books.state.clone(),
            runner,
            project_root: root.to_path_buf(),
        };
        let app = crate::server::router_books(state, books.hub.clone(), runtime, audit, books);
        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/project")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    /// test-project 实形状（无 llm 键）：Node `?? {}` + noop 填充 → 200。
    #[tokio::test]
    async fn missing_llm_key_noop_fills_like_node() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("inkos.json"),
            r#"{ "name": "legacy", "version": "0.1.0", "language": "zh", "notify": [] }"#,
        )
        .unwrap();
        let (status, body) = run(dir.path()).await;
        assert_eq!(status, StatusCode::OK, "无 llm 键应 noop 填充返回 200: {body}");
        assert_eq!(body["model"], "noop-model");
        assert_eq!(body["provider"], "openai");
        assert_eq!(body["baseUrl"], "https://example.invalid/v1");
        assert_eq!(body["name"], "legacy");
        assert_eq!(body["language"], "zh");
    }

    /// 仓根实形状（llm 存在但 baseUrl/model 空串）：fillNoopLLMDefaults 空串
    /// 替换语义 → 200。
    #[tokio::test]
    async fn empty_llm_strings_noop_fill() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("inkos.json"),
            r#"{ "name": "x", "llm": { "provider": "openai", "baseUrl": "", "model": "" } }"#,
        )
        .unwrap();
        let (status, body) = run(dir.path()).await;
        assert_eq!(status, StatusCode::OK, "空串应 noop 填充: {body}");
        assert_eq!(body["baseUrl"], "https://example.invalid/v1");
        assert_eq!(body["model"], "noop-model");
    }

    /// `"llm": null`：TS `null ?? {}` → 空对象 → 200。
    #[tokio::test]
    async fn null_llm_treated_as_empty() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("inkos.json"),
            r#"{ "name": "x", "llm": null }"#,
        )
        .unwrap();
        let (status, body) = run(dir.path()).await;
        assert_eq!(status, StatusCode::OK, "llm null 应等价缺失: {body}");
        assert_eq!(body["model"], "noop-model");
    }

    /// 文件缺失：Node readProjectConfig 指引文案（含 root 路径）。
    #[tokio::test]
    async fn missing_file_message_matches_node() {
        let dir = tempfile::tempdir().unwrap(); // 无 inkos.json
        let (status, body) = run(dir.path()).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body["error"]["code"], "PROJECT_CONFIG_INVALID");
        let msg = body["error"]["message"].as_str().unwrap();
        assert!(
            msg.starts_with("Failed to load inkos.json: inkos.json not found in "),
            "文案应以 Node 指引开头: {msg}"
        );
        assert!(msg.contains("Make sure you are inside an InkOS project directory"), "{msg}");
    }

    /// JSON 非法：Node 文案。
    #[tokio::test]
    async fn invalid_json_message_matches_node() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("inkos.json"), "{ broken").unwrap();
        let (status, body) = run(dir.path()).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        let msg = body["error"]["message"].as_str().unwrap();
        assert!(
            msg.contains("is not valid JSON. Check the file for syntax errors."),
            "文案应为 Node 语法错误指引: {msg}"
        );
    }

    /// 合法 JSON 但根非对象：TS 展开语义得 {} → name 校验 500（而非读取层拦）。
    #[tokio::test]
    async fn non_object_root_fails_on_name_not_read_gate() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("inkos.json"), "42").unwrap();
        let (status, body) = run(dir.path()).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        let msg = body["error"]["message"].as_str().unwrap();
        assert!(
            msg.contains("name must contain at least 1 character(s)"),
            "非对象根应落到 name 校验（zod 终验面）: {msg}"
        );
    }
}
