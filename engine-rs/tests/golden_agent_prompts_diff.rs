//! 230/231/234 号：聊天面系统提示词 golden 差分。
//!
//! 事实源 = `packages/core/src/__tests__/golden/agent-prompts.json`
//! （core `golden-agent-prompts.test.ts` 从 buildAgentSystemPrompt 各面生成）。
//! engine `chat_prompts.rs` 的手抄移植与 TS 逐字比对——防手抄漂移。

use serde_json::Value;

const GOLDEN: &str = include_str!("../../packages/core/src/__tests__/golden/agent-prompts.json");

#[test]
fn chat_prompts_match_ts_snapshot() {
    let golden: Value = serde_json::from_str(GOLDEN).expect("golden json");
    for (key, expected) in golden.as_object().expect("golden object") {
        let (kind, lang) = key.rsplit_once('.').expect("key form");
        let is_zh = lang == "zh";
        let got = match kind {
            "chat" => inkos_engine::interaction::chat_prompts::build_chat_prompt(is_zh),
            "book" => inkos_engine::interaction::chat_prompts::build_book_prompt("b1", is_zh),
            "edit.bound" => {
                inkos_engine::interaction::chat_prompts::build_edit_prompt(Some("b1"), is_zh)
            }
            "edit.unbound" => inkos_engine::interaction::chat_prompts::build_edit_prompt(None, is_zh),
            "book-create.staging" => {
                inkos_engine::interaction::chat_prompts::build_book_create_prompt(is_zh)
            }
            "short.clarify" => inkos_engine::interaction::chat_prompts::build_short_prompt(is_zh),
            "script.clarify" => {
                inkos_engine::interaction::chat_prompts::build_script_prompt(is_zh)
            }
            "storyboard.clarify" => {
                inkos_engine::interaction::chat_prompts::build_storyboard_prompt(is_zh)
            }
            "film.clarify" => {
                inkos_engine::interaction::chat_prompts::build_interactive_film_prompt(is_zh)
            }
            "play.no-world" => {
                inkos_engine::interaction::chat_prompts::build_play_prompt_no_world(is_zh)
            }
            other => panic!("未知 golden key: {other}"),
        };
        assert_eq!(got, expected.as_str().unwrap_or(""), "prompt '{key}' 漂移");
    }
}





