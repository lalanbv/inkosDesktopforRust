//! R43/565 号：会话 transcript derive/restore 共享 golden 差分。
//!
//! 唯一事实源 = `packages/core/src/__tests__/golden/session-transcript-derive-vectors.json`
//! （TS 侧 session-transcript-derive.test.ts 以同一 golden 锁定 `committedMessageEvents`
//! 与 `restoreCommittedDialogueScan`、`deriveBookSessionFromTranscript`）。
//! 本测试逐 case 断言 committed 过滤（未提交轮整体排除，即中断尾部确定性修复：
//! 撕裂尾剔除且后续轮不受影响）、kind 过滤（仅模型面；display derive 不传 kind，
//! 双端同构不对称，详见向量 comment）、工具轮对话排除与 legacy 无 kind 轮 user
//! 保留、空文本 assistant 丢弃。事件 wire JSON 双端解析必须同收。
use inkos_engine::interaction::session_restore::{
    committed_message_events, derive_book_session_from_transcript,
    restore_committed_dialogue_scan,
};
use inkos_engine::interaction::session_transcript::read_transcript_events;
use serde_json::Value;

const VECTORS: &str =
    include_str!("../../packages/core/src/__tests__/golden/session-transcript-derive-vectors.json");

/// scanMessages.content 口径：文本块拍平（TS 侧测试同构映射）。
fn flatten_content(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(|block| block.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}

fn role_as_str(role: inkos_engine::llm::provider::LLMRole) -> &'static str {
    match role {
        inkos_engine::llm::provider::LLMRole::System => "system",
        inkos_engine::llm::provider::LLMRole::User => "user",
        inkos_engine::llm::provider::LLMRole::Assistant => "assistant",
        inkos_engine::llm::provider::LLMRole::Tool => "tool",
    }
}

#[tokio::test]
async fn golden_session_transcript_derive_replay() {
    let vectors: Value = serde_json::from_str(VECTORS).expect("vectors json");
    assert_eq!(vectors["version"].as_u64(), Some(1));
    let cases = vectors["cases"].as_array().expect("cases");
    assert!(!cases.is_empty());

    for case in cases {
        let name = case["name"].as_str().expect("case name");
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().to_path_buf();
        let session_id = "s";
        let events_dir = root.join(".inkos").join("sessions");
        std::fs::create_dir_all(&events_dir).expect("mkdir");
        let raw_lines = case["events"]
            .as_array()
            .expect("events")
            .iter()
            .map(|event| serde_json::to_string(event).expect("event line"))
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(events_dir.join(format!("{session_id}.jsonl")), format!("{raw_lines}\n"))
            .expect("write jsonl");

        // 双端解析器同收：坏行跳过语义下，解析数 < 写入数即本测试红。
        let events = read_transcript_events(&root, session_id).await;
        assert_eq!(
            events.len(),
            case["events"].as_array().unwrap().len(),
            "{name}: 解析器必须全收向量事件"
        );

        let kind = case["deriveSessionKind"].as_str();
        let committed = committed_message_events(&events, kind);
        let committed_uuids: Vec<&str> =
            committed.iter().filter_map(|event| event.message_uuid()).collect();
        let expected_uuids: Vec<&str> = case["expected"]["committedUuids"]
            .as_array()
            .unwrap()
            .iter()
            .map(|u| u.as_str().expect("uuid"))
            .collect();
        assert_eq!(committed_uuids, expected_uuids, "{name}: committed uuid 序");

        let scan = restore_committed_dialogue_scan(&events, kind);
        let scan_pairs: Vec<(String, String)> = scan
            .messages
            .iter()
            .map(|message| (role_as_str(message.role).to_string(), message.content.clone()))
            .collect();
        let expected_scan: Vec<(String, String)> = case["expected"]["scanMessages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|message| {
                (
                    message["role"].as_str().expect("role").to_string(),
                    flatten_content(&message["content"]),
                )
            })
            .collect();
        assert_eq!(scan_pairs, expected_scan, "{name}: restore 对话面");

        // display derive：BookSession.messages（工具轮折叠卡形态）。
        let session = derive_book_session_from_transcript(&root, session_id)
            .await
            .unwrap_or_else(|| panic!("{name}: derive 必须产出会话"));
        let derive_roles: Vec<String> = session
            .messages
            .iter()
            .filter_map(|message| message.get("role").and_then(Value::as_str))
            .map(str::to_string)
            .collect();
        let expected_roles: Vec<String> = case["expected"]["deriveRoles"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r.as_str().expect("role").to_string())
            .collect();
        assert_eq!(derive_roles, expected_roles, "{name}: derive 角色序");

        if let Some(expected_cards) = case["expected"]["deriveToolCardCount"].as_u64() {
            let card_count = session
                .messages
                .iter()
                .filter(|message| {
                    message.get("toolExecutions").map(Value::is_array).unwrap_or(false)
                })
                .count();
            assert_eq!(card_count as u64, expected_cards, "{name}: 工具卡消息数");
        }
    }
}
