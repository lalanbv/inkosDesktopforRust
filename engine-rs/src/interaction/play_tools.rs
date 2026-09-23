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

use crate::interaction::project_tools::{error_result, ToolResult};
use crate::interaction::registry::{schema_description, schema_parameters, MutationKind, ToolDef};
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

fn no_world_edit_result(language: &str) -> ToolResult {
    let text = if language == "en" {
        "There is no interactive world to edit yet. Start one with play_start first."
    } else {
        "还没有可编辑的互动世界。先用 play_start 开一局。"
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
    let agents = PlayAgents { router: deps.router, root: deps.project_root };
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
    let agents = PlayAgents { router: deps.router, root: deps.project_root };
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

/// `mergeContract`（TS 逐字）：整文替换优先 → from/to 全量替换（缺失跳过）→
/// 末尾追加（已含跳过；空文直取 add；否则 `trim\n- add`）。
fn merge_contract(
    existing: &str,
    replacement: Option<&str>,
    replacements: Option<&Value>,
    addition: Option<&str>,
) -> String {
    if let Some(next) = replacement.map(str::trim).filter(|value| !value.is_empty()) {
        return next.to_string();
    }
    let mut current = existing.to_string();
    for patch in replacements.and_then(Value::as_array).into_iter().flatten() {
        let from = patch.get("from").and_then(Value::as_str).map(str::trim).unwrap_or_default();
        let to = patch.get("to").and_then(Value::as_str).map(str::trim).unwrap_or_default();
        if from.is_empty() || to.is_empty() || !current.contains(from) {
            continue;
        }
        current = current.replace(from, to);
    }
    let Some(add) = addition.map(str::trim).filter(|value| !value.is_empty()) else {
        return current;
    };
    if current.contains(add) {
        return current;
    }
    let trimmed = current.trim();
    if trimmed.is_empty() {
        return add.to_string();
    }
    format!("{trimmed}\n- {add}")
}

/// `playEditEntityId`：label 小写 → 非 [a-z0-9 汉字] 连续段折叠单 `_` →
/// 首尾 `_` 剥离 → 48 码元截断 → `{type}_{ascii}`；空 → `{type}_{ms 36 进制}`。
fn play_edit_entity_id(entity_type: &str, label: &str) -> String {
    let lowered = label.trim().to_lowercase();
    let mut collapsed = String::new();
    let mut in_run = false;
    for ch in lowered.chars() {
        let allowed = ch.is_ascii_lowercase()
            || ch.is_ascii_digit()
            || ('\u{4e00}'..='\u{9fff}').contains(&ch);
        if allowed {
            collapsed.push(ch);
            in_run = false;
        } else if !in_run {
            collapsed.push('_');
            in_run = true;
        }
    }
    let trimmed = collapsed.trim_matches('_');
    let mut ascii = String::new();
    let mut units = 0usize;
    for ch in trimmed.chars() {
        let ch_units = ch.len_utf16();
        if units + ch_units > 48 {
            break;
        }
        units += ch_units;
        ascii.push(ch);
    }
    if ascii.is_empty() {
        let ms = crate::interaction::session::utc_now_ms();
        format!("{entity_type}_{}", to_radix36(ms))
    } else {
        format!("{entity_type}_{ascii}")
    }
}

fn to_radix36(mut value: u64) -> String {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if value == 0 {
        return "0".to_string();
    }
    let mut out = Vec::new();
    while value > 0 {
        out.push(DIGITS[(value % 36) as usize] as char);
        value /= 36;
    }
    out.into_iter().rev().collect()
}

/// `resolvePlayEditEntityId`：显式 id 优先；label 精确匹配
/// snapshot.entities 的 label 或 id（首个命中）。
fn resolve_play_edit_entity_id(
    db: &crate::play_graph::PlayGraphDb,
    update: &Value,
) -> Option<String> {
    if let Some(id) = update
        .get("id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(id.to_string());
    }
    let label = update
        .get("label")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    db.snapshot()
        .get("entities")
        .and_then(Value::as_array)
        .and_then(|entities| {
            entities
                .iter()
                .find(|entity| {
                    entity.get("label").and_then(Value::as_str) == Some(label)
                        || entity.get("id").and_then(Value::as_str) == Some(label)
                })
                .and_then(|entity| entity.get("id").and_then(Value::as_str))
                .map(String::from)
        })
}

/// `upsertPlayEditEntity`：七字段规范实体（manual-edit 事件位）；既无 id
/// 可解析也无非空 label → 跳过（false）。summary/status 保留显式空串
/// （JS ?? 语义：清空 vs 未提供），label 走真值语义。
fn upsert_play_edit_entity(
    db: &mut crate::play_graph::PlayGraphDb,
    update: &Value,
) -> Result<bool, String> {
    let summary = update.get("summary").and_then(Value::as_str).map(str::trim);
    let status = update.get("status").and_then(Value::as_str).map(str::trim);
    let label = update.get("label").and_then(Value::as_str).map(str::trim);
    let entity_id = resolve_play_edit_entity_id(db, update);
    if entity_id.is_none() && label.is_none_or(|value| value.is_empty()) {
        return Ok(false);
    }
    let existing = entity_id.as_deref().and_then(|id| db.get_entity(id));
    let existing_field = |field: &str| -> Option<String> {
        existing
            .as_ref()
            .and_then(|entity| entity.get(field))
            .and_then(Value::as_str)
            .map(String::from)
    };
    let id = entity_id.clone().unwrap_or_else(|| {
        let type_for_id = update.get("type").and_then(Value::as_str).unwrap_or("actor");
        play_edit_entity_id(type_for_id, label.unwrap_or_default())
    });
    let entity_type = update
        .get("type")
        .and_then(Value::as_str)
        .map(String::from)
        .or_else(|| existing_field("type"))
        .unwrap_or_else(|| "actor".to_string());
    let label_value = label
        .filter(|value| !value.is_empty())
        .map(String::from)
        .or_else(|| existing_field("label"))
        .unwrap_or_else(|| id.clone());
    let summary_value = summary.map(String::from).or_else(|| existing_field("summary")).unwrap_or_default();
    let status_value = status.map(String::from).or_else(|| existing_field("status")).unwrap_or_default();
    db.upsert_entity(&json!({
        "id": id,
        "type": entity_type,
        "label": label_value,
        "summary": summary_value,
        "status": status_value,
        "createdEventId": existing_field("createdEventId").map(Value::String).unwrap_or(json!("manual-edit")),
        "updatedEventId": "manual-edit",
    }))?;
    Ok(true)
}

/// `play_edit`：持久编辑世界卡（契约/视觉/premise/player persona/实体卡），
/// 不推进时间、不生成新场景。
async fn tool_play_edit(deps: &PlayToolDeps<'_>, args: &Value) -> ToolResult {
    let Ok(world_id) = safe_play_id(Some(deps.session_id), deps.session_id) else {
        return error_result(format!("Invalid play id: {:?}", deps.session_id));
    };
    let run_id = "main";
    let Some(world) = crate::play::load_world(deps.project_root, &world_id).await else {
        return no_world_edit_result(deps.language);
    };
    let is_zh = world
        .get("language")
        .and_then(Value::as_str)
        .unwrap_or("zh")
        != "en";
    let world_contract = world.get("worldContract").and_then(Value::as_str).unwrap_or_default();
    let visual_contract = world.get("visualContract").and_then(Value::as_str).unwrap_or_default();

    let mut patch = serde_json::Map::new();
    let next_world_contract = merge_contract(
        world_contract,
        args.get("worldContract").and_then(Value::as_str),
        args.get("worldContractReplacements"),
        args.get("worldContractAppend").and_then(Value::as_str),
    );
    let next_visual_contract = merge_contract(
        visual_contract,
        args.get("visualContract").and_then(Value::as_str),
        args.get("visualContractReplacements"),
        args.get("visualContractAppend").and_then(Value::as_str),
    );
    if next_world_contract != world_contract {
        patch.insert("worldContract".into(), json!(next_world_contract));
    }
    if next_visual_contract != visual_contract {
        patch.insert("visualContract".into(), json!(next_visual_contract));
    }
    let premise = args
        .get("premise")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(premise) = &premise {
        if world.get("premise").and_then(Value::as_str) != Some(premise) {
            patch.insert("premise".into(), json!(premise));
        }
    }
    let updated_world = if patch.is_empty() {
        world.clone()
    } else {
        match crate::play::update_world(deps.project_root, &world_id, &Value::Object(patch.clone())).await {
            Ok(world) => world,
            Err(message) => return error_result(message),
        }
    };

    let edit = (async {
        crate::play::ensure_run(deps.project_root, &world_id, run_id).await?;
        let run_dir = crate::play::run_dir(deps.project_root, &world_id, run_id)?;
        let mut db = crate::play_graph::open_play_graph_db(&run_dir)?;
        let mut updated_entities = 0usize;
        let player_persona = args
            .get("playerPersona")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());
        if let Some(persona) = player_persona {
            let existing_player = db.get_entity("actor_player");
            let label = existing_player
                .as_ref()
                .and_then(|entity| entity.get("label"))
                .and_then(Value::as_str)
                .map(String::from)
                .unwrap_or_else(|| if is_zh { "玩家" } else { "Player" }.to_string());
            if upsert_play_edit_entity(
                &mut db,
                &json!({
                    "id": "actor_player",
                    "type": "actor",
                    "label": label,
                    "summary": persona,
                    "status": if is_zh { "已更新" } else { "Updated" },
                }),
            )? {
                updated_entities += 1;
            }
        }
        for update in args
            .get("entityUpdates")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if upsert_play_edit_entity(&mut db, update)? {
                updated_entities += 1;
            }
        }
        db.flush()?;
        let graph = db.snapshot();
        let current_state = crate::play::load_current_state(deps.project_root, &world_id, run_id)
            .await
            .filter(|state| state.is_object())
            .unwrap_or_else(|| json!({}));
        let mut next_state = current_state.as_object().cloned().unwrap_or_default();
        next_state.insert(
            "worldContract".into(),
            updated_world.get("worldContract").cloned().unwrap_or_default(),
        );
        next_state.insert(
            "visualContract".into(),
            updated_world.get("visualContract").cloned().unwrap_or_default(),
        );
        next_state.insert(
            "premise".into(),
            updated_world.get("premise").cloned().unwrap_or_default(),
        );
        next_state.insert(
            "graphEditedAt".into(),
            json!(crate::utils::utc_time::utc_now_iso()),
        );
        crate::play::save_current_state(
            deps.project_root,
            &world_id,
            run_id,
            &Value::Object(next_state),
        )
        .await?;
        Ok::<(usize, Value), String>((updated_entities, graph))
    })
    .await;
    let (updated_entities, graph) = match edit {
        Ok(outcome) => outcome,
        Err(message) => return error_result(message),
    };

    let note = args
        .get("note")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let text = note.unwrap_or(if is_zh {
        "互动世界设定已更新。"
    } else {
        "Interactive world settings updated."
    });
    text_result(
        text,
        Some(json!({
            "kind": "play_world_updated",
            "worldId": world_id,
            "runId": run_id,
            "world": updated_world,
            "updatedWorldContract": next_world_contract != world_contract,
            "updatedVisualContract": next_visual_contract != visual_contract,
            "updatedPremise": patch.contains_key("premise"),
            "updatedEntities": updated_entities,
            "graph": graph,
        })),
    )
}
/// play 工具 schema（OpenAI function 形态，PlayStepParams/PlayReviseParams/
/// PlayEditParams 逐字）。
pub fn play_tool_schemas() -> Vec<Value> {
    vec![
        json!({
            "type": "function",
            "function": {
                "name": "play_edit",
                "description": "Persistently edit the active InkOS Play world card, visual contract, player persona, or entity/role cards without advancing time or narrating a turn. Use when the user says to change world rules, visual rules, character goals/persona/status, or long-lived play contracts.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "worldContract": {
                            "type": "string",
                            "description": "Full updated world contract after applying the user's requested rule change. Use when the user edits world rules, time semantics, item semantics, role autonomy, taboos, or costs.",
                        },
                        "worldContractReplacements": {
                            "type": "array",
                            "description": "Exact replacements for existing world-contract wording. Use when the user says to change/replace X into Y; do not append the new rule while leaving the old wording in place.",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "from": { "type": "string", "description": "Exact old wording to replace in the existing contract." },
                                    "to": { "type": "string", "description": "New wording that should replace the old wording." },
                                },
                                "required": ["from", "to"],
                            },
                        },
                        "worldContractAppend": {
                            "type": "string",
                            "description": "A narrow new world-contract addition. Do not use this for replacements such as 'change X to Y'; use worldContractReplacements or full worldContract instead.",
                        },
                        "visualContract": {
                            "type": "string",
                            "description": "Full updated visual contract after applying the user's requested image/visual-rule change.",
                        },
                        "visualContractReplacements": {
                            "type": "array",
                            "description": "Exact replacements for existing visual-contract wording. Use when the user says to change/replace one visual rule into another.",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "from": { "type": "string", "description": "Exact old wording to replace in the existing contract." },
                                    "to": { "type": "string", "description": "New wording that should replace the old wording." },
                                },
                                "required": ["from", "to"],
                            },
                        },
                        "visualContractAppend": {
                            "type": "string",
                            "description": "A narrow new visual-contract addition. Do not use this for replacements such as 'change X to Y'; use worldContractReplacements or full visualContract instead.",
                        },
                        "premise": {
                            "type": "string",
                            "description": "Updated world premise only when the user explicitly changes premise/backstory. Do not rewrite premise for ordinary turns.",
                        },
                        "playerPersona": {
                            "type": "string",
                            "description": "Updated player persona/identity/goals. This updates the reserved actor_player entity.",
                        },
                        "entityUpdates": {
                            "type": "array",
                            "description": "Character, object, place, or rule-card updates requested by the user. Use for role goals, status, motives, taboos, or known facts.",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "id": { "type": "string", "description": "Existing entity id to update. Use actor_player for the player persona." },
                                    "label": { "type": "string", "description": "Existing entity label to update when id is unknown." },
                                    "type": {
                                        "type": "string",
                                        "enum": ["actor", "location", "item", "evidence", "clue", "claim", "proof_chain", "organization", "rule", "scene", "event"],
                                        "description": "Entity type when creating a missing entity. Usually actor for character/persona edits.",
                                    },
                                    "summary": { "type": "string", "description": "Replacement or enriched entity summary, including goals/motives/persona when relevant." },
                                    "status": { "type": "string", "description": "Natural-language current status. Do not invent numeric meters unless the user asked for them." },
                                },
                            },
                        },
                        "note": {
                            "type": "string",
                            "description": "Short human-readable note summarizing what changed.",
                        },
                    },
                },
            },
        }),
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




// ── 注册模块（R38b）：play 三件——推进/修订/编辑互动世界（TS 剔除名单
//    不含 play 面）；available = play 世界在场；schema 经同文件
//    play_tool_schemas 拆解引用。 ──

crate::interaction::registry::tool_def!(
    PlayStep,
    "play_step",
    MutationKind::ProjectWrite,
    ctx, args,
    { {
        let schemas = play_tool_schemas();
        schema_description(&schemas, "play_step")
    } },
    { {
        let schemas = play_tool_schemas();
        schema_parameters(&schemas, "play_step")
    } },
    ctx.play_deps.is_some(),
    tool_play_step(ctx.play_deps.as_ref().expect("available 门控"), args).await
);

crate::interaction::registry::tool_def!(
    PlayRevise,
    "play_revise",
    MutationKind::ProjectWrite,
    ctx, args,
    { {
        let schemas = play_tool_schemas();
        schema_description(&schemas, "play_revise")
    } },
    { {
        let schemas = play_tool_schemas();
        schema_parameters(&schemas, "play_revise")
    } },
    ctx.play_deps.is_some(),
    tool_play_revise(ctx.play_deps.as_ref().expect("available 门控"), args).await
);

crate::interaction::registry::tool_def!(
    PlayEdit,
    "play_edit",
    MutationKind::ProjectWrite,
    ctx, args,
    { {
        let schemas = play_tool_schemas();
        schema_description(&schemas, "play_edit")
    } },
    { {
        let schemas = play_tool_schemas();
        schema_parameters(&schemas, "play_edit")
    } },
    ctx.play_deps.is_some(),
    tool_play_edit(ctx.play_deps.as_ref().expect("available 门控"), args).await
);

/// 注册表汇聚口（registry 装配序 = 原 execute_play_tool match 序）。
pub(crate) fn defs() -> Vec<Box<dyn ToolDef>> {
    vec![
        Box::new(PlayStep),
        Box::new(PlayRevise),
        Box::new(PlayEdit),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    /// 注册表路由测试通道（R38b）：与生产同路径（find→execute），替代原
    /// match 分发壳——装配错名在此红。
    async fn route(
        deps: &PlayToolDeps<'_>,
        name: &str,
        args: &Value,
    ) -> Option<ToolResult> {
        let mut ctx = crate::interaction::registry::ToolCtx::root_only(deps.project_root);
        ctx.play_deps = Some(deps);
        let def = crate::interaction::registry::ToolRegistry::global().find(name, &ctx)?;
        Some(def.execute(&ctx, args).await)
    }

    use std::collections::HashMap;

    use crate::llm::agent_router::LlmEndpointConfig;

    fn dummy_router() -> AgentRouter {
        AgentRouter::new(
            LlmEndpointConfig {
                context_window_tokens: 128_000,
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
        let result = route(&deps, "play_step", &json!({ "input": "   " }))
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
        let step = route(&deps, "play_step", &json!({ "input": "我走" }))
            .await
            .unwrap();
        assert_eq!(step.text, "还没有可推进的互动世界。先用 play_start 开一局。");
        assert!(!step.is_error);
        let revise = route(
            &deps,
            "play_revise",
            &json!({ "action": "regenerate_last" }),
        )
        .await
        .unwrap();
        assert_eq!(revise.text, "还没有可重做的互动世界。先用 play_start 开一局。");
        // 英文表面语言。
        let deps_en = PlayToolDeps { language: "en", ..deps };
        let step_en = route(&deps_en, "play_step", &json!({ "input": "go" }))
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
        let missing = route(
            &deps,
            "play_revise",
            &json!({ "action": "restore_variant", "turn": 1 }),
        )
        .await
        .unwrap();
        assert_eq!(missing.text, "恢复版本需要 turn 和 variantId。");
        assert!(!missing.is_error);
        // edit_last_input 缺 replacement。
        let edit = route(
            &deps,
            "play_revise",
            &json!({ "action": "edit_last_input" }),
        )
        .await
        .unwrap();
        assert_eq!(edit.text, "编辑上一条玩家动作需要提供新的 input。");
        // 非法 action → 错误（zod 枚举层的 Rust 等价）。
        let bogus = route(&deps, "play_revise", &json!({ "action": "bogus" }))
            .await
            .unwrap();
        assert!(bogus.is_error);
        assert!(bogus.text.contains("Invalid play_revise action"), "{}", bogus.text);
        // 未知工具 → None 回落。
        // read 不归 play 族——注册表下落文件层（缺参数错误为文件层语义）。
        let fallback = route(&deps, "read", &json!({})).await.unwrap();
        assert!(fallback.is_error && fallback.text.contains("read requires a path"));
    }

    #[test]
    fn schemas_and_prompt_shape() {
        let schemas = play_tool_schemas();
        assert_eq!(schemas.len(), 3);
        assert_eq!(schemas[0]["function"]["name"], "play_edit");
        assert_eq!(
            schemas[0]["function"]["parameters"]["properties"]["entityUpdates"]["items"]
                ["properties"]["type"]["enum"]
                .as_array()
                .unwrap()
                .len(),
            11
        );
        assert_eq!(schemas[1]["function"]["name"], "play_step");
        assert_eq!(
            schemas[1]["function"]["parameters"]["required"][0], "input"
        );
        assert_eq!(schemas[2]["function"]["name"], "play_revise");
        let actions = schemas[2]["function"]["parameters"]["properties"]["action"]["enum"]
            .as_array()
            .unwrap();
        assert_eq!(actions.len(), 3);
        let zh = play_chat_system_prompt(false);
        assert!(zh.contains("【铁律】") && zh.contains("play_step") && zh.contains("play_edit"));
        let en = play_chat_system_prompt(true);
        assert!(en.contains("[HARD RULE]") && en.contains("play_revise"));
    }

    #[test]
    fn merge_contract_branches() {
        // 整文替换优先。
        assert_eq!(
            merge_contract("旧规则", Some("  新规则  "), None, Some("追加")),
            "新规则"
        );
        // 替换全量 + 缺失/空跳过。
        let replacements = json!([
            { "from": "普通差错", "to": "轻微差错" },
            { "from": "不存在的段落", "to": "X" },
            { "from": "", "to": "Y" },
        ]);
        assert_eq!(
            merge_contract("风险：普通差错 / 复核", None, Some(&replacements), None),
            "风险：轻微差错 / 复核"
        );
        // 追加：非空拼接 + 已含跳过 + 空文直取。
        assert_eq!(
            merge_contract("已有规则", None, None, Some("新条款")),
            "已有规则\n- 新条款"
        );
        assert_eq!(merge_contract("已有规则", None, None, Some("已有规则")), "已有规则");
        assert_eq!(merge_contract("", None, None, Some("首条")), "首条");
    }

    #[test]
    fn play_edit_entity_id_collapse_and_fallback() {
        assert_eq!(play_edit_entity_id("actor", " 室友 林青！ "), "actor_室友_林青");
        assert_eq!(play_edit_entity_id("rule", "No.1 taboo"), "rule_no_1_taboo");
        // 纯符号 → 时间戳 36 进制回退。
        let fallback = play_edit_entity_id("actor", "！！");
        assert!(fallback.starts_with("actor_"), "{fallback}");
        assert!(fallback["actor_".len()..].chars().all(|c| c.is_ascii_alphanumeric()));
        // 48 码元截断。
        let long = "字".repeat(60);
        let id = play_edit_entity_id("item", &long);
        assert_eq!(id.trim_start_matches("item_").chars().count(), 48);
    }

    async fn seed_edit_world(root: &std::path::Path, world_id: &str) {
        crate::play::create_world(
            root,
            &crate::play::PlayWorldInput {
                id: world_id,
                title: "雨夜合租屋",
                premise: "我刚搬进合租屋。",
                world_contract: "时间按动作语义推进。",
                visual_contract: "雨夜冷光，不使用游戏 UI。",
                mode: "open",
                language: "zh",
            },
        )
        .await
        .unwrap();
        crate::play::ensure_run(root, world_id, "main").await.unwrap();
        crate::play::save_current_state(
            root,
            world_id,
            "main",
            &json!({ "scene": "餐桌上有一只旧瓷杯。" }),
        )
        .await
        .unwrap();
        let run_dir = crate::play::run_dir(root, world_id, "main").unwrap();
        let mut db = crate::play_graph::open_play_graph_db(&run_dir).unwrap();
        for entity in [
            json!({ "id": "actor_player", "type": "actor", "label": "新租客", "summary": "刚搬进合租屋。", "status": "观察" }),
            json!({ "id": "actor_linqing", "type": "actor", "label": "室友林青", "summary": "旧目标", "status": "观望" }),
        ] {
            db.upsert_entity(&entity).unwrap();
        }
        db.flush().unwrap();
    }

    #[tokio::test]
    async fn play_edit_full_flow_persists_world_persona_and_entities() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        seed_edit_world(&root, "play-edit-session").await;
        let router = dummy_router();
        let deps = PlayToolDeps {
            project_root: &root,
            session_id: "play-edit-session",
            router: &router,
            language: "zh",
        };
        let result = route(
            &deps,
            "play_edit",
            &json!({
                "worldContractAppend": "室友会自主行动，玩家等待时她也会推进自己的目标。",
                "visualContract": "物件情绪重量通过摆放距离、磨损、光线和人物反应体现。",
                "playerPersona": "我是刚搬进来的租客，想查清停电夜。",
                "entityUpdates": [{
                    "label": "室友林青",
                    "type": "actor",
                    "summary": "隐瞒停电夜真相，目标是试探玩家是否可信。",
                    "status": "戒备",
                }],
                "note": "合租屋规则已更新。",
            }),
        )
        .await
        .unwrap();
        assert_eq!(result.text, "合租屋规则已更新。");
        let details = result.details.unwrap();
        assert_eq!(details["kind"], "play_world_updated");
        assert_eq!(details["worldId"], "play-edit-session");
        assert_eq!(details["runId"], "main");
        assert_eq!(details["updatedWorldContract"], true);
        assert_eq!(details["updatedVisualContract"], true);
        assert_eq!(details["updatedPremise"], false);
        assert_eq!(details["updatedEntities"], 2);

        let world = crate::play::load_world(&root, "play-edit-session").await.unwrap();
        assert!(
            world["worldContract"].as_str().unwrap().contains("室友会自主行动"),
            "{}",
            world["worldContract"]
        );
        assert!(world["visualContract"].as_str().unwrap().contains("物件情绪重量"));
        assert!(world["updatedAt"].as_str().is_some_and(|v| !v.is_empty()));

        let state = crate::play::load_current_state(&root, "play-edit-session", "main")
            .await
            .unwrap();
        assert_eq!(state["scene"], "餐桌上有一只旧瓷杯。");
        assert!(
            state["worldContract"].as_str().unwrap().contains("室友会自主行动"),
            "{state}"
        );
        assert!(state["graphEditedAt"].as_str().is_some_and(|v| !v.is_empty()));

        let graph = details["graph"].clone();
        let player = graph["entities"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["id"] == "actor_player")
            .unwrap();
        assert_eq!(player["label"], "新租客");
        assert_eq!(player["summary"], "我是刚搬进来的租客，想查清停电夜。");
        assert_eq!(player["status"], "已更新");
        assert_eq!(player["updatedEventId"], "manual-edit");
        let linqing = graph["entities"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["id"] == "actor_linqing")
            .unwrap();
        assert_eq!(linqing["status"], "戒备");
        assert_eq!(linqing["summary"], "隐瞒停电夜真相，目标是试探玩家是否可信。");
    }

    #[tokio::test]
    async fn play_edit_replaces_wording_instead_of_appending() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        crate::play::create_world(
            &root,
            &crate::play::PlayWorldInput {
                id: "play-contract-replace",
                title: "午夜药房",
                premise: "实习药剂师值夜班。",
                world_contract: "风险重量：普通差错 / 需要复核 / 可能追责 / 不能公开。时间按动作自然流动。",
                visual_contract: "监控冷光。",
                mode: "open",
                language: "zh",
            },
        )
        .await
        .unwrap();
        crate::play::ensure_run(&root, "play-contract-replace", "main")
            .await
            .unwrap();
        crate::play::save_current_state(
            &root,
            "play-contract-replace",
            "main",
            &json!({ "turn": 0, "worldContract": "风险重量：普通差错 / 需要复核 / 可能追责 / 不能公开。时间按动作自然流动。" }),
        )
        .await
        .unwrap();
        let router = dummy_router();
        let deps = PlayToolDeps {
            project_root: &root,
            session_id: "play-contract-replace",
            router: &router,
            language: "zh",
        };
        let result = route(
            &deps,
            "play_edit",
            &json!({
                "worldContractReplacements": [{
                    "from": "普通差错 / 需要复核 / 可能追责 / 不能公开",
                    "to": "普通差错 / 需要复核 / 涉及追责 / 需要主任签字",
                }],
                "note": "风险重量已替换。",
            }),
        )
        .await
        .unwrap();
        let details = result.details.unwrap();
        assert_eq!(details["updatedWorldContract"], true);
        assert_eq!(details["updatedVisualContract"], false);
        assert_eq!(details["updatedEntities"], 0);
        let contract = details["world"]["worldContract"].as_str().unwrap();
        assert!(contract.contains("涉及追责 / 需要主任签字"), "{contract}");
        assert!(!contract.contains("可能追责 / 不能公开"), "{contract}");
        let world = crate::play::load_world(&root, "play-contract-replace").await.unwrap();
        assert!(world["worldContract"].as_str().unwrap().contains("涉及追责"));
        let state = crate::play::load_current_state(&root, "play-contract-replace", "main")
            .await
            .unwrap();
        assert_eq!(state["turn"], 0);
        assert!(
            state["worldContract"].as_str().unwrap().contains("涉及追责 / 需要主任签字"),
            "{state}"
        );
    }

    #[tokio::test]
    async fn play_edit_without_world_returns_graceful_text() {
        let dir = tempfile::tempdir().unwrap();
        let router = dummy_router();
        let deps = PlayToolDeps {
            project_root: dir.path(),
            session_id: "1783000000003-t4",
            router: &router,
            language: "zh",
        };
        let result = route(&deps, "play_edit", &json!({})).await.unwrap();
        assert_eq!(result.text, "还没有可编辑的互动世界。先用 play_start 开一局。");
        assert!(!result.is_error);
        // 空参数 + 已有世界 → 默认文案 + 零更新。
        crate::play::create_world(
            dir.path(),
            &crate::play::PlayWorldInput {
                id: "1783000000003-t4",
                title: "空",
                premise: "p",
                world_contract: "",
                visual_contract: "",
                mode: "open",
                language: "zh",
            },
        )
        .await
        .unwrap();
        let result = route(&deps, "play_edit", &json!({})).await.unwrap();
        assert_eq!(result.text, "互动世界设定已更新。");
        let details = result.details.unwrap();
        assert_eq!(details["updatedEntities"], 0);
        assert_eq!(details["updatedWorldContract"], false);
    }
}
