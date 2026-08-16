//! play 回合执行流（play-runner.ts + play-agents.ts 代理面，73 号；79 号补
//! sceneReconciler 对账 + regenerateLastTurn 变体/检查点面）。
//!
//! step 链：interpret（动作归一，fail-open 降级为 do）→ mutate（状态草案，
//! fail-open 降级为 blocked 回合）→ render（场景正文，fail-open 降级为原始
//! prose/占位）→ **reconcile（正文↔图谱对账，补充 mutation 合成，fail-open
//! 空补充）→ checkpoint（before-turn-{N} 先于 apply 落盘）** → 全部提交
//! （render 先行、提交在后——回合 all-or-nothing）。
//!
//! regenerateLastTurn：当前态存变体 → 回滚 before-turn 检查点 → 重放（带
//! replayContext 约束"重写非新回合/时间不倒退"）→ 新态再存变体；restoreVariant
//! 按变体 id 整体恢复。
//!
//! 82 号：三代理 + context 标签 + 开场播种全量双语（en 分支对齐
//! play-agents.ts / play-runner.ts 的 en 文案；zh 维持 73 号形态）。

use std::path::Path;

use serde_json::{json, Value};

use crate::llm::agent_router::AgentRouter;
use crate::llm::provider::{LLMMessage, LLMRole};
use crate::play_graph::{apply_play_mutation, open_play_graph_db, seed_play_graph};

// ── 三代理 trait ────────────────────────────────────────────────

#[async_trait::async_trait]
pub trait PlayActionInterpreter: Send + Sync {
    async fn interpret(&self, input: &str, scene_brief: &str, language: &str) -> Value;
}

#[async_trait::async_trait]
pub trait PlayWorldMutator: Send + Sync {
    async fn propose_mutation(&self, turn: i64, input: &str, action: &Value, context: &str, language: &str) -> Value;
}

#[async_trait::async_trait]
pub trait PlaySceneRenderer: Send + Sync {
    #[allow(clippy::too_many_arguments)]
    async fn render(
        &self,
        input: &str,
        action: &Value,
        mutation_summary: &str,
        state_brief: &str,
        mode: &str,
        world_premise: &str,
        replay_context: Option<&str>,
        language: &str,
    ) -> RenderedScene;
}

#[derive(Debug, Clone)]
pub struct RenderedScene {
    pub scene_text: String,
    pub suggested_actions: Vec<String>,
}

/// `PlaySceneReconcilerLike.reconcile`：正文↔图谱对账（返回补充 mutation；
/// fail-open 语义由实现承担——失败返回空补充）。
#[async_trait::async_trait]
pub trait PlaySceneReconciler: Send + Sync {
    #[allow(clippy::too_many_arguments)]
    async fn reconcile(
        &self,
        turn: i64,
        input: &str,
        action: &Value,
        mutation: &Value,
        scene_text: &str,
        context: &str,
        state_brief: &str,
        world_premise: &str,
        language: &str,
    ) -> Value;
}

pub struct PlayAgents<'a> {
    pub router: &'a AgentRouter,
}

fn parse_json_value(raw: &str) -> Option<Value> {
    let trimmed = raw.trim();
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        return Some(value);
    }
    // fence 剥离 + 首 { 到末 }。
    let fence = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .and_then(|body| body.strip_suffix("```"))
        .map(str::trim);
    if let Some(body) = fence {
        if let Ok(value) = serde_json::from_str::<Value>(body) {
            return Some(value);
        }
    }
    let start = trimmed.find('{')?;
    let end = trimmed.rfind('}')?;
    if end > start {
        serde_json::from_str(&trimmed[start..=end]).ok()
    } else {
        None
    }
}

fn is_retryable_error(message: &str) -> bool {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::Regex::new(
            r"(?i)50[0-9]|429|temporarily unavailable|timeout|timed out|socket|terminated|econn|network|error sending request|bad gateway|service unavailable|rate limit",
        )
        .unwrap()
    });
    re.is_match(&message.to_lowercase())
}

async fn chat_with_retry(
    router: &AgentRouter,
    agent: &str,
    messages: Vec<LLMMessage>,
    temperature: f64,
    max_tokens: u32,
) -> Result<String, String> {
    let mut last_err = String::new();
    for attempt in 0..=2 {
        match router.chat(agent, messages.clone(), temperature, Some(max_tokens)).await {
            Ok(outcome) => return Ok(outcome.content),
            Err(error) => {
                let message = error.to_string();
                last_err = message.clone();
                if attempt >= 2 || !is_retryable_error(&message) {
                    return Err(message);
                }
                tokio::time::sleep(std::time::Duration::from_millis(400 * (attempt as u64 + 1))).await;
            }
        }
    }
    Err(last_err)
}

#[async_trait::async_trait]
impl PlayActionInterpreter for PlayAgents<'_> {
    async fn interpret(&self, input: &str, scene_brief: &str, language: &str) -> Value {
        // fail-open：瞬时错误/不可解析 → 通用动作（玩家原话作 do）。
        let raw = chat_with_retry(
            self.router,
            "play-action-interpreter",
            vec![
                LLMMessage { role: LLMRole::System, content: action_interpreter_system_prompt(language), tool_calls: None, tool_call_id: None },
                LLMMessage { role: LLMRole::User, content: action_interpreter_user_prompt(input, scene_brief, language), tool_calls: None, tool_call_id: None },
            ],
            0.15,
            1024,
        )
        .await
        .ok()
        .and_then(|content| parse_json_value(&content))
        .unwrap_or_else(|| json!({}));
        crate::play_parser::normalize_action_intent(&raw)
    }
}

#[async_trait::async_trait]
impl PlayWorldMutator for PlayAgents<'_> {
    async fn propose_mutation(&self, turn: i64, input: &str, action: &Value, context: &str, language: &str) -> Value {
        let raw = chat_with_retry(
            self.router,
            "play-world-mutator",
            vec![
                LLMMessage { role: LLMRole::System, content: world_mutator_system_prompt(language), tool_calls: None, tool_call_id: None },
                LLMMessage { role: LLMRole::User, content: world_mutator_user_prompt(turn, input, action, context, language), tool_calls: None, tool_call_id: None },
            ],
            0.25,
            4096,
        )
        .await
        .ok()
        .and_then(|content| parse_json_value(&content))
        .unwrap_or_else(|| {
            // fail-open：blocked 无操作回合（带原因）。
            json!({
                "turn": turn,
                "actionKind": action.get("actionKind").cloned().unwrap_or(json!("do")),
                "blocked": true,
                "blockedReason": "模型输出无法解析为有效的状态变更，本回合未推进世界状态。",
            })
        });
        let mut normalized = crate::play_parser::normalize_play_mutation(&raw);
        // eventId 缺省补 evt-{turn}（mutator 惯例）。
        let event_id = normalized
            .get("eventId")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if event_id.is_empty() {
            if let Some(obj) = normalized.as_object_mut() {
                obj.insert("eventId".into(), json!(format!("evt-{turn}")));
            }
        }
        normalized
    }
}

#[async_trait::async_trait]
impl PlaySceneRenderer for PlayAgents<'_> {
    #[allow(clippy::too_many_arguments)]
    async fn render(
        &self,
        input: &str,
        action: &Value,
        mutation_summary: &str,
        state_brief: &str,
        mode: &str,
        world_premise: &str,
        replay_context: Option<&str>,
        language: &str,
    ) -> RenderedScene {
        let system = scene_renderer_system_prompt(mode, language);
        let user = scene_renderer_user_prompt(input, action, mutation_summary, state_brief, world_premise, replay_context, language);
        let mut messages = vec![
            LLMMessage { role: LLMRole::System, content: system, tool_calls: None, tool_call_id: None },
            LLMMessage { role: LLMRole::User, content: user, tool_calls: None, tool_call_id: None },
        ];
        // 渲染永不抛错：瞬时重试 + 一次严格 JSON 追问 + prose 兜底。
        let mut last_content = String::new();
        for _attempt in 0..3 {
            let content = match chat_with_retry(
                self.router,
                "play-scene-renderer",
                messages.clone(),
                0.45,
                4096,
            )
            .await
            {
                Ok(content) => content,
                Err(_) => break,
            };
            if content.is_empty() {
                continue;
            }
            last_content = content.clone();
            if let Some(parsed) = parse_json_value(&content) {
                let scene_text = parsed.get("sceneText").and_then(Value::as_str).unwrap_or_default();
                if !scene_text.is_empty() {
                    let suggested: Vec<String> = parsed
                        .get("suggestedActions")
                        .and_then(Value::as_array)
                        .map(|items| {
                            items
                                .iter()
                                .filter_map(|item| item.as_str().map(String::from))
                                .take(4)
                                .collect()
                        })
                        .unwrap_or_default();
                    return RenderedScene { scene_text: scene_text.to_string(), suggested_actions: suggested };
                }
            }
            messages.push(LLMMessage { role: LLMRole::Assistant, content, tool_calls: None, tool_call_id: None });
            messages.push(LLMMessage {
                role: LLMRole::User,
                content: if language == "en" {
                    "That was not strict JSON. Output ONLY one JSON object {\"sceneText\": \"...\", \"suggestedActions\": [\"...\"]} and nothing else.".to_string()
                } else {
                    "上面不是严格 JSON。只输出一个 JSON 对象 {\"sceneText\": \"...\", \"suggestedActions\": [\"...\"]}，不要任何其他文字。".to_string()
                },
                tool_calls: None,
                tool_call_id: None,
            });
        }
        let prose = last_content
            .trim()
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim()
            .to_string();
        RenderedScene {
            scene_text: if prose.is_empty() {
                if language == "en" {
                    "(The moment holds, unresolved.)".to_string()
                } else {
                    "（这一拍悬着，没有落定。）".to_string()
                }
            } else {
                prose
            },
            suggested_actions: Vec::new(),
        }
    }
}

#[async_trait::async_trait]
impl PlaySceneReconciler for PlayAgents<'_> {
    #[allow(clippy::too_many_arguments)]
    async fn reconcile(
        &self,
        turn: i64,
        input: &str,
        action: &Value,
        mutation: &Value,
        scene_text: &str,
        context: &str,
        state_brief: &str,
        world_premise: &str,
        language: &str,
    ) -> Value {
        let action_kind = action.get("actionKind").and_then(Value::as_str).unwrap_or("do");
        let event_id = format!("evt-{turn}");
        // fail-open：任何失败（LLM/解析/schema）→ 空补充（emptyReconciliation）。
        let content = chat_with_retry(
            self.router,
            "play-scene-reconciler",
            vec![
                LLMMessage { role: LLMRole::System, content: scene_reconciler_system_prompt(language), tool_calls: None, tool_call_id: None },
                LLMMessage { role: LLMRole::User, content: scene_reconciler_user_prompt(
                    turn, action_kind, input, mutation, scene_text, context, state_brief, world_premise, language,
                ), tool_calls: None, tool_call_id: None },
            ],
            0.1,
            2048,
        )
        .await
        .ok()
        .and_then(|content| parse_json_value(&content))
        .map(|raw| {
            // PlayMutationSchema.parse + eventId/turn/actionKind 补齐。
            let mut normalized = crate::play_parser::normalize_play_mutation(&raw);
            if let Some(obj) = normalized.as_object_mut() {
                let has_event_id = obj.get("eventId").and_then(Value::as_str).is_some_and(|v| !v.is_empty());
                if !has_event_id {
                    obj.insert("eventId".into(), json!(event_id));
                }
                let has_turn = obj.get("turn").and_then(Value::as_i64).unwrap_or(0) != 0;
                if !has_turn {
                    obj.insert("turn".into(), json!(turn));
                }
                let has_kind = obj.get("actionKind").and_then(Value::as_str).is_some_and(|v| !v.is_empty());
                if !has_kind {
                    obj.insert("actionKind".into(), json!(action_kind));
                }
            }
            normalized
        })
        .unwrap_or_else(|| empty_reconciliation(turn, action_kind));
        content
    }
}

/// `emptyReconciliation`：空补充 mutation（fail-open 兜底形态）。
fn empty_reconciliation(turn: i64, action_kind: &str) -> Value {
    json!({
        "eventId": format!("evt-{turn}"),
        "turn": turn,
        "actionKind": action_kind,
        "summary": "",
        "entities": { "upsert": [] },
        "edges": { "upsert": [], "expire": [] },
        "stateSlots": { "upsert": [] },
        "evidence": { "transitions": [] },
        "blocked": false,
        "blockedReason": "",
        "notes": [],
    })
}

fn scene_reconciler_system_prompt(language: &str) -> String {
    if language == "en" {
        [
            "You reconcile an interactive-fiction scene with the world graph.",
            "Compare the rendered prose against the already applied changes and current state summary.",
            "If the prose introduced a concrete named object, clue, evidence, location, organization, or person that is not represented in the applied changes/current state, output ONLY supplemental PlayMutation entries for those missing graph facts.",
            "Do not rewrite prose. Do not invent facts that are not in the rendered scene. If nothing is missing, output an empty PlayMutation with empty arrays.",
            "Use the same eventId/turn/actionKind. For tangible things the player now physically holds, add a holding edge from actor_player with value.role=\"holding\"; if the target is evidence/clue/claim/proof_chain rather than an item, also set value.physical=true. Observed phenomena or learned facts are not holdings.",
            "Output strict JSON matching PlayMutation.",
        ]
        .join("\n")
    } else {
        [
            "你负责把互动小说正文和世界图谱对齐。",
            "对照已经应用的本回合变化、当前状态摘要和最终正文。",
            "如果正文里出现了具体且具名的新物件、线索、证据、地点、组织或人物，但它还没有体现在已应用变化/当前状态里，只输出这些缺失图谱事实的补充 PlayMutation。",
            "不要改正文，不要发明正文没有的事实。没有缺失就输出空的 PlayMutation，各数组留空。",
            "沿用同一个 eventId/turn/actionKind。玩家获得或拿在手里的实物，需要补一条 actor_player 指向该实体、value.role=\"holding\" 的 edge；如果目标是 evidence/clue/claim/proof_chain 而不是 item，还要设置 value.physical=true。观察到的现象或知道的信息不是持有物。",
            "输出严格 JSON，必须符合 PlayMutation。",
        ]
        .join("\n")
    }
}

#[allow(clippy::too_many_arguments)]
fn scene_reconciler_user_prompt(
    turn: i64,
    action_kind: &str,
    input: &str,
    mutation: &Value,
    scene_text: &str,
    context: &str,
    state_brief: &str,
    world_premise: &str,
    language: &str,
) -> String {
    let event_id = format!("evt-{turn}");
    let applied = serde_json::to_string_pretty(mutation).unwrap_or_default();
    if language == "en" {
        let mut lines = vec![
            format!("eventId: {event_id}"),
            format!("turn: {turn}"),
            format!("actionKind: {action_kind}"),
            String::new(),
        ];
        if !world_premise.is_empty() {
            lines.push("World setting:".to_string());
            lines.push(world_premise.to_string());
            lines.push(String::new());
        }
        lines.extend([
            "Player input:".to_string(),
            input.to_string(),
            String::new(),
            "Current context before this turn:".to_string(),
            context.to_string(),
            String::new(),
            "Applied mutation:".to_string(),
            applied,
            String::new(),
            "Current state summary:".to_string(),
            state_brief.to_string(),
            String::new(),
            "Rendered scene:".to_string(),
            scene_text.to_string(),
        ]);
        lines.join("\n")
    } else {
        let mut lines = vec![
            format!("eventId: {event_id}"),
            format!("turn: {turn}"),
            format!("actionKind: {action_kind}"),
            String::new(),
        ];
        if !world_premise.is_empty() {
            lines.push("世界设定：".to_string());
            lines.push(world_premise.to_string());
            lines.push(String::new());
        }
        lines.extend([
            "玩家输入：".to_string(),
            input.to_string(),
            String::new(),
            "本回合前的当前上下文：".to_string(),
            context.to_string(),
            String::new(),
            "已应用 mutation：".to_string(),
            applied,
            String::new(),
            "当前状态摘要：".to_string(),
            state_brief.to_string(),
            String::new(),
            "最终正文：".to_string(),
            scene_text.to_string(),
        ]);
        lines.join("\n")
    }
}

// ── 提示词（zh 逐字；reconciler 与 replayContext 双语，79 号） ──

fn action_interpreter_system_prompt(language: &str) -> String {
    if language == "en" {
        return [
            "You are an interactive-fiction action interpreter.",
            "Your job is to normalize one line of the player's natural language into one of five action kinds: look / say / move / do / wait.",
            "Do not add drama for the player, do not advance the plot, do not write scene prose.",
            "look = observe/examine/recall a clue; say = speak/probe/confront; move = move to a location; do = perform an action/use an item/investigate; wait = wait/stall/watch.",
            "Output strict JSON, no explanation.",
        ]
        .join("\n");
    }
    [
        "你是互动小说动作理解器。",
        "你的任务是把玩家一句自然语言，归一成五类动作之一：look / say / move / do / wait。",
        "不要替玩家加戏，不要直接推进剧情，不要写场景正文。",
        "look=观察/检查/回忆线索；say=说话/试探/质问；move=移动到地点；do=执行动作/使用物品/调查；wait=等待/拖延/旁观。",
        "输出严格 JSON，不要解释。",
    ]
    .join("\n")
}

fn action_interpreter_user_prompt(input: &str, scene_brief: &str, language: &str) -> String {
    if language == "en" {
        return [
            "Current scene:",
            scene_brief,
            "",
            "Player input:",
            input,
            "",
            "Output fields: actionKind, targetEntityLabel?, targetLocationLabel?, intent, manner, risk, ambiguity, secondaryActions.",
        ]
        .join("\n");
    }
    [
        "当前场景：",
        scene_brief,
        "",
        "玩家输入：",
        input,
        "",
        "输出字段：actionKind, targetEntityLabel?, targetLocationLabel?, intent, manner, risk, ambiguity, secondaryActions。",
    ]
    .join("\n")
}

fn world_mutator_system_prompt(language: &str) -> String {
    if language == "en" {
        return [
            "You are an interactive-fiction world-state drafter.",
            "Based only on the player's action and the current context, propose this turn's possible state changes as a draft.",
            "Do not write final prose; do not commit to the store on the reducer's behalf; do not let key states jump to completion out of nowhere.",
            "One player input advances one adjacent beat. If the player gives a chain of actions, apply only the literal chain up to the nearest new pressure point; do not skip through off-screen aftermath, rewards, or resolution beyond what the input directly attempts.",
            "Do not leap over the process. If the player runs toward, reaches for, uses, opens, or confronts something, account for the movement, resistance, interruption, or immediate pressure inside this same beat instead of jumping straight to an after-the-fact state.",
            "This engine is genre-neutral: romance, adventure, wuxia, mystery, slice-of-life all use the same structure. Entity types: actor/location/item/evidence/clue/claim/proof_chain/organization/rule/scene/event — use as needed.",
            "Give every new or important entity a one-line summary (who/what it is and why it matters), not just a status word — the player expands this summary in the side panel.",
            "Tangible things the player discovers or holds (a clue, a document, a weapon, a token, key evidence) MUST be their own entity (item/evidence/clue), never folded into a person's status — only then can they enter the player's holdings and be tracked. Observed phenomena, knowledge, impressions, or environmental signs are NOT holdings.",
            "Use entity.status to record state progress for any genre, with status words suited to this world's genre, advancing step by step without skipping (e.g. relationship: stranger -> curious -> attracted -> lover; injury: healthy -> bleeding -> critical; clue: found -> collected -> confirmed).",
            "The player entity id is fixed: always use id actor_player for the player character. Never rename this id; only replace its label, summary, and status with this world's player identity.",
            "Whenever a meaningful relationship forms or shifts between entities (ally / rival / kin / suspicion / debt / master-servant …), record it in edges.upsert as {\"fromId\":\"<entity>\",\"type\":\"<relation>\",\"toId\":\"<entity>\",\"value\":{\"role\":\"relation\"}} — this is the ONLY source for the relationship panel, so over-record rather than skip; add a fresh edge when a relationship changes.",
            "When the player physically holds/carries/keeps/takes a tangible thing, record an edge from actor_player to that entity and set value.role=\"holding\". If the held target is evidence/clue/claim/proof_chain rather than an item, also set value.physical=true. If the player only observes or learns something, use value.role=\"observed\" or a normal relation, never holding.",
            "The current context may include an entity roster. Reuse those exact ids in entities, edges, evidence, and stateSlots. If you only know a name, use the exact roster label; never invent a new id for the same person/thing (or the panel shows duplicates).",
            "State tracking is optional and governed by the user's world contract. If the world contract rejects stats, numeric panels, levels, RPG framing, or quantified meters, do NOT output stateSlots; express progress as natural-language entity.status / summary / evidence transitions instead.",
            "When stateSlots are appropriate, prefer natural-language values unless the user explicitly asked for quantitative tracking or the fiction contains a concrete count/clock/deadline. Do not create numbers just because the schema supports them.",
            "Early on (the first few turns), seed only the state the premise already establishes: a concrete deadline may become a timer slot if the world permits quantified tracking; the central mystery/objective -> its first clue/evidence entity; already-named key characters -> actor entities with a one-line summary. Don't leave the opening world nearly empty.",
            "Restraint: only create entities and meters the story actually makes real — never invent gratuitous stats or items just to fill the panel.",
            "Only use evidence.transitions for the evidence lifecycle when this world is genuinely an investigation/mystery; otherwise leave it empty.",
            "If the player's action is invalid or information is insufficient, set blocked=true and write blockedReason.",
            "Time is a synchronization axis, not a fixed tick. For every non-opening turn, set timeAdvance with: elapsed = the natural-language duration spent by this action; anchor = the world time/phase after the action if the world has a clock, season, phase, day/night, retreat period, deadline, or other temporal anchor; rationale = why this duration is right; synchronized = what relevant NPCs/places/pressures changed during the same elapsed time. A glance may pass seconds, a trip half a day, cultivation three years — obey the user's world contract; never invent a universal turn length.",
            "Output strict JSON matching PlayMutation: eventId, turn, actionKind, summary, timeAdvance, entities, edges, stateSlots, evidence, blocked, blockedReason, notes.",
            "The following is only a JSON-shape example. Do not reuse its labels, names, or story facts in the actual world; the reserved player id actor_player is the only example id you must keep for the player entity:",
            r#"{"eventId":"evt-1","turn":1,"actionKind":"look","summary":"The player-character finds a sample clue and a sample key.","timeAdvance":{"elapsed":"a few breaths","anchor":"still in the same rain-soaked minute","rationale":"The player only examined the immediate scene.","synchronized":["The counterpart notices the pause but does not act openly yet."]},"entities":{"upsert":[{"id":"actor_player","type":"actor","label":"player-character","summary":"Reserved player entity id; replace label, summary, and status with the current world's player identity.","status":"alert","updatedEventId":"evt-1"},{"id":"actor_counterpart","type":"actor","label":"counterpart","summary":"Placeholder for a relevant person in the current world; replace with the real roster id/label.","status":"guarded","updatedEventId":"evt-1"},{"id":"evidence_sample_clue","type":"evidence","label":"sample clue","summary":"A tangible clue discovered this turn; replace with a real object from the scene.","status":"seen","updatedEventId":"evt-1"},{"id":"item_sample_key","type":"item","label":"sample key","summary":"A tangible item collected this turn; replace with a real object from the scene.","status":"collected","updatedEventId":"evt-1"}]},"edges":{"upsert":[{"fromId":"actor_player","type":"suspicious_of","toId":"actor_counterpart","value":{"role":"relation"}},{"fromId":"actor_player","type":"holds","toId":"item_sample_key","value":{"role":"holding"}},{"fromId":"actor_player","type":"holds","toId":"evidence_sample_clue","value":{"role":"holding","physical":true}}]},"stateSlots":{"upsert":[{"id":"slot_sample_timer","kind":"timer","label":"sample timer","value":3,"updatedEventId":"evt-1"}]}}"#,
        ]
        .join("\n");
    }
    [
        "你是互动小说世界状态草案员。",
        "你只根据玩家动作和当前上下文，提出本回合可能发生的状态变化草案。",
        "不要写最终正文；不要越权替 reducer 落库；不要凭空让关键状态一步到位。",
        "一个玩家输入只推进相邻一拍。玩家把多个动作写在一句里时，只处理原话直接包含的动作链，到最近的新压力点就停；不要跳过过程去写场外后果、完整回报或问题解决。",
        "不要替玩家越过过程。玩家奔向、伸手、使用、打开或对峙某物时，必须把移动、阻力、干扰、敌人压近或即时压力算进这一拍，不能直接跳到事后状态。",
        "这套引擎是品类中立的：恋情、冒险、武侠、悬疑、日常等都用同一套结构表达。实体类型用 actor/location/item/evidence/clue/claim/proof_chain/organization/rule/scene/event，按需选用。",
        "给每个新出现或重要的实体写一句 summary（他是谁/这是什么、为什么重要），不要只靠 status 一句话——玩家会在侧栏里展开看这条 summary。",
        "玩家发现或获得的「实物」（线索、文件、凶器、信物、关键证据等）必须建成独立实体（item/evidence/clue），不要塞进某个人物的 status——这样它们才能进入玩家的「持有物」并被追踪。观察到的现象、知识、印象、环境征兆不是持有物。",
        "用 entity.status 记录任意品类的状态推进，状态词按这个世界的题材自定，循序渐进、不要跳级（例如关系：陌生→好奇→心动→恋人；伤势：健康→流血→重伤；线索：发现→收集→坐实）。",
        "玩家本人实体 id 是固定保留字：必须始终用 actor_player。绝不要把它改成本局名字或别的 id；只替换 label、summary、status 为本局玩家身份。",
        "人物之间（或人物与组织/地点之间）一旦形成或改变有意义的关系（盟友/敌对/亲属/怀疑/欠债/上下级/师徒等），就在 edges.upsert 里记一条 {\"fromId\":\"<实体>\",\"type\":\"<关系词>\",\"toId\":\"<实体>\",\"value\":{\"role\":\"relation\"}}——这是侧栏「关系网」的唯一来源，宁可多记勿漏；关系一旦变化（如怀疑→敌对）也补一条新边。",
        "玩家实际持有/携带/收进包里/拿走某个实物时，必须记一条 actor_player 指向该实体的 edge，并设置 value.role=\"holding\"。如果目标是 evidence/clue/claim/proof_chain 而不是 item，还必须设置 value.physical=true。玩家只是观察或知道某件事时，用 value.role=\"observed\" 或普通关系，绝不能写成 holding。",
        "当前上下文可能包含实体名册。entities、edges、evidence、stateSlots 都优先复用名册里的精确 id；如果只知道名字，就用名册里的精确 label。绝不要把同一个人/物换个新 id 重建（否则侧栏会出现重复节点）。",
        "状态追踪是可选的，必须服从用户的世界契约。世界契约禁止数值、面板、等级、RPG 化或量化进度时，不要输出 stateSlots；改用 entity.status / summary / evidence transitions 写自然语言状态。",
        "确实需要 stateSlots 时，也优先用自然语言 value；只有用户明确要求量化追踪，或文本里存在具体倒计时/钟点/数量，才写数字。不要因为 schema 支持就硬造数值。",
        "开局阶段（前几回合），只播种前提里已经确立的状态：明确期限在世界允许量化时才可成为 timer；核心谜题/目标物→第一条 clue/evidence 实体；已点名的关键人物→actor 实体并配一句 summary。不要让开场世界几乎空着。",
        "克制：只建剧情真正落地的实体和数值，不要为了填满侧栏而硬造属性或物品。",
        "只有当这个世界确实是调查/推理题材时，才用 evidence.transitions 走证据生命周期；其他题材留空即可。",
        "如果玩家动作无效或信息不足，blocked=true 并写 blockedReason。",
        "时间是世界同步轴，不是固定 tick。每个非开场回合都要写 timeAdvance：elapsed=本动作按语义经过了多久；anchor=动作结束后世界处在什么时间/阶段（若本局有钟点、昼夜、季节、闭关期、期限、潮汐、巡逻节奏等时间锚点）；rationale=为什么是这段时间；synchronized=同一段时间里相关人物/地点/压力发生了什么同步变化。看一眼可能几息，赶路可能半天，闭关可能三年——遵守用户的世界契约，绝不要发明统一回合长度。",
        "输出严格 JSON，必须符合 PlayMutation：eventId, turn, actionKind, summary, timeAdvance, entities, edges, stateSlots, evidence, blocked, blockedReason, notes。",
    ]
    .join("\n")
}

fn world_mutator_user_prompt(turn: i64, input: &str, action: &Value, context: &str, language: &str) -> String {
    if language == "en" {
        return [
            format!("turn: {turn}"),
            "Player's words:".to_string(),
            input.to_string(),
            String::new(),
            "Action interpretation:".to_string(),
            serde_json::to_string_pretty(action).unwrap_or_default(),
            String::new(),
            "Current context:".to_string(),
            context.to_string(),
            String::new(),
            format!("Requirement: use eventId evt-{turn}; every new or referenced entity id must be stable, readable, and short."),
        ]
        .join("\n");
    }
    [
        format!("turn: {turn}"),
        "玩家原话：".to_string(),
        input.to_string(),
        String::new(),
        "动作理解：".to_string(),
        serde_json::to_string_pretty(action).unwrap_or_default(),
        String::new(),
        "当前上下文：".to_string(),
        context.to_string(),
        String::new(),
        format!("要求：eventId 使用 evt-{turn}；所有新增或引用的实体 id 要稳定、可读、短小。"),
    ]
    .join("\n")
}

fn scene_renderer_system_prompt(mode: &str, language: &str) -> String {
    if language == "en" {
        let base = [
            "You are an interactive-fiction scene-response author.",
            "Write the response only from the already-applied state; do not overturn the reducer's results.",
            "Concrete new objects, clues, evidence, locations, organizations, or named people can only appear if they are already present in Applied changes or Current state summary. If the prose needs a new concrete thing, it must have been created by the mutator first; otherwise describe mood, pressure, or an unnamed detail instead.",
            "It should read like a playable novel — action, senses, pressure, breathing room — never a system log and never a menu-narration that herds the player into picking something.",
            "Bridge from the player's action first. Even though the state is already applied, do not start as if everything is already over; write the follow-through, contact, resistance, interruption, and immediate consequence so the action connects to the new state.",
            "Do not jump straight to the after-action result, and do not write epilogue-style summaries, morals, or closing-theme lines. End on an immediate sensory pressure, changed position, exposed detail, or nearby consequence.",
            "Stay strictly inside the world the premise established — era, place, tech level, genre tone must stay consistent. Never introduce elements that don't belong: a modern-city story must not grow night-watchmen / oil lamps; a historical/wuxia story must not sprout phones / cars / computers. Every detail lands inside the given world.",
            "The player is not always 'acting'. When they merely observe, linger, feel, idle-chat, or do nothing, give an immersive beat — one living detail, a smell, a bystander's small movement, a thought crossing their mind. NEVER say 'there's nothing more to see' / 'you already looked' / 'stop stalling', and never nag them to hurry up and act. Let the beat breathe.",
            "The world is not inert. Time moves, the deadline closes in, side characters act on their own, something stirs in the distance, off-screen events happen. Even on a turn where the player did nothing, nudge the world forward a little — so the pull to move forward comes from the STORY (the trail goes cold / the deadline nears / someone moved first), not from the narration pestering them to choose.",
            "If Current state summary includes a Time section, treat elapsed and anchor as canonical. Render the scene after exactly that elapsed interval, at that resulting world time/phase, and include the synchronized pressure/character movement naturally in prose. Do not invent another clock reading, another elapsed amount, or a fixed tick label.",
            "Respect negative player intent as fact. If the player's words say they did NOT touch, open, take, leave, attack, or speak, do not narrate them doing it by implication; write the restraint itself and the world's response to that restraint.",
            "Do not end with herding questions like 'What do you do?' / 'Which way?'. And do NOT route the same pressure through a companion who keeps listing options ('go to A, or B?') — a sidekick is not an options dispenser. Most beats should NOT end on a pending question at all: land on an image, a sound, a smell, or a hanging tension, and stop. Only when the player is genuinely at a fork that demands a decision may a question surface — sparingly.",
            "sceneText is PURE narrative prose. Never put a choice list in the body — no 'Options:' / 'What do you do?' followed by A/B/C, no '- ' bulleted options — no matter how urgent or fork-like the moment is (a tense escape is NOT an excuse for a menu). Weave the available routes into the scene itself (the bamboo by the wall, the half-open skylight, the alley toward the river) and let the player decide by free input. Any springboard goes ONLY in the suggestedActions field, kept sparse — never a menu in the prose.",
            "Example (applies even at a life-or-death beat) — [WRONG, never write this] 'The zombie lunges, the axe is stuck. React now:\\n- yank the axe and swing\\n- squeeze sideways through\\n- roll back'; [RIGHT] 'Its claws are already spread, the sour reek of rot in your nose. Your axe is wedged in the twenty-centimeter gap of the door, and it won't come free. Its weight bears down—'. Take the danger to its peak, then stop, and hand the 'what now' entirely to the player's free input — never list options for them.",
        ];
        let actions_rule = if mode == "guided" {
            "suggestedActions: give 0-3 as optional springboards ('you could…'), ONLY at a genuine decision point — not every turn. They are hints, not the only way forward; the player can type freely or just stay put at any time."
        } else {
            "suggestedActions: 0-3 short hints, optional, never restricting the player's input; omit them when there is no real decision point."
        };
        let mut lines: Vec<String> = base.iter().map(|line| line.to_string()).collect();
        lines.push(actions_rule.to_string());
        lines.push("Output strict JSON: sceneText, suggestedActions.".to_string());
        return lines.join("\n");
    }
    let mut lines: Vec<String> = [
        "你是互动小说场景应答作者。",
        "只依据已经应用的状态写回应；不要推翻 reducer 的结果。",
        "具体的新物件、线索、证据、地点、组织或具名人物只能来自「已应用变化」或「当前状态摘要」里已存在的条目。正文需要新具象物时，必须先由 mutator 建立它；否则改为描写氛围、压力或无名的细节。",
        "读起来要像可玩的小说——动作、感官、压力、呼吸感——绝不能是系统日志，也不能是菜单式叙述把玩家往选项上赶。",
        "先从玩家的动作接住。状态虽已应用，但不要开笔就写成一切都已结束；写出后续动作、接触、阻力、打断和即时后果，让动作和新状态衔接。",
        "不要直接跳到事后结果，不要写收尾式的总结、道理或点题句。收在一个即时感官压力、位置变化、暴露的细节或近旁的后果上。",
        "严格待在前提确立的世界里——时代、地点、技术水平、题材基调必须保持一致。绝不引入不属于这个世界的东西：现代都市不能冒出打更人和油灯；历史/武侠不能长出手机、汽车、电脑。每个细节都落在给定世界内。",
        "玩家并不总是在「行动」。当玩家只是观察、停留、感受、闲聊或什么都没做时，给一个沉浸的小节拍——一个活的细节、一种气味、路人的小动作、一个掠过心头的念头。绝不说「没有什么可看的了」「你已经看过了」「别磨蹭」，也不催促玩家赶紧行动。让这一拍自然呼吸。",
        "世界不是静止的。时间在走、期限在逼近、配角自行行动、远处有动静、场外事件在发生。即使玩家这回合什么都没做，也让世界向前挪一点——前进的引力来自故事（线索变冷/期限临近/有人先动了），而不是叙述催促玩家做选择。",
        "如果「当前状态摘要」里有 Time 部分，elapsed 和 anchor 是权威。场景必须严格渲染经过那段时长之后、落在那个世界时间/阶段上，并把同步变化的压力/人物动向自然写进正文。不要另造钟点、另写经过时长或固定的回合标签。",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    if mode == "guided" {
        lines.push("结尾附上 suggestedActions：2-4 个具体、有差异、贴合当前压力的下一步动作建议（每个一句话，不要编号列表说明文字）。".to_string());
    } else {
        lines.push("open 模式：不主动罗列建议动作；suggestedActions 留空数组，让玩家自由输入。".to_string());
    }
    lines.push("输出严格 JSON：{\"sceneText\": \"...\", \"suggestedActions\": [...]}；除该对象外不要输出任何文字。".to_string());
    lines.join("\n")
}

fn scene_renderer_user_prompt(
    input: &str,
    action: &Value,
    mutation_summary: &str,
    state_brief: &str,
    world_premise: &str,
    replay_context: Option<&str>,
    language: &str,
) -> String {
    // 重写约束（regenerate 重放时注入——双语标签逐字）。
    let replay_block = |lines: &mut Vec<String>, label: &str| {
        if let Some(context) = replay_context.filter(|context| !context.trim().is_empty()) {
            lines.push(String::new());
            lines.push(label.to_string());
            lines.push(context.trim().to_string());
        }
    };
    if language == "en" {
        let mut lines = Vec::new();
        if !world_premise.trim().is_empty() {
            lines.push("World setting (always obey):".to_string());
            lines.push(world_premise.trim().to_string());
            lines.push(String::new());
        }
        lines.push("Player's words:".to_string());
        lines.push(input.to_string());
        lines.push(String::new());
        lines.push("Action:".to_string());
        lines.push(serde_json::to_string_pretty(action).unwrap_or_default());
        lines.push(String::new());
        lines.push("Applied changes this turn:".to_string());
        lines.push(mutation_summary.to_string());
        lines.push(String::new());
        lines.push("Current state summary:".to_string());
        lines.push(state_brief.to_string());
        replay_block(&mut lines, "Replay constraints:");
        return lines.join("\n");
    }
    let mut lines = Vec::new();
    if !world_premise.is_empty() {
        lines.push(world_premise.to_string());
        lines.push(String::new());
    }
    lines.push(format!("玩家输入：{input}"));
    lines.push(format!("动作理解：{}", serde_json::to_string(action).unwrap_or_default()));
    lines.push(format!("已应用变化摘要：{mutation_summary}"));
    lines.push(String::new());
    lines.push("当前状态摘要：".to_string());
    lines.push(state_brief.to_string());
    replay_block(&mut lines, "重写约束：");
    lines.join("\n")
}

// ── Runner ──────────────────────────────────────────────────────

pub struct PlayRunner<'a> {
    pub project_root: &'a Path,
    pub world_id: String,
    pub run_id: String,
}

#[derive(Debug)]
pub struct PlayStepOutcome {
    pub scene_text: String,
    pub suggested_actions: Vec<String>,
    pub action: Value,
    pub mutation: Value,
}

/// `PlayReplayResult`：step 结果 + 变体对（重放前/重放后）+ 重放输入。
#[derive(Debug)]
pub struct PlayReplayOutcome {
    pub scene_text: String,
    pub suggested_actions: Vec<String>,
    pub action: Value,
    pub mutation: Value,
    pub previous_variant_id: Option<String>,
    pub variant_id: Option<String>,
    pub replayed_input: String,
}

/// `PlayVariantRestoreResult`。
#[derive(Debug)]
pub struct PlayVariantRestoreOutcome {
    pub turn: i64,
    pub variant_id: String,
    pub scene_text: String,
}

impl PlayRunner<'_> {
    /// `seedOpening`：开场播种（已有实体/槽 → None；正常播种 evt-0）。
    pub async fn seed_opening(
        &self,
        mutator: &dyn PlayWorldMutator,
        scene_text: &str,
        suggested_actions: &[String],
    ) -> Result<Option<Value>, String> {
        crate::play::ensure_run(self.project_root, &self.world_id, &self.run_id).await?;
        let run_dir = crate::play::run_dir(self.project_root, &self.world_id, &self.run_id)?;
        let mut db = open_play_graph_db(&run_dir)?;
        let existing = db.snapshot();
        let has_content = existing.get("entities").and_then(Value::as_array).is_some_and(|a| !a.is_empty())
            || existing
                .get("stateSlots")
                .and_then(Value::as_array)
                .is_some_and(|a| !a.is_empty());
        if has_content {
            return Ok(None);
        }
        let world = crate::play::load_world(self.project_root, &self.world_id).await;
        let language = world
            .as_ref()
            .and_then(|w| w.get("language"))
            .and_then(Value::as_str)
            .unwrap_or("zh")
            .to_string();
        let action = json!({
            "actionKind": "look",
            "intent": if language == "en" {
                "Seed the opening state for the first playable scene."
            } else {
                "播种第一幕已成立的开场状态。"
            },
            "manner": "",
            "risk": "",
            "ambiguity": "",
            "secondaryActions": [],
        });
        let world_context = render_world_context(world.as_ref(), &language);
        let context = self.build_context_brief(scene_text, &world, &language).await;
        let opening_input = build_opening_seed_input(scene_text, suggested_actions, &world_context, &language);
        let mut mutation = mutator.propose_mutation(0, &opening_input, &action, &context, &language).await;
        if let Some(obj) = mutation.as_object_mut() {
            obj.insert("eventId".into(), json!("evt-0"));
            obj.insert("turn".into(), json!(0));
            obj.insert("actionKind".into(), json!("look"));
        }
        seed_play_graph(&mut db, &mutation)?;
        db.flush()?;
        crate::play::write_projection(
            self.project_root,
            &self.world_id,
            &self.run_id,
            "projections/state.md",
            &render_state_brief(&action, &mutation),
        )
        .await?;
        Ok(Some(mutation))
    }

    /// `step`：一回合全链（render 先行 → reconcile 对账 → checkpoint → 提交）。
    #[allow(clippy::too_many_arguments)]
    pub async fn step(
        &self,
        interpreter: &dyn PlayActionInterpreter,
        mutator: &dyn PlayWorldMutator,
        renderer: &dyn PlaySceneRenderer,
        reconciler: Option<&dyn PlaySceneReconciler>,
        input: &str,
        replay_context: Option<&str>,
    ) -> Result<PlayStepOutcome, String> {
        let raw_input = input.trim();
        if raw_input.is_empty() {
            return Err("Play input is empty.".to_string());
        }
        crate::play::ensure_run(self.project_root, &self.world_id, &self.run_id).await?;
        let turn = crate::play::read_events(self.project_root, &self.world_id, &self.run_id)
            .await
            .len() as i64
            + 1;
        let world = crate::play::load_world(self.project_root, &self.world_id).await;
        let language = world
            .as_ref()
            .and_then(|w| w.get("language"))
            .and_then(Value::as_str)
            .unwrap_or("zh")
            .to_string();
        let mode = world
            .as_ref()
            .and_then(|w| w.get("mode"))
            .and_then(Value::as_str)
            .unwrap_or("open")
            .to_string();
        let scene_brief = crate::play::read_projection(
            self.project_root,
            &self.world_id,
            &self.run_id,
            "projections/scene.md",
        )
        .await
        .unwrap_or_default();
        let scene_brief_or_default = if scene_brief.trim().is_empty() {
            if language == "en" {
                "A new turn begins; carry over the current world state.".to_string()
            } else {
                "新回合开始，沿用当前世界状态。".to_string()
            }
        } else {
            scene_brief.clone()
        };
        let action = interpreter.interpret(raw_input, &scene_brief_or_default, &language).await;
        let world_context = render_world_context(world.as_ref(), &language);
        let context = self.build_context_brief(&scene_brief, &world, &language).await;
        let mutation = mutator
            .propose_mutation(turn, raw_input, &action, &context, &language)
            .await;
        let state_brief = render_state_brief(&action, &mutation);

        // render 先行：场景到手前不落任何盘（回合 all-or-nothing）。
        let mutation_summary = mutation
            .get("summary")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .or_else(|| mutation.get("blockedReason").and_then(Value::as_str))
            .unwrap_or_default()
            .to_string();
        let render = renderer
            .render(raw_input, &action, &mutation_summary, &state_brief, &mode, &world_context, replay_context, &language)
            .await;

        // reconcile：非 blocked 回合做正文↔图谱对账；空补充不动原 mutation。
        let blocked = mutation.get("blocked").and_then(Value::as_bool).unwrap_or(false);
        let mut final_mutation = mutation.clone();
        let mut final_state_brief = state_brief.clone();
        if let Some(reconciler) = reconciler.filter(|_| !blocked) {
            let supplement = reconciler
                .reconcile(
                    turn,
                    raw_input,
                    &action,
                    &mutation,
                    &render.scene_text,
                    &context,
                    &state_brief,
                    &world_context,
                    &language,
                )
                .await;
            if !is_empty_mutation_supplement(&supplement) {
                final_mutation = merge_play_mutations(&mutation, &supplement);
                final_state_brief = render_state_brief(&action, &final_mutation);
            }
        }

        // checkpoint 先于 apply：before-turn-{N} 完整可恢复态（regenerate 的回滚点）。
        let run_dir = crate::play::run_dir(self.project_root, &self.world_id, &self.run_id)?;
        {
            let db = open_play_graph_db(&run_dir)?;
            let before_graph = db.snapshot();
            let checkpoint = crate::play::capture_run_snapshot(
                self.project_root,
                &self.world_id,
                &self.run_id,
                &format!("before-turn-{turn}"),
                turn,
                before_graph,
            )
            .await?;
            crate::play::save_checkpoint(self.project_root, &self.world_id, &self.run_id, &checkpoint).await?;
        }

        // 提交：事件 + 图 + 投影 + 状态 + transcript。
        let mut db = open_play_graph_db(&run_dir)?;
        let applied = apply_play_mutation(&mut db, &final_mutation, raw_input)?;
        db.flush()?;
        crate::play::append_event(self.project_root, &self.world_id, &self.run_id, &applied.event).await?;
        crate::play::write_projection(
            self.project_root,
            &self.world_id,
            &self.run_id,
            "projections/state.md",
            &final_state_brief,
        )
        .await?;
        crate::play::save_current_state(
            self.project_root,
            &self.world_id,
            &self.run_id,
            &json!({
                "turn": turn,
                "lastEventId": applied.event.get("id"),
                "lastAction": action,
                "lastSummary": final_mutation.get("summary"),
                "timeAdvance": final_mutation.get("timeAdvance").filter(|v| !v.is_null()),
                "blocked": applied.blocked,
                "worldContract": world.as_ref().and_then(|w| w.get("worldContract")).cloned().unwrap_or_default(),
                "visualContract": world.as_ref().and_then(|w| w.get("visualContract")).cloned().unwrap_or_default(),
            }),
        )
        .await?;
        crate::play::write_projection(
            self.project_root,
            &self.world_id,
            &self.run_id,
            "projections/scene.md",
            &format!("{}\n", render.scene_text),
        )
        .await?;
        crate::play::append_transcript_turn(self.project_root, &self.world_id, &self.run_id, "user", raw_input).await?;
        crate::play::append_transcript_turn(
            self.project_root,
            &self.world_id,
            &self.run_id,
            "assistant",
            &render.scene_text,
        )
        .await?;

        Ok(PlayStepOutcome {
            scene_text: render.scene_text,
            suggested_actions: render.suggested_actions,
            action,
            mutation: final_mutation,
        })
    }

    /// `regenerateLastTurn`：当前态存变体 → 回滚 before-turn 检查点 → 重放 →
    /// 新态再存变体（重写非新回合；replayContext 约束时间不倒退）。
    pub async fn regenerate_last_turn(
        &self,
        interpreter: &dyn PlayActionInterpreter,
        mutator: &dyn PlayWorldMutator,
        renderer: &dyn PlaySceneRenderer,
        reconciler: Option<&dyn PlaySceneReconciler>,
        input: Option<&str>,
    ) -> Result<PlayReplayOutcome, String> {
        let events = crate::play::read_events(self.project_root, &self.world_id, &self.run_id).await;
        let Some(last) = events.last() else {
            return Err("No Play turn to regenerate.".to_string());
        };
        let last_turn = last.get("turn").and_then(Value::as_i64).unwrap_or(0);
        let last_raw_input = last
            .get("rawInput")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let replayed_input = input
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(last_raw_input.trim())
            .to_string();

        // 当前态存变体（回滚前的现场）。
        let run_dir = crate::play::run_dir(self.project_root, &self.world_id, &self.run_id)?;
        let current_graph = {
            let db = open_play_graph_db(&run_dir)?;
            db.snapshot()
        };
        let current_snapshot = crate::play::capture_run_snapshot(
            self.project_root,
            &self.world_id,
            &self.run_id,
            &format!("current-turn-{last_turn}"),
            last_turn,
            current_graph,
        )
        .await?;
        let previous_variant_id = crate::play::save_variant(
            self.project_root,
            &self.world_id,
            &self.run_id,
            last_turn,
            &current_snapshot,
        )
        .await?;

        // 回滚 before-turn 检查点（缺失 → 不可安全重做）。
        let checkpoint = crate::play::load_checkpoint(
            self.project_root,
            &self.world_id,
            &self.run_id,
            &format!("before-turn-{last_turn}"),
        )
        .await
        .ok_or_else(|| {
            format!("Missing checkpoint before turn {last_turn}; cannot regenerate safely.")
        })?;
        crate::play::restore_run_snapshot(self.project_root, &self.world_id, &self.run_id, &checkpoint).await?;

        let language = crate::play::load_world(self.project_root, &self.world_id)
            .await
            .and_then(|w| w.get("language").and_then(Value::as_str).map(String::from))
            .unwrap_or_else(|| "zh".to_string());
        let replay_context = build_replay_context(&last_raw_input, input, &language);
        let result = self
            .step(
                interpreter,
                mutator,
                renderer,
                reconciler,
                &replayed_input,
                Some(&replay_context),
            )
            .await?;

        // 新态存变体。
        let next_graph = {
            let db = open_play_graph_db(&run_dir)?;
            db.snapshot()
        };
        let next_snapshot = crate::play::capture_run_snapshot(
            self.project_root,
            &self.world_id,
            &self.run_id,
            &format!("regenerated-turn-{last_turn}"),
            last_turn,
            next_graph,
        )
        .await?;
        let variant_id = crate::play::save_variant(
            self.project_root,
            &self.world_id,
            &self.run_id,
            last_turn,
            &next_snapshot,
        )
        .await?;

        Ok(PlayReplayOutcome {
            scene_text: result.scene_text,
            suggested_actions: result.suggested_actions,
            action: result.action,
            mutation: result.mutation,
            previous_variant_id: Some(previous_variant_id),
            variant_id: Some(variant_id),
            replayed_input,
        })
    }

    /// `restoreVariant`：按变体 id 整体恢复（图 + 五路原文）。
    pub async fn restore_variant(
        &self,
        turn: i64,
        variant_id: &str,
    ) -> Result<PlayVariantRestoreOutcome, String> {
        let Some(snapshot) =
            crate::play::load_variant(self.project_root, &self.world_id, &self.run_id, turn, variant_id).await
        else {
            return Err(format!("Play variant not found: turn {turn} / {variant_id}"));
        };
        crate::play::restore_run_snapshot(self.project_root, &self.world_id, &self.run_id, &snapshot).await?;
        Ok(PlayVariantRestoreOutcome {
            turn,
            variant_id: variant_id.to_string(),
            scene_text: snapshot.scene_projection.trim().to_string(),
        })
    }

    async fn build_context_brief(&self, scene_brief: &str, world: &Option<Value>, language: &str) -> String {
        let is_en = language == "en";
        let state_brief = crate::play::read_projection(
            self.project_root,
            &self.world_id,
            &self.run_id,
            "projections/state.md",
        )
        .await
        .unwrap_or_default();
        let world_context = render_world_context(world.as_ref(), language);
        let run_dir = crate::play::run_dir(self.project_root, &self.world_id, &self.run_id);
        let roster = run_dir
            .ok()
            .and_then(|dir| crate::play::play_graph_snapshot(&dir).get("entities").cloned());
        let entity_roster = render_entity_roster(
            roster
                .as_ref()
                .and_then(Value::as_array)
                .map(|a| a.as_slice())
                .unwrap_or_default(),
            language,
        );
        let scene_label = if is_en { "Current scene:" } else { "当前场景：" };
        let state_label = if is_en { "Current state:" } else { "当前状态：" };
        let mut blocks: Vec<String> = Vec::new();
        if !world_context.is_empty() {
            blocks.push(world_context);
        }
        if !entity_roster.is_empty() {
            blocks.push(entity_roster);
        }
        if !scene_brief.trim().is_empty() {
            blocks.push(format!("{scene_label}\n{scene_brief}"));
        }
        if !state_brief.trim().is_empty() {
            blocks.push(format!("{state_label}\n{state_brief}"));
        }
        if blocks.is_empty() {
            if is_en {
                "No persisted state yet.".to_string()
            } else {
                "暂无持久化状态。".to_string()
            }
        } else {
            blocks.join("\n\n")
        }
    }
}

// ── merge 辅助（play-runner.ts L486-548 逐字） ──────────────────

/// `mergeById`：按 id 去重（后写胜），保持首现顺序。
fn merge_by_id(items: Vec<Value>) -> Vec<Value> {
    let mut order: Vec<String> = Vec::new();
    let mut by_id: std::collections::HashMap<String, Value> = std::collections::HashMap::new();
    for item in items {
        let id = item
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if !by_id.contains_key(&id) {
            order.push(id.clone());
        }
        by_id.insert(id, item);
    }
    order
        .into_iter()
        .map(|id| by_id.get(&id).cloned().unwrap_or(Value::Null))
        .collect()
}

/// `normalizeSummaryForDedupe`：剥标点/空白（TS 字符类逐字）+ lowercase。
fn normalize_summary_for_dedupe(value: &str) -> String {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::Regex::new(
            r#"[\s，,。.!！?？；;：:、"'“”‘’「」『』（）()\[\]{}《》<>—\-…]+"#,
        )
        .unwrap()
    });
    re.replace_all(value, "").to_lowercase()
}

/// `mergeMutationSummary`：空侧直返；归一后相等/互含取宽侧；否则 `左；右` 拼接。
pub fn merge_mutation_summary(base: &str, supplement: &str) -> String {
    let left = base.trim();
    let right = supplement.trim();
    if right.is_empty() {
        return left.to_string();
    }
    if left.is_empty() {
        return right.to_string();
    }
    let normalized_left = normalize_summary_for_dedupe(left);
    let normalized_right = normalize_summary_for_dedupe(right);
    if normalized_left == normalized_right || normalized_left.contains(&normalized_right) {
        left.to_string()
    } else if normalized_right.contains(&normalized_left) {
        right.to_string()
    } else {
        format!("{left}；{right}")
    }
}

/// `isEmptyMutationSupplement`：全空数组 + 空 summary + 未 blocked。
pub fn is_empty_mutation_supplement(mutation: &Value) -> bool {
    let array_empty = |path: &str| {
        mutation
            .pointer(path)
            .and_then(Value::as_array)
            .map(|a| a.is_empty())
            .unwrap_or(true)
    };
    array_empty("/entities/upsert")
        && array_empty("/edges/upsert")
        && array_empty("/edges/expire")
        && array_empty("/stateSlots/upsert")
        && array_empty("/evidence/transitions")
        && array_empty("/notes")
        && mutation
            .get("summary")
            .and_then(Value::as_str)
            .map(|s| s.trim().is_empty())
            .unwrap_or(true)
        && !mutation.get("blocked").and_then(Value::as_bool).unwrap_or(false)
}

/// `mergePlayMutations`：base 骨架 + supplement 合成（upsert 按 id 去重后写胜；
/// expire/transitions/notes 拼接；timeAdvance base 优先）→ normalize。
pub fn merge_play_mutations(base: &Value, supplement: &Value) -> Value {
    if is_empty_mutation_supplement(supplement) {
        return base.clone();
    }
    let concat = |path: &str| -> Vec<Value> {
        let mut items = base
            .pointer(path)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        items.extend(
            supplement
                .pointer(path)
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
        );
        items
    };
    let summary = merge_mutation_summary(
        base.get("summary").and_then(Value::as_str).unwrap_or_default(),
        supplement.get("summary").and_then(Value::as_str).unwrap_or_default(),
    );
    let mut merged = base.clone();
    if let Some(obj) = merged.as_object_mut() {
        obj.insert("summary".into(), json!(summary));
        obj.insert(
            "entities".into(),
            json!({ "upsert": merge_by_id(concat("/entities/upsert")) }),
        );
        obj.insert(
            "edges".into(),
            json!({
                "upsert": merge_by_id(concat("/edges/upsert")),
                "expire": concat("/edges/expire"),
            }),
        );
        obj.insert(
            "stateSlots".into(),
            json!({ "upsert": merge_by_id(concat("/stateSlots/upsert")) }),
        );
        obj.insert(
            "evidence".into(),
            json!({ "transitions": concat("/evidence/transitions") }),
        );
        let time_advance = base
            .get("timeAdvance")
            .filter(|v| !v.is_null())
            .cloned()
            .or_else(|| supplement.get("timeAdvance").filter(|v| !v.is_null()).cloned());
        obj.insert("timeAdvance".into(), time_advance.unwrap_or(Value::Null));
        obj.insert("notes".into(), json!(concat("/notes")));
    }
    crate::play_parser::normalize_play_mutation(&merged)
}

/// `buildReplayContext`（双语逐字）：重写非新回合 + 时间权威 + 禁新增事实。
pub fn build_replay_context(original_input: &str, replacement_input: Option<&str>, language: &str) -> String {
    let replacement = replacement_input.map(str::trim).filter(|s| !s.is_empty());
    let differs = replacement.is_some_and(|value| value != original_input);
    if language == "en" {
        [
            "This is a regeneration of the previous turn, not a new next turn.".to_string(),
            format!("Original player input: {original_input}"),
            if differs {
                format!("Replacement instruction from user: {}", replacement.unwrap_or_default())
            } else {
                String::new()
            },
            "Keep it as the same player action unless the replacement explicitly changes that action.".to_string(),
            "The Current state summary is authoritative, especially the Time section. Do not move the clock backward, invent a different elapsed time, or write another timestamp.".to_string(),
            "Do not add new player actions the user did not take. Vary prose, sensory detail, pressure, and emphasis while staying inside the same applied state.".to_string(),
            "Concrete new facts, people, objects, locations, or clues must already be present in Applied changes or Current state summary.".to_string(),
        ]
        .into_iter()
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
    } else {
        [
            "这是在重写上一回合，不是推进新的下一回合。".to_string(),
            format!("原玩家动作：{original_input}"),
            if differs {
                format!("用户替换说明：{}", replacement.unwrap_or_default())
            } else {
                String::new()
            },
            "除非替换说明明确改变动作，否则保持同一个玩家动作。".to_string(),
            "当前状态摘要是权威，尤其是 Time/时间段：不得倒退时间，不得另写经过时长，也不得写另一个钟点。".to_string(),
            "不要加入玩家没有做的新动作。可以换表达、感官细节、压迫和侧重点，但必须留在同一份已应用状态里。".to_string(),
            "具体新事实、人物、物件、地点或线索必须已经出现在已应用变化或当前状态摘要中。".to_string(),
        ]
        .into_iter()
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
    }
}

fn render_world_context(world: Option<&Value>, language: &str) -> String {
    let Some(world) = world else {
        return String::new();
    };
    let is_en = language == "en";
    let field = |name: &str| {
        world
            .get(name)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    let mut blocks = Vec::new();
    if let Some(premise) = field("premise") {
        let label = if is_en { "World setting" } else { "世界设定" };
        blocks.push(format!("{label}:\n{premise}"));
    }
    if let Some(contract) = field("worldContract") {
        let label = if is_en {
            "World contract (high priority; obey before genre defaults)"
        } else {
            "世界契约（高优先级，先于题材惯例）"
        };
        blocks.push(format!("{label}:\n{contract}"));
    }
    if let Some(visual) = field("visualContract") {
        let label = if is_en {
            "Visual contract (for scene and image consistency)"
        } else {
            "视觉契约（保持场景和配图一致）"
        };
        blocks.push(format!("{label}:\n{visual}"));
    }
    blocks.join("\n\n")
}

fn build_opening_seed_input(
    scene_text: &str,
    suggested_actions: &[String],
    premise: &str,
    language: &str,
) -> String {
    if language == "en" {
        let mut blocks = vec![
            "Seed only the state that already exists at the opening of this playable world.".to_string(),
            "Do not advance time, do not solve the mystery, and do not narrate a new turn.".to_string(),
            "If the premise or opening scene says the player already holds, carries, keeps, wears, or starts with a tangible object, that object is already established: create its entity and add an actor_player holding edge. Do not hide held objects inside the player summary.".to_string(),
        ];
        if !premise.is_empty() {
            blocks.push(format!("Premise:\n{premise}"));
        }
        blocks.push(format!("Opening scene:\n{scene_text}"));
        if !suggested_actions.is_empty() {
            let lines: Vec<String> = suggested_actions
                .iter()
                .map(|action| format!("- {action}"))
                .collect();
            blocks.push(format!("Suggested player actions:\n{}", lines.join("\n")));
        }
        return blocks.join("\n\n");
    }
    let mut blocks = vec![
        "只播种这个互动世界开场已经成立的状态。".to_string(),
        "不要推进时间，不要解谜，不要写新的回合剧情。".to_string(),
        "如果世界前提或开场正文说玩家已经拿着、带着、揣着、穿着、携带或开局拥有某个实物，这就是已成立状态：必须为该实物建立实体，并补一条 actor_player 指向它、value.role=\"holding\" 的持有边。不要把已持有实物只藏在玩家 summary 里。".to_string(),
    ];
    if !premise.is_empty() {
        blocks.push(format!("世界前提：\n{premise}"));
    }
    blocks.push(format!("开场正文：\n{scene_text}"));
    if !suggested_actions.is_empty() {
        let lines: Vec<String> = suggested_actions
            .iter()
            .map(|action| format!("- {action}"))
            .collect();
        blocks.push(format!("建议动作：\n{}", lines.join("\n")));
    }
    blocks.join("\n\n")
}

fn render_entity_roster(entities: &[Value], language: &str) -> String {
    if entities.is_empty() {
        return String::new();
    }
    let is_en = language == "en";
    let header = if is_en {
        "Current entity roster (reuse these ids; do not recreate the same person/thing):"
    } else {
        "当前实体名册（复用这些 id；不要把同一个人/物换新 id 重建）："
    };
    let mut lines = vec![header.to_string()];
    for entity in entities.iter().take(40) {
        let id = entity.get("id").and_then(Value::as_str).unwrap_or_default();
        let entity_type = entity.get("type").and_then(Value::as_str).unwrap_or_default();
        let label = entity.get("label").and_then(Value::as_str).unwrap_or_default();
        let summary = entity.get("summary").and_then(Value::as_str).unwrap_or_default();
        let status = entity.get("status").and_then(Value::as_str).unwrap_or_default();
        let mut detail = summary.to_string();
        if !status.is_empty() {
            if !detail.is_empty() {
                detail.push_str(if is_en { "; " } else { "；" });
            }
            detail.push_str(&format!("{}: {status}", if is_en { "status" } else { "状态" }));
        }
        let compact: String = detail.split_whitespace().collect::<Vec<_>>().join(" ");
        let clamped = if compact.chars().count() > 120 {
            format!("{}...", compact.chars().take(117).collect::<String>())
        } else {
            compact
        };
        if clamped.is_empty() {
            lines.push(format!("- {id} [{entity_type}]: {label}"));
        } else {
            lines.push(format!("- {id} [{entity_type}]: {label} — {clamped}"));
        }
    }
    lines.join("\n")
}

pub fn render_state_brief(action: &Value, mutation: &Value) -> String {
    let mut lines = vec![
        "# Play State".to_string(),
        String::new(),
        format!(
            "- action: {} {}",
            action.get("actionKind").and_then(Value::as_str).unwrap_or_default(),
            action.get("intent").and_then(Value::as_str).unwrap_or_default()
        )
        .trim()
        .to_string(),
        format!(
            "- summary: {}",
            mutation
                .get("summary")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .or_else(|| mutation.get("blockedReason").and_then(Value::as_str))
                .unwrap_or_default()
        ),
    ];
    let entities = mutation.pointer("/entities/upsert").and_then(Value::as_array);
    if let Some(entities) = entities.filter(|a| !a.is_empty()) {
        lines.push(String::new());
        lines.push("## Entities".to_string());
        for entity in entities {
            lines.push(format!(
                "- {} [{}]: {}{}",
                entity.get("id").and_then(Value::as_str).unwrap_or_default(),
                entity.get("type").and_then(Value::as_str).unwrap_or_default(),
                entity.get("label").and_then(Value::as_str).unwrap_or_default(),
                entity
                    .get("summary")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .map(|s| format!(" — {s}"))
                    .unwrap_or_default()
            ));
        }
    }
    let edges = mutation.pointer("/edges/upsert").and_then(Value::as_array);
    if let Some(edges) = edges.filter(|a| !a.is_empty()) {
        lines.push(String::new());
        lines.push("## Edges".to_string());
        for edge in edges {
            let role = edge
                .pointer("/value/role")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|r| !r.is_empty())
                .map(|r| format!(" role={r}"))
                .unwrap_or_default();
            lines.push(format!(
                "- {} -[{}{}]-> {}",
                edge.get("fromId").and_then(Value::as_str).unwrap_or_default(),
                edge.get("type").and_then(Value::as_str).unwrap_or_default(),
                role,
                edge.get("toId").and_then(Value::as_str).unwrap_or_default()
            ));
        }
    }
    let slots = mutation.pointer("/stateSlots/upsert").and_then(Value::as_array);
    if let Some(slots) = slots.filter(|a| !a.is_empty()) {
        lines.push(String::new());
        lines.push("## State Slots".to_string());
        for slot in slots {
            lines.push(format!(
                "- {}: {}",
                slot.get("id").and_then(Value::as_str).unwrap_or_default(),
                serde_json::to_string(slot.get("value").unwrap_or(&Value::Null)).unwrap_or_default()
            ));
        }
    }
    if let Some(time_advance) = mutation.get("timeAdvance").filter(|v| v.is_object()) {
        lines.push(String::new());
        lines.push("## Time".to_string());
        for (key, label) in [("elapsed", "elapsed"), ("anchor", "anchor"), ("rationale", "rationale")] {
            if let Some(value) = time_advance.get(key).and_then(Value::as_str).filter(|s| !s.is_empty()) {
                lines.push(format!("- {label}: {value}"));
            }
        }
        if let Some(items) = time_advance.get("synchronized").and_then(Value::as_array) {
            if !items.is_empty() {
                lines.push("- synchronized:".to_string());
                for item in items {
                    if let Some(text) = item.as_str() {
                        lines.push(format!("  - {text}"));
                    }
                }
            }
        }
    }
    let transitions = mutation.pointer("/evidence/transitions").and_then(Value::as_array);
    if let Some(transitions) = transitions.filter(|a| !a.is_empty()) {
        lines.push(String::new());
        lines.push("## Evidence".to_string());
        for transition in transitions {
            lines.push(format!(
                "- {}: {}{}",
                transition.get("entityId").and_then(Value::as_str).unwrap_or_default(),
                transition.get("to").and_then(Value::as_str).unwrap_or_default(),
                transition
                    .get("reason")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .map(|s| format!(" — {s}"))
                    .unwrap_or_default()
            ));
        }
    }
    format!("{}\n", lines.join("\n"))
}

// ── 测试（79 号） ────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn mutation_base(summary: &str, entity_id: &str) -> Value {
        json!({
            "eventId": "evt-1", "turn": 1, "actionKind": "do",
            "summary": summary,
            "entities": { "upsert": [ { "id": entity_id, "type": "item", "label": "物件" } ] },
            "edges": { "upsert": [], "expire": [] },
            "stateSlots": { "upsert": [] },
            "evidence": { "transitions": [] },
            "blocked": false, "blockedReason": "", "notes": [],
        })
    }

    #[test]
    fn merge_mutation_summary_branches() {
        // 空侧直返。
        assert_eq!(merge_mutation_summary("左", ""), "左");
        assert_eq!(merge_mutation_summary("", "右"), "右");
        // 归一后相等（标点/空白差异）取左。
        assert_eq!(merge_mutation_summary("拿到凶器。", "拿到凶器"), "拿到凶器。");
        // 互含取宽侧。
        assert_eq!(merge_mutation_summary("拿到凶器", "拿到凶器，并离开房间"), "拿到凶器，并离开房间");
        assert_eq!(merge_mutation_summary("拿到凶器，并离开房间", "拿到凶器"), "拿到凶器，并离开房间");
        // 无关 → 全角分号拼接。
        assert_eq!(merge_mutation_summary("拿到凶器", "离开房间"), "拿到凶器；离开房间");
    }

    #[test]
    fn merge_play_mutations_dedupe_and_concat() {
        let base = mutation_base("拿到凶器", "item-a");
        let supplement = json!({
            "eventId": "evt-1", "turn": 1, "actionKind": "do",
            "summary": "拿到凶器",
            "entities": { "upsert": [ { "id": "item-a", "type": "item", "label": "凶器（补）" }, { "id": "item-b", "type": "clue", "label": "血迹" } ] },
            "edges": { "upsert": [ { "id": "edge-h", "fromId": "actor_player", "type": "持有", "toId": "item-b", "value": { "role": "holding" } } ], "expire": [ "edge-old" ] },
            "stateSlots": { "upsert": [] },
            "evidence": { "transitions": [] },
            "notes": ["补充说明"], "blocked": false, "blockedReason": "",
            "timeAdvance": { "elapsed": "十分钟", "anchor": "深夜", "rationale": "", "synchronized": [] },
        });
        let merged = merge_play_mutations(&base, &supplement);
        // 同 id 后写胜 + 新 id 追加。
        let entities = merged.pointer("/entities/upsert").and_then(Value::as_array).unwrap();
        assert_eq!(entities.len(), 2, "{merged}");
        assert_eq!(entities[0]["id"], "item-a");
        assert_eq!(entities[0]["label"], "凶器（补）");
        assert_eq!(entities[1]["id"], "item-b");
        // edges upsert 合入 + expire 拼接。
        assert_eq!(merged.pointer("/edges/upsert").and_then(Value::as_array).unwrap().len(), 1);
        assert_eq!(merged.pointer("/edges/expire").and_then(Value::as_array).unwrap(), &vec![json!("edge-old")]);
        // notes 拼接 + timeAdvance base 优先（base 无 → 取 supplement）。
        assert_eq!(merged.pointer("/notes").and_then(Value::as_array).unwrap().len(), 1);
        assert_eq!(merged.pointer("/timeAdvance/elapsed"), Some(&json!("十分钟")));
        // summary 归一相等取左。
        assert_eq!(merged.get("summary"), Some(&json!("拿到凶器")));

        // base 有 timeAdvance 时优先于 supplement。
        let mut base_with_time = mutation_base("左", "item-a");
        if let Some(obj) = base_with_time.as_object_mut() {
            obj.insert("timeAdvance".into(), json!({ "elapsed": "五分钟", "anchor": "黄昏", "rationale": "", "synchronized": [] }));
        }
        let merged2 = merge_play_mutations(&base_with_time, &supplement);
        assert_eq!(merged2.pointer("/timeAdvance/elapsed"), Some(&json!("五分钟")));

        // 空补充 → 原样返回 base。
        let empty = empty_reconciliation(1, "do");
        let unchanged = merge_play_mutations(&base, &empty);
        assert_eq!(unchanged, base);
    }

    #[test]
    fn empty_supplement_gate() {
        assert!(is_empty_mutation_supplement(&empty_reconciliation(3, "look")));
        let mut with_summary = empty_reconciliation(3, "look");
        if let Some(obj) = with_summary.as_object_mut() {
            obj.insert("summary".into(), json!("有补充"));
        }
        assert!(!is_empty_mutation_supplement(&with_summary));
        let mut blocked = empty_reconciliation(3, "look");
        if let Some(obj) = blocked.as_object_mut() {
            obj.insert("blocked".into(), json!(true));
        }
        assert!(!is_empty_mutation_supplement(&blocked));
    }

    #[test]
    fn en_agent_prompts_and_labels() {
        // interpreter：en system 逐字 + user 标签。
        let interpreter_system = action_interpreter_system_prompt("en");
        assert!(
            interpreter_system.starts_with("You are an interactive-fiction action interpreter."),
            "{interpreter_system}"
        );
        assert!(interpreter_system.contains("Output strict JSON, no explanation."));
        let interpreter_user =
            action_interpreter_user_prompt("I look around", "A rainy hall.", "en");
        assert!(interpreter_user.contains("Current scene:\nA rainy hall."), "{interpreter_user}");
        assert!(interpreter_user.contains("Player input:\nI look around"), "{interpreter_user}");
        assert!(interpreter_user.contains("Output fields: actionKind,"), "{interpreter_user}");

        // mutator：en system 关键行 + JSON 范例（保留 actor_player）。
        let mutator_system = world_mutator_system_prompt("en");
        assert!(mutator_system.starts_with("You are an interactive-fiction world-state drafter."));
        assert!(mutator_system.contains("Time is a synchronization axis, not a fixed tick."));
        assert!(mutator_system.contains("sample clue"), "范例 JSON 应在内");
        assert!(mutator_system.contains("reserved player id actor_player") || mutator_system.contains("actor_player"));
        let mutator_user = world_mutator_user_prompt(
            2,
            "I search the desk",
            &json!({ "actionKind": "do", "intent": "search" }),
            "context here",
            "en",
        );
        assert!(mutator_user.contains("Player's words:\nI search the desk"), "{mutator_user}");
        assert!(mutator_user.contains("Action interpretation:"), "{mutator_user}");
        assert!(mutator_user.contains("Current context:\ncontext here"), "{mutator_user}");
        assert!(mutator_user.contains("Requirement: use eventId evt-2;"), "{mutator_user}");

        // renderer：en system 关键行（含生死例句）+ user 标签 + Replay constraints。
        let renderer_system = scene_renderer_system_prompt("open", "en");
        assert!(renderer_system.starts_with("You are an interactive-fiction scene-response author."));
        assert!(renderer_system.contains("The zombie lunges"), "生死例句应在内");
        assert!(renderer_system.contains("Output strict JSON: sceneText, suggestedActions."));
        let guided = scene_renderer_system_prompt("guided", "en");
        assert!(guided.contains("ONLY at a genuine decision point"), "{guided}");
        let renderer_user = scene_renderer_user_prompt(
            "I open the door",
            &json!({ "actionKind": "do" }),
            "A key is found.",
            "# Play State",
            "A rainy night house.",
            Some("This is a regeneration of the previous turn."),
            "en",
        );
        assert!(renderer_user.contains("World setting (always obey):\nA rainy night house."), "{renderer_user}");
        assert!(renderer_user.contains("Player's words:\nI open the door"), "{renderer_user}");
        assert!(renderer_user.contains("Applied changes this turn:\nA key is found."), "{renderer_user}");
        assert!(renderer_user.contains("Current state summary:"), "{renderer_user}");
        assert!(
            renderer_user.contains("Replay constraints:\nThis is a regeneration"),
            "{renderer_user}"
        );

        // zh 分支维持 73 号形态（不受 en 分支影响）。
        assert!(action_interpreter_system_prompt("zh").contains("动作理解器"));
        assert!(scene_renderer_user_prompt("输入", &json!({}), "摘要", "状态", "前提", Some("约束"), "zh").contains("重写约束："));
    }

    #[test]
    fn en_context_labels_and_seed_input() {
        // 世界上下文：en 标签。
        let world = json!({
            "premise": "A manor on a snowy night.",
            "worldContract": "Time flows by action.",
            "visualContract": "Cold lantern light.",
        });
        let en_context = render_world_context(Some(&world), "en");
        assert!(en_context.contains("World setting:\nA manor on a snowy night."), "{en_context}");
        assert!(en_context.contains("World contract (high priority; obey before genre defaults):"), "{en_context}");
        assert!(en_context.contains("Visual contract (for scene and image consistency):"), "{en_context}");

        // 实体名册：en 头部 + status 标签 + "; " 分隔。
        let roster = render_entity_roster(
            &[json!({ "id": "actor_player", "type": "actor", "label": "Tenant", "summary": "New tenant", "status": "watchful" })],
            "en",
        );
        assert!(roster.contains("Current entity roster (reuse these ids;"), "{roster}");
        assert!(roster.contains("status: watchful"), "{roster}");
        assert!(roster.contains("New tenant; status: watchful"), "{roster}");

        // 开场播种：en 分支逐字。
        let seed_input = build_opening_seed_input(
            "The hall is dim.",
            &["Check the ledger".to_string()],
            "World setting:\nA manor.",
            "en",
        );
        assert!(seed_input.starts_with("Seed only the state that already exists"), "{seed_input}");
        assert!(seed_input.contains("Opening scene:\nThe hall is dim."), "{seed_input}");
        assert!(seed_input.contains("Suggested player actions:\n- Check the ledger"), "{seed_input}");
        // zh 分支不变。
        let zh_seed = build_opening_seed_input("厅堂昏暗。", &[], "", "zh");
        assert!(zh_seed.contains("只播种这个互动世界开场已经成立的状态。"));
    }

    #[test]
    fn replay_context_zh_en_shapes() {
        let zh = build_replay_context("打开抽屉", None, "zh");
        assert!(zh.starts_with("这是在重写上一回合，不是推进新的下一回合。"), "{zh}");
        assert!(zh.contains("原玩家动作：打开抽屉"));
        assert!(!zh.contains("用户替换说明"), "无替换不出现替换行：{zh}");
        assert!(zh.contains("不得倒退时间"), "{zh}");

        let zh_replace = build_replay_context("打开抽屉", Some("撬开柜子"), "zh");
        assert!(zh_replace.contains("用户替换说明：撬开柜子"), "{zh_replace}");

        let en = build_replay_context("open the drawer", Some("open the drawer"), "en");
        assert!(en.starts_with("This is a regeneration of the previous turn"), "{en}");
        assert!(!en.contains("Replacement instruction"), "相同替换不出现：{en}");
        assert!(en.contains("Do not move the clock backward"), "{en}");
    }

    #[test]
    fn renderer_prompt_injects_replay_constraints() {
        let action = json!({ "actionKind": "do", "intent": "x" });
        let without = scene_renderer_user_prompt("输入", &action, "摘要", "状态", "前提", None, "zh");
        assert!(!without.contains("重写约束"), "{without}");
        let with = scene_renderer_user_prompt("输入", &action, "摘要", "状态", "前提", Some("重写上一回合"), "zh");
        assert!(with.contains("重写约束：\n重写上一回合"), "{with}");
    }

    /// 全链集成：seed → step（reconcile 合成 + checkpoint）→ regenerate（变体对 +
    /// 回滚重放）→ restore_variant（恢复第一版现场）。
    struct FakeAgents {
        mutator_calls: AtomicUsize,
        render_calls: AtomicUsize,
        reconcile_calls: AtomicUsize,
    }

    impl FakeAgents {
        fn new() -> Self {
            Self {
                mutator_calls: AtomicUsize::new(0),
                render_calls: AtomicUsize::new(0),
                reconcile_calls: AtomicUsize::new(0),
            }
        }
    }

    #[async_trait::async_trait]
    impl PlayActionInterpreter for FakeAgents {
        async fn interpret(&self, input: &str, _scene_brief: &str, _language: &str) -> Value {
            json!({ "actionKind": "do", "intent": input, "manner": "", "risk": "", "ambiguity": "", "secondaryActions": [] })
        }
    }

    #[async_trait::async_trait]
    impl PlayWorldMutator for FakeAgents {
        async fn propose_mutation(&self, turn: i64, input: &str, _action: &Value, _context: &str, _language: &str) -> Value {
            let n = self.mutator_calls.fetch_add(1, Ordering::SeqCst);
            json!({
                "eventId": format!("evt-{turn}"), "turn": turn, "actionKind": "do",
                "summary": format!("第{n}次推进：{input}"),
                "entities": { "upsert": [
                    { "id": "actor_player", "type": "actor", "label": "玩家" },
                    { "id": format!("item-{n}"), "type": "item", "label": format!("物件{n}") }
                ] },
                "edges": { "upsert": [], "expire": [] },
                "stateSlots": { "upsert": [] },
                "evidence": { "transitions": [] },
                "blocked": false, "blockedReason": "", "notes": [],
            })
        }
    }

    #[async_trait::async_trait]
    impl PlaySceneRenderer for FakeAgents {
        async fn render(
            &self,
            _input: &str,
            _action: &Value,
            _mutation_summary: &str,
            _state_brief: &str,
            _mode: &str,
            _world_premise: &str,
            _replay_context: Option<&str>,
            _language: &str,
        ) -> RenderedScene {
            let n = self.render_calls.fetch_add(1, Ordering::SeqCst) + 1;
            RenderedScene { scene_text: format!("场景 v{n}：紧张推进。"), suggested_actions: vec![] }
        }
    }

    #[async_trait::async_trait]
    impl PlaySceneReconciler for FakeAgents {
        async fn reconcile(
            &self,
            turn: i64,
            _input: &str,
            action: &Value,
            _mutation: &Value,
            _scene_text: &str,
            _context: &str,
            _state_brief: &str,
            _world_premise: &str,
            _language: &str,
        ) -> Value {
            let n = self.reconcile_calls.fetch_add(1, Ordering::SeqCst);
            if n == 0 {
                let action_kind = action.get("actionKind").and_then(Value::as_str).unwrap_or("do");
                json!({
                    "eventId": format!("evt-{turn}"), "turn": turn, "actionKind": action_kind,
                    "summary": "正文补充：物件落地",
                    "entities": { "upsert": [ { "id": "item-extra", "type": "evidence", "label": "掉落的纽扣" } ] },
                    "edges": { "upsert": [ { "id": "edge-hold", "fromId": "actor_player", "type": "持有", "toId": "item-extra", "value": { "role": "holding", "physical": true } } ], "expire": [] },
                    "stateSlots": { "upsert": [] },
                    "evidence": { "transitions": [] },
                    "blocked": false, "blockedReason": "", "notes": ["对账补充"],
                })
            } else {
                empty_reconciliation(turn, "do")
            }
        }
    }

    #[tokio::test]
    async fn seed_step_regenerate_restore_chain() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        crate::play::create_world(
            &root,
            &crate::play::PlayWorldInput {
                id: "w79",
                title: "雾都",
                premise: "雨夜城市。",
                world_contract: "",
                visual_contract: "",
                mode: "open",
                language: "zh",
            },
        )
        .await
        .unwrap();
        let runner = PlayRunner { project_root: &root, world_id: "w79".into(), run_id: "main".into() };
        let fake = FakeAgents::new();

        // seed：播种 evt-0。
        let seeded = runner.seed_opening(&fake, "雨夜开场。", &[]).await.unwrap();
        assert!(seeded.is_some());

        // step 1：reconcile 补充合入（item-extra + holding 边）+ checkpoint 前置。
        let step1 = runner
            .step(&fake, &fake, &fake, Some(&fake), "打开抽屉", None)
            .await
            .unwrap();
        assert_eq!(step1.scene_text, "场景 v1：紧张推进。");
        let summary = step1.mutation.get("summary").and_then(Value::as_str).unwrap();
        assert!(summary.contains("；"), "summary 合成：{summary}");
        assert!(summary.contains("第1次推进"), "{summary}");
        assert!(summary.contains("正文补充：物件落地"), "{summary}");
        let run_dir = crate::play::run_dir(&root, "w79", "main").unwrap();
        let graph = crate::play::play_graph_snapshot(&run_dir);
        let entity_ids: Vec<&str> = graph["entities"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e.get("id").and_then(Value::as_str).unwrap_or_default())
            .collect();
        assert!(entity_ids.contains(&"item-1"), "entities: {entity_ids:?}");
        assert!(entity_ids.contains(&"item-extra"), "reconcile 补充入图：{entity_ids:?}");
        let edge_labels: Vec<&str> = graph["edges"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e.get("id").and_then(Value::as_str).unwrap_or_default())
            .collect();
        assert!(edge_labels.contains(&"edge-hold"), "edges: {edge_labels:?}");
        // checkpoint before-turn-1 已落盘；state.md 含补充实体。
        assert!(run_dir.join("checkpoints/before-turn-1.json").is_file());
        let state_md = tokio::fs::read_to_string(run_dir.join("projections/state.md")).await.unwrap();
        assert!(state_md.contains("item-extra"), "{state_md}");
        assert_eq!(crate::play::read_events(&root, "w79", "main").await.len(), 1);

        // regenerate：无替换 → 重放原输入；场景 v2；变体对；事件数不涨。
        let replay = runner
            .regenerate_last_turn(&fake, &fake, &fake, Some(&fake), None)
            .await
            .unwrap();
        assert_eq!(replay.replayed_input, "打开抽屉");
        assert_eq!(replay.scene_text, "场景 v2：紧张推进。");
        let previous_variant = replay.previous_variant_id.clone().unwrap();
        let regenerated_variant = replay.variant_id.clone().unwrap();
        assert_ne!(previous_variant, regenerated_variant);
        assert!(previous_variant.starts_with("v-"));
        assert_eq!(crate::play::read_events(&root, "w79", "main").await.len(), 1, "重放覆盖不新增事件");
        // current state 仍是 turn 1。
        let current = crate::play::load_current_state(&root, "w79", "main").await.unwrap();
        assert_eq!(current.get("turn"), Some(&json!(1)));
        // 两个变体文件 + before-turn-1 检查点仍在。
        let variants_dir = run_dir.join("variants/turn-1");
        let variant_files: Vec<_> = std::fs::read_dir(&variants_dir).unwrap().collect();
        assert_eq!(variant_files.len(), 2, "变体对：{variant_files:?}");
        assert!(run_dir.join("checkpoints/before-turn-1.json").is_file());
        // 重放后图 = item-1（回滚重放，reconcile 空补充）。
        let graph2 = crate::play::play_graph_snapshot(&run_dir);
        let ids2: Vec<&str> = graph2["entities"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e.get("id").and_then(Value::as_str).unwrap_or_default())
            .collect();
        assert!(ids2.contains(&"item-2"), "重放实体：{ids2:?}");

        // restore previous variant：恢复第一版现场（场景 v1 + item-0/item-extra）。
        let restored = runner.restore_variant(1, &previous_variant).await.unwrap();
        assert_eq!(restored.scene_text, "场景 v1：紧张推进。");
        let scene_md = tokio::fs::read_to_string(run_dir.join("projections/scene.md")).await.unwrap();
        assert!(scene_md.contains("场景 v1"), "{scene_md}");
        let graph3 = crate::play::play_graph_snapshot(&run_dir);
        let ids3: Vec<&str> = graph3["entities"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e.get("id").and_then(Value::as_str).unwrap_or_default())
            .collect();
        assert!(ids3.contains(&"item-1") && ids3.contains(&"item-extra"), "恢复第一版：{ids3:?}");
        assert!(!ids3.contains(&"item-2"), "重放实体被回滚：{ids3:?}");

        // 缺失检查点/变体的错误面。
        let missing_variant = runner.restore_variant(9, "v-none").await.unwrap_err();
        assert!(missing_variant.contains("Play variant not found: turn 9 / v-none"), "{missing_variant}");
    }
}
