//! 结算增量解析器（settler-delta-parser）。
//!
//! 移植自 `packages/core/src/agents/settler-delta-parser.ts`（53 行）。
//! 从结算 agent 输出提取 `RUNTIME_STATE_DELTA` 段，反序列化为 [`RuntimeStateDelta`]。

use regex::Regex;
use std::sync::OnceLock;

use crate::agents::settler_parser::extract_tag;
use crate::models::runtime_state::RuntimeStateDelta;

/// 结算增量输出（对齐 TS `SettlerDeltaOutput`）。
#[derive(Debug, Clone, PartialEq)]
pub struct SettlerDeltaOutput {
    pub post_settlement: String,
    pub runtime_state_delta: RuntimeStateDelta,
}

/// 从结算内容提取增量。对齐 TS `parseSettlerDeltaOutput`。
///
/// RUNTIME_STATE_DELTA 段缺失 → Constraint 错误；JSON 解析失败 → Constraint 错误；
/// schema 校验失败（serde 反序列化）→ Constraint 错误。
pub fn parse_settler_delta_output(content: &str) -> crate::Result<SettlerDeltaOutput> {
    let raw_delta = extract_tag(content, "RUNTIME_STATE_DELTA");
    if raw_delta.is_empty() {
        return Err(crate::EngineError::Constraint(
            "runtime state delta block is missing".to_string(),
        ));
    }
    let json_payload = strip_code_fence(&raw_delta);
    let sanitized = sanitize_json(&json_payload);
    let parsed: serde_json::Value = serde_json::from_str(&sanitized).map_err(|e| {
        crate::EngineError::Constraint(format!("runtime state delta is not valid JSON: {e}"))
    })?;
    // R23/394 号：LLM 边界的 kind 容错归一化——词表外的值删除而不是拒收
    // 整个结算增量（对齐 TS sanitizeHookKinds）。
    let parsed = sanitize_hook_kinds(parsed);
    let delta: RuntimeStateDelta = serde_json::from_value(parsed).map_err(|e| {
        crate::EngineError::Constraint(format!("runtime state delta failed schema validation: {e}"))
    })?;
    Ok(SettlerDeltaOutput {
        post_settlement: extract_tag(content, "POST_SETTLEMENT"),
        runtime_state_delta: delta,
    })
}

/// R23/394 号：upsert 条目与候选的 kind 值经别名表归一；不可归一的删除。
/// 对齐 TS `sanitizeHookKinds`。
fn sanitize_hook_kinds(mut parsed: serde_json::Value) -> serde_json::Value {
    use crate::utils::hook_kind::normalize_hook_kind;
    if let Some(upsert) = parsed
        .get_mut("hookOps")
        .and_then(|ops| ops.get_mut("upsert"))
        .and_then(serde_json::Value::as_array_mut)
    {
        for entry in upsert.iter_mut() {
            sanitize_entry_kind(entry);
        }
    }
    if let Some(candidates) = parsed
        .get_mut("newHookCandidates")
        .and_then(serde_json::Value::as_array_mut)
    {
        for entry in candidates.iter_mut() {
            sanitize_entry_kind(entry);
        }
    }
    parsed
}

fn sanitize_entry_kind(entry: &mut serde_json::Value) {
    use crate::utils::hook_kind::normalize_hook_kind;
    let Some(obj) = entry.as_object_mut() else {
        return;
    };
    if let Some(kind) = obj.remove("kind") {
        let raw = kind.as_str().unwrap_or_default();
        if let Some(normalized) = normalize_hook_kind(raw) {
            obj.insert(
                "kind".to_string(),
                serde_json::Value::String(
                    crate::utils::hook_kind::hook_kind_id(normalized).to_string(),
                ),
            );
        }
    }
}

/// 剥离 ```json ... ``` 代码围栏。对齐 TS `stripCodeFence`。
fn strip_code_fence(value: &str) -> String {
    let trimmed = value.trim();
    let re: &Regex = fence_re();
    match re.captures(trimmed) {
        Some(caps) => caps.get(1).map(|m| m.as_str().trim().to_string()).unwrap_or_else(|| trimmed.to_string()),
        None => trimmed.to_string(),
    }
}

fn fence_re() -> &'static Regex {
    // TS: /^```(?:json)?\s*([\s\S]*?)\s*```$/i
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)^```(?:json)?\s*([\s\S]*?)\s*```$").expect("fence regex"))
}

/// 清理 JSON：剥离控制字符（保留 \t \n \r）+ 尾逗号。对齐 TS `sanitizeJSON`。
fn sanitize_json(s: &str) -> String {
    // 先剥离控制字符（0x00-0x08, 0x0B, 0x0C, 0x0E-0x1F, 0x7F）。
    let cleaned: String = s
        .chars()
        .filter(|&c| !matches!(c as u32, 0x00..=0x08 | 0x0B | 0x0C | 0x0E..=0x1F | 0x7F))
        .collect();
    // 再去尾逗号：`,}` 或 `,]`（含空白）→ `}`/`]`。重复处理（多个尾逗号）。
    trailing_comma_re().replace_all(&cleaned, "$1").to_string()
}

fn trailing_comma_re() -> &'static Regex {
    // TS: /,\s*([}\]])/g
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r",\s*([}\]])").expect("trailing comma regex"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn delta_json() -> &'static str {
        r#"{"chapter":3,"hookOps":{"upsert":[],"mention":[],"resolve":[],"defer":[]},"newHookCandidates":[],"subplotOps":[],"emotionalArcOps":[],"characterMatrixOps":[],"notes":[]}"#
    }

    #[test]
    fn parses_delta_block_with_code_fence() {
        let content = format!(
            "=== POST_SETTLEMENT ===\n收尾文本\n\n=== RUNTIME_STATE_DELTA ===\n```json\n{}\n```\n",
            delta_json()
        );
        let out = parse_settler_delta_output(&content).unwrap();
        assert_eq!(out.post_settlement, "收尾文本");
        assert_eq!(out.runtime_state_delta.chapter, 3);
    }

    #[test]
    fn parses_delta_block_without_fence() {
        let content = format!("=== RUNTIME_STATE_DELTA ===\n{}\n", delta_json());
        let out = parse_settler_delta_output(&content).unwrap();
        assert_eq!(out.runtime_state_delta.chapter, 3);
    }

    #[test]
    fn missing_delta_block_is_constraint_error() {
        let err = parse_settler_delta_output("=== POST_SETTLEMENT ===\nx").unwrap_err();
        assert!(matches!(err, crate::EngineError::Constraint(m) if m.contains("missing")));
    }

    #[test]
    fn invalid_json_is_constraint_error() {
        let content = "=== RUNTIME_STATE_DELTA ===\nnot json\n";
        let err = parse_settler_delta_output(content).unwrap_err();
        assert!(matches!(err, crate::EngineError::Constraint(m) if m.contains("not valid JSON")));
    }

    #[test]
    fn sanitize_json_strips_control_chars_and_trailing_commas() {
        let dirty = "{\"chapter\":1,\x00\"notes\":[1,2,3,],\"hookOps\":{},}";
        let cleaned = sanitize_json(dirty);
        // 控制字符已剥，尾逗号已去。
        assert!(!cleaned.contains('\u{0}'));
        assert!(!cleaned.contains(",]"));
        assert!(!cleaned.contains(",}"));
        // sanitize 本身只保证 JSON 合法（字段对 RuntimeStateDelta 不必完整）。
        assert!(serde_json::from_str::<serde_json::Value>(&cleaned).is_ok());
    }

    #[test]
    fn strip_code_fence_handles_plain_and_fenced() {
        assert_eq!(strip_code_fence("```json\n{x}\n```"), "{x}");
        assert_eq!(strip_code_fence("```\n{x}\n```"), "{x}");
        assert_eq!(strip_code_fence("plain"), "plain");
        assert_eq!(strip_code_fence("  {x}  "), "{x}");
    }

    #[test]
    fn delta_block_stops_at_next_tag() {
        let content = format!(
            "=== RUNTIME_STATE_DELTA ===\n{}\n=== POST_SETTLEMENT ===\n后段",
            delta_json()
        );
        let out = parse_settler_delta_output(&content).unwrap();
        assert_eq!(out.runtime_state_delta.chapter, 3);
        assert_eq!(out.post_settlement, "后段");
    }
}
