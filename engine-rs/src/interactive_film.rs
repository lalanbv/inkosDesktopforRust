//! interactive-film 域本体（69 号）——故事图谱 schema / 存储 / delta / 校验 /
//! 路径枚举 / 情感分析 / 导出（ink / html / tar.gz）。
//!
//! 移植自 `packages/core/src/interactive-film/`（16 模块 1203 行）的消费面子集：
//! - graph-schema.ts → serde 结构（zod default 逐字段对齐；VarValue 联合用
//!   自定义反序列化拒绝 null/数组/对象）
//! - graph-store.ts → `load_story_graph` / `save_story_graph`
//! - delta.ts → `apply_story_graph_delta`（upsert/remove + worldAnchor 合并 +
//!   ending 引用完整性）
//! - evaluator.ts → 条件求值 / 效果应用 / 可见选项 / 变量初值
//! - paths.ts → `enumerate_runtime_paths`（DFS + node+varState 去环，200/50 上限）
//! - emotion.ts → 情感词典（含否定翻转）+ 弧线 + 分布
//! - validation.ts → `validate`/`review`（14 规则全量）
//! - export-ink.ts / export-html.ts → `export_ink` / `build_playable_html`
//! - authoring-store.ts 的 rev/snapshot/锁面由 server 层（路由文件）装配
//!
//! authoring-generate / authoring-tools / film-context / memory-link / generate
//! （film-authoring LLM 链）与 node-image 生图执行链随后续号接线（偏差备案）。

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

// ── VarValue（z.number() | z.string() | z.boolean()） ────────────

fn deserialize_var_value<'de, D>(deserializer: D) -> Result<Value, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    match &value {
        Value::Number(_) | Value::String(_) | Value::Bool(_) => Ok(value),
        _ => Err(serde::de::Error::custom("var value must be number, string or boolean")),
    }
}

// ── 图谱 schema（graph-schema.ts 逐字段） ────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CondOp {
    #[serde(rename = ">=")]
    Ge,
    #[serde(rename = "<=")]
    Le,
    #[serde(rename = ">")]
    Gt,
    #[serde(rename = "<")]
    Lt,
    #[serde(rename = "==")]
    Eq,
    #[serde(rename = "!=")]
    Ne,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Condition {
    pub var: String,
    pub op: CondOp,
    #[serde(deserialize_with = "deserialize_var_value")]
    pub value: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EffectOp {
    Set,
    Add,
    Sub,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Effect {
    pub var: String,
    pub op: EffectOp,
    #[serde(deserialize_with = "deserialize_var_value")]
    pub value: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChoiceWeight {
    Light,
    Heavy,
    Critical,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Choice {
    pub id: String,
    pub text: String,
    #[serde(rename = "targetNodeId")]
    pub target_node_id: String,
    pub condition: Option<Condition>,
    #[serde(default)]
    pub effects: Vec<Effect>,
    pub weight: Option<ChoiceWeight>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DialogueLine {
    pub speaker: String,
    pub text: String,
    #[serde(default)]
    pub emotion: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageSlot {
    #[serde(default)]
    pub prompt: String,
    pub asset_ref: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeType {
    Start,
    Normal,
    Branch,
    Merge,
    Ending,
    Explore,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceProfile {
    #[serde(default)]
    pub speaking_rhythm: String,
    #[serde(default)]
    pub vocabulary: String,
    #[serde(default)]
    pub sample_lines: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum CharacterRole {
    Protagonist,
    Antagonist,
    Support,
    #[default]
    Other,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Character {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub role: CharacterRole,
    #[serde(default)]
    pub motivation: String,
    pub voice_profile: Option<VoiceProfile>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorldAnchor {
    #[serde(default)]
    pub story_core: String,
    #[serde(default)]
    pub theme: String,
    #[serde(default)]
    pub genre: String,
    #[serde(default)]
    pub world_rules: String,
    #[serde(default)]
    pub duration_minutes: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Position {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoryNode {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(rename = "type")]
    pub node_type: NodeType,
    #[serde(default)]
    pub scene_desc: String,
    #[serde(default)]
    pub dialogue: Vec<DialogueLine>,
    #[serde(default)]
    pub choices: Vec<Choice>,
    pub image_slot: Option<ImageSlot>,
    #[serde(default)]
    pub act: String,
    pub position: Option<Position>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VariableType {
    Flag,
    Counter,
    Relationship,
    Item,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Variable {
    pub name: String,
    #[serde(rename = "type")]
    pub variable_type: VariableType,
    #[serde(deserialize_with = "deserialize_var_value")]
    pub default: Value,
    #[serde(default)]
    pub desc: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EndingType {
    Good,
    Bad,
    Neutral,
    Secret,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ending {
    pub id: String,
    #[serde(rename = "nodeId")]
    pub node_id: String,
    pub title: String,
    #[serde(rename = "type")]
    pub ending_type: EndingType,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoryGraph {
    pub schema_version: u32,
    pub project_id: String,
    pub title: String,
    pub world_anchor: Option<WorldAnchor>,
    #[serde(default)]
    pub characters: Vec<Character>,
    #[serde(default)]
    pub variables: Vec<Variable>,
    #[serde(default)]
    pub nodes: Vec<StoryNode>,
    #[serde(default)]
    pub endings: Vec<Ending>,
}

impl StoryGraph {
    /// zod `schemaVersion: z.literal(1)` 的解析后校验。
    pub fn validate_schema_version(&self) -> Result<(), String> {
        if self.schema_version == 1 {
            Ok(())
        } else {
            Err(format!(
                "Invalid literal value, expected 1 at schemaVersion (got {})",
                self.schema_version
            ))
        }
    }
}

/// NodeType / CharacterRole 的 serde 串（作者上下文摘要用）。
pub fn node_type_str(node_type: NodeType) -> &'static str {
    match node_type {
        NodeType::Start => "start",
        NodeType::Normal => "normal",
        NodeType::Branch => "branch",
        NodeType::Merge => "merge",
        NodeType::Ending => "ending",
        NodeType::Explore => "explore",
    }
}

pub fn character_role_str(role: CharacterRole) -> &'static str {
    match role {
        CharacterRole::Protagonist => "protagonist",
        CharacterRole::Antagonist => "antagonist",
        CharacterRole::Support => "support",
        CharacterRole::Other => "other",
    }
}

pub fn empty_graph(project_id: &str) -> StoryGraph {
    StoryGraph {
        schema_version: 1,
        project_id: project_id.to_string(),
        title: String::new(),
        world_anchor: None,
        characters: Vec::new(),
        variables: Vec::new(),
        nodes: Vec::new(),
        endings: Vec::new(),
    }
}

// ── graph-store（graph-store.ts） ────────────────────────────────

pub fn story_graph_path(project_root: &Path, project_id: &str) -> std::path::PathBuf {
    project_root
        .join("interactive-films")
        .join(project_id)
        .join("story-graph.json")
}

/// `loadStoryGraph`：缺失 → None；坏 JSON/zod 失败 → Err（端点 500 / 列表 skip）。
pub async fn load_story_graph(
    project_root: &Path,
    project_id: &str,
) -> Result<Option<StoryGraph>, String> {
    let raw = match tokio::fs::read_to_string(story_graph_path(project_root, project_id)).await {
        Ok(raw) => raw,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    let value: Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    let graph: StoryGraph = serde_json::from_value(value).map_err(|e| e.to_string())?;
    graph.validate_schema_version()?;
    Ok(Some(graph))
}

/// `saveStoryGraph`：保存前再走一次校验（serde 结构已保证字段形态）。
pub async fn save_story_graph(
    project_root: &Path,
    project_id: &str,
    graph: &StoryGraph,
) -> Result<(), String> {
    graph.validate_schema_version()?;
    let path = story_graph_path(project_root, project_id);
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| e.to_string())?;
    }
    let mut serialized = serde_json::to_string_pretty(graph).map_err(|e| e.to_string())?;
    serialized.push('\n');
    tokio::fs::write(&path, serialized).await.map_err(|e| e.to_string())
}

// ── evaluator（evaluator.ts） ────────────────────────────────────

pub type VarState = BTreeMap<String, Value>;

fn value_as_f64(value: &Value) -> f64 {
    value.as_f64().unwrap_or(f64::NAN)
}

pub fn evaluate_condition(condition: Option<&Condition>, vars: &VarState) -> bool {
    let Some(condition) = condition else {
        return true;
    };
    let lhs = vars.get(&condition.var).cloned().unwrap_or(Value::Null);
    let rhs = &condition.value;
    match condition.op {
        CondOp::Eq => lhs == *rhs,
        CondOp::Ne => lhs != *rhs,
        CondOp::Ge => value_as_f64(&lhs) >= value_as_f64(rhs),
        CondOp::Le => value_as_f64(&lhs) <= value_as_f64(rhs),
        CondOp::Gt => value_as_f64(&lhs) > value_as_f64(rhs),
        CondOp::Lt => value_as_f64(&lhs) < value_as_f64(rhs),
    }
}

pub fn apply_effects(vars: &VarState, effects: &[Effect]) -> VarState {
    if effects.is_empty() {
        return vars.clone();
    }
    let mut next = vars.clone();
    for effect in effects {
        match effect.op {
            EffectOp::Set => {
                next.insert(effect.var.clone(), effect.value.clone());
            }
            EffectOp::Add | EffectOp::Sub => {
                let cur = next.get(&effect.var).and_then(Value::as_f64).unwrap_or(0.0);
                let delta = value_as_f64(&effect.value);
                let applied = if effect.op == EffectOp::Add {
                    cur + delta
                } else {
                    cur - delta
                };
                next.insert(effect.var.clone(), json!(applied));
            }
        }
    }
    next
}

pub fn visible_choices<'n>(node: &'n StoryNode, vars: &VarState) -> Vec<&'n Choice> {
    node.choices
        .iter()
        .filter(|choice| evaluate_condition(choice.condition.as_ref(), vars))
        .collect()
}

pub fn init_var_state(variables: &[Variable]) -> VarState {
    variables
        .iter()
        .map(|variable| (variable.name.clone(), variable.default.clone()))
        .collect()
}

// ── delta（delta.ts） ────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpsertRemove<T> {
    #[serde(default = "Vec::new")]
    pub upsert: Vec<T>,
    #[serde(default = "Vec::new")]
    pub remove: Vec<String>,
}

/// worldAnchor 的 delta：partial（所有字段可选）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WorldAnchorDelta {
    pub story_core: Option<String>,
    pub theme: Option<String>,
    pub genre: Option<String>,
    pub world_rules: Option<String>,
    pub duration_minutes: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoryGraphDelta {
    pub world_anchor: Option<WorldAnchorDelta>,
    pub characters: Option<UpsertRemove<Character>>,
    pub nodes: Option<UpsertRemove<StoryNode>>,
    pub variables: Option<UpsertRemove<Variable>>,
    pub endings: Option<UpsertRemove<Ending>>,
    #[serde(default)]
    pub notes: Vec<String>,
}

// ---- delta builders（TS authoring-tools.ts 逐字，252 号） ----

pub fn build_world_anchor_delta(patch: WorldAnchorDelta) -> StoryGraphDelta {
    StoryGraphDelta {
        world_anchor: Some(patch),
        characters: None,
        nodes: None,
        variables: None,
        endings: None,
        notes: Vec::new(),
    }
}

pub fn build_add_variable_delta(v: Variable) -> StoryGraphDelta {
    StoryGraphDelta {
        world_anchor: None,
        characters: None,
        nodes: None,
        variables: Some(UpsertRemove { upsert: vec![v], remove: Vec::new() }),
        endings: None,
        notes: Vec::new(),
    }
}

pub fn build_define_ending_delta(e: Ending) -> StoryGraphDelta {
    StoryGraphDelta {
        world_anchor: None,
        characters: None,
        nodes: None,
        variables: None,
        endings: Some(UpsertRemove { upsert: vec![e], remove: Vec::new() }),
        notes: Vec::new(),
    }
}

pub fn build_remove_node_delta(node_id: &str) -> StoryGraphDelta {
    StoryGraphDelta {
        world_anchor: None,
        characters: None,
        nodes: Some(UpsertRemove { upsert: Vec::new(), remove: vec![node_id.to_string()] }),
        variables: None,
        endings: None,
        notes: Vec::new(),
    }
}

pub fn build_connect_choice_delta(node: StoryNode) -> StoryGraphDelta {
    StoryGraphDelta {
        world_anchor: None,
        characters: None,
        nodes: Some(UpsertRemove { upsert: vec![node], remove: Vec::new() }),
        variables: None,
        endings: None,
        notes: Vec::new(),
    }
}

pub fn build_upsert_characters_delta(chars: Vec<Character>) -> StoryGraphDelta {
    StoryGraphDelta {
        world_anchor: None,
        characters: Some(UpsertRemove { upsert: chars, remove: Vec::new() }),
        nodes: None,
        variables: None,
        endings: None,
        notes: Vec::new(),
    }
}


fn apply_upsert_remove<T>(
    current: &[T],
    ops: Option<&UpsertRemove<T>>,
    key: impl Fn(&T) -> &str,
) -> Vec<T>
where
    T: Clone,
{
    let Some(ops) = ops else {
        return current.to_vec();
    };
    // BTreeMap：键序即输出序（TS Map 插入序近似——先删后插等价语义下，
    // 与 TS 的差集仅在新项插到中段时的相对顺序，前端按 id 索引无感）。
    let mut map: std::collections::BTreeMap<String, T> = current
        .iter()
        .map(|item| (key(item).to_string(), item.clone()))
        .collect();
    for id in &ops.remove {
        map.remove(id);
    }
    for item in &ops.upsert {
        map.insert(key(item).to_string(), item.clone());
    }
    map.into_values().collect()
}

/// `applyStoryGraphDelta`：upsert/remove 合并 + worldAnchor 浅合并 + ending
/// 引用完整性（指向缺失节点 → Err）。
pub fn apply_story_graph_delta(
    graph: &StoryGraph,
    delta: &StoryGraphDelta,
) -> Result<StoryGraph, String> {
    graph.validate_schema_version()?;

    let world_anchor = if let Some(delta_anchor) = &delta.world_anchor {
        let mut merged = graph.world_anchor.clone().unwrap_or_default();
        if let Some(v) = &delta_anchor.story_core {
            merged.story_core = v.clone();
        }
        if let Some(v) = &delta_anchor.theme {
            merged.theme = v.clone();
        }
        if let Some(v) = &delta_anchor.genre {
            merged.genre = v.clone();
        }
        if let Some(v) = &delta_anchor.world_rules {
            merged.world_rules = v.clone();
        }
        if let Some(v) = delta_anchor.duration_minutes {
            merged.duration_minutes = v;
        }
        Some(merged)
    } else {
        graph.world_anchor.clone()
    };

    let next = StoryGraph {
        schema_version: graph.schema_version,
        project_id: graph.project_id.clone(),
        title: graph.title.clone(),
        world_anchor,
        characters: apply_upsert_remove(&graph.characters, delta.characters.as_ref(), |c| &c.id),
        nodes: apply_upsert_remove(&graph.nodes, delta.nodes.as_ref(), |n| &n.id),
        variables: apply_upsert_remove(&graph.variables, delta.variables.as_ref(), |v| &v.name),
        endings: apply_upsert_remove(&graph.endings, delta.endings.as_ref(), |e| &e.id),
    };

    let node_ids: HashSet<&str> = next.nodes.iter().map(|n| n.id.as_str()).collect();
    for ending in &next.endings {
        if !node_ids.contains(ending.node_id.as_str()) {
            return Err(format!(
                "ending {} references missing node {}",
                ending.id, ending.node_id
            ));
        }
    }
    Ok(next)
}

// ── paths（paths.ts） ────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimePath {
    pub node_ids: Vec<String>,
    pub ending_id: Option<String>,
    pub length: usize,
}

const DEFAULT_MAX_PATHS: usize = 200;
const DEFAULT_MAX_DEPTH: usize = 50;

fn var_state_key(vars: &VarState) -> String {
    vars.iter()
        .map(|(key, value)| format!("{key}:{}", serde_json::to_string(value).unwrap_or_default()))
        .collect::<Vec<_>>()
        .join("|")
}

/// `enumerateRuntimePaths`：DFS + (node, varState) 去环；ending / 无可见选项
/// 死端都记为终态路径。
pub fn enumerate_runtime_paths(graph: &StoryGraph) -> (Vec<RuntimePath>, bool) {
    let node_by_id: HashMap<&str, &StoryNode> =
        graph.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let ending_by_node_id: HashMap<&str, &str> = graph
        .endings
        .iter()
        .map(|e| (e.node_id.as_str(), e.id.as_str()))
        .collect();
    let Some(start) = graph.nodes.iter().find(|n| n.node_type == NodeType::Start) else {
        return (Vec::new(), false);
    };

    #[allow(clippy::too_many_arguments)]
    fn walk(
        node_id: &str,
        vars: &VarState,
        trail: &[String],
        on_path: &HashSet<String>,
        depth: usize,
        node_by_id: &HashMap<&str, &StoryNode>,
        ending_by_node_id: &HashMap<&str, &str>,
        paths: &mut Vec<RuntimePath>,
        truncated: &mut bool,
    ) {
        if paths.len() >= DEFAULT_MAX_PATHS {
            *truncated = true;
            return;
        }
        if depth > DEFAULT_MAX_DEPTH {
            *truncated = true;
            return;
        }
        let visit_key = format!("{node_id}\u{0}{}", var_state_key(vars));
        if on_path.contains(&visit_key) {
            return;
        }
        let Some(node) = node_by_id.get(node_id) else {
            return;
        };
        let mut next_trail = trail.to_vec();
        next_trail.push(node_id.to_string());
        if node.node_type == NodeType::Ending {
            let length = next_trail.len();
            paths.push(RuntimePath {
                node_ids: next_trail,
                ending_id: ending_by_node_id.get(node_id).map(|id| id.to_string()),
                length,
            });
            return;
        }
        let choices = visible_choices(node, vars);
        if choices.is_empty() {
            let length = next_trail.len();
            paths.push(RuntimePath {
                node_ids: next_trail,
                ending_id: None,
                length,
            });
            return;
        }
        let mut next_on_path = on_path.clone();
        next_on_path.insert(visit_key);
        for choice in choices {
            if paths.len() >= DEFAULT_MAX_PATHS {
                *truncated = true;
                return;
            }
            let next_vars = apply_effects(vars, &choice.effects);
            walk(
                &choice.target_node_id,
                &next_vars,
                &next_trail,
                &next_on_path,
                depth + 1,
                node_by_id,
                ending_by_node_id,
                paths,
                truncated,
            );
        }
    }

    let initial = init_var_state(&graph.variables);
    let mut paths = Vec::new();
    let mut truncated = false;
    walk(
        &start.id,
        &initial,
        &[],
        &HashSet::new(),
        0,
        &node_by_id,
        &ending_by_node_id,
        &mut paths,
        &mut truncated,
    );
    (paths, truncated)
}

// ── emotion（emotion.ts） ────────────────────────────────────────

/// 情感词典（[-1,1] 效价；逐字对齐）。
fn emotion_lexicon(word: &str) -> Option<f64> {
    Some(match word {
        "喜悦" => 0.9,
        "高兴" => 0.8,
        "快乐" => 0.8,
        "希望" => 0.6,
        "坚定" => 0.5,
        "温暖" => 0.6,
        "感动" => 0.6,
        "释然" => 0.4,
        "平静" => 0.0,
        "中性" => 0.0,
        "紧张" => -0.4,
        "焦虑" => -0.5,
        "愤怒" => -0.6,
        "恐惧" => -0.7,
        "悲伤" => -0.8,
        "绝望" => -0.95,
        "痛苦" => -0.8,
        "失落" => -0.5,
        "犹豫" => -0.2,
        "冷漠" => -0.3,
        _ => return None,
    })
}

const LEXICON_KEYS: &[&str] = &[
    "喜悦", "高兴", "快乐", "希望", "坚定", "温暖", "感动", "释然", "平静", "中性",
    "紧张", "焦虑", "愤怒", "恐惧", "悲伤", "绝望", "痛苦", "失落", "犹豫", "冷漠",
];

/// `emotionScore`：全词命中 → 词典值；包含命中 → 否定字符（不没无别未）翻转。
pub fn emotion_score(word: &str) -> f64 {
    let trimmed = word.trim();
    if trimmed.is_empty() {
        return 0.0;
    }
    if let Some(score) = emotion_lexicon(trimmed) {
        return score;
    }
    for key in LEXICON_KEYS {
        if let Some(key_idx) = trimmed.find(key) {
            // key 前一个字符（char 边界）为否定字符 → 翻转效价。
            let char_before = trimmed[..key_idx].chars().next_back().unwrap_or('\0');
            let score = emotion_lexicon(key).unwrap_or(0.0);
            return if "不没无别未".contains(char_before) {
                -score
            } else {
                score
            };
        }
    }
    0.0
}

pub fn node_emotion(node: &StoryNode) -> f64 {
    if node.dialogue.is_empty() {
        return 0.0;
    }
    let sum: f64 = node
        .dialogue
        .iter()
        .map(|line| emotion_score(&line.emotion))
        .sum();
    sum / node.dialogue.len() as f64
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmotionalArc {
    pub ending_id: Option<String>,
    pub points: Vec<ArcPoint>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArcPoint {
    pub node_id: String,
    pub score: f64,
}

/// `analyzeEmotionalArcs`：每条运行路径逐节点情感分曲线。
pub fn analyze_emotional_arcs(graph: &StoryGraph) -> (Vec<EmotionalArc>, bool) {
    let (paths, truncated) = enumerate_runtime_paths(graph);
    let node_by_id: HashMap<&str, &StoryNode> =
        graph.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let arcs = paths
        .iter()
        .map(|path| EmotionalArc {
            ending_id: path.ending_id.clone(),
            points: path
                .node_ids
                .iter()
                .map(|id| ArcPoint {
                    node_id: id.clone(),
                    score: node_by_id.get(id.as_str()).map(|n| node_emotion(n)).unwrap_or(0.0),
                })
                .collect(),
        })
        .collect();
    (arcs, truncated)
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PathDistribution {
    pub total: usize,
    pub truncated: bool,
    pub by_ending: BTreeMap<String, usize>,
    pub length_histogram: BTreeMap<usize, usize>,
}

/// `analyzePathDistribution`：按结局与路径长度的分布。
pub fn analyze_path_distribution(graph: &StoryGraph) -> PathDistribution {
    let (paths, truncated) = enumerate_runtime_paths(graph);
    let mut by_ending: BTreeMap<String, usize> = BTreeMap::new();
    let mut length_histogram: BTreeMap<usize, usize> = BTreeMap::new();
    for path in &paths {
        let key = path.ending_id.clone().unwrap_or_else(|| "(dead-end)".to_string());
        *by_ending.entry(key).or_insert(0) += 1;
        *length_histogram.entry(path.length).or_insert(0) += 1;
    }
    PathDistribution {
        total: paths.len(),
        truncated,
        by_ending,
        length_histogram,
    }
}

// ── validation（validation.ts：14 规则） ─────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ValidationIssue {
    pub code: &'static str,
    pub level: &'static str,
    pub message: String,
    pub node_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ValidationReport {
    pub ok: bool,
    pub issues: Vec<ValidationIssue>,
}

fn node_label(node: &StoryNode) -> &str {
    if node.title.is_empty() {
        &node.id
    } else {
        &node.title
    }
}

/// `validateStoryGraph`：结构错误四规则（BROKEN_LINK / DEAD_END / UNREACHABLE /
/// NO_PATH_TO_ENDING）。
pub fn validate_story_graph(graph: &StoryGraph) -> ValidationReport {
    let mut issues = Vec::new();
    let ids: HashSet<&str> = graph.nodes.iter().map(|n| n.id.as_str()).collect();
    let node_map: HashMap<&str, &StoryNode> =
        graph.nodes.iter().map(|n| (n.id.as_str(), n)).collect();

    for node in &graph.nodes {
        for choice in &node.choices {
            if !ids.contains(choice.target_node_id.as_str()) {
                issues.push(ValidationIssue {
                    code: "BROKEN_LINK",
                    level: "error",
                    message: format!(
                        "节点「{}」的选项「{}」指向不存在的节点 {}",
                        node_label(node), choice.text, choice.target_node_id
                    ),
                    node_ids: vec![node.id.clone()],
                });
            }
        }
    }

    for node in &graph.nodes {
        if node.node_type == NodeType::Ending {
            continue;
        }
        let has_exit = node.choices.iter().any(|c| ids.contains(c.target_node_id.as_str()));
        if !has_exit {
            issues.push(ValidationIssue {
                code: "DEAD_END",
                level: "error",
                message: format!("节点「{}」是死路：没有任何有效出口", node_label(node)),
                node_ids: vec![node.id.clone()],
            });
        }
    }

    let start = graph
        .nodes
        .iter()
        .find(|n| n.node_type == NodeType::Start)
        .or_else(|| graph.nodes.first());
    let mut reachable: HashSet<String> = HashSet::new();
    if let Some(start) = start {
        let mut queue = std::collections::VecDeque::from(vec![start.id.clone()]);
        while let Some(cur) = queue.pop_front() {
            if reachable.contains(&cur) {
                continue;
            }
            reachable.insert(cur.clone());
            let Some(node) = node_map.get(cur.as_str()) else {
                continue;
            };
            for choice in &node.choices {
                if ids.contains(choice.target_node_id.as_str())
                    && !reachable.contains(&choice.target_node_id)
                {
                    queue.push_back(choice.target_node_id.clone());
                }
            }
        }
    }

    for node in &graph.nodes {
        if graph.nodes.len() > 1 && !reachable.contains(&node.id) {
            issues.push(ValidationIssue {
                code: "UNREACHABLE",
                level: "warning",
                message: format!("节点「{}」从开场无法到达", node_label(node)),
                node_ids: vec![node.id.clone()],
            });
        }
    }

    if let Some(start) = start {
        let can_end = reachable
            .iter()
            .any(|id| node_map.get(id.as_str()).is_some_and(|n| n.node_type == NodeType::Ending));
        if !can_end {
            issues.push(ValidationIssue {
                code: "NO_PATH_TO_ENDING",
                level: "error",
                message: format!("从开场节点「{}」出发无法到达任何结局", node_label(start)),
                node_ids: vec![start.id.clone()],
            });
        }
    }

    let ok = !issues.iter().any(|issue| issue.level == "error");
    ValidationReport { ok, issues }
}

/// `reviewStoryGraph`：validate + 评审规则十项（VARIABLE_UNWRITTEN /
/// VARIABLE_UNUSED / ENDING_VARIETY / IMAGE_MISSING / GATED_UNREACHABLE /
/// ENDING_UNREACHABLE / LINEAR_GRAPH / ISOLATED_NODE / ILLUSORY_BRANCH /
/// LONG_LINEAR_CHAIN）。
pub fn review_story_graph(graph: &StoryGraph) -> ValidationReport {
    let mut issues = validate_story_graph(graph).issues;

    let mut reads: HashSet<&str> = HashSet::new();
    let mut writes: HashSet<&str> = HashSet::new();
    for node in &graph.nodes {
        for choice in &node.choices {
            if let Some(condition) = &choice.condition {
                reads.insert(condition.var.as_str());
            }
            for effect in &choice.effects {
                writes.insert(effect.var.as_str());
            }
        }
    }
    for var in &reads {
        if !writes.contains(var) {
            issues.push(ValidationIssue {
                code: "VARIABLE_UNWRITTEN",
                level: "warning",
                message: format!(
                    "变量「{var}」被选项条件读取，但没有任何选项写入它——该条件门除默认值外永远不会改变"
                ),
                node_ids: Vec::new(),
            });
        }
    }
    for variable in &graph.variables {
        if !reads.contains(variable.name.as_str()) && !writes.contains(variable.name.as_str()) {
            issues.push(ValidationIssue {
                code: "VARIABLE_UNUSED",
                level: "info",
                message: format!(
                    "变量「{}」声明了但没有任何选项写入、也没有任何条件读取它——这是个多余的声明",
                    variable.name
                ),
                node_ids: Vec::new(),
            });
        }
    }

    if graph.endings.len() >= 2 {
        let types: HashSet<&str> = graph
            .endings
            .iter()
            .map(|e| match e.ending_type {
                EndingType::Good => "good",
                EndingType::Bad => "bad",
                EndingType::Neutral => "neutral",
                EndingType::Secret => "secret",
            })
            .collect();
        if types.len() == 1 {
            let only = types.into_iter().next().unwrap_or("");
            issues.push(ValidationIssue {
                code: "ENDING_VARIETY",
                level: "info",
                message: format!(
                    "{} 个结局都是同一类型（{only}），重玩价值低——考虑设计不同基调的结局",
                    graph.endings.len()
                ),
                node_ids: graph.endings.iter().map(|e| e.node_id.clone()).collect(),
            });
        }
    }

    for node in &graph.nodes {
        if node.node_type != NodeType::Ending
            && node.image_slot.as_ref().and_then(|slot| slot.asset_ref.as_deref()).is_none()
        {
            issues.push(ValidationIssue {
                code: "IMAGE_MISSING",
                level: "info",
                message: format!("节点「{}」还没有配图", node_label(node)),
                node_ids: vec![node.id.clone()],
            });
        }
    }

    // --- P6 规则（依赖完整路径枚举；截断时跳过不可达断言） ---
    let (paths, truncated) = enumerate_runtime_paths(graph);
    let mut reached_node_ids: HashSet<&str> = HashSet::new();
    for path in &paths {
        for id in &path.node_ids {
            reached_node_ids.insert(id.as_str());
        }
    }
    let edge_reachable = compute_edge_reachable(graph);

    if !truncated {
        for node in &graph.nodes {
            if matches!(node.node_type, NodeType::Start | NodeType::Ending) {
                continue;
            }
            if edge_reachable.contains(node.id.as_str())
                && !reached_node_ids.contains(node.id.as_str())
            {
                issues.push(ValidationIssue {
                    code: "GATED_UNREACHABLE",
                    level: "warning",
                    message: format!(
                        "节点「{}」连边可达，但没有任何满足变量条件的路径能到达——它被一个永远不成立的条件挡住了",
                        node_label(node)
                    ),
                    node_ids: vec![node.id.clone()],
                });
            }
        }
        for ending in &graph.endings {
            if !reached_node_ids.contains(ending.node_id.as_str()) {
                issues.push(ValidationIssue {
                    code: "ENDING_UNREACHABLE",
                    level: "warning",
                    message: format!("结局「{}」没有任何真实路径能到达", ending.title),
                    node_ids: vec![ending.node_id.clone()],
                });
            }
        }
    }

    let has_branch = graph.nodes.iter().any(|n| n.choices.len() >= 2);
    let has_normal = graph.nodes.iter().any(|n| n.node_type == NodeType::Normal);
    if graph.nodes.iter().any(|n| n.node_type == NodeType::Start)
        && !graph.endings.is_empty()
        && has_normal
        && !has_branch
    {
        issues.push(ValidationIssue {
            code: "LINEAR_GRAPH",
            level: "info",
            message: "整个故事没有任何分叉选择——更像线性剧本而非互动影游，考虑加入分支".to_string(),
            node_ids: Vec::new(),
        });
    }

    let mut incoming: HashSet<&str> = HashSet::new();
    for node in &graph.nodes {
        for choice in &node.choices {
            incoming.insert(choice.target_node_id.as_str());
        }
    }
    for node in &graph.nodes {
        if node.node_type != NodeType::Start
            && node.node_type != NodeType::Ending
            && !incoming.contains(node.id.as_str())
            && !issues.iter().any(|issue| {
                issue.code == "UNREACHABLE" && issue.node_ids.contains(&node.id)
            })
        {
            issues.push(ValidationIssue {
                code: "ISOLATED_NODE",
                level: "info",
                message: format!("节点「{}」没有任何选项指向它——孤立节点", node_label(node)),
                node_ids: vec![node.id.clone()],
            });
        }
    }

    for node in &graph.nodes {
        if node.choices.len() >= 2 {
            let targets: HashSet<&str> = node
                .choices
                .iter()
                .map(|c| c.target_node_id.as_str())
                .collect();
            let all_no_effect = node.choices.iter().all(|c| c.effects.is_empty());
            if targets.len() == 1 && all_no_effect {
                issues.push(ValidationIssue {
                    code: "ILLUSORY_BRANCH",
                    level: "info",
                    message: format!(
                        "节点「{}」的所有选项都通向同一个节点且没有不同效果——这是个假分支",
                        node_label(node)
                    ),
                    node_ids: vec![node.id.clone()],
                });
            }
        }
    }

    const CHAIN_THRESHOLD: usize = 5;
    for id in find_long_linear_chain_heads(graph, CHAIN_THRESHOLD) {
        issues.push(ValidationIssue {
            code: "LONG_LINEAR_CHAIN",
            level: "info",
            message: format!(
                "从节点「{id}」开始有一段较长的无分支直链（≥{CHAIN_THRESHOLD} 个单选项节点）——节奏可能偏拖，考虑插入分支或事件"
            ),
            node_ids: vec![id],
        });
    }

    let ok = !issues.iter().any(|issue| issue.level == "error");
    ValidationReport { ok, issues }
}

fn compute_edge_reachable(graph: &StoryGraph) -> HashSet<String> {
    let Some(start) = graph.nodes.iter().find(|n| n.node_type == NodeType::Start) else {
        return HashSet::new();
    };
    let node_by_id: HashMap<&str, &StoryNode> =
        graph.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let mut reachable = HashSet::new();
    let mut queue = std::collections::VecDeque::from(vec![start.id.clone()]);
    while let Some(cur) = queue.pop_front() {
        if reachable.contains(&cur) {
            continue;
        }
        reachable.insert(cur.clone());
        let Some(node) = node_by_id.get(cur.as_str()) else {
            continue;
        };
        for choice in &node.choices {
            if !reachable.contains(&choice.target_node_id) {
                queue.push_back(choice.target_node_id.clone());
            }
        }
    }
    reachable
}

fn find_long_linear_chain_heads(graph: &StoryGraph, threshold: usize) -> Vec<String> {
    let node_by_id: HashMap<&str, &StoryNode> =
        graph.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let is_chain_node = |id: &str| -> bool {
        node_by_id
            .get(id)
            .is_some_and(|n| n.node_type == NodeType::Normal && n.choices.len() == 1)
    };
    // 前驱是链节点的节点排除（链中段）。
    let mut preceded_by_chain: HashSet<&str> = HashSet::new();
    for node in &graph.nodes {
        if is_chain_node(&node.id) {
            for choice in &node.choices {
                preceded_by_chain.insert(choice.target_node_id.as_str());
            }
        }
    }
    let mut heads = Vec::new();
    for node in &graph.nodes {
        if !is_chain_node(&node.id) || preceded_by_chain.contains(node.id.as_str()) {
            continue;
        }
        let mut length = 0usize;
        let mut cur = Some(node.id.as_str());
        let mut visited: HashSet<&str> = HashSet::new();
        while let Some(id) = cur {
            if !is_chain_node(id) || visited.contains(id) {
                break;
            }
            visited.insert(id);
            length += 1;
            let chain_node = node_by_id.get(id).expect("is_chain_node 已验证");
            cur = Some(chain_node.choices[0].target_node_id.as_str());
        }
        if length >= threshold {
            heads.push(node.id.clone());
        }
    }
    heads
}

// ── export-ink（export-ink.ts） ─────────────────────────────────

fn ink_sanitize(id: &str) -> String {
    let sanitized: String = id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' })
        .collect();
    if sanitized.starts_with(|c: char| c.is_ascii_digit()) {
        format!("n_{sanitized}")
    } else {
        sanitized
    }
}

fn ink_knot(id: &str) -> String {
    format!("node_{}", ink_sanitize(id))
}

fn ink_var_value(value: &Value) -> String {
    match value {
        Value::String(text) => serde_json::to_string(text).unwrap_or_default(),
        other => other.to_string(),
    }
}

fn ink_effect_line(effect: &Effect) -> String {
    let value = ink_var_value(&effect.value);
    match effect.op {
        EffectOp::Add => format!("    ~ {} += {value}", ink_sanitize(&effect.var)),
        EffectOp::Sub => format!("    ~ {} -= {value}", ink_sanitize(&effect.var)),
        EffectOp::Set => format!("    ~ {} = {value}", ink_sanitize(&effect.var)),
    }
}

/// `exportInk`：导出 Ink 脚本（VAR 声明 + knot + 选项条件/效果 + -> END）。
pub fn export_ink(graph: &StoryGraph) -> String {
    let mut lines: Vec<String> = Vec::new();
    lines.push(format!(
        "// {} — exported from InkOS interactive film",
        if graph.title.is_empty() { &graph.project_id } else { &graph.title }
    ));
    for variable in &graph.variables {
        lines.push(format!(
            "VAR {} = {}",
            ink_sanitize(&variable.name),
            ink_var_value(&variable.default)
        ));
    }
    let start = graph
        .nodes
        .iter()
        .find(|n| n.node_type == NodeType::Start)
        .or_else(|| graph.nodes.first());
    if let Some(start) = start {
        lines.push(String::new());
        lines.push(format!("-> {}", ink_knot(&start.id)));
    }
    let ending_node_ids: HashSet<&str> = graph.endings.iter().map(|e| e.node_id.as_str()).collect();
    for node in &graph.nodes {
        lines.push(String::new());
        lines.push(format!("=== {} ===", ink_knot(&node.id)));
        if !node.title.is_empty() {
            lines.push(format!("# {}", node.title));
        }
        if !node.scene_desc.is_empty() {
            lines.push(node.scene_desc.clone());
        }
        for line in &node.dialogue {
            lines.push(format!("{}: {}", line.speaker, line.text));
        }
        if node.node_type == NodeType::Ending || ending_node_ids.contains(node.id.as_str()) {
            lines.push("-> END".to_string());
            continue;
        }
        if node.choices.is_empty() {
            lines.push("-> END".to_string());
            continue;
        }
        for choice in &node.choices {
            let cond = match &choice.condition {
                Some(condition) => format!(
                    " {{{} {} {}}}",
                    ink_sanitize(&condition.var),
                    match condition.op {
                        CondOp::Ge => ">=",
                        CondOp::Le => "<=",
                        CondOp::Gt => ">",
                        CondOp::Lt => "<",
                        CondOp::Eq => "==",
                        CondOp::Ne => "!=",
                    },
                    ink_var_value(&condition.value)
                ),
                None => String::new(),
            };
            lines.push(format!("*{cond} [{}]", choice.text));
            for effect in &choice.effects {
                lines.push(ink_effect_line(effect));
            }
            lines.push(format!("    -> {}", ink_knot(&choice.target_node_id)));
        }
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

// ── export-html（export-html.ts） ────────────────────────────────

const PLAYER_JS: &str = r#"
(function(){
  function h(s){ return String(s==null?"":s).replace(/&/g,"&amp;").replace(/</g,"&lt;").replace(/>/g,"&gt;"); }
  var vars = {};
  (GRAPH.variables||[]).forEach(function(v){ vars[v.name] = v.default; });
  var nodeById = {}; (GRAPH.nodes||[]).forEach(function(n){ nodeById[n.id] = n; });
  var endingByNode = {}; (GRAPH.endings||[]).forEach(function(e){ endingByNode[e.nodeId] = e; });
  function evalCond(c){ if(!c) return true; var v = vars[c.var];
    switch(c.op){ case ">=": return Number(v)>=Number(c.value); case "<=": return Number(v)<=Number(c.value); case ">": return Number(v)>Number(c.value); case "<": return Number(v)<Number(c.value); case "==": return v===c.value; case "!=": return v!==c.value; } return true; }
  function applyEffects(effects){ (effects||[]).forEach(function(e){
    if(e.op==="add") vars[e.var]=Number(vars[e.var]||0)+Number(e.value); else if(e.op==="sub") vars[e.var]=Number(vars[e.var]||0)-Number(e.value); else vars[e.var]=e.value; }); }
  function visible(node){ return (node.choices||[]).filter(function(c){ return evalCond(c.condition); }); }
  var root = document.getElementById("if-player");
  function hud(){ var s = Object.keys(vars).map(function(k){ return h(k)+": "+h(String(vars[k])); }).join("  ·  "); return s ? '<div class="hud">'+s+'</div>' : ''; }
  function render(node){
    if(!node){ root.innerHTML = "<p>节点缺失</p>"; return; }
    var html = hud();
    if(node.imageSlot && node.imageSlot.assetRef && ASSETS[node.imageSlot.assetRef]) html += '<img class="scene" src="'+ASSETS[node.imageSlot.assetRef]+'" alt=""/>';
    if(node.title) html += '<h2>'+h(node.title)+'</h2>';
    if(node.sceneDesc) html += '<p class="scene-desc">'+h(node.sceneDesc)+'</p>';
    (node.dialogue||[]).forEach(function(d){ html += '<p class="line"><b>'+h(d.speaker)+'：</b>'+h(d.text)+'</p>'; });
    var end = endingByNode[node.id] || node.type==="ending";
    if(end){ var e = endingByNode[node.id];
      html += '<div class="ending"><div class="ending-type">'+h(e?e.type:"ending")+'</div><div class="ending-title">'+h(e?e.title:(node.title||"结局"))+'</div></div>';
      html += '<button class="restart">重新开始</button>';
      root.innerHTML = html;
      root.querySelector(".restart").onclick = function(){ start(); };
      return;
    }
    var vis = visible(node);
    html += '<div class="choices">';
    vis.forEach(function(c,i){ html += '<button class="choice" data-i="'+i+'">'+h(c.text)+'</button>'; });
    html += '</div>';
    if(vis.length===0) html += '<p class="deadend">（没有可走的选项）</p>';
    root.innerHTML = html;
    Array.prototype.forEach.call(root.querySelectorAll(".choice"), function(btn){
      btn.onclick = function(){ var c = vis[parseInt(btn.getAttribute("data-i"),10)]; applyEffects(c.effects); render(nodeById[c.targetNodeId]); };
    });
  }
  function start(){ vars = {}; (GRAPH.variables||[]).forEach(function(v){ vars[v.name]=v.default; }); var s = (GRAPH.nodes||[]).filter(function(n){return n.type==="start";})[0] || GRAPH.nodes[0]; render(s); }
  start();
})();
"#;

const PLAYER_CSS: &str = r#"
  body{font-family:system-ui,'PingFang SC',sans-serif;background:#14110f;color:#eee;margin:0;display:flex;justify-content:center;}
  #wrap{max-width:680px;width:100%;padding:24px;}
  h1{font-size:18px;color:#caa;}
  #if-player .scene{width:100%;border-radius:10px;margin-bottom:12px;}
  #if-player h2{font-size:20px;margin:8px 0;}
  .scene-desc{color:#bbb;} .line{margin:6px 0;} .line b{color:#d8b27a;}
  .hud{font-size:12px;color:#998;border:1px solid #333;border-radius:8px;padding:6px 10px;margin-bottom:12px;display:inline-block;}
  .choices{display:flex;flex-direction:column;gap:8px;margin-top:16px;}
  .choice{text-align:left;padding:12px 16px;border:1px solid #444;border-radius:10px;background:#1d1916;color:#eee;cursor:pointer;font-size:15px;}
  .choice:hover{border-color:#caa;}
  .ending{margin-top:20px;padding:16px;border:1px solid #553;border-radius:10px;}
  .ending-type{font-size:12px;color:#caa;text-transform:uppercase;} .ending-title{font-size:22px;margin-top:4px;}
  .restart{margin-top:12px;padding:10px 18px;border-radius:8px;background:#caa;color:#14110f;border:none;cursor:pointer;}
"#;

/// `esc`：JSON 串里仅转义 `<`（防 `</script>` 逃逸；JSON.parse 读回）。
fn esc_script_json(serialized: &str) -> String {
    serialized.replace('<', "\\u003c")
}

fn esc_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// `buildPlayableHtml`：自包含可玩 HTML（内嵌图谱 JSON + 资源 data URI + 播放器）。
pub fn build_playable_html(
    graph: &StoryGraph,
    asset_data_uris: &BTreeMap<String, String>,
) -> String {
    let title = if graph.title.is_empty() {
        &graph.project_id
    } else {
        &graph.title
    };
    let graph_json = esc_script_json(&serde_json::to_string(graph).unwrap_or_default());
    let assets_json = esc_script_json(&serde_json::to_string(asset_data_uris).unwrap_or_default());
    format!(
        "<!doctype html>\n<html lang=\"zh\"><head><meta charset=\"utf-8\"/><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"/>\n<title>{}</title><style>{}</style></head>\n<body><div id=\"wrap\"><h1>{}</h1><div id=\"if-player\" data-if-player></div></div>\n<script>var GRAPH={};var ASSETS={};</script>\n<script>{}</script>\n</body></html>",
        esc_html(title),
        PLAYER_CSS,
        esc_html(title),
        graph_json,
        assets_json,
        PLAYER_JS,
    )
}

// ── node-image 路径面（node-image.ts 的安全段；生图执行链偏差备案） ──

/// `safeAssetSegment`：encodeURIComponent 严格集 + `!'()*` 补充转义。
fn safe_asset_segment(value: &str) -> String {
    crate::server::task_store::js_encode_uri_component(value)
        .replace('!', "%21")
        .replace('\'', "%27")
        .replace('(', "%28")
        .replace(')', "%29")
        .replace('*', "%2A")
}

/// `nodeImageRelPath`：GET /project/files/<this> 可服务的 posix 相对路径。
pub fn node_image_rel_path(project_id: &str, node_id: &str, ext: &str) -> String {
    format!(
        "interactive-films/{project_id}/assets/nodes/{}.{}",
        safe_asset_segment(node_id),
        ext
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str, node_type: NodeType, choices: Vec<Choice>) -> StoryNode {
        StoryNode {
            id: id.into(),
            title: String::new(),
            node_type,
            scene_desc: String::new(),
            dialogue: Vec::new(),
            choices,
            image_slot: None,
            act: String::new(),
            position: None,
        }
    }

    fn choice(id: &str, target: &str) -> Choice {
        Choice {
            id: id.into(),
            text: format!("去 {target}"),
            target_node_id: target.into(),
            condition: None,
            effects: Vec::new(),
            weight: None,
        }
    }

    /// start →（选项 a/b）→ ending；b 由条件 courage>=2 门控。
    fn sample_graph() -> StoryGraph {
        let mut gated = choice("b", "end1");
        gated.condition = Some(Condition {
            var: "courage".into(),
            op: CondOp::Ge,
            value: json!(2),
        });
        let mut brave = choice("a", "end1");
        brave.effects = vec![Effect { var: "courage".into(), op: EffectOp::Add, value: json!(1) }];
        StoryGraph {
            schema_version: 1,
            project_id: "p1".into(),
            title: "样本".into(),
            world_anchor: None,
            characters: Vec::new(),
            variables: vec![Variable {
                name: "courage".into(),
                variable_type: VariableType::Counter,
                default: json!(1),
                desc: String::new(),
            }],
            nodes: vec![
                node("start", NodeType::Start, vec![brave, gated]),
                node("end1", NodeType::Ending, Vec::new()),
            ],
            endings: vec![Ending {
                id: "e1".into(),
                node_id: "end1".into(),
                title: "结局".into(),
                ending_type: EndingType::Good,
                description: String::new(),
            }],
        }
    }

    #[test]
    fn schema_rejects_bad_var_value_and_version() {
        let bad = serde_json::from_value::<Condition>(json!({
            "var": "x", "op": ">=", "value": null
        }));
        assert!(bad.is_err());

        let mut graph = sample_graph();
        graph.schema_version = 2;
        assert!(graph.validate_schema_version().is_err());
    }

    #[test]
    fn schema_fills_zod_defaults() {
        let parsed: StoryNode =
            serde_json::from_value(json!({ "id": "n1", "type": "normal" })).unwrap();
        assert_eq!(parsed.title, "");
        assert!(parsed.dialogue.is_empty());
        assert!(parsed.choices.is_empty());
        assert_eq!(parsed.act, "");
    }

    #[test]
    fn delta_upsert_remove_and_referential_integrity() {
        let graph = sample_graph();
        let delta = serde_json::from_value::<StoryGraphDelta>(json!({
            "nodes": { "upsert": [{ "id": "extra", "type": "normal", "choices": [] }] },
            "endings": { "upsert": [{ "id": "e2", "nodeId": "ghost", "title": "悬空", "type": "bad" }] }
        }))
        .unwrap();
        let err = apply_story_graph_delta(&graph, &delta).unwrap_err();
        assert!(err.contains("ending e2 references missing node ghost"), "{err}");

        let ok_delta = serde_json::from_value::<StoryGraphDelta>(json!({
            "worldAnchor": { "theme": "救赎" },
            "endings": { "remove": ["e1"] }
        }))
        .unwrap();
        let next = apply_story_graph_delta(&graph, &ok_delta).unwrap();
        assert!(next.endings.is_empty());
        assert_eq!(next.world_anchor.as_ref().unwrap().theme, "救赎");
        assert_eq!(next.nodes.len(), 2);
    }

    #[test]
    fn condition_evaluation_matches_js_coercion() {
        let vars: VarState = [("courage".to_string(), json!(1))].into_iter().collect();
        let cond = |op: CondOp, value: Value| {
            Some(Condition { var: "courage".into(), op, value })
        };
        assert!(evaluate_condition(cond(CondOp::Eq, json!(1)).as_ref(), &vars));
        assert!(evaluate_condition(cond(CondOp::Ne, json!(2)).as_ref(), &vars));
        assert!(!evaluate_condition(cond(CondOp::Ge, json!(2)).as_ref(), &vars));
        assert!(evaluate_condition(None, &vars));
    }

    #[test]
    fn runtime_paths_enumerate_gated_branch() {
        let (paths, truncated) = enumerate_runtime_paths(&sample_graph());
        assert!(!truncated);
        // 初值 courage=1：b 门（>=2）关 → 唯一路径 start→end1。
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].node_ids, vec!["start", "end1"]);
        assert_eq!(paths[0].ending_id.as_deref(), Some("e1"));

        // 无 start → 空。
        let mut no_start = sample_graph();
        no_start.nodes[0].node_type = NodeType::Normal;
        let (empty, _) = enumerate_runtime_paths(&no_start);
        assert!(empty.is_empty());
    }

    #[test]
    fn emotion_lexicon_and_negation_flip() {
        assert!((emotion_score("喜悦") - 0.9).abs() < 1e-9);
        assert!((emotion_score("很高兴") - 0.8).abs() < 1e-9);
        assert!((emotion_score("不高兴") + 0.8).abs() < 1e-9, "否定翻转");
        assert_eq!(emotion_score("中性"), 0.0);
        assert_eq!(emotion_score(""), 0.0);

        let (arcs, _) = analyze_emotional_arcs(&sample_graph());
        assert_eq!(arcs.len(), 1);
        assert_eq!(arcs[0].points.len(), 2);

        let distribution = analyze_path_distribution(&sample_graph());
        assert_eq!(distribution.total, 1);
        assert_eq!(distribution.by_ending.get("e1"), Some(&1));
        assert_eq!(distribution.length_histogram.get(&2), Some(&1));
    }

    #[test]
    fn validation_reports_structural_and_review_issues() {
        // 完好图谱：IMAGE_MISSING 两条 info（start/end1 无配图——end 豁免，仅 start）。
        let report = review_story_graph(&sample_graph());
        assert!(report.ok, "issues: {:?}", report.issues.iter().map(|i| i.code).collect::<Vec<_>>());
        assert!(report.issues.iter().any(|i| i.code == "IMAGE_MISSING" && i.node_ids == ["start"]));

        // 断链 + 死路 + 不可达结局。
        let mut broken = sample_graph();
        broken.nodes[0].choices[0].target_node_id = "ghost".into();
        broken.nodes[1].node_type = NodeType::Normal;
        let report = review_story_graph(&broken);
        assert!(!report.ok);
        let codes: Vec<&str> = report.issues.iter().map(|i| i.code).collect();
        assert!(codes.contains(&"BROKEN_LINK"));
        assert!(codes.contains(&"NO_PATH_TO_ENDING"));

        // VARIABLE_UNWRITTEN：条件读了 courage 但无写入（去掉 a 的效果）。
        let mut unwritten = sample_graph();
        unwritten.nodes[0].choices[0].effects.clear();
        let report = review_story_graph(&unwritten);
        assert!(report.issues.iter().any(|i| i.code == "VARIABLE_UNWRITTEN"));

        // ILLUSORY_BRANCH：双选项同目标无效果。
        let mut illusory = sample_graph();
        illusory.nodes[0].choices[0].effects.clear();
        illusory.nodes[0].choices[1].condition = None;
        illusory.nodes[0].choices[1].target_node_id = "end1".into();
        let report = review_story_graph(&illusory);
        assert!(report.issues.iter().any(|i| i.code == "ILLUSORY_BRANCH"));

        // LONG_LINEAR_CHAIN：≥5 单选项 normal 链。
        let mut chain_nodes = vec![node("c0", NodeType::Start, vec![choice("g1", "c1")])];
        for i in 1..=5 {
            chain_nodes.push(node(&format!("c{i}"), NodeType::Normal, vec![choice(&format!("g{i}+1"), &format!("c{}", i + 1))]));
        }
        chain_nodes.push(node("c6", NodeType::Ending, Vec::new()));
        let chain_graph = StoryGraph {
            schema_version: 1,
            project_id: "chain".into(),
            title: String::new(),
            world_anchor: None,
            characters: Vec::new(),
            variables: Vec::new(),
            nodes: chain_nodes,
            endings: vec![Ending { id: "ce".into(), node_id: "c6".into(), title: "终".into(), ending_type: EndingType::Neutral, description: String::new() }],
        };
        let report = review_story_graph(&chain_graph);
        assert!(report.issues.iter().any(|i| i.code == "LONG_LINEAR_CHAIN"), "codes: {:?}", report.issues.iter().map(|i| i.code).collect::<Vec<_>>());
    }

    #[test]
    fn export_ink_shape() {
        let ink = export_ink(&sample_graph());
        assert!(ink.starts_with("// 样本 — exported from InkOS interactive film"));
        assert!(ink.contains("VAR courage = 1"));
        assert!(ink.contains("-> node_start"));
        assert!(ink.contains("=== node_start ==="));
        assert!(ink.contains("* [去 end1]"));
        assert!(ink.contains("~ courage += 1"));
        assert!(ink.contains("-> END"));
        assert!(ink.ends_with('\n'));
    }

    #[test]
    fn playable_html_embeds_graph_and_escapes() {
        let mut graph = sample_graph();
        graph.title = "危险</script>标题".into();
        let html = build_playable_html(&graph, &Default::default());
        assert!(html.starts_with("<!doctype html>"));
        assert!(!html.contains("</script>标题</title>"), "标题应被 HTML 转义");
        assert!(html.contains("\\u003c"), "图谱 JSON 内的 < 应转义");
        assert!(html.contains("if-player"));
    }

    #[test]
    fn node_image_rel_path_encoding() {
        assert_eq!(
            node_image_rel_path("p1", "node 1", "png"),
            "interactive-films/p1/assets/nodes/node%201.png"
        );
        assert_eq!(
            node_image_rel_path("p1", "n*'()", "jpg"),
            "interactive-films/p1/assets/nodes/n%2A%27%28%29.jpg"
        );
    }

    #[tokio::test]
    async fn graph_store_roundtrip_and_missing() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        assert!(load_story_graph(root, "nope").await.unwrap().is_none());

        save_story_graph(root, "p1", &sample_graph()).await.unwrap();
        assert!(story_graph_path(root, "p1").is_file());
        let loaded = load_story_graph(root, "p1").await.unwrap().unwrap();
        assert_eq!(loaded, sample_graph());

        // 坏 schemaVersion → Err（loadStoryGraph 的 zod 失败面）。
        let raw = story_graph_path(root, "p1");
        let mut bad: Value = serde_json::from_str(&tokio::fs::read_to_string(&raw).await.unwrap()).unwrap();
        bad["schemaVersion"] = json!(3);
        tokio::fs::write(&raw, serde_json::to_string(&bad).unwrap()).await.unwrap();
        assert!(load_story_graph(root, "p1").await.is_err());
    }
}

#[cfg(test)]
mod delta_builder_tests {
    use super::*;

    fn variable() -> Variable {
        Variable {
            name: "信任度".into(),
            variable_type: VariableType::Counter,
            default: serde_json::json!(0),
            desc: String::new(),
        }
    }

    fn ending() -> Ending {
        Ending {
            id: "e1".into(),
            node_id: "n9".into(),
            title: "真相大白".into(),
            ending_type: EndingType::Good,
            description: String::new(),
        }
    }

    fn story_node() -> StoryNode {
        StoryNode {
            id: "n1".into(),
            title: String::new(),
            node_type: NodeType::Normal,
            scene_desc: String::new(),
            dialogue: Vec::new(),
            choices: Vec::new(),
            image_slot: None,
            act: String::new(),
            position: None,
        }
    }

    /// 252 号：六类 delta builder 纯函数（TS authoring-tools.ts 逐字）。
    #[test]
    fn delta_builders_produce_single_segment_deltas() {
        let wa = build_world_anchor_delta(WorldAnchorDelta {
            story_core: Some("复仇".into()),
            ..Default::default()
        });
        assert!(wa.world_anchor.is_some());
        assert!(wa.nodes.is_none() && wa.variables.is_none() && wa.characters.is_none() && wa.endings.is_none());
        assert!(wa.notes.is_empty());

        let var_delta = build_add_variable_delta(variable());
        assert_eq!(var_delta.variables.as_ref().unwrap().upsert.len(), 1);
        assert!(var_delta.world_anchor.is_none());

        let end_delta = build_define_ending_delta(ending());
        assert_eq!(end_delta.endings.as_ref().unwrap().upsert.len(), 1);

        let rm = build_remove_node_delta("n1");
        assert_eq!(rm.nodes.as_ref().unwrap().remove, vec!["n1"]);
        assert!(rm.nodes.as_ref().unwrap().upsert.is_empty());

        let node = story_node();
        let cc = build_connect_choice_delta(node);
        assert_eq!(cc.nodes.as_ref().unwrap().upsert.len(), 1);

        let chars_delta = build_upsert_characters_delta(Vec::new());
        assert!(chars_delta.characters.as_ref().unwrap().upsert.is_empty());
    }
}
