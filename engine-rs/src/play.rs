//! play 域只读消费面（71 号）：PlayStore 路径 + 图谱快照（sqlite 优先 / json
//! 回退）+ 插图 sidecar（manifest/settings/prompt 构建）。
//!
//! 移植自 `packages/core/src/play/play-store.ts`（441 行）的读取面与
//! `play/play-image.ts`（纯函数段）+ `play/play-db-factory.ts` 的后端选择
//! （TS sqlite 可用即 sqlite——Rust 等价：play.db 存在走 sqlite 只读快照，
//! 否则 play-graph.json，再否则空快照）。
//!
//! 写入面（createWorld/saveCurrentState/appendTranscriptTurn/graph 变更）与
//! play-runner（互动回合 LLM 执行流）不在本号端点消费面，随后续号。
//! 生图执行链（generatePlayImage → cover 基础设施）未移植——generate-image
//! 端点返回 needsCoverConfig 兜底（偏差备案见 71 号记录）。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

// ── 路径安全（play-store.ts isSafeSegment / assertSafeSegment） ──

fn is_safe_segment(value: &str) -> bool {
    !value.is_empty()
        && !value.contains('/')
        && !value.contains('\\')
        && !value.contains('\0')
        && value != "."
        && value != ".."
}

pub fn world_dir(project_root: &Path, world_id: &str) -> Result<PathBuf, String> {
    if !is_safe_segment(world_id) {
        return Err(format!("Unsafe play path segment: {world_id}"));
    }
    Ok(project_root.join("worlds").join(world_id))
}

pub fn run_dir(project_root: &Path, world_id: &str, run_id: &str) -> Result<PathBuf, String> {
    if !is_safe_segment(run_id) {
        return Err(format!("Unsafe play path segment: {run_id}"));
    }
    Ok(world_dir(project_root, world_id)?.join("runs").join(run_id))
}

/// `safeRunChildPath`：run 内相对路径（禁绝对/空/NUL/.. 逃逸）。
fn safe_run_child_path(run_dir: &Path, relative_path: &str) -> Result<PathBuf, String> {
    if relative_path.is_empty() || relative_path.starts_with('/') || relative_path.contains('\0') {
        return Err(format!("Unsafe play path: {relative_path}"));
    }
    if relative_path.split(['/', '\\']).any(|part| part == "..") {
        return Err(format!("Unsafe play path: {relative_path}"));
    }
    Ok(run_dir.join(relative_path))
}

// ── PlayStore 读取面 ────────────────────────────────────────────

/// `readTranscript`：transcript.jsonl 逐行宽松解析（坏行/空行跳过）。
pub async fn read_transcript(project_root: &Path, world_id: &str, run_id: &str) -> Vec<Value> {
    let Ok(run_dir) = run_dir(project_root, world_id, run_id) else {
        return Vec::new();
    };
    let Ok(raw) = tokio::fs::read_to_string(run_dir.join("transcript.jsonl")).await else {
        return Vec::new();
    };
    raw.lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| {
            let value: Value = serde_json::from_str(line).ok()?;
            // zod 形态校验（role/content/timestamp）；非法行跳过。
            let role = value.get("role")?.as_str()?;
            if !matches!(role, "user" | "assistant" | "system" | "tool") {
                return None;
            }
            value.get("content")?.as_str()?;
            value.get("timestamp")?.as_u64()?;
            Some(value)
        })
        .collect()
}

/// `loadCurrentState`：state/current.json（缺失/坏 → None；端点 catch null）。
pub async fn load_current_state(project_root: &Path, world_id: &str, run_id: &str) -> Option<Value> {
    let run_dir = run_dir(project_root, world_id, run_id).ok()?;
    let raw = tokio::fs::read_to_string(run_dir.join("state").join("current.json"))
        .await
        .ok()?;
    serde_json::from_str(&raw).ok()
}

/// `loadWorld`：worlds/{id}/world.json（宽松；title 消费）。
pub async fn load_world(project_root: &Path, world_id: &str) -> Option<Value> {
    let world_dir = world_dir(project_root, world_id).ok()?;
    let raw = tokio::fs::read_to_string(world_dir.join("world.json"))
        .await
        .ok()?;
    serde_json::from_str(&raw).ok()
}

/// `readProjection`：run 内相对路径读取（unsafe → Err）。
pub async fn read_projection(
    project_root: &Path,
    world_id: &str,
    run_id: &str,
    relative_path: &str,
) -> Result<String, String> {
    let run_dir = run_dir(project_root, world_id, run_id)?;
    let target = safe_run_child_path(&run_dir, relative_path)?;
    tokio::fs::read_to_string(&target).await.map_err(|e| e.to_string())
}

// ── 图谱快照（createPlayDB(runDir).snapshot()） ─────────────────

/// `createPlayDB` 后端选择的读取等价：play.db（sqlite 主后端）存在 → sqlite
/// 只读快照；否则 play-graph.json（文件回退）；再否则空快照。
pub fn play_graph_snapshot(run_dir: &Path) -> Value {
    let sqlite_path = run_dir.join("play.db");
    if sqlite_path.is_file() {
        if let Some(snapshot) = snapshot_from_sqlite(&sqlite_path) {
            return snapshot;
        }
    }
    snapshot_from_file(&run_dir.join("play-graph.json"))
}

/// 文件后端快照（play-file-db.ts load + snapshot）：四 record 逐项宽松
/// （safeParse 失败跳过——parseRecord 语义），排序 entities/edges/slots by
/// id、events by (turn, id)。
fn snapshot_from_file(path: &Path) -> Value {
    let empty = || json!({ "entities": [], "edges": [], "stateSlots": [], "events": [] });
    let Ok(raw) = std::fs::read_to_string(path) else {
        return empty();
    };
    let Ok(parsed) = serde_json::from_str::<Value>(&raw) else {
        return empty();
    };
    let record_items = |name: &str, validator: fn(&Value) -> bool| -> Vec<Value> {
        parsed
            .get(name)
            .and_then(Value::as_object)
            .map(|record| {
                record
                    .values()
                    .filter(|item| validator(item))
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    };
    let mut entities = record_items("entities", is_valid_entity);
    entities.sort_by_key(id_of);
    let mut edges = record_items("edges", is_valid_edge);
    edges.sort_by_key(id_of);
    let mut state_slots = record_items("stateSlots", is_valid_state_slot);
    state_slots.sort_by_key(id_of);
    let mut events = record_items("events", is_valid_event);
    events.sort_by_key(|event| (turn_of(event), id_of(event)));
    json!({
        "entities": entities,
        "edges": edges,
        "stateSlots": state_slots,
        "events": events
    })
}

fn id_of(value: &Value) -> String {
    value
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn turn_of(value: &Value) -> i64 {
    value.get("turn").and_then(Value::as_i64).unwrap_or(0)
}

// zod 校验等价（必填字段形态；default 字段允许缺省）。
fn non_empty_str(value: &Value, field: &str) -> bool {
    value
        .get(field)
        .and_then(Value::as_str)
        .is_some_and(|s| !s.is_empty())
}

fn is_valid_entity(value: &Value) -> bool {
    non_empty_str(value, "id") && non_empty_str(value, "label")
}

fn is_valid_edge(value: &Value) -> bool {
    non_empty_str(value, "id")
        && non_empty_str(value, "fromId")
        && non_empty_str(value, "type")
        && non_empty_str(value, "toId")
        && non_empty_str(value, "validFromEventId")
        && non_empty_str(value, "sourceEventId")
}

fn is_valid_state_slot(value: &Value) -> bool {
    non_empty_str(value, "id")
        && non_empty_str(value, "label")
        && non_empty_str(value, "updatedEventId")
}

fn is_valid_event(value: &Value) -> bool {
    non_empty_str(value, "id")
        && non_empty_str(value, "rawInput")
        && non_empty_str(value, "createdAt")
        && value.get("turn").and_then(Value::as_i64).is_some_and(|t| t >= 0)
}

/// sqlite 主后端只读快照（play-db.ts snapshot 的 SELECT + 行转换）。
fn snapshot_from_sqlite(path: &Path) -> Option<Value> {
    use rusqlite::OpenFlags;
    let conn = rusqlite::Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()?;

    let mut entities: Vec<Value> = Vec::new();
    {
        let mut stmt = conn
            .prepare(
                "SELECT id, type, label, summary, status, created_event, updated_event FROM entities ORDER BY id",
            )
            .ok()?;
        let rows = stmt
            .query_map([], |row| {
                Ok(json!({
                    "id": row.get::<_, String>(0)?,
                    "type": row.get::<_, String>(1)?,
                    "label": row.get::<_, String>(2)?,
                    "summary": row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                    "status": row.get::<_, Option<String>>(4)?.unwrap_or_default(),
                    "createdEventId": row.get::<_, Option<String>>(5)?,
                    "updatedEventId": row.get::<_, Option<String>>(6)?,
                }))
            })
            .ok()?;
        for row in rows.flatten() {
            entities.push(row);
        }
    }
    let mut edges: Vec<Value> = Vec::new();
    {
        let mut stmt = conn
            .prepare(
                "SELECT id, from_id, type, to_id, value_json, valid_from_event, valid_until_event, source_event_id, visibility_json, strength, confidence FROM edges ORDER BY id",
            )
            .ok()?;
        let rows = stmt
            .query_map([], |row| {
                let value: Value = serde_json::from_str(&row.get::<_, String>(4).unwrap_or_default())
                    .unwrap_or_else(|_| json!({}));
                let visibility: Value =
                    serde_json::from_str(&row.get::<_, String>(8).unwrap_or_default())
                        .unwrap_or_else(|_| json!({}));
                Ok(json!({
                    "id": row.get::<_, String>(0)?,
                    "fromId": row.get::<_, String>(1)?,
                    "type": row.get::<_, String>(2)?,
                    "toId": row.get::<_, String>(3)?,
                    "value": value,
                    "validFromEventId": row.get::<_, String>(5)?,
                    "validUntilEventId": row.get::<_, Option<String>>(6)?,
                    "sourceEventId": row.get::<_, String>(7)?,
                    "visibility": visibility,
                    "strength": row.get::<_, Option<f64>>(9)?,
                    "confidence": row.get::<_, Option<f64>>(10)?,
                }))
            })
            .ok()?;
        for row in rows.flatten() {
            edges.push(row);
        }
    }
    let mut state_slots: Vec<Value> = Vec::new();
    {
        let mut stmt = conn
            .prepare(
                "SELECT id, owner_entity_id, kind, label, value_json, updated_event FROM state_slots ORDER BY id",
            )
            .ok()?;
        let rows = stmt
            .query_map([], |row| {
                let value: Value = serde_json::from_str(row.get::<_, Option<String>>(4).unwrap_or_default().as_deref().unwrap_or("null"))
                    .unwrap_or(Value::Null);
                Ok(json!({
                    "id": row.get::<_, String>(0)?,
                    "ownerEntityId": row.get::<_, Option<String>>(1)?,
                    "kind": row.get::<_, String>(2)?,
                    "label": row.get::<_, String>(3)?,
                    "value": value,
                    "updatedEventId": row.get::<_, String>(5)?,
                }))
            })
            .ok()?;
        for row in rows.flatten() {
            state_slots.push(row);
        }
    }
    let mut events: Vec<Value> = Vec::new();
    {
        let mut stmt = conn
            .prepare(
                "SELECT id, turn, action_kind, raw_input, outcome_summary, created_at FROM events ORDER BY turn, id",
            )
            .ok()?;
        let rows = stmt
            .query_map([], |row| {
                Ok(json!({
                    "id": row.get::<_, String>(0)?,
                    "turn": row.get::<_, i64>(1)?,
                    "actionKind": row.get::<_, String>(2)?,
                    "rawInput": row.get::<_, String>(3)?,
                    "outcomeSummary": row.get::<_, Option<String>>(4)?.unwrap_or_default(),
                    "createdAt": row.get::<_, String>(5)?,
                }))
            })
            .ok()?;
        for row in rows.flatten() {
            events.push(row);
        }
    }
    Some(json!({
        "entities": entities,
        "edges": edges,
        "stateSlots": state_slots,
        "events": events,
    }))
}

// ── 插图 sidecar（play-image.ts 纯函数 + manifest/settings） ─────

/// `playImageFileName`：非 [a-zA-Z0-9_-] 折叠 `_` + 80 码元 + 默认 image。
pub fn play_image_file_name(key: &str, extension: &str) -> String {
    let safe: String = key
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let mut out = String::new();
    let mut units = 0usize;
    for ch in safe.chars() {
        let ch_units = ch.len_utf16();
        if units + ch_units > 80 {
            break;
        }
        out.push(ch);
        units += ch_units;
    }
    let stem = if out.is_empty() { "image".to_string() } else { out };
    format!("{stem}.{extension}")
}

/// `readPlayImageManifest`：run/images/manifest.json（坏 → 空）。
pub async fn read_play_image_manifest(run_dir: &Path) -> BTreeMap<String, Value> {
    let Ok(raw) = tokio::fs::read_to_string(run_dir.join("images").join("manifest.json")).await
    else {
        return BTreeMap::new();
    };
    match serde_json::from_str::<Value>(&raw) {
        Ok(Value::Object(map)) => map.into_iter().collect(),
        _ => BTreeMap::new(),
    }
}

/// `readPlayImageSettings`：三开关（默认全关；坏文件回退默认）。
pub async fn read_play_image_settings(run_dir: &Path) -> Value {
    let read = tokio::fs::read_to_string(run_dir.join("images").join("settings.json")).await;
    let parsed = read.ok().and_then(|raw| serde_json::from_str::<Value>(&raw).ok());
    let flag = |name: &str| -> bool {
        parsed
            .as_ref()
            .and_then(|value| value.get(name))
            .map(|v| matches!(v, Value::Bool(true)))
            .unwrap_or(false)
    };
    json!({
        "actors": flag("actors"),
        "moments": flag("moments"),
        "inventory": flag("inventory")
    })
}

/// `writePlayImageSettings`：目录递归 + pretty JSON。
pub async fn write_play_image_settings(run_dir: &Path, settings: &Value) -> Result<(), String> {
    let flag = |name: &str| -> bool {
        settings
            .get(name)
            .and_then(Value::as_bool)
            .unwrap_or(false)
    };
    let (actors, moments, inventory) = (flag("actors"), flag("moments"), flag("inventory"));
    let dir = run_dir.join("images");
    tokio::fs::create_dir_all(&dir).await.map_err(|e| e.to_string())?;
    let payload = format!(
        "{}\n",
        serde_json::to_string_pretty(&json!({
            "actors": actors,
            "moments": moments,
            "inventory": inventory
        }))
        .unwrap_or_default()
    );
    tokio::fs::write(dir.join("settings.json"), payload)
        .await
        .map_err(|e| e.to_string())
}

/// `clamp`：UTF-16 码元截断 + 省略号。
fn clamp_text(text: &str, max: usize) -> String {
    let trimmed = text.trim();
    if trimmed.encode_utf16().count() <= max {
        return trimmed.to_string();
    }
    let mut out = String::new();
    let mut units = 0usize;
    for ch in trimmed.chars() {
        let ch_units = ch.len_utf16();
        if units + ch_units > max {
            break;
        }
        out.push(ch);
        units += ch_units;
    }
    out.push('…');
    out
}

fn shot_by_type(entity_type: &str) -> &'static str {
    match entity_type {
        "actor" => "为这个角色生成配图",
        "location" => "为这个地点生成配图",
        "item" => "为这件物品生成配图",
        "evidence" => "为这件证物生成配图",
        "clue" => "为这条线索生成配图",
        "claim" => "为这个主张生成配图",
        "proof_chain" => "为这条证据链生成配图",
        "organization" => "为这个组织生成配图",
        _ => "为这个对象生成配图",
    }
}

/// 世界上下文三元组（generate-image 端点装配：premise/worldContract/visualContract）。
pub struct PlayWorldContext<'a> {
    pub premise: Option<&'a str>,
    pub world_contract: Option<&'a str>,
    pub visual_contract: Option<&'a str>,
}

fn render_world_context(context: &Option<PlayWorldContext<'_>>) -> String {
    let Some(context) = context else {
        return String::new();
    };
    let mut lines: Vec<String> = Vec::new();
    if let Some(premise) = context.premise.map(str::trim).filter(|p| !p.is_empty()) {
        lines.push(format!(
            "世界设定（决定时代、场景与整体美术风格，必须贴合）：{}",
            clamp_text(premise, 600)
        ));
    }
    if let Some(contract) = context.world_contract.map(str::trim).filter(|c| !c.is_empty()) {
        lines.push(format!(
            "世界契约（只遵守用户定义的规则，不要自行发明 RPG/数值/等级系统）：{}",
            clamp_text(contract, 700)
        ));
    }
    if let Some(visual) = context.visual_contract.map(str::trim).filter(|c| !c.is_empty()) {
        lines.push(format!(
            "视觉契约（图片必须按这条表达语义）：{}",
            clamp_text(visual, 700)
        ));
    }
    lines.join("\n")
}

/// `buildPlayEntityImagePrompt`。
pub fn build_play_entity_image_prompt(
    entity_type: &str,
    label: &str,
    summary: Option<&str>,
    world_context: &Option<PlayWorldContext<'_>>,
) -> String {
    let mut lines: Vec<String> = Vec::new();
    let context = render_world_context(world_context);
    if !context.is_empty() {
        lines.push(context);
    }
    lines.push(shot_by_type(entity_type).to_string());
    lines.push(format!("对象：{label}"));
    if let Some(summary) = summary.map(str::trim).filter(|s| !s.is_empty()) {
        lines.push(format!("细节：{}", clamp_text(summary, 400)));
    }
    lines.join("\n")
}

/// `buildPlaySceneImagePrompt`。
pub fn build_play_scene_image_prompt(
    scene_text: &str,
    world_context: &Option<PlayWorldContext<'_>>,
) -> String {
    let mut lines: Vec<String> = Vec::new();
    let context = render_world_context(world_context);
    if !context.is_empty() {
        lines.push(context);
    }
    lines.push("为下面这一刻生成配图，捕捉当下的动作、氛围与情绪：".to_string());
    lines.push(clamp_text(scene_text, 900));
    lines.join("\n")
}

// ── 写入面（73 号：play-store.ts 写方法） ─────────────────────────

/// `ensureRun`：run 目录骨架（state/projections/summaries/checkpoints）。
pub async fn ensure_run(project_root: &Path, world_id: &str, run_id: &str) -> Result<PathBuf, String> {
    let dir = run_dir(project_root, world_id, run_id)?;
    for sub in ["state", "projections", "summaries", "checkpoints"] {
        tokio::fs::create_dir_all(dir.join(sub))
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(dir)
}

/// `createWorld`：zod 校验形态（id safeSegment + 必填 title/createdAt）。
pub async fn create_world(
    project_root: &Path,
    input: &PlayWorldInput<'_>,
) -> Result<Value, String> {
    let id = input.id.trim();
    if id.is_empty() || !is_safe_segment(id) {
        return Err(format!("Invalid play world id: {id}"));
    }
    if input.title.trim().is_empty() {
        return Err("Play world title is required".to_string());
    }
    let now = crate::utils::utc_time::utc_now_iso();
    let world = json!({
        "id": id,
        "title": input.title.trim(),
        "premise": input.premise.trim(),
        "worldContract": input.world_contract.trim(),
        "visualContract": input.visual_contract.trim(),
        "mode": input.mode,
        "language": input.language,
        "createdAt": now,
        "updatedAt": now,
    });
    let world_dir = world_dir(project_root, id)?;
    tokio::fs::create_dir_all(&world_dir)
        .await
        .map_err(|e| e.to_string())?;
    let payload = format!("{}\n", serde_json::to_string_pretty(&world).unwrap_or_default());
    tokio::fs::write(world_dir.join("world.json"), payload)
        .await
        .map_err(|e| e.to_string())?;
    Ok(world)
}

pub struct PlayWorldInput<'a> {
    pub id: &'a str,
    pub title: &'a str,
    pub premise: &'a str,
    pub world_contract: &'a str,
    pub visual_contract: &'a str,
    pub mode: &'a str,
    pub language: &'a str,
}

/// `saveCurrentState`：state/current.json（ensureRun + pretty + 尾换行）。
pub async fn save_current_state(
    project_root: &Path,
    world_id: &str,
    run_id: &str,
    state: &Value,
) -> Result<(), String> {
    ensure_run(project_root, world_id, run_id).await?;
    let run = run_dir(project_root, world_id, run_id)?;
    let payload = format!("{}\n", serde_json::to_string_pretty(state).unwrap_or_default());
    tokio::fs::write(run.join("state").join("current.json"), payload)
        .await
        .map_err(|e| e.to_string())
}

fn transcript_jsonl_path(run_dir: &Path) -> PathBuf {
    run_dir.join("transcript.jsonl")
}

fn events_jsonl_path(run_dir: &Path) -> PathBuf {
    run_dir.join("events.jsonl")
}

/// `appendTranscriptTurn`：{role,content,timestamp} 行追加。
pub async fn append_transcript_turn(
    project_root: &Path,
    world_id: &str,
    run_id: &str,
    role: &str,
    content: &str,
) -> Result<(), String> {
    ensure_run(project_root, world_id, run_id).await?;
    let run = run_dir(project_root, world_id, run_id)?;
    let line = json!({ "role": role, "content": content, "timestamp": crate::interaction::session::utc_now_ms() });
    append_json_line(&transcript_jsonl_path(&run), &line).await
}

/// `appendEvent`：事件行追加（形态校验由调用方 reducer 保证）。
pub async fn append_event(project_root: &Path, world_id: &str, run_id: &str, event: &Value) -> Result<(), String> {
    ensure_run(project_root, world_id, run_id).await?;
    let run = run_dir(project_root, world_id, run_id)?;
    append_json_line(&events_jsonl_path(&run), event).await
}

/// `readEvents`：events.jsonl 宽松读取（与 transcript 同坏行策略）。
pub async fn read_events(project_root: &Path, world_id: &str, run_id: &str) -> Vec<Value> {
    let Ok(run) = run_dir(project_root, world_id, run_id) else {
        return Vec::new();
    };
    let Ok(raw) = tokio::fs::read_to_string(events_jsonl_path(&run)).await else {
        return Vec::new();
    };
    raw.lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

/// `writeProjection`：run 内相对路径写入（safe_child 防逃逸）。
pub async fn write_projection(
    project_root: &Path,
    world_id: &str,
    run_id: &str,
    relative_path: &str,
    content: &str,
) -> Result<(), String> {
    ensure_run(project_root, world_id, run_id).await?;
    let run = run_dir(project_root, world_id, run_id)?;
    let target = safe_run_child_path(&run, relative_path)?;
    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|e| e.to_string())?;
    }
    tokio::fs::write(&target, content).await.map_err(|e| e.to_string())
}

async fn append_json_line(path: &Path, value: &Value) -> Result<(), String> {
    use tokio::io::AsyncWriteExt;
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await
        .map_err(|e| e.to_string())?;
    let line = format!("{}\n", serde_json::to_string(value).unwrap_or_default());
    file.write_all(line.as_bytes()).await.map_err(|e| e.to_string())?;
    // 见 68 号：tokio File 缓冲需显式 flush 后对同进程读取可见。
    let _ = file.flush().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_and_child_path_safety() {
        assert!(world_dir(Path::new("/r"), "w1").is_ok());
        assert!(world_dir(Path::new("/r"), "a/b").is_err());
        assert!(world_dir(Path::new("/r"), "..").is_err());
        assert!(world_dir(Path::new("/r"), ".").is_err());
        assert!(run_dir(Path::new("/r"), "w", "r 1").is_ok());
        assert!(run_dir(Path::new("/r"), "w", "a\\b").is_err());

        let run = Path::new("/r/worlds/w/runs/r1");
        assert!(safe_run_child_path(run, "projections/scene.md").is_ok());
        assert!(safe_run_child_path(run, "").is_err());
        assert!(safe_run_child_path(run, "/abs").is_err());
        assert!(safe_run_child_path(run, "../escape").is_err());
        assert!(safe_run_child_path(run, "a/../../escape").is_err());
    }

    #[tokio::test]
    async fn transcript_skips_bad_lines_and_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let run = run_dir(root, "w", "r1").unwrap();
        std::fs::create_dir_all(&run).unwrap();
        std::fs::write(
            run.join("transcript.jsonl"),
            "{\"role\":\"user\",\"content\":\"进屋\",\"timestamp\":1}\n\
             \n\
             not-json\n\
             {\"role\":\"bogus\",\"content\":\"x\",\"timestamp\":2}\n\
             {\"role\":\"assistant\",\"content\":\"你推开了门。\",\"timestamp\":3}\n",
        )
        .unwrap();
        let transcript = read_transcript(root, "w", "r1").await;
        assert_eq!(transcript.len(), 2);
        assert_eq!(transcript[1]["content"], "你推开了门。");
        // 缺失文件 → 空。
        assert!(read_transcript(root, "w", "ghost").await.is_empty());
    }

    #[tokio::test]
    async fn file_backend_snapshot_sorts_and_skips() {
        let dir = tempfile::tempdir().unwrap();
        let run = dir.path();
        std::fs::write(
            run.join("play-graph.json"),
            serde_json::json!({
                "entities": {
                    "b": { "id": "b", "label": "B" },
                    "a": { "id": "a", "label": "A" },
                    "bad": { "id": "", "label": "坏项" },
                    "nolabel": { "id": "x" }
                },
                "edges": {},
                "stateSlots": {},
                "events": {
                    "e2": { "id": "e2", "turn": 0, "rawInput": "i", "createdAt": "t" },
                    "e1": { "id": "e1", "turn": 1, "rawInput": "i", "createdAt": "t" }
                }
            })
            .to_string(),
        )
        .unwrap();
        let snapshot = play_graph_snapshot(run);
        let ids: Vec<&str> = snapshot["entities"].as_array().unwrap().iter().map(|e| e["id"].as_str().unwrap()).collect();
        assert_eq!(ids, ["a", "b"]);
        // events 按 (turn, id)：e2(turn 0) 先于 e1(turn 1)。
        let events: Vec<&str> = snapshot["events"].as_array().unwrap().iter().map(|e| e["id"].as_str().unwrap()).collect();
        assert_eq!(events, ["e2", "e1"]);

        // 无任何后端 → 空快照四数组。
        let empty_dir = tempfile::tempdir().unwrap();
        let snapshot = play_graph_snapshot(empty_dir.path());
        assert!(snapshot["entities"].as_array().unwrap().is_empty());
    }

    #[test]
    fn sqlite_backend_snapshot_reads_four_tables() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("play.db");
        {
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            conn.execute_batch(
                "CREATE TABLE entities (id TEXT PRIMARY KEY, type TEXT, label TEXT, summary TEXT, status TEXT, created_event TEXT, updated_event TEXT);
                 CREATE TABLE edges (id TEXT PRIMARY KEY, from_id TEXT, type TEXT, to_id TEXT, value_json TEXT, valid_from_event TEXT, valid_until_event TEXT, source_event_id TEXT, visibility_json TEXT, strength REAL, confidence REAL);
                 CREATE TABLE state_slots (id TEXT PRIMARY KEY, owner_entity_id TEXT, kind TEXT, label TEXT, value_json TEXT, updated_event TEXT);
                 CREATE TABLE events (id TEXT PRIMARY KEY, turn INTEGER, action_kind TEXT, raw_input TEXT, outcome_summary TEXT, created_at TEXT);
                 INSERT INTO entities VALUES ('hero','actor','林动','坚韧少年','active','e1','e2');
                 INSERT INTO edges VALUES ('rel-1','hero','ally','npc','{\"bond\":1}','e1',NULL,'e2','{}',0.8,NULL);
                 INSERT INTO state_slots VALUES ('hp','hero','counter','HP','42','e2');
                 INSERT INTO events VALUES ('e1',0,'user_input','进屋','你进门','t1');",
            )
            .unwrap();
        }
        let snapshot = play_graph_snapshot(dir.path());
        assert_eq!(snapshot["entities"][0]["label"], "林动");
        assert_eq!(snapshot["entities"][0]["createdEventId"], "e1");
        let edge = &snapshot["edges"][0];
        assert_eq!(edge["value"]["bond"], 1);
        assert!(edge["validUntilEventId"].is_null());
        assert_eq!(edge["strength"], 0.8);
        assert_eq!(snapshot["stateSlots"][0]["value"], 42);
        assert_eq!(snapshot["events"][0]["actionKind"], "user_input");
    }

    #[tokio::test]
    async fn image_settings_and_manifest_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let run = dir.path();
        // 默认全关（文件缺失）。
        let defaults = read_play_image_settings(run).await;
        assert_eq!(defaults["actors"], false);
        // 写后回读。
        write_play_image_settings(run, &json!({ "actors": true, "moments": true }))
            .await
            .unwrap();
        let read_back = read_play_image_settings(run).await;
        assert_eq!(read_back["actors"], true);
        assert_eq!(read_back["moments"], true);
        assert_eq!(read_back["inventory"], false);
        let raw = std::fs::read_to_string(run.join("images").join("settings.json")).unwrap();
        assert!(raw.ends_with("}\n"));
        // manifest：坏文件 → 空表。
        assert!(read_play_image_manifest(run).await.is_empty());
    }

    #[test]
    fn image_prompts_and_file_name() {
        assert_eq!(play_image_file_name("act or/1", "png"), "act_or_1.png");
        assert_eq!(play_image_file_name("", "jpg"), "image.jpg");

        let world = Some(PlayWorldContext {
            premise: Some(" 东方修真世界 "),
            world_contract: None,
            visual_contract: Some("水墨风格"),
        });
        let entity_prompt = build_play_entity_image_prompt("actor", "林动", Some("坚韧少年"), &world);
        assert!(entity_prompt.contains("世界设定（决定时代、场景与整体美术风格，必须贴合）：东方修真世界"), "{entity_prompt}");
        assert!(entity_prompt.contains("为这个角色生成配图"));
        assert!(entity_prompt.contains("对象：林动"));
        assert!(entity_prompt.contains("细节：坚韧少年"));
        assert!(!entity_prompt.contains("世界契约"));

        let scene_prompt = build_play_scene_image_prompt("雪夜孤灯。", &world);
        assert!(scene_prompt.contains("为下面这一刻生成配图，捕捉当下的动作、氛围与情绪："));
        assert!(scene_prompt.contains("雪夜孤灯。"));
        assert!(scene_prompt.contains("视觉契约（图片必须按这条表达语义）：水墨风格"));

        // 截断：600 码元以上补省略号。
        let long_premise = "长".repeat(700);
        let prompt = build_play_entity_image_prompt("actor", "X", None, &Some(PlayWorldContext {
            premise: Some(&long_premise), world_contract: None, visual_contract: None,
        }));
        assert!(prompt.contains('…'));
    }
}
