//! 553 号：R32 会话压缩共享 golden 差分。
//!
//! 唯一事实源 = `packages/core/src/__tests__/golden/r32-compaction-vectors.json`
//! （prompt 码点机械提取自 pi-agent-core 0.87 dist/harness/compaction/compaction.js，
//! TS 侧 session-compaction.test.ts 以同一 golden 锁定自持副本；本测试锁 Rust
//! 码点副本与自研算法同 TS/上游一致——触发口径、切点选择、序列化/组装模板、
//! 常量表四方全同构）。
use inkos_engine::interaction::session_compaction::{
    build_summary_prompt, estimate_context_tokens, estimate_tokens, find_cut_point,
    serialize_conversation, should_compact, CompactionSettings, DEFAULT_COMPACTION_SETTINGS,
    SUMMARIZATION_PROMPT, SUMMARIZATION_SYSTEM_PROMPT, SUMMARY_MAX_TOKENS,
    UPDATE_SUMMARIZATION_PROMPT,
};
use inkos_engine::llm::provider::{LLMMessage, LLMRole};
use serde_json::Value;

const VECTORS: &str =
    include_str!("../../packages/core/src/__tests__/golden/r32-compaction-vectors.json");

fn user_msg(content: &str) -> LLMMessage {
    LLMMessage { role: LLMRole::User, content: content.into(), tool_calls: None, tool_call_id: None }
}

fn assistant_msg(content: &str) -> LLMMessage {
    LLMMessage { role: LLMRole::Assistant, content: content.into(), tool_calls: None, tool_call_id: None }
}

fn vector_messages(raw: &[Value]) -> Vec<LLMMessage> {
    raw.iter()
        .map(|message| {
            let content = message["content"].as_str().expect("content");
            if message["role"].as_str() == Some("user") {
                user_msg(content)
            } else {
                assistant_msg(content)
            }
        })
        .collect()
}

/// 平行 raw JSON（assistant 携带 usage/stopReason；None 处 Null）。
fn vector_raws(raw: &[Value]) -> Vec<Value> {
    raw.iter()
        .map(|message| {
            if message["role"].as_str() == Some("assistant") {
                serde_json::json!({
                    "stopReason": message.get("stopReason").cloned().unwrap_or(Value::Null),
                    "usage": message.get("usage").cloned().unwrap_or(Value::Null),
                })
            } else {
                Value::Null
            }
        })
        .collect()
}

fn roles_of(messages: &[LLMMessage]) -> Vec<Option<&'static str>> {
    messages
        .iter()
        .map(|message| match message.role {
            LLMRole::User => Some("user"),
            LLMRole::Assistant => Some("assistant"),
            _ => Some("system"),
        })
        .collect()
}

#[test]
fn settings_and_summary_budget_match_golden() {
    let parsed: Value = serde_json::from_str(VECTORS).expect("golden json");
    let settings = &parsed["settings"];
    assert_eq!(settings["enabled"].as_bool(), Some(DEFAULT_COMPACTION_SETTINGS.enabled));
    assert_eq!(settings["reserveTokens"].as_u64(), Some(DEFAULT_COMPACTION_SETTINGS.reserve_tokens));
    assert_eq!(
        settings["keepRecentTokens"].as_u64(),
        Some(DEFAULT_COMPACTION_SETTINGS.keep_recent_tokens)
    );
    assert_eq!(parsed["summaryMaxTokens"].as_u64(), Some(SUMMARY_MAX_TOKENS));
}

#[test]
fn prompts_match_golden_codepoints() {
    let parsed: Value = serde_json::from_str(VECTORS).expect("golden json");
    assert_eq!(SUMMARIZATION_SYSTEM_PROMPT, parsed["prompts"]["system"].as_str().expect("system"));
    assert_eq!(SUMMARIZATION_PROMPT, parsed["prompts"]["summarization"].as_str().expect("summarization"));
    assert_eq!(
        UPDATE_SUMMARIZATION_PROMPT,
        parsed["prompts"]["updateSummarization"].as_str().expect("updateSummarization")
    );
}

#[test]
fn trigger_vectors_match_golden() {
    let parsed: Value = serde_json::from_str(VECTORS).expect("golden json");
    for vector in parsed["triggerVectors"].as_array().expect("triggerVectors") {
        let name = vector["name"].as_str().expect("name");
        let messages = vector_messages(vector["messages"].as_array().expect("messages"));
        let raws = vector_raws(vector["messages"].as_array().expect("messages"));
        let context_window = vector["contextWindow"].as_u64().expect("contextWindow");
        let settings = match vector["settings"] {
            Value::Null => DEFAULT_COMPACTION_SETTINGS,
            ref custom => CompactionSettings {
                enabled: custom["enabled"].as_bool().expect("enabled"),
                reserve_tokens: custom["reserveTokens"].as_u64().expect("reserveTokens"),
                keep_recent_tokens: custom["keepRecentTokens"].as_u64().expect("keepRecentTokens"),
            },
        };
        let expected = &vector["expected"];

        let usage = estimate_context_tokens(&messages, &raws);
        assert_eq!(usage.tokens, expected["tokens"].as_u64().unwrap_or(0), "{name}: tokens");
        assert_eq!(
            usage.usage_tokens,
            expected["usageTokens"].as_u64().unwrap_or(0),
            "{name}: usageTokens"
        );
        // invalid-window 短路是 should_compact 的防御分支（窗口 0）。
        let compact = should_compact(usage.tokens, context_window, &settings);
        assert_eq!(compact, expected["compact"].as_bool().expect("compact"), "{name}: compact");
    }
}

#[test]
fn cut_point_vectors_match_golden() {
    let parsed: Value = serde_json::from_str(VECTORS).expect("golden json");
    for vector in parsed["cutPointVectors"].as_array().expect("cutPointVectors") {
        let name = vector["name"].as_str().expect("name");
        let messages = vector_messages(vector["messages"].as_array().expect("messages"));
        let roles = roles_of(&messages);
        let keep_recent = vector["keepRecentTokens"].as_u64().expect("keepRecentTokens");
        let expected = &vector["expected"];
        let cut = find_cut_point(&roles, &messages, 0, messages.len(), keep_recent);
        assert_eq!(
            cut.first_kept_entry_index,
            expected["firstKeptEntryIndex"].as_u64().expect("firstKept") as usize,
            "{name}: firstKeptEntryIndex"
        );
        // golden 的 turnStartIndex=-1 是上游「无轮首」哨兵；Rust None 对应。
        assert_eq!(
            cut.turn_start_index.map_or(-1, |index| index as i64),
            expected["turnStartIndex"].as_i64().expect("turnStartIndex"),
            "{name}: turnStartIndex"
        );
        assert_eq!(cut.is_split_turn, expected["isSplitTurn"].as_bool().expect("isSplitTurn"), "{name}");
    }
}

#[test]
fn serialize_vectors_match_golden() {
    let parsed: Value = serde_json::from_str(VECTORS).expect("golden json");
    for vector in parsed["serializeVectors"].as_array().expect("serializeVectors") {
        let messages = vector_messages(vector["messages"].as_array().expect("messages"));
        assert_eq!(
            serialize_conversation(&messages),
            vector["expected"].as_str().expect("expected"),
            "{}",
            vector["name"].as_str().expect("name")
        );
    }
}

#[test]
fn prompt_assembly_vectors_match_golden() {
    let parsed: Value = serde_json::from_str(VECTORS).expect("golden json");
    for vector in parsed["promptAssemblyVectors"].as_array().expect("promptAssemblyVectors") {
        let previous = vector["previousSummary"].as_str();
        assert_eq!(
            build_summary_prompt(vector["serialized"].as_str().expect("serialized"), previous),
            vector["expected"].as_str().expect("expected"),
            "{}",
            vector["name"].as_str().expect("name")
        );
    }
}

#[test]
fn estimate_tokens_is_utf16_ascii_consistent() {
    // 向量内容全 ASCII；UTF-16 码元数=字符数=JS .length。
    assert_eq!(estimate_tokens(&user_msg("tail-of-context")), 4);
}
