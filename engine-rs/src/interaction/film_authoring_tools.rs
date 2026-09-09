//! interactive-film-authoring 会话工具面（TS film-authoring-tools.ts 聊天子集，
//! 256 号）。
//!
//! 未确认面（TS `buildFilmAuthoringToolNames` 无 confirmedIntent 分支）：
//! 直写四件（set_world_anchor / upsert_characters / add_variable /
//! define_ending）+ LLM 两件（fill_node / revise_node）+ generate_node_image；
//! propose_action / use_skill 由 agent_route 注册矩阵按 TS 追加。确认类三件
//! （draft_structure / connect_choice / remove_node）在 Rust 架构下走
//! agent_production 确认链直接执行（与 chat_prompts 同一备案），聊天循环
//! 不可达，不在此移植。
//!
//! LLM 调用面（TS runWorkerAgentTool submit_story_node 工具提交制）沿用
//! agent_production::execute_draft_structure 的既定适配：纯 chat + JSON 提取
//! + serde 校验（107 号先例），双语提示词与 prompt-pack 附加段逐字对齐。

use std::path::Path;

use serde_json::{json, Value};

use crate::interaction::project_tools::ToolResult;
use crate::llm::provider::{LLMMessage, LLMRole};
use crate::state::memory_db::{MemoryDb, NewFact};

use crate::interactive_film as film;

/// LLM 聊天端口（AgentRouter.chat 的最小投影，测试注入 mock）。
#[async_trait::async_trait]
pub trait FilmAuthoringLLM: Send + Sync {
    async fn chat(
        &self,
        agent: &str,
        messages: Vec<LLMMessage>,
        temperature: f64,
        max_tokens: Option<u32>,
    ) -> Result<String, String>;
}

/// 生产实现：`AgentRouter.chat`（"film-authoring" agent 角色，TS
/// createAgentContext("film-authoring") 对应物）。
pub struct RouterFilmAuthoringLLM<'a>(pub &'a crate::llm::agent_router::AgentRouter);

#[async_trait::async_trait]
impl FilmAuthoringLLM for RouterFilmAuthoringLLM<'_> {
    async fn chat(
        &self,
        agent: &str,
        messages: Vec<LLMMessage>,
        temperature: f64,
        max_tokens: Option<u32>,
    ) -> Result<String, String> {
        self.0
            .chat(agent, messages, temperature, max_tokens)
            .await
            .map(|outcome| outcome.content)
    }
}

/// 工具面依赖：项目根 + 影游项目 id（会话 bookId）+ 项目语言 + LLM 端口。
pub struct FilmAuthoringDeps<'a> {
    pub root: &'a Path,
    pub project_id: &'a str,
    pub language: &'a str,
    pub llm: &'a dyn FilmAuthoringLLM,
}

// ── schema（TS TypeBox 形态 → OpenAI function JSON，描述逐字） ──────

/// 未确认面工具集（TS `buildFilmAuthoringToolNames(undefined)` 去 propose_action
/// ——propose/use_skill 由注册矩阵统一追加）。
pub fn film_authoring_tool_schemas() -> Vec<Value> {
    vec![
        json!({
            "type": "function",
            "function": {
                "name": "set_world_anchor",
                "description": "interactive-film authoring: set/update the world anchor (story core, theme, rules, duration). Applies immediately.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "storyCore": { "type": "string", "description": "one-sentence story core" },
                        "theme": { "type": "string", "description": "theme of the story" },
                        "genre": { "type": "string", "description": "genre, free text (e.g. suspense, romance)" },
                        "worldRules": { "type": "string", "description": "world rules that constrain the plot" },
                        "durationMinutes": { "type": "number", "description": "target playthrough duration in minutes" }
                    }
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "upsert_characters",
                "description": "interactive-film authoring: add/update characters with voice profiles. Applies immediately and records them to memory for cross-node voice consistency.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "characters": {
                            "type": "array",
                            "description": "characters to add or update",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "id": { "type": "string" },
                                    "name": { "type": "string" },
                                    "role": { "type": "string", "enum": ["protagonist", "antagonist", "support", "other"] },
                                    "motivation": { "type": "string" },
                                    "voiceProfile": {
                                        "type": "object",
                                        "properties": {
                                            "speakingRhythm": { "type": "string" },
                                            "vocabulary": { "type": "string" },
                                            "sampleLines": { "type": "array", "items": { "type": "string" } }
                                        }
                                    }
                                },
                                "required": ["id", "name"]
                            }
                        }
                    },
                    "required": ["characters"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "add_variable",
                "description": "interactive-film authoring: add/update a variable. Applies immediately.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string", "description": "variable name (unique key)" },
                        "type": { "type": "string", "enum": ["flag", "counter", "relationship", "item"] },
                        "default": { "description": "default value", "type": ["number", "string", "boolean"] },
                        "desc": { "type": "string", "description": "what it tracks" }
                    },
                    "required": ["name", "type", "default"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "define_ending",
                "description": "interactive-film authoring: define/update an ending (its nodeId must exist). Applies immediately.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string", "description": "ending id" },
                        "nodeId": { "type": "string", "description": "the ending node this describes (must exist)" },
                        "title": { "type": "string" },
                        "type": { "type": "string", "enum": ["good", "bad", "neutral", "secret"] },
                        "description": { "type": "string" }
                    },
                    "required": ["id", "nodeId", "title", "type"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "fill_node",
                "description": "interactive-film authoring: write/rewrite one node's scene, dialogue and choices via the model. Applies immediately.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "nodeId": { "type": "string", "description": "the node to fill/rewrite" },
                        "instruction": { "type": "string", "description": "what this scene should contain (beats, who speaks, choices)" }
                    },
                    "required": ["nodeId", "instruction"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "revise_node",
                "description": "interactive-film authoring: revise one existing node per instruction. Applies immediately.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "nodeId": { "type": "string", "description": "the node to fill/rewrite" },
                        "instruction": { "type": "string", "description": "what this scene should contain (beats, who speaks, choices)" }
                    },
                    "required": ["nodeId", "instruction"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "generate_node_image",
                "description": "interactive-film authoring: generate a shot image for a node (from its imageSlot.prompt or sceneDesc) and attach it. Applies immediately.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "nodeId": { "type": "string", "description": "the node to generate a shot image for (uses its imageSlot.prompt or sceneDesc)" },
                        "size": {
                            "type": "string",
                            "enum": ["1536x1024", "1024x1536", "1024x1024"],
                            "description": "output image size; use 1536x1024 for landscape film frames, 1024x1536 for portrait, or 1024x1024 for square"
                        }
                    },
                    "required": ["nodeId"]
                }
            }
        }),
    ]
}

/// 分发面（未知工具返回 None，落回后续执行器）。
pub async fn execute_film_authoring_tool(
    deps: &FilmAuthoringDeps<'_>,
    name: &str,
    args: &Value,
) -> Option<ToolResult> {
    match name {
        "set_world_anchor" => Some(tool_set_world_anchor(deps, args).await),
        "upsert_characters" => Some(tool_upsert_characters(deps, args).await),
        "add_variable" => Some(tool_add_variable(deps, args).await),
        "define_ending" => Some(tool_define_ending(deps, args).await),
        "fill_node" => Some(tool_fill_or_revise_node(deps, args, NodeWriteKind::Fill).await),
        "revise_node" => Some(tool_fill_or_revise_node(deps, args, NodeWriteKind::Revise).await),
        "generate_node_image" => Some(tool_generate_node_image(deps, args).await),
        _ => None,
    }
}

fn text_result(text: impl Into<String>, details: Option<Value>) -> ToolResult {
    ToolResult { text: text.into(), details, is_error: false }
}

fn error_result(text: impl Into<String>) -> ToolResult {
    crate::interaction::project_tools::error_result(text.into())
}

fn required_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, ToolResult> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| error_result(format!("{key} is required")))
}

async fn apply_delta(
    deps: &FilmAuthoringDeps<'_>,
    delta: film::StoryGraphDelta,
    phase: Option<&'static str>,
) -> Result<i64, String> {
    crate::server::interactive_film_routes::apply_graph_delta(deps.root, deps.project_id, &delta, phase)
        .await
        .map(|(_, rev)| rev)
}

// ── set_world_anchor ──────────────────────────────────────────────

async fn tool_set_world_anchor(deps: &FilmAuthoringDeps<'_>, args: &Value) -> ToolResult {
    let opt_str = |key: &str| {
        args.get(key)
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    let patch = film::WorldAnchorDelta {
        story_core: opt_str("storyCore"),
        theme: opt_str("theme"),
        genre: opt_str("genre"),
        world_rules: opt_str("worldRules"),
        duration_minutes: args.get("durationMinutes").and_then(Value::as_f64),
    };
    let core_preview = patch.story_core.clone().unwrap_or_default();
    let delta = film::build_world_anchor_delta(patch);
    match apply_delta(deps, delta, Some("world")).await {
        Ok(rev) => text_result(
            format!("World anchor updated (rev {rev}). core={core_preview}"),
            Some(json!({ "kind": "graph_updated", "rev": rev })),
        ),
        Err(message) => error_result(message),
    }
}

// ── upsert_characters（含 memory 角色事实，TS writeCharacterFacts） ──

async fn tool_upsert_characters(deps: &FilmAuthoringDeps<'_>, args: &Value) -> ToolResult {
    let Some(chars_json) = args.get("characters").and_then(Value::as_array).cloned() else {
        return error_result("characters is required");
    };
    let mut chars = Vec::with_capacity(chars_json.len());
    for raw in chars_json {
        match serde_json::from_value::<film::Character>(raw) {
            Ok(character) => chars.push(character),
            Err(e) => return error_result(format!("invalid character: {e}")),
        }
    }
    let delta = film::build_upsert_characters_delta(chars.clone());
    let rev = match apply_delta(deps, delta, None).await {
        Ok(rev) => rev,
        Err(message) => return error_result(message),
    };
    // TS：MemoryDB(join(projectRoot, "interactive-films", projectId)) + 写入
    // motivation / voice 事实（跨节点口吻一致性的检索面）。story/ 目录由
    // open 前建好（MemoryDb::open 不建父目录）。
    let book_dir = deps.root.join("interactive-films").join(deps.project_id);
    if tokio::fs::create_dir_all(book_dir.join("story")).await.is_ok() {
        if let Ok(db) = MemoryDb::open(&book_dir) {
            write_character_facts(&db, &chars, rev);
            let _ = db.close();
        }
    }
    text_result(
        format!("Upserted {} character(s) (rev {rev}).", chars.len()),
        Some(json!({ "kind": "graph_updated", "rev": rev })),
    )
}

/// TS `writeCharacterFacts`（memory-link.ts 逐字）：motivation 与
/// speakingRhythm/vocabulary 合成 voice 两条 predicate。
fn write_character_facts(db: &MemoryDb, chars: &[film::Character], rev: i64) {
    for c in chars {
        if !c.motivation.is_empty() {
            let _ = db.add_fact(&NewFact {
                subject: c.name.clone(),
                predicate: "motivation".to_string(),
                object: c.motivation.clone(),
                valid_from_chapter: rev,
                valid_until_chapter: None,
                source_chapter: rev,
            });
        }
        if let Some(vp) = &c.voice_profile {
            if !vp.speaking_rhythm.is_empty() || !vp.vocabulary.is_empty() {
                let object = [vp.speaking_rhythm.as_str(), vp.vocabulary.as_str()]
                    .iter()
                    .filter(|part| !part.is_empty())
                    .map(|part| part.to_string())
                    .collect::<Vec<_>>()
                    .join(" / ");
                let _ = db.add_fact(&NewFact {
                    subject: c.name.clone(),
                    predicate: "voice".to_string(),
                    object,
                    valid_from_chapter: rev,
                    valid_until_chapter: None,
                    source_chapter: rev,
                });
            }
        }
    }
}

// ── add_variable ──────────────────────────────────────────────────

async fn tool_add_variable(deps: &FilmAuthoringDeps<'_>, args: &Value) -> ToolResult {
    let Ok(name) = required_str(args, "name") else {
        return error_result("name is required");
    };
    let Some(var_type) = args
        .get("type")
        .and_then(Value::as_str)
        .and_then(|t| serde_json::from_value::<film::VariableType>(json!(t)).ok())
    else {
        return error_result("add_variable: type must be one of flag/counter/relationship/item");
    };
    let Some(default) = args.get("default").cloned() else {
        return error_result("default is required");
    };
    if !(default.is_number() || default.is_string() || default.is_boolean()) {
        return error_result("add_variable: default must be a number, string or boolean");
    }
    let desc = args.get("desc").and_then(Value::as_str).unwrap_or("");
    let variable = film::Variable {
        name: name.to_string(),
        variable_type: var_type,
        default,
        desc: desc.to_string(),
    };
    let delta = film::build_add_variable_delta(variable);
    match apply_delta(deps, delta, None).await {
        Ok(rev) => text_result(
            format!("Variable \"{name}\" added (rev {rev})."),
            Some(json!({ "kind": "graph_updated", "rev": rev })),
        ),
        Err(message) => error_result(message),
    }
}

// ── define_ending ─────────────────────────────────────────────────

async fn tool_define_ending(deps: &FilmAuthoringDeps<'_>, args: &Value) -> ToolResult {
    let (Ok(id), Ok(node_id), Ok(title)) = (
        required_str(args, "id"),
        required_str(args, "nodeId"),
        required_str(args, "title"),
    ) else {
        return error_result("id, nodeId and title are required");
    };
    let Some(ending_type) = args
        .get("type")
        .and_then(Value::as_str)
        .and_then(|t| serde_json::from_value::<film::EndingType>(json!(t)).ok())
    else {
        return error_result("define_ending: type must be one of good/bad/neutral/secret");
    };
    let description = args.get("description").and_then(Value::as_str).unwrap_or("");
    let ending = film::Ending {
        id: id.to_string(),
        node_id: node_id.to_string(),
        title: title.to_string(),
        ending_type,
        description: description.to_string(),
    };
    let delta = film::build_define_ending_delta(ending);
    match apply_delta(deps, delta, None).await {
        Ok(rev) => text_result(
            format!("Ending \"{title}\" defined (rev {rev})."),
            Some(json!({ "kind": "graph_updated", "rev": rev })),
        ),
        Err(message) => error_result(message),
    }
}

// ── fill_node / revise_node（LLM 工具） ───────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum NodeWriteKind {
    Fill,
    Revise,
}

const NODE_SYSTEM_ZH: &str = "你是互动影游编剧。根据当前图上下文和指令，写出指定节点的完整场景、对白、选项和配图方向。choices[].targetNodeId 必须指向已存在的节点 id。完成后调用 submit_story_node。";
const NODE_SYSTEM_EN: &str = "You are an interactive film scriptwriter. Using the current graph context and the instruction, write the requested node's complete scene, dialogue, choices, and image direction. Every choices[].targetNodeId must point to an existing node id. Finish by calling submit_story_node.";

fn node_system_prompt(language: &str) -> &'static str {
    if language == "en" { NODE_SYSTEM_EN } else { NODE_SYSTEM_ZH }
}

/// prompt-pack 附加段（107 号出口同款：失败回原提示词）。
async fn append_prompt_pack(base: &str, prompt_id: &str, root: &Path) -> String {
    crate::prompts::prompt_pack::append_prompt_pack_guidance(
        &crate::state::store::FsStateStore,
        base,
        &crate::prompts::prompt_pack::LoadPromptPackPromptInput {
            prompt_id: prompt_id.to_string(),
            project_root: Some(root.display().to_string()),
            user_root: None,
        },
    )
    .await
    .unwrap_or_else(|_| base.to_string())
}

async fn load_authoring_context(deps: &FilmAuthoringDeps<'_>) -> (Option<film::StoryGraph>, String) {
    match film::load_story_graph(deps.root, deps.project_id).await {
        Ok(Some(graph)) => {
            let context = film::build_film_authoring_context(&graph);
            (Some(graph), context)
        }
        _ => (None, "(empty graph)".to_string()),
    }
}

async fn tool_fill_or_revise_node(
    deps: &FilmAuthoringDeps<'_>,
    args: &Value,
    kind: NodeWriteKind,
) -> ToolResult {
    let tool_name = match kind {
        NodeWriteKind::Fill => "fill_node",
        NodeWriteKind::Revise => "revise_node",
    };
    let (Ok(node_id), Ok(instruction)) = (
        required_str(args, "nodeId"),
        required_str(args, "instruction"),
    ) else {
        return error_result("nodeId and instruction are required");
    };
    let (graph, context) = load_authoring_context(deps).await;
    let base_prompt = node_system_prompt(deps.language);
    let system_prompt = append_prompt_pack(base_prompt, "interactive-film.script", deps.root).await;
    let user_prompt = match kind {
        NodeWriteKind::Fill => {
            if deps.language == "en" {
                format!("{context}\n\nNode id to fill: {node_id}\nInstruction: {instruction}")
            } else {
                format!("{context}\n\n要填的节点 id：{node_id}\n指令：{instruction}")
            }
        }
        NodeWriteKind::Revise => {
            let current = graph
                .as_ref()
                .and_then(|g| g.nodes.iter().find(|n| n.id == node_id));
            let current_json = current
                .and_then(|n| serde_json::to_value(n).ok())
                .unwrap_or_else(|| json!({}));
            if deps.language == "en" {
                format!(
                    "{context}\n\nNode id to revise: {node_id}\nCurrent content: {current_json}\nRevision instruction: {instruction}"
                )
            } else {
                format!(
                    "{context}\n\n要修改的节点 id：{node_id}\n现有内容：{current_json}\n修改指令：{instruction}"
                )
            }
        }
    };
    // TS submitNode：temperature 0.6 / maxTokens 4000。
    let raw = deps
        .llm
        .chat(
            "film-authoring",
            vec![
                LLMMessage { role: LLMRole::System, content: system_prompt, tool_calls: None, tool_call_id: None },
                LLMMessage { role: LLMRole::User, content: user_prompt, tool_calls: None, tool_call_id: None },
            ],
            0.6,
            Some(4000),
        )
        .await;
    let raw = match raw {
        Ok(content) => content,
        Err(message) => return error_result(format!("{tool_name}: {message}")),
    };
    let parsed = match crate::server::agent_production::extract_json_object(&raw) {
        Some(value) => value,
        None => return error_result(format!("{tool_name}: LLM returned no JSON")),
    };
    let mut node = match serde_json::from_value::<film::StoryNode>(parsed) {
        Ok(node) => node,
        Err(e) => return error_result(format!("{tool_name}: invalid node: {e}")),
    };
    // TS StoryNodeSchema.parse({ ...submitted, id: nodeId })——宿主持有节点 id。
    node.id = node_id.to_string();
    let delta = film::StoryGraphDelta {
        world_anchor: None,
        characters: None,
        nodes: Some(film::UpsertRemove { upsert: vec![node], remove: Vec::new() }),
        variables: None,
        endings: None,
        notes: Vec::new(),
    };
    match apply_delta(deps, delta, Some("workshop")).await {
        Ok(rev) => {
            let verb = match kind {
                NodeWriteKind::Fill => "filled",
                NodeWriteKind::Revise => "revised",
            };
            text_result(
                format!("Node {node_id} {verb} (rev {rev})."),
                Some(json!({
                    "kind": "graph_updated",
                    "rev": rev,
                    "promptPacks": ["interactive-film.script"],
                    "skillIds": [],
                })),
            )
        }
        Err(message) => error_result(message),
    }
}

// ── generate_node_image（复用 cover 生图基建，74 号同链） ──────────

async fn tool_generate_node_image(deps: &FilmAuthoringDeps<'_>, args: &Value) -> ToolResult {
    let Ok(node_id) = required_str(args, "nodeId") else {
        return error_result("nodeId is required");
    };
    let graph = match film::load_story_graph(deps.root, deps.project_id).await {
        Ok(Some(graph)) => graph,
        Ok(None) => {
            return error_result(format!(
                "interactive-film project {} has no story graph",
                deps.project_id
            ))
        }
        Err(message) => return error_result(message),
    };
    let Some(node) = graph.nodes.iter().find(|n| n.id == node_id).cloned() else {
        return error_result(format!("node {node_id} not found"));
    };
    let prompt = node
        .image_slot
        .as_ref()
        .map(|slot| slot.prompt.trim().to_string())
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| node.scene_desc.trim().to_string());
    if prompt.is_empty() {
        return error_result(format!(
            "node {node_id} has no imageSlot.prompt or sceneDesc to generate an image from"
        ));
    }
    // TS generateNodeImage：size ?? env INKOS_FILM_IMAGE_SIZE ?? "1536x1024"。
    let size = args
        .get("size")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| {
            std::env::var("INKOS_FILM_IMAGE_SIZE")
                .ok()
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_else(|| "1536x1024".to_string());
    let request = match crate::llm::cover::resolve_cover_generation_request(deps.root).await {
        Ok(request) => request,
        Err(message) => return error_result(message),
    };
    let image = match crate::llm::cover::generate_image_from_prompt(&request, &prompt, &size).await {
        Ok(image) => image,
        Err(message) => return error_result(message),
    };
    let asset_ref = film::node_image_rel_path(deps.project_id, node_id, image.extension);
    let abs = deps.root.join(&asset_ref);
    if let Some(parent) = abs.parent() {
        if let Err(e) = tokio::fs::create_dir_all(parent).await {
            return error_result(e.to_string());
        }
    }
    if let Err(e) = tokio::fs::write(&abs, &image.bytes).await {
        return error_result(e.to_string());
    }
    // buildSetImageRefDelta：upsert 节点 + imageSlot {prompt, assetRef}。
    let mut node_with_image = node.clone();
    node_with_image.image_slot = Some(film::ImageSlot {
        prompt: prompt.clone(),
        asset_ref: Some(asset_ref.clone()),
    });
    let delta = film::build_connect_choice_delta(node_with_image);
    match apply_delta(deps, delta, None).await {
        Ok(rev) => text_result(
            format!("Generated image for node {node_id} (rev {rev})."),
            Some(json!({ "kind": "graph_updated", "rev": rev, "assetRef": asset_ref })),
        ),
        Err(message) => error_result(message),
    }
}

// ── context-transform（TS createInteractiveFilmContextTransform） ──

/// 每轮注入的完整权威剧情图谱 user 消息（注入位置：system 之后、历史与
/// 本轮指令之前——TS `[injected, ...messages]` 对应位）。图谱缺失时不注入。
pub fn film_graph_context_message(graph: &film::StoryGraph) -> LLMMessage {
    let payload = serde_json::to_string(graph).unwrap_or_default();
    LLMMessage {
        role: LLMRole::User,
        content: format!(
            "[以下是当前互动影游的完整权威剧情图谱，每轮从磁盘重新读取。编辑时必须使用其中真实的 node id、choice id、变量和结局 id；不要凭空臆造。]\n{payload}"
        ),
        tool_calls: None,
        tool_call_id: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn temp_root(tag: &str) -> std::path::PathBuf {
        let base = std::env::temp_dir().join(format!(
            "if-authoring-{tag}-{}-{}",
            std::process::id(),
            crate::interaction::session::utc_now_ms()
        ));
        std::fs::create_dir_all(base.join("interactive-films").join("p")).unwrap();
        base
    }

    fn deps_for<'a>(
        root: &'a Path,
        llm: &'a dyn FilmAuthoringLLM,
        language: &'a str,
    ) -> FilmAuthoringDeps<'a> {
        FilmAuthoringDeps { root, project_id: "p", language, llm }
    }

    fn save_graph(root: &Path, graph: &film::StoryGraph) {
        let path = film::story_graph_path(root, "p");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            path,
            serde_json::to_string_pretty(graph).unwrap(),
        )
        .unwrap();
    }

    fn sample_graph() -> film::StoryGraph {
        serde_json::from_value(json!({
            "schemaVersion": 1,
            "projectId": "p",
            "title": "T",
            "variables": [],
            "nodes": [
                { "id": "n1", "type": "branch", "title": "抉择", "choices": [], "sceneDesc": "" },
                { "id": "e", "type": "ending", "choices": [], "sceneDesc": "" }
            ],
            "endings": []
        }))
        .unwrap()
    }

    fn no_llm() -> impl FilmAuthoringLLM {
        struct Reject;
        #[async_trait::async_trait]
        impl FilmAuthoringLLM for Reject {
            async fn chat(&self, _: &str, _: Vec<LLMMessage>, _: f64, _: Option<u32>) -> Result<String, String> {
                Err("should not be called".to_string())
            }
        }
        Reject
    }

    #[tokio::test]
    async fn direct_write_tools_apply_and_report_rev() {
        let root = temp_root("direct");
        // 预置含 e 结局节点的图谱（define_ending 校验 nodeId 存在）。
        save_graph(&root, &sample_graph());
        let llm = no_llm();
        let deps = deps_for(&root, &llm, "zh");

        let result = execute_film_authoring_tool(
            &deps,
            "set_world_anchor",
            &json!({ "storyCore": "孤儿觉醒", "durationMinutes": 30 }),
        )
        .await
        .unwrap();
        assert!(!result.is_error);
        assert_eq!(result.text, "World anchor updated (rev 1). core=孤儿觉醒");
        assert_eq!(result.details.unwrap()["kind"], json!("graph_updated"));

        let graph = film::load_story_graph(&root, "p").await.unwrap().unwrap();
        assert_eq!(graph.world_anchor.as_ref().unwrap().story_core, "孤儿觉醒");
        assert_eq!(graph.world_anchor.as_ref().unwrap().duration_minutes, 30.0);

        // add_variable：校验枚举 + 落盘。
        let bad = execute_film_authoring_tool(
            &deps,
            "add_variable",
            &json!({ "name": "trust", "type": "bogus", "default": 0 }),
        )
        .await
        .unwrap();
        assert!(bad.is_error);

        let variable = execute_film_authoring_tool(
            &deps,
            "add_variable",
            &json!({ "name": "trust", "type": "counter", "default": 0, "desc": "信任度" }),
        )
        .await
        .unwrap();
        assert_eq!(variable.text, "Variable \"trust\" added (rev 2).");

        let ending = execute_film_authoring_tool(
            &deps,
            "define_ending",
            &json!({ "id": "end1", "nodeId": "e", "title": "真相", "type": "good", "description": "揭开真相" }),
        )
        .await
        .unwrap();
        assert_eq!(ending.text, "Ending \"真相\" defined (rev 3).");

        let graph = film::load_story_graph(&root, "p").await.unwrap().unwrap();
        assert_eq!(graph.variables[0].name, "trust");
        assert_eq!(graph.endings[0].title, "真相");
        assert_eq!(graph.world_anchor.as_ref().unwrap().duration_minutes, 30.0);

        // authoring-state：rev 推进 + phase 保留 world（直写件不带 phase，除锚外）。
        let state = crate::server::interactive_film_routes::load_authoring_state(&root, "p").await;
        assert_eq!(state.rev, 3);
        assert_eq!(state.phase, "world");

        std::fs::remove_dir_all(&root).ok();
    }

    #[tokio::test]
    async fn upsert_characters_writes_memory_facts() {
        let root = temp_root("chars");
        let llm = no_llm();
        let deps = deps_for(&root, &llm, "zh");

        let result = execute_film_authoring_tool(
            &deps,
            "upsert_characters",
            &json!({
                "characters": [
                    {
                        "id": "c1",
                        "name": "林夏",
                        "role": "protagonist",
                        "motivation": "找回失踪的妹妹",
                        "voiceProfile": { "speakingRhythm": "短句急促", "vocabulary": "市井口语" }
                    },
                    { "id": "c2", "name": "无名" }
                ]
            }),
        )
        .await
        .unwrap();
        assert_eq!(result.text, "Upserted 2 character(s) (rev 1).");

        let db = MemoryDb::open(root.join("interactive-films").join("p")).unwrap();
        let facts = db
            .get_facts_for_characters(&["林夏".to_string()])
            .unwrap();
        let motivations: Vec<_> = facts.iter().filter(|f| f.predicate == "motivation").collect();
        let voices: Vec<_> = facts.iter().filter(|f| f.predicate == "voice").collect();
        assert_eq!(motivations.len(), 1);
        assert_eq!(motivations[0].object, "找回失踪的妹妹");
        assert_eq!(voices.len(), 1);
        assert_eq!(voices[0].object, "短句急促 / 市井口语");
        // TS：角色缺省值 role=other / motivation=""——无名无事实。
        let none = db.get_facts_for_characters(&["无名".to_string()]).unwrap();
        assert!(none.is_empty());

        let graph = film::load_story_graph(&root, "p").await.unwrap().unwrap();
        assert_eq!(graph.characters[1].role, film::CharacterRole::Other);
        assert_eq!(graph.characters[1].motivation, "");

        std::fs::remove_dir_all(&root).ok();
    }

    struct ScriptedLLM {
        calls: Mutex<Vec<(String, String)>>,
        response: String,
    }

    #[async_trait::async_trait]
    impl FilmAuthoringLLM for ScriptedLLM {
        async fn chat(
            &self,
            _agent: &str,
            messages: Vec<LLMMessage>,
            _temperature: f64,
            _max_tokens: Option<u32>,
        ) -> Result<String, String> {
            let system = messages
                .iter()
                .find(|m| matches!(m.role, LLMRole::System))
                .map(|m| m.content.clone())
                .unwrap_or_default();
            let user = messages
                .iter()
                .filter(|m| matches!(m.role, LLMRole::User))
                .map(|m| m.content.clone())
                .collect::<Vec<_>>()
                .join("\n---\n");
            self.calls.lock().unwrap().push((system, user));
            Ok(self.response.clone())
        }
    }

    #[tokio::test]
    async fn fill_node_zh_default_en_switch_and_apply() {
        let root = temp_root("fill");
        save_graph(&root, &sample_graph());
        let response = serde_json::json!({
            "id": "ignored",
            "type": "branch",
            "title": "雨夜抉择",
            "sceneDesc": "天台对峙",
            "dialogue": [{ "speaker": "林夏", "text": "你到底是谁", "emotion": "紧张" }],
            "choices": [{ "id": "ch1", "text": "追问", "targetNodeId": "e" }]
        })
        .to_string();
        let llm = ScriptedLLM { calls: Mutex::new(Vec::new()), response };
        let deps = deps_for(&root, &llm, "zh");

        let result = execute_film_authoring_tool(
            &deps,
            "fill_node",
            &json!({ "nodeId": "n1", "instruction": "写抉择场景" }),
        )
        .await
        .unwrap();
        assert_eq!(result.text, "Node n1 filled (rev 1).");
        let details = result.details.unwrap();
        assert_eq!(details["promptPacks"], json!(["interactive-film.script"]));
        assert_eq!(details["skillIds"], json!([]));

        let (system, user) = llm.calls.lock().unwrap()[0].clone();
        assert!(system.contains("你是互动影游编剧"), "zh system: {system}");
        assert!(user.contains("要填的节点 id：n1"));
        assert!(user.contains("指令：写抉择场景"));
        // 上下文注入：图谱摘要在提示词头部。
        assert!(user.contains("# 互动影游：T"));

        let graph = film::load_story_graph(&root, "p").await.unwrap().unwrap();
        // BTreeMap 合并按 id 序输出——按 id 查找而非下标。
        let filled = graph.nodes.iter().find(|n| n.id == "n1").unwrap();
        assert_eq!(filled.title, "雨夜抉择", "宿主强制节点 id");
        assert_eq!(filled.dialogue.len(), 1);
        // phase 推进 workshop。
        let state = crate::server::interactive_film_routes::load_authoring_state(&root, "p").await;
        assert_eq!(state.phase, "workshop");

        // en 面提示词切换。
        let deps_en = deps_for(&root, &llm, "en");
        let result = execute_film_authoring_tool(
            &deps_en,
            "fill_node",
            &json!({ "nodeId": "n1", "instruction": "Write the decision scene" }),
        )
        .await
        .unwrap();
        assert_eq!(result.text, "Node n1 filled (rev 2).");
        let (system, user) = llm.calls.lock().unwrap()[1].clone();
        assert!(system.contains("You are an interactive film scriptwriter"));
        assert!(!system.contains("你是互动影游编剧"));
        assert!(user.contains("Node id to fill: n1"));
        assert!(user.contains("Instruction: Write the decision scene"));

        std::fs::remove_dir_all(&root).ok();
    }

    #[tokio::test]
    async fn revise_node_includes_current_content_and_error_paths() {
        let root = temp_root("revise");
        save_graph(&root, &sample_graph());
        let llm = ScriptedLLM {
            calls: Mutex::new(Vec::new()),
            response: serde_json::json!({ "id": "x", "type": "branch", "title": "改" }).to_string(),
        };
        let deps = deps_for(&root, &llm, "zh");

        let result = execute_film_authoring_tool(
            &deps,
            "revise_node",
            &json!({ "nodeId": "n1", "instruction": "收紧对白" }),
        )
        .await
        .unwrap();
        assert_eq!(result.text, "Node n1 revised (rev 1).");
        let (system, user) = llm.calls.lock().unwrap()[0].clone();
        assert!(system.contains("你是互动影游编剧"));
        assert!(user.contains("要修改的节点 id：n1"));
        assert!(user.contains("现有内容："));
        assert!(user.contains("修改指令：收紧对白"));

        // LLM 无 JSON → 错误结果。
        let llm_bad = ScriptedLLM { calls: Mutex::new(Vec::new()), response: "纯散文回复".to_string() };
        let deps_bad = deps_for(&root, &llm_bad, "zh");
        let result = execute_film_authoring_tool(
            &deps_bad,
            "fill_node",
            &json!({ "nodeId": "n1", "instruction": "写" }),
        )
        .await
        .unwrap();
        assert!(result.is_error);
        assert!(result.text.contains("fill_node: LLM returned no JSON"));

        // LLM 坏 JSON 结构 → invalid node。
        let llm_invalid = ScriptedLLM { calls: Mutex::new(Vec::new()), response: "{\"id\":1}".to_string() };
        let deps_invalid = deps_for(&root, &llm_invalid, "zh");
        let result = execute_film_authoring_tool(
            &deps_invalid,
            "revise_node",
            &json!({ "nodeId": "n1", "instruction": "改" }),
        )
        .await
        .unwrap();
        assert!(result.is_error);
        assert!(result.text.contains("revise_node: invalid node:"));

        std::fs::remove_dir_all(&root).ok();
    }

    #[tokio::test]
    async fn generate_node_image_error_paths() {
        let root = temp_root("image");
        let llm = no_llm();
        let deps = deps_for(&root, &llm, "zh");

        // 图谱缺失。
        let result = execute_film_authoring_tool(
            &deps,
            "generate_node_image",
            &json!({ "nodeId": "n1" }),
        )
        .await
        .unwrap();
        assert!(result.is_error);
        assert_eq!(
            result.text,
            "interactive-film project p has no story graph"
        );

        // 节点缺失。
        save_graph(&root, &sample_graph());
        let result = execute_film_authoring_tool(
            &deps,
            "generate_node_image",
            &json!({ "nodeId": "missing" }),
        )
        .await
        .unwrap();
        assert!(result.is_error);
        assert_eq!(result.text, "node missing not found");

        // 无 prompt 无 sceneDesc。
        let result = execute_film_authoring_tool(
            &deps,
            "generate_node_image",
            &json!({ "nodeId": "n1" }),
        )
        .await
        .unwrap();
        assert!(result.is_error);
        assert_eq!(
            result.text,
            "node n1 has no imageSlot.prompt or sceneDesc to generate an image from"
        );

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn context_message_shape() {
        let graph = sample_graph();
        let message = film_graph_context_message(&graph);
        assert!(matches!(message.role, LLMRole::User));
        assert!(message
            .content
            .starts_with("[以下是当前互动影游的完整权威剧情图谱"));
        assert!(message.content.contains("\"projectId\":\"p\""));
    }

    #[test]
    fn schemas_cover_unconfirmed_surface() {
        let names: Vec<String> = film_authoring_tool_schemas()
            .into_iter()
            .map(|schema| {
                schema["function"]["name"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string()
            })
            .collect();
        assert_eq!(
            names,
            vec![
                "set_world_anchor",
                "upsert_characters",
                "add_variable",
                "define_ending",
                "fill_node",
                "revise_node",
                "generate_node_image",
            ]
        );
    }
}
