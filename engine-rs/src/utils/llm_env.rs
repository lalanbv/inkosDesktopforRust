//! LLM 环境变量分层与合并（纯子集）。
//!
//! 移植自 `packages/core/src/utils/llm-env.ts`。当前移植纯函数：
//! - [`merge_env_maps`]：多层 env map 合并（后覆盖前，跳过 None）
//! - [`parse_env_value`]：env 字符串值转 number/bool/json/string
//! - [`parse_env_boolean`]：true/1/yes → true
//!
//! ## 待移植（需 dotenv + 文件 IO）
//! loadLLMEnvLayers（读 ~/.inkos/.env + project/.env，需 dotenv 解析）。
//! studioIgnoredEnv/cliOverlayEnv/legacyEnv 在 TS 仅是 mergeEnvMaps 三层，等价 merge_env_maps。

use std::collections::HashMap;

/// env map：key → Option<String>（None 表示该层未设置）。
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
