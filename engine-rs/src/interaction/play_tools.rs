//! play 聊天工具面（80 号）。
//!
//! 移植自 `packages/core/src/agent/agent-tools.ts` 的
//! `createPlayStepTool` / `createPlayReviseTool`：世界与会话绑定
//! （worldId === sessionId）、step 失败优雅降级（不把原始错误交给外层
//! agent 即兴发挥）、revise 三分支（重做/改输入/恢复变体）。
//! 工具注册条件（TS agent-session：sessionKind==="play" && playWorldExists）
//! 与 play 会话系统提示词（agent-system-prompt `buildPlayPrompt` 的
//! playWorldExists 分支）由 [`crate::server::agent_route`] 消费。

use std::path::Path;

use serde_json::{json, Value};

use crate::interaction::agent_loop::LoopToolExecutor;
use crate::interaction::project_tools::{error_result, ToolResult};
use crate::llm::agent_router::AgentRouter;
use crate::play_runner::{PlayAgents, PlayRunner};

/// play 工具依赖：世界绑定会话 + LLM 路由（PlayAgents）+ 表面语言。
pub struct PlayToolDeps<'a> {
    pub project_root: &'a Path,
    pub session_id: &'a str,
    pub router: &'a AgentRouter,
    /// 表面语言（"zh"/"en"）：无世界文案的用语。
    pub language: &'a str,
}

/// `safePlayId`：trim || fallback → 80 码元截断 → 拒绝 `.`/`..`/`/`/`\`/NUL。
fn safe_play_id(value: Option<&str>, fallback: &str) -> Result<String, String> {
    let base = value.map(str::trim).filter(|s| !s.is_empty()).unwrap_or(fallback);
    let mut raw = String::new();
    let mut units = 0usize;
    for ch in base.chars() {
        let ch_units = ch.len_utf16();
        if units + ch_units > 80 {
            break;
        }
        units += ch_units;
        raw.push(ch);
    }
    if raw.is_empty()
        || raw == "."
        || raw == ".."
        || raw.contains('/')
        || raw.contains('\\')
        || raw.contains('\0')
    {
        return Err(format!("Invalid play id: {value:?}"));
    }
    Ok(raw)
}

/// 会话绑定的世界是否存在（注册条件探测；非法 id 视为不存在）。
pub async fn session_world_exists(project_root: &Path, session_id: &str) -> bool {
    match safe_play_id(Some(session_id), session_id) {
        Ok(world_id) => crate::play::load_world(project_root, &world_id).await.is_some(),
        Err(_) => false,
    }
}

/// play 工具分发：play_step / play_revise；其余 → None（回落文件工具）。
pub async fn execute_play_tool(
    deps: &PlayToolDeps<'_>,
    name: &str,
    args: &Value,
) -> Option<ToolResult> {
    match name {
        "play_step" => Some(tool_play_step(deps, args).await),
        "play_revise" => Some(tool_play_revise(deps, args).await),
        _ => None,
    }
}

fn text_result(text: impl Into<String>, details: Option<Value>) -> ToolResult {
    ToolResult { text: text.into(), details, is_error: false }
}

fn no_world_result(language: &str, redo: bool) -> ToolResult {
    let text = if language == "en" {
        if redo {
            "There is no interactive world to redo yet. Start one with play_start first."
        } else {
            "There is no interactive world to advance yet. Start one with play_start first."
        }
    } else if redo {
        "还没有可重做的互动世界。先用 play_start 开一局。"
    } else {
        "还没有可推进的互动世界。先用 play_start 开一局。"
    };
    text_result(text, None)
}

/// `play_step`：推进会话绑定的世界一回合。
async fn tool_play_step(deps: &PlayToolDeps<'_>, args: &Value) -> ToolResult {
    let input = args.get("input").and_then(Value::as_str).unwrap_or_default().trim().to_string();
    if input.is_empty() {
        return text_result("Play input is empty.", None);
    }
    let Ok(world_id) = safe_play_id(Some(deps.session_id), deps.session_id) else {
        return error_result(format!("Invalid play id: {:?}", deps.session_id));
    };
    let run_id = "main";
    let Some(world) = crate::play::load_world(deps.project_root, &world_id).await else {
        return no_world_result(deps.language, false);
    };
    let agents = PlayAgents { router: deps.router };
    let runner = PlayRunner {
        project_root: deps.project_root,
        world_id: world_id.clone(),
        run_id: run_id.to_string(),
    };
    // step 抛错不透传给外层 agent：返回固定的优雅降级文案，回合可恢复地失败。
    let step = match runner
        .step(&agents, &agents, &agents, Some(&agents), &input, None)
        .await
    {
        Ok(step) => step,
        Err(error) => {
            let is_zh = world
                .get("language")
                .and_then(Value::as_str)
                .unwrap_or("zh")
                != "en";
            let text = if is_zh {
                "（系统刚才卡了一下，这一步没能展开。把你刚才想做的再说一遍，我就接着推进。）"
            } else {
                "(The system hiccuped and this step didn't resolve. Say what you just did again and I'll continue.)"
            };
            return text_result(
                text,
                Some(json!({
                    "kind": "play_step_failed",
                    "worldId": world_id,
                    "runId": run_id,
                    "error": error,
                })),
            );
        }
    };
    let graph = crate::play::run_dir(deps.project_root, &world_id, run_id)
        .ok()
        .map(|dir| crate::play::play_graph_snapshot(&dir))
        .unwrap_or_else(|| json!({}));
    let current_state =
        crate::play::load_current_state(deps.project_root, &world_id, run_id).await;
    text_result(
        step.scene_text.clone(),
        Some(json!({
            "kind": "play_turn_advanced",
            "worldId": world_id,
            "runId": run_id,
            "title": world.get("title").cloned().unwrap_or(Value::Null),
            "sceneText": step.scene_text,
            "suggestedActions": step.suggested_actions,
            "action": step.action,
            "mutation": step.mutation,
            "currentState": current_state,
            "graph": graph,
        })),
    )
}

/// `play_revise`：重做/改输入/恢复变体。
async fn tool_play_revise(deps: &PlayToolDeps<'_>, args: &Value) -> ToolResult {
    let action = args.get("action").and_then(Value::as_str).unwrap_or_default().to_string();
    if !matches!(action.as_str(), "regenerate_last" | "edit_last_input" | "restore_variant") {
        return error_result(format!(
            "Invalid play_revise action: {action}（expected regenerate_last | edit_last_input | restore_variant）"
        ));
    }
    let Ok(world_id) = safe_play_id(Some(deps.session_id), deps.session_id) else {
        return error_result(format!("Invalid play id: {:?}", deps.session_id));
    };
    let run_id = "main";
    let Some(world) = crate::play::load_world(deps.project_root, &world_id).await else {
        return no_world_result(deps.language, true);
    };
    let is_zh = world
        .get("language")
        .and_then(Value::as_str)
        .unwrap_or("zh")
        != "en";
    let agents = PlayAgents { router: deps.router };
    let runner = PlayRunner {
        project_root: deps.project_root,
        world_id: world_id.clone(),
        run_id: run_id.to_string(),
    };

    if action == "restore_variant" {
        let turn = args
            .get("turn")
            .and_then(Value::as_f64)
            .filter(|value| value.is_finite());
        let variant_id = args
            .get("variantId")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let (Some(turn), Some(variant_id)) = (turn, variant_id) else {
            return text_result(
                if is_zh {
                    "恢复版本需要 turn 和 variantId。"
                } else {
                    "Restoring a variant requires both turn and variantId."
                },
                None,
            );
        };
        // restoreVariant 的错误不吞：原始消息透传（TS 该分支无 catch）。
        return match runner.restore_variant(turn.trunc() as i64, variant_id).await {
            Ok(restored) => {
                let fallback = if is_zh {
                    "已切换到指定互动回合版本。"
                } else {
                    "Switched to the requested play turn variant."
                };
                let text = if restored.scene_text.is_empty() {
                    fallback.to_string()
                } else {
                    restored.scene_text.clone()
                };
                text_result(
                    text,
                    Some(json!({
                        "kind": "play_variant_restored",
                        "worldId": world_id,
                        "runId": run_id,
                        "title": world.get("title").cloned().unwrap_or(Value::Null),
                        "turn": restored.turn,
                        "variantId": restored.variant_id,
                        "sceneText": restored.scene_text,
                    })),
                )
            }
            Err(message) => error_result(message),
        };
    }

    let replacement = if action == "edit_last_input" {
        args.get("input").and_then(Value::as_str).map(str::trim).filter(|value| !value.is_empty())
    } else {
        None
    };
    if action == "edit_last_input" && replacement.is_none() {
        return text_result(
            if is_zh {
                "编辑上一条玩家动作需要提供新的 input。"
            } else {
                "Editing the previous player action requires a new input."
            },
            None,
        );
    }
    // regenerateLastTurn 抛错 → 固定优雅降级（对齐 play_step 的失败语义）。
    let replay = match runner
        .regenerate_last_turn(&agents, &agents, &agents, Some(&agents), replacement)
        .await
    {
        Ok(replay) => replay,
        Err(error) => {
            return text_result(
                if is_zh {
                    "（上一回合暂时不能安全重做。继续输入新的动作，我会从当前状态推进。）"
                } else {
                    "(The previous turn cannot be safely regenerated yet. Enter a new action and I will continue from the current state.)"
                },
                Some(json!({
                    "kind": "play_revise_failed",
                    "worldId": world_id,
                    "runId": run_id,
                    "error": error,
                })),
            );
        }
    };
    let graph = crate::play::run_dir(deps.project_root, &world_id, run_id)
        .ok()
        .map(|dir| crate::play::play_graph_snapshot(&dir))
        .unwrap_or_else(|| json!({}));
    let current_state =
        crate::play::load_current_state(deps.project_root, &world_id, run_id).await;
    text_result(
        replay.scene_text.clone(),
        Some(json!({
            "kind": "play_turn_revised",
            "worldId": world_id,
            "runId": run_id,
            "title": world.get("title").cloned().unwrap_or(Value::Null),
            "sceneText": replay.scene_text,
            "suggestedActions": replay.suggested_actions,
            "action": replay.action,
            "mutation": replay.mutation,
            "replayedInput": replay.replayed_input,
            "previousVariantId": replay.previous_variant_id,
            "variantId": replay.variant_id,
            "currentState": current_state,
            "graph": graph,
        })),
    )
}

/// play 工具 schema（OpenAI function 形态，PlayStepParams/PlayReviseParams 逐字）。
pub fn play_tool_schemas() -> Vec<Value> {
    vec![
        json!({
            "type": "function",
            "function": {
                "name": "play_step",
                "description": "Advance the current InkOS Play world by one player action. Use after play_start when the user keeps acting in the interactive scene.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "input": {
                            "type": "string",
                            "description": "The player's next free-form action or chosen option.",
                        },
                    },
                    "required": ["input"],
                },
            },
        }),
        json!({
            "type": "function",
            "function": {
                "name": "play_revise",
                "description": "Regenerate, edit, or restore the latest InkOS Play turn using saved turn checkpoints. Use when the user says to redo the previous turn, try another version, swipe, or replace their last player input.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "action": {
                            "type": "string",
                            "enum": ["regenerate_last", "edit_last_input", "restore_variant"],
                            "description": "How to revise the latest play turn: regenerate the same player input, edit the previous player input, or restore a saved variant.",
                        },
                        "input": {
                            "type": "string",
                            "description": "Replacement player input when action=edit_last_input.",
                        },
                        "turn": {
                            "type": "number",
                            "description": "Turn number when restoring a saved variant.",
                        },
                        "variantId": {
                            "type": "string",
                            "description": "Saved variant id when action=restore_variant.",
                        },
                    },
                    "required": ["action"],
                },
            },
        }),
    ]
}

/// play 会话系统提示词（`buildPlayPrompt` playWorldExists 分支逐字，含
/// `commonOutputRules`）。play_edit 工具本体随后续轮次接入。
pub fn play_chat_system_prompt(is_en: bool) -> String {
    if is_en {
        return r#"You are the InkOS Play assistant. This surface only runs interactive worlds.

## Available Tools

- play_edit: persistently edit the current world's world contract, visual contract, player persona, or role/object/rule cards; it does not advance time or generate a new scene.
- play_revise: regenerate the previous turn, try another version/swipe, edit the previous player input, or restore a saved turn variant.
- play_step: advance the current interactive world by one player action, speech, observation, movement, choice, or item use.

## Decision

- If the user asks to change world rules, time semantics, role goals/status, player identity, visual rules, or durable object/clue/equipment semantics, call play_edit; do not treat that edit as a story turn. When the user says "change X to Y" or "replace X with Y", use play_edit replacements; do not append the new rule while leaving the old rule in place.
- If the user asks to redo the previous turn, try another version, regenerate, swipe, or says their previous action should have been X instead of Y, call play_revise; do not treat it as the next new turn.
- If the user is already playing and enters an action, speech, observation, movement, or choice, call play_step.
- If the user clearly says they want to exit, stop playing, switch back to chat, or do something else, do not call play_step; answer directly.

## Boundary

- Do not create long-form books.
- Do not generate standalone short-fiction deliverables.
- Do not turn a setup/card/contract edit into a scene advance; durable edits must go through play_edit.
- Do not reduce player actions to ordinary Q&A; in play mode, actions should advance the scene.
- **[HARD RULE] Whenever the user is playing (a world is active and they enter an action/speech/observation/movement/choice), your ONLY action this turn is to call play_step immediately — never write any scene prose, narration, or description yourself. The scene comes from play_step, not from you; narrating it yourself = failure and breaks the whole play machinery (state, the panel, the world graph). If the user edits rules/cards/persona/visual contracts, use play_edit; if the user regenerates/swipes/edits the previous turn, use play_revise; do not call play_step.**

## Output Rules

- Do not use emoji.
- Answer ordinary discussion directly. When a tool call is needed, the tool call itself is the answer; do not add filler, acknowledgement, or a plain-text confirmation first.
- Use short bullets when structure helps; do not claim side effects without successful tool results."#
            .to_string();
    }
    r#"你是 InkOS Play 助手。当前入口只负责互动世界。

## 可用工具

- play_edit：持久编辑当前互动世界的世界契约、视觉契约、玩家 persona、角色/物件/规则卡；不推进时间、不生成新场景。
- play_revise：重做上一回合、换一版、swipe、编辑上一条玩家输入，或恢复已保存的回合版本。
- play_step：推进当前互动世界里用户的一次动作、说话、观察、移动、选择或使用物品。

## 判断

- 用户要求修改世界规则、时间语义、角色目标/状态、玩家身份、视觉规则、装备/物件/证据的长期语义时，调用 play_edit；不要把这类编辑当成一回合剧情。用户说“把 X 改成 Y / 从 X 换成 Y”时，用 play_edit 的 replacements 字段替换旧规则，不要用 append 留下旧规则。
- 用户要求“重来上一回合 / 换一版 / regenerate / swipe / 刚才我不是 X 而是 Y / 编辑上一条动作”时，调用 play_revise；不要把这类请求当成新的下一回合。
- 用户已经在玩，继续输入动作、台词、观察、移动或选择时，调用 play_step。
- 用户明确说不玩了、退出、切回聊天或要做别的事时，停止调用 play_step，直接回答。

## 边界

- 不要创建长篇书籍。
- 不要生成短篇成品。
- 不要把设定编辑请求写成场景推进；设定编辑必须通过 play_edit 持久化。
- 不要把玩家动作总结成普通问答；在 play 模式中，动作应推进场景。
- **【铁律】只要用户是在玩（已有互动世界、正在输入动作/台词/观察/移动/选择），你这一轮唯一要做的就是立即调用 play_step 工具——严禁自己输出任何场景正文、旁白或叙述。场景由 play_step 生成，不是你来写；你自己讲故事 = 失败，会让整个互动机制（状态、面板、世界图谱）失效。用户是在改规则/角色卡/persona/视觉契约时，用 play_edit；用户是在重做/换版/改上一条时，用 play_revise；不要调用 play_step。**

## 输出要求

- 不要使用表情符号。
- 普通讨论要直接回答；明确需要调用工具时，工具调用本身就是回答，不要先写寒暄、理解说明或空泛确认。
- 需要结构时用短列表；不要虚报工具执行结果。"#
        .to_string()
}

/// 聊天回环的工具执行器：play 工具优先（启用时），回落项目文件工具。
pub struct PlayChatToolExecutor<'a> {
    pub root: &'a Path,
    pub deps: Option<PlayToolDeps<'a>>,
}

#[async_trait::async_trait]
impl LoopToolExecutor for PlayChatToolExecutor<'_> {
    async fn execute(&self, name: &str, args: &Value) -> ToolResult {
        if let Some(deps) = &self.deps {
            if let Some(result) = execute_play_tool(deps, name, args).await {
                return result;
            }
        }
        crate::interaction::project_tools::execute_tool(self.root, name, args).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    use crate::llm::agent_router::LlmEndpointConfig;

    fn dummy_router() -> AgentRouter {
        AgentRouter::new(
            LlmEndpointConfig {
                base_url: "http://127.0.0.1:9".into(),
                api_key: "k".into(),
                model: "m".into(),
                max_tokens: 16,
                extra_headers: HashMap::new(),
            },
            HashMap::new(),
        )
    }

    #[test]
    fn safe_play_id_mirrors_ts() {
        assert_eq!(safe_play_id(Some("  w1  "), "fb").unwrap(), "w1");
        assert_eq!(safe_play_id(Some(""), "fb").unwrap(), "fb");
        assert_eq!(safe_play_id(None, "fb").unwrap(), "fb");
        // 80 码元截断（UTF-16）。
        let long = "x".repeat(90);
        assert_eq!(safe_play_id(Some(&long), "fb").unwrap().chars().count(), 80);
        for bad in [".", "..", "a/b", "a\\b", "a\0b"] {
            assert!(safe_play_id(Some(bad), "fb").is_err(), "{bad:?}");
        }
    }

    #[tokio::test]
    async fn play_step_empty_input_short_circuits() {
        let router = dummy_router();
        let deps = PlayToolDeps {
            project_root: Path::new("/nonexistent"),
            session_id: "1783000000000-t1",
            router: &router,
            language: "zh",
        };
        let result = execute_play_tool(&deps, "play_step", &json!({ "input": "   " }))
            .await
            .unwrap();
        assert_eq!(result.text, "Play input is empty.");
        assert!(!result.is_error);
    }

    #[tokio::test]
    async fn play_tools_without_world_return_graceful_text() {
        let dir = tempfile::tempdir().unwrap();
        let router = dummy_router();
        let deps = PlayToolDeps {
            project_root: dir.path(),
            session_id: "1783000000001-t2",
            router: &router,
            language: "zh",
        };
        let step = execute_play_tool(&deps, "play_step", &json!({ "input": "我走" }))
            .await
            .unwrap();
        assert_eq!(step.text, "还没有可推进的互动世界。先用 play_start 开一局。");
        assert!(!step.is_error);
        let revise = execute_play_tool(
            &deps,
            "play_revise",
            &json!({ "action": "regenerate_last" }),
        )
        .await
        .unwrap();
        assert_eq!(revise.text, "还没有可重做的互动世界。先用 play_start 开一局。");
        // 英文表面语言。
        let deps_en = PlayToolDeps { language: "en", ..deps };
        let step_en = execute_play_tool(&deps_en, "play_step", &json!({ "input": "go" }))
            .await
            .unwrap();
        assert_eq!(
            step_en.text,
            "There is no interactive world to advance yet. Start one with play_start first."
        );
    }

    #[tokio::test]
    async fn play_revise_validation_branches() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        crate::play::create_world(
            &root,
            &crate::play::PlayWorldInput {
                id: "1783000000002-t3",
                title: "试炼",
                premise: "p",
                world_contract: "",
                visual_contract: "",
                mode: "open",
                language: "zh",
            },
        )
        .await
        .unwrap();
        let router = dummy_router();
        let deps = PlayToolDeps {
            project_root: &root,
            session_id: "1783000000002-t3",
            router: &router,
            language: "zh",
        };
        // restore 缺 turn / variantId。
        let missing = execute_play_tool(
            &deps,
            "play_revise",
            &json!({ "action": "restore_variant", "turn": 1 }),
        )
        .await
        .unwrap();
        assert_eq!(missing.text, "恢复版本需要 turn 和 variantId。");
        assert!(!missing.is_error);
        // edit_last_input 缺 replacement。
        let edit = execute_play_tool(
            &deps,
            "play_revise",
            &json!({ "action": "edit_last_input" }),
        )
        .await
        .unwrap();
        assert_eq!(edit.text, "编辑上一条玩家动作需要提供新的 input。");
        // 非法 action → 错误（zod 枚举层的 Rust 等价）。
        let bogus = execute_play_tool(&deps, "play_revise", &json!({ "action": "bogus" }))
            .await
            .unwrap();
        assert!(bogus.is_error);
        assert!(bogus.text.contains("Invalid play_revise action"), "{}", bogus.text);
        // 未知工具 → None 回落。
        assert!(execute_play_tool(&deps, "read", &json!({})).await.is_none());
    }

    #[test]
    fn schemas_and_prompt_shape() {
        let schemas = play_tool_schemas();
        assert_eq!(schemas.len(), 2);
        assert_eq!(schemas[0]["function"]["name"], "play_step");
        assert_eq!(
            schemas[0]["function"]["parameters"]["required"][0], "input"
        );
        assert_eq!(schemas[1]["function"]["name"], "play_revise");
        let actions = schemas[1]["function"]["parameters"]["properties"]["action"]["enum"]
            .as_array()
            .unwrap();
        assert_eq!(actions.len(), 3);
        let zh = play_chat_system_prompt(false);
        assert!(zh.contains("【铁律】") && zh.contains("play_step") && zh.contains("play_edit"));
        let en = play_chat_system_prompt(true);
        assert!(en.contains("[HARD RULE]") && en.contains("play_revise"));
    }
}
