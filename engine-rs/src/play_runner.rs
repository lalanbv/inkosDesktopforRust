//! play 回合执行流（play-runner.ts + play-agents.ts 三必需代理，73 号）。
//!
//! step 链：interpret（动作归一，fail-open 降级为 do）→ mutate（状态草案，
//! fail-open 降级为 blocked 回合）→ render（场景正文，fail-open 降级为原始
//! prose/占位）→ 全部落盘（render 先行、提交在后——回合 all-or-nothing）。
//!
//! 暂缓（偏差备案见 73 号记录）：sceneReconciler 对账代理、regenerateLastTurn
//! 变体/检查点面、en 提示词（当前 zh 全量，en 世界回退 zh 提示词）。

use std::path::Path;

use serde_json::{json, Value};

use crate::llm::agent_router::AgentRouter;
use crate::llm::provider::{LLMMessage, LLMRole};
use crate::play_graph::{apply_play_mutation, open_play_graph_db, seed_play_graph};

// ── 三代理 trait ────────────────────────────────────────────────

#[async_trait::async_trait]
pub trait PlayActionInterpreter: Send + Sync {
    async fn interpret(&self, input: &str, scene_brief: &str) -> Value;
}

#[async_trait::async_trait]
pub trait PlayWorldMutator: Send + Sync {
    async fn propose_mutation(&self, turn: i64, input: &str, action: &Value, context: &str) -> Value;
}

#[async_trait::async_trait]
pub trait PlaySceneRenderer: Send + Sync {
    async fn render(
        &self,
        input: &str,
        action: &Value,
        mutation_summary: &str,
        state_brief: &str,
        mode: &str,
        world_premise: &str,
    ) -> RenderedScene;
}

#[derive(Debug, Clone)]
pub struct RenderedScene {
    pub scene_text: String,
    pub suggested_actions: Vec<String>,
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
    async fn interpret(&self, input: &str, scene_brief: &str) -> Value {
        // fail-open：瞬时错误/不可解析 → 通用动作（玩家原话作 do）。
        let raw = chat_with_retry(
            self.router,
            "play-action-interpreter",
            vec![
                LLMMessage { role: LLMRole::System, content: action_interpreter_system_prompt(), tool_calls: None, tool_call_id: None },
                LLMMessage { role: LLMRole::User, content: action_interpreter_user_prompt(input, scene_brief), tool_calls: None, tool_call_id: None },
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
    async fn propose_mutation(&self, turn: i64, input: &str, action: &Value, context: &str) -> Value {
        let raw = chat_with_retry(
            self.router,
            "play-world-mutator",
            vec![
                LLMMessage { role: LLMRole::System, content: world_mutator_system_prompt(), tool_calls: None, tool_call_id: None },
                LLMMessage { role: LLMRole::User, content: world_mutator_user_prompt(turn, input, action, context), tool_calls: None, tool_call_id: None },
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
    async fn render(
        &self,
        input: &str,
        action: &Value,
        mutation_summary: &str,
        state_brief: &str,
        mode: &str,
        world_premise: &str,
    ) -> RenderedScene {
        let system = scene_renderer_system_prompt(mode);
        let user = scene_renderer_user_prompt(input, action, mutation_summary, state_brief, world_premise);
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
                content: "上面不是严格 JSON。只输出一个 JSON 对象 {\"sceneText\": \"...\", \"suggestedActions\": [\"...\"]}，不要任何其他文字。".to_string(),
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
                "（这一拍悬着，没有落定。）".to_string()
            } else {
                prose
            },
            suggested_actions: Vec::new(),
        }
    }
}

// ── 提示词（zh 逐字；en 暂缓——偏差备案） ────────────────────────

fn action_interpreter_system_prompt() -> String {
    [
        "你是互动小说动作理解器。",
        "你的任务是把玩家一句自然语言，归一成五类动作之一：look / say / move / do / wait。",
        "不要替玩家加戏，不要直接推进剧情，不要写场景正文。",
        "look=观察/检查/回忆线索；say=说话/试探/质问；move=移动到地点；do=执行动作/使用物品/调查；wait=等待/拖延/旁观。",
        "输出严格 JSON，不要解释。",
    ]
    .join("\n")
}

fn action_interpreter_user_prompt(input: &str, scene_brief: &str) -> String {
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

fn world_mutator_system_prompt() -> String {
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

fn world_mutator_user_prompt(turn: i64, input: &str, action: &Value, context: &str) -> String {
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

fn scene_renderer_system_prompt(mode: &str) -> String {
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
) -> String {
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
        let action = json!({
            "actionKind": "look",
            "intent": "播种第一幕已成立的开场状态。",
            "manner": "",
            "risk": "",
            "ambiguity": "",
            "secondaryActions": [],
        });
        let world_context = render_world_context(world.as_ref());
        let context = self.build_context_brief(scene_text, &world).await;
        let opening_input = build_opening_seed_input(scene_text, suggested_actions, &world_context);
        let mut mutation = mutator.propose_mutation(0, &opening_input, &action, &context).await;
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

    /// `step`：一回合全链（render 先行、提交在后）。
    pub async fn step(
        &self,
        interpreter: &dyn PlayActionInterpreter,
        mutator: &dyn PlayWorldMutator,
        renderer: &dyn PlaySceneRenderer,
        input: &str,
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
            "新回合开始，沿用当前世界状态。".to_string()
        } else {
            scene_brief.clone()
        };
        let action = interpreter.interpret(raw_input, &scene_brief_or_default).await;
        let world_context = render_world_context(world.as_ref());
        let context = self.build_context_brief(&scene_brief, &world).await;
        let mutation = mutator
            .propose_mutation(turn, raw_input, &action, &context)
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
            .render(raw_input, &action, &mutation_summary, &state_brief, &mode, &world_context)
            .await;

        // 提交：事件 + 图 + 投影 + 状态 + transcript。
        let run_dir = crate::play::run_dir(self.project_root, &self.world_id, &self.run_id)?;
        let mut db = open_play_graph_db(&run_dir)?;
        let applied = apply_play_mutation(&mut db, &mutation, raw_input)?;
        db.flush()?;
        crate::play::append_event(self.project_root, &self.world_id, &self.run_id, &applied.event).await?;
        crate::play::write_projection(
            self.project_root,
            &self.world_id,
            &self.run_id,
            "projections/state.md",
            &state_brief,
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
                "lastSummary": mutation.get("summary"),
                "timeAdvance": mutation.get("timeAdvance").filter(|v| !v.is_null()),
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
            mutation,
        })
    }

    async fn build_context_brief(&self, scene_brief: &str, world: &Option<Value>) -> String {
        let state_brief = crate::play::read_projection(
            self.project_root,
            &self.world_id,
            &self.run_id,
            "projections/state.md",
        )
        .await
        .unwrap_or_default();
        let world_context = render_world_context(world.as_ref());
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
        );
        let mut blocks: Vec<String> = Vec::new();
        if !world_context.is_empty() {
            blocks.push(world_context);
        }
        if !entity_roster.is_empty() {
            blocks.push(entity_roster);
        }
        if !scene_brief.trim().is_empty() {
            blocks.push(format!("当前场景：\n{scene_brief}"));
        }
        if !state_brief.trim().is_empty() {
            blocks.push(format!("当前状态：\n{state_brief}"));
        }
        if blocks.is_empty() {
            "暂无持久化状态。".to_string()
        } else {
            blocks.join("\n\n")
        }
    }
}

fn render_world_context(world: Option<&Value>) -> String {
    let Some(world) = world else {
        return String::new();
    };
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
        blocks.push(format!("世界设定:\n{premise}"));
    }
    if let Some(contract) = field("worldContract") {
        blocks.push(format!("世界契约（高优先级，先于题材惯例）:\n{contract}"));
    }
    if let Some(visual) = field("visualContract") {
        blocks.push(format!("视觉契约（保持场景和配图一致）:\n{visual}"));
    }
    blocks.join("\n\n")
}

fn build_opening_seed_input(scene_text: &str, suggested_actions: &[String], premise: &str) -> String {
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

fn render_entity_roster(entities: &[Value]) -> String {
    if entities.is_empty() {
        return String::new();
    }
    let mut lines = vec!["当前实体名册（复用这些 id；不要把同一个人/物换新 id 重建）：".to_string()];
    for entity in entities.iter().take(40) {
        let id = entity.get("id").and_then(Value::as_str).unwrap_or_default();
        let entity_type = entity.get("type").and_then(Value::as_str).unwrap_or_default();
        let label = entity.get("label").and_then(Value::as_str).unwrap_or_default();
        let summary = entity.get("summary").and_then(Value::as_str).unwrap_or_default();
        let status = entity.get("status").and_then(Value::as_str).unwrap_or_default();
        let mut detail = summary.to_string();
        if !status.is_empty() {
            if !detail.is_empty() {
                detail.push('；');
            }
            detail.push_str(&format!("状态: {status}"));
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
