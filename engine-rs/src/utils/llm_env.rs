//! LLM 环境变量分层与合并。
//!
//! 移植自 `packages/core/src/utils/llm-env.ts` + `server.ts` 的手写 .env 读取层：
//! - [`merge_env_maps`]：多层 env map 合并（后覆盖前，跳过 None）
//! - [`parse_env_value`]：env 字符串值转 number/bool/json/string
//! - [`parse_env_boolean`]：true/1/yes → true
//! - [`global_env_path`]：`~/.inkos/.env`（GLOBAL_ENV_PATH）
//! - [`read_env_config_values`]：`.env` 解析（KEY=VALUE + 引号剥离，读取失败全空态）

use std::collections::HashMap;
use std::path::PathBuf;

/// env map：key → `Option<String>`（None 表示该层未设置）。
pub type LLMEnvMap = HashMap<String, Option<String>>;

/// 合并多层 env map：后覆盖前，跳过 None（对齐 TS mergeEnvMaps）。
pub fn merge_env_maps(layers: &[&LLMEnvMap]) -> LLMEnvMap {
    let mut merged: LLMEnvMap = HashMap::new();
    for layer in layers {
        for (k, v) in layer.iter() {
            if v.is_some() {
                merged.insert(k.clone(), v.clone());
            }
        }
    }
    merged
}

/// env 字符串值转 Rust 值：纯数字→f64，true/false→bool，{ 或 [ 开头→JSON.parse，否则原串。
/// 对齐 TS parseEnvValue。
pub fn parse_env_value(value: &str) -> serde_json::Value {
    if let Ok(n) = value.parse::<f64>() {
        // 仅当全是数字/小数点（对齐 /^\d+(\.\d+)?$/）
        if value.chars().all(|c| c.is_ascii_digit() || c == '.') && !value.is_empty() {
            return serde_json::json!(n);
        }
    }
    match value {
        "true" => return serde_json::Value::Bool(true),
        "false" => return serde_json::Value::Bool(false),
        _ => {}
    }
    if value.starts_with('{') || value.starts_with('[') {
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(value) {
            return parsed;
        }
    }
    serde_json::Value::String(value.to_string())
}

/// env 布尔：true/1/yes → true（对齐 TS parseBoolean）。
pub fn parse_env_boolean(value: &str) -> bool {
    matches!(value, "true" | "1" | "yes")
}

/// `~/.inkos/.env`（GLOBAL_ENV_PATH；HOME → USERPROFILE 回退，均缺 → None）。
pub fn global_env_path() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(|home| PathBuf::from(home).join(".inkos").join(".env"))
}

/// `.env` 值引号剥离（`"v"` / `'v'` 成对包裹时去引号）。对齐 TS `unquoteEnvValue`。
fn unquote_env_value(value: &str) -> String {
    let trimmed = value.trim();
    let bytes = trimmed.as_bytes();
    if bytes.len() >= 2 {
        let (first, last) = (bytes[0], bytes[bytes.len() - 1]);
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return trimmed[1..trimmed.len() - 1].to_string();
        }
    }
    trimmed.to_string()
}

/// INKOS_LLM_* env 配置值（server.ts `readEnvConfigValues` 手写解析逐字：
/// `^([A-Za-z_][A-Za-z0-9_]*)=(.*)$` 行匹配 + `#` 注释跳过 + 引号剥离）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EnvConfigValues {
    pub detected: bool,
    pub provider: Option<String>,
    pub service: Option<String>,
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub has_api_key: bool,
    pub api_key: Option<String>,
}

fn empty_env_values() -> EnvConfigValues {
    EnvConfigValues::default()
}

/// 读单个 `.env` 文件；任何读取失败 → 全空态（对齐 TS catch 分支）。
pub async fn read_env_config_values(path: &std::path::Path) -> EnvConfigValues {
    let Ok(raw) = tokio::fs::read_to_string(path).await else {
        return empty_env_values();
    };
    let mut values: HashMap<String, String> = HashMap::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        // KEY=VALUE：KEY 匹配 [A-Za-z_][A-Za-z0-9_]*，VALUE 为余下全部（含 = 号）
        let Some(eq) = trimmed.find('=') else {
            continue;
        };
        let (key, value) = trimmed.split_at(eq);
        let key_valid = key
            .chars()
            .next()
            .map(|c| c.is_ascii_alphabetic() || c == '_')
            .unwrap_or(false)
            && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !key_valid {
            continue;
        }
        values.insert(key.to_string(), unquote_env_value(&value[1..]));
    }
    let get = |k: &str| values.get(k).cloned();
    let provider = get("INKOS_LLM_PROVIDER");
    let service = get("INKOS_LLM_SERVICE");
    let base_url = get("INKOS_LLM_BASE_URL");
    let model = get("INKOS_LLM_MODEL");
    let api_key = get("INKOS_LLM_API_KEY").unwrap_or_default();
    let detected = provider.is_some()
        || service.is_some()
        || base_url.is_some()
        || model.is_some()
        || !api_key.is_empty();
    EnvConfigValues {
        detected,
        provider,
        service,
        base_url,
        model,
        has_api_key: !api_key.is_empty(),
        api_key: (!api_key.is_empty()).then_some(api_key),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layer(pairs: &[(&str, Option<&str>)]) -> LLMEnvMap {
        pairs.iter().map(|(k, v)| (k.to_string(), v.map(|s| s.to_string()))).collect()
    }

    #[test]
    fn merge_later_overrides_earlier() {
        let a = layer(&[("X", Some("1")), ("Y", Some("a"))]);
        let b = layer(&[("Y", Some("b")), ("Z", Some("2"))]);
        let merged = merge_env_maps(&[&a, &b]);
        assert_eq!(merged.get("X").unwrap(), &Some("1".into()));
        assert_eq!(merged.get("Y").unwrap(), &Some("b".into())); // b 覆盖 a
        assert_eq!(merged.get("Z").unwrap(), &Some("2".into()));
    }

    #[test]
    fn merge_skips_none() {
        let a = layer(&[("X", Some("1"))]);
        let b = layer(&[("X", None)]); // None 不覆盖
        let merged = merge_env_maps(&[&a, &b]);
        assert_eq!(merged.get("X").unwrap(), &Some("1".into()));
    }

    #[test]
    fn parse_env_value_types() {
        assert_eq!(parse_env_value("123"), serde_json::json!(123.0));
        assert_eq!(parse_env_value("1.5"), serde_json::json!(1.5));
        assert_eq!(parse_env_value("true"), serde_json::Value::Bool(true));
        assert_eq!(parse_env_value("false"), serde_json::Value::Bool(false));
        assert_eq!(parse_env_value(r#"{"a":1}"#), serde_json::json!({"a": 1}));
        assert_eq!(parse_env_value("[1,2]"), serde_json::json!([1, 2]));
        assert_eq!(parse_env_value("hello"), serde_json::Value::String("hello".into()));
        // 非纯数字（含字母）→ string
        assert_eq!(parse_env_value("12abc"), serde_json::Value::String("12abc".into()));
    }

    #[test]
    fn parse_boolean_truthy() {
        assert!(parse_env_boolean("true"));
        assert!(parse_env_boolean("1"));
        assert!(parse_env_boolean("yes"));
        assert!(!parse_env_boolean("false"));
        assert!(!parse_env_boolean("0"));
        assert!(!parse_env_boolean("no"));
    }
}

#[cfg(test)]
mod env_file_tests {
    use super::*;

    #[tokio::test]
    async fn parses_env_file_with_quotes_and_comments() {
        let dir = tempfile::tempdir().unwrap();
        let env = dir.path().join(".env");
        std::fs::write(
            &env,
            "# comment\nINKOS_LLM_PROVIDER=\"anthropic\"\nINKOS_LLM_SERVICE=deepseek\nINKOS_LLM_API_KEY='sk-abc'\nBAD KEY=x\n=bad\n\n",
        )
        .unwrap();
        let values = read_env_config_values(&env).await;
        assert!(values.detected);
        assert_eq!(values.provider.as_deref(), Some("anthropic"));
        assert_eq!(values.service.as_deref(), Some("deepseek"));
        assert_eq!(values.api_key.as_deref(), Some("sk-abc"));
        assert!(values.has_api_key);
    }

    #[tokio::test]
    async fn missing_file_is_empty_state() {
        let values = read_env_config_values(std::path::Path::new("/nonexistent/.env")).await;
        assert!(!values.detected);
        assert!(!values.has_api_key);
        assert!(values.api_key.is_none());
    }
}
