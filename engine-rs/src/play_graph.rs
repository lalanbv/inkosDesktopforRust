//! play 图谱写后端 + reducer（73 号）。
//!
//! - `PlayGraphDb`：**sqlite 主后端**（play.db 存在时读写四表——与 71 号读取
//!   面的后端选择一致）+ **文件回退**（play-graph.json 四 record，内存事务）。
//! - `apply_play_mutation` / `seed_play_graph`（play-reducer.ts 全量）：
//!   玩家 id 规范化（legacy "player" → actor_player）/ 实体别名解析 / 校验
//!   （slot owner 存在性 + 证据链不回退）/ fail-open 关系边 / holding 归一 /
//!   数值槽 min-max 夹取 / 证据状态槽推进。
//!
//! mutation 消费 serde_json::Value（经 play_parser::normalize_play_mutation
//! 归一后的形状），与 TS zod preprocess 等价。

use std::collections::HashMap;
use std::path::Path;

use serde_json::{json, Value};

// ── 图后端 ──────────────────────────────────────────────────────

enum Backend {
    File {
        path: std::path::PathBuf,
        data: FileData,
    },
    Sqlite {
        conn: rusqlite::Connection,
    },
}

#[derive(Default, Clone)]
struct FileData {
    entities: HashMap<String, Value>,
    edges: HashMap<String, Value>,
    state_slots: HashMap<String, Value>,
    events: HashMap<String, Value>,
}

pub struct PlayGraphDb {
    backend: Backend,
}

/// 工厂（createPlayDB 后端选择）：play.db 存在 → sqlite 读写；否则文件。
pub fn open_play_graph_db(run_dir: &Path) -> Result<PlayGraphDb, String> {
    let sqlite_path = run_dir.join("play.db");
    if sqlite_path.is_file() {
        let conn = rusqlite::Connection::open(&sqlite_path).map_err(|e| e.to_string())?;
        ensure_sqlite_schema(&conn)?;
        Ok(PlayGraphDb { backend: Backend::Sqlite { conn } })
    } else {
        let path = run_dir.join("play-graph.json");
        let data = load_file_data(&path);
        Ok(PlayGraphDb { backend: Backend::File { path, data } })
    }
}

fn load_file_data(path: &Path) -> FileData {
    let mut data = FileData::default();
    let Ok(raw) = std::fs::read_to_string(path) else {
        return data;
    };
    let Ok(parsed) = serde_json::from_str::<Value>(&raw) else {
        return data;
    };
    let collect = |name: &str, map: &mut HashMap<String, Value>| {
        if let Some(record) = parsed.get(name).and_then(Value::as_object) {
            for (key, item) in record {
                map.insert(key.clone(), item.clone());
            }
        }
    };
    collect("entities", &mut data.entities);
    collect("edges", &mut data.edges);
    collect("stateSlots", &mut data.state_slots);
    collect("events", &mut data.events);
    data
}

fn ensure_sqlite_schema(conn: &rusqlite::Connection) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS entities (
            id TEXT PRIMARY KEY, type TEXT NOT NULL, label TEXT NOT NULL,
            summary TEXT, status TEXT, created_event TEXT, updated_event TEXT);
         CREATE TABLE IF NOT EXISTS edges (
            id TEXT PRIMARY KEY, from_id TEXT NOT NULL, type TEXT NOT NULL, to_id TEXT NOT NULL,
            value_json TEXT NOT NULL, valid_from_event TEXT NOT NULL, valid_until_event TEXT,
            source_event_id TEXT NOT NULL, visibility_json TEXT NOT NULL,
            strength REAL, confidence REAL);
         CREATE TABLE IF NOT EXISTS state_slots (
            id TEXT PRIMARY KEY, owner_entity_id TEXT, kind TEXT NOT NULL, label TEXT NOT NULL,
            value_json TEXT NOT NULL, updated_event TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS events (
            id TEXT PRIMARY KEY, turn INTEGER NOT NULL, action_kind TEXT NOT NULL,
            raw_input TEXT NOT NULL, outcome_summary TEXT, created_at TEXT NOT NULL);",
    )
    .map_err(|e| e.to_string())
}

impl PlayGraphDb {
    /// `snapshot()`：四集合排序快照（与 71 号读取面形态一致）。
    pub fn snapshot(&self) -> Value {
        match &self.backend {
            Backend::File { data, .. } => {
                let sorted = |map: &HashMap<String, Value>| {
                    let mut items: Vec<&Value> = map.values().collect();
                    items.sort_by_key(|item| id_of(item));
                    items.into_iter().cloned().collect::<Vec<_>>()
                };
                let mut events: Vec<&Value> = data.events.values().collect();
                events.sort_by_key(|event| (turn_of(event), id_of(event)));
                json!({
                    "entities": sorted(&data.entities),
                    "edges": sorted(&data.edges),
                    "stateSlots": sorted(&data.state_slots),
                    "events": events.into_iter().cloned().collect::<Vec<_>>(),
                })
            }
            Backend::Sqlite { conn } => snapshot_from_sqlite(conn),
        }
    }

    pub fn get_entity(&self, id: &str) -> Option<Value> {
        match &self.backend {
            Backend::File { data, .. } => data.entities.get(id).cloned(),
            Backend::Sqlite { conn } => conn
                .query_row(
                    "SELECT id, type, label, summary, status, created_event, updated_event FROM entities WHERE id = ?1",
                    [id],
                    |row| {
                        Ok(json!({
                            "id": row.get::<_, String>(0)?,
                            "type": row.get::<_, String>(1)?,
                            "label": row.get::<_, String>(2)?,
                            "summary": row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                            "status": row.get::<_, Option<String>>(4)?.unwrap_or_default(),
                            "createdEventId": row.get::<_, Option<String>>(5)?,
                            "updatedEventId": row.get::<_, Option<String>>(6)?,
                        }))
                    },
                )
                .ok(),
        }
    }

    pub fn upsert_entity(&mut self, entity: &Value) -> Result<(), String> {
        let id = entity.get("id").and_then(Value::as_str).unwrap_or_default().to_string();
        if id.is_empty() {
            return Ok(());
        }
        match &mut self.backend {
            Backend::File { data, .. } => {
                data.entities.insert(id, entity.clone());
                Ok(())
            }
            Backend::Sqlite { conn } => {
                let get = |name: &str| entity.get(name).and_then(Value::as_str).unwrap_or_default();
                conn.execute(
                    "INSERT OR REPLACE INTO entities (id, type, label, summary, status, created_event, updated_event) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    rusqlite::params![
                        get("id"),
                        get("type"),
                        get("label"),
                        get("summary"),
                        get("status"),
                        entity.get("createdEventId").and_then(Value::as_str),
                        entity.get("updatedEventId").and_then(Value::as_str),
                    ],
                )
                .map_err(|e| e.to_string())?;
                Ok(())
            }
        }
    }

    pub fn upsert_edge(&mut self, edge: &Value) -> Result<(), String> {
        let id = edge.get("id").and_then(Value::as_str).unwrap_or_default().to_string();
        if id.is_empty() {
            return Ok(());
        }
        match &mut self.backend {
            Backend::File { data, .. } => {
                data.edges.insert(id, edge.clone());
                Ok(())
            }
            Backend::Sqlite { conn } => {
                let text = |name: &str| edge.get(name).and_then(Value::as_str).unwrap_or_default().to_string();
                let value_json = serde_json::to_string(edge.get("value").unwrap_or(&json!({}))).unwrap_or_else(|_| "{}".into());
                let visibility_json =
                    serde_json::to_string(edge.get("visibility").unwrap_or(&json!({}))).unwrap_or_else(|_| "{}".into());
                conn.execute(
                    "INSERT OR REPLACE INTO edges (id, from_id, type, to_id, value_json, valid_from_event, valid_until_event, source_event_id, visibility_json, strength, confidence) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                    rusqlite::params![
                        text("id"),
                        text("fromId"),
                        text("type"),
                        text("toId"),
                        value_json,
                        text("validFromEventId"),
                        edge.get("validUntilEventId").and_then(Value::as_str),
                        text("sourceEventId"),
                        visibility_json,
                        edge.get("strength").and_then(Value::as_f64),
                        edge.get("confidence").and_then(Value::as_f64),
                    ],
                )
                .map_err(|e| e.to_string())?;
                Ok(())
            }
        }
    }

    pub fn expire_edge(&mut self, edge_id: &str, valid_until_event_id: &str) -> Result<(), String> {
        match &mut self.backend {
            Backend::File { data, .. } => {
                if let Some(edge) = data.edges.get_mut(edge_id) {
                    if let Some(obj) = edge.as_object_mut() {
                        obj.insert("validUntilEventId".into(), json!(valid_until_event_id));
                    }
                }
                Ok(())
            }
            Backend::Sqlite { conn } => {
                conn.execute(
                    "UPDATE edges SET valid_until_event = ?2 WHERE id = ?1",
                    rusqlite::params![edge_id, valid_until_event_id],
                )
                .map_err(|e| e.to_string())?;
                Ok(())
            }
        }
    }

    pub fn upsert_state_slot(&mut self, slot: &Value) -> Result<(), String> {
        let id = slot.get("id").and_then(Value::as_str).unwrap_or_default().to_string();
        if id.is_empty() {
            return Ok(());
        }
        match &mut self.backend {
            Backend::File { data, .. } => {
                data.state_slots.insert(id, slot.clone());
                Ok(())
            }
            Backend::Sqlite { conn } => {
                let text = |name: &str| slot.get(name).and_then(Value::as_str).unwrap_or_default().to_string();
                let value_json = serde_json::to_string(slot.get("value").unwrap_or(&Value::Null)).unwrap_or_else(|_| "null".into());
                conn.execute(
                    "INSERT OR REPLACE INTO state_slots (id, owner_entity_id, kind, label, value_json, updated_event) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    rusqlite::params![
                        text("id"),
                        slot.get("ownerEntityId").and_then(Value::as_str),
                        text("kind"),
                        text("label"),
                        value_json,
                        text("updatedEventId"),
                    ],
                )
                .map_err(|e| e.to_string())?;
                Ok(())
            }
        }
    }

    pub fn state_slots_for_entity(&self, entity_id: &str) -> Vec<Value> {
        match &self.backend {
            Backend::File { data, .. } => data
                .state_slots
                .values()
                .filter(|slot| slot.get("ownerEntityId").and_then(Value::as_str) == Some(entity_id))
                .cloned()
                .collect(),
            Backend::Sqlite { conn } => {
                let mut stmt = match conn.prepare(
                    "SELECT id, owner_entity_id, kind, label, value_json, updated_event FROM state_slots WHERE owner_entity_id = ?1",
                ) {
                    Ok(stmt) => stmt,
                    Err(_) => return Vec::new(),
                };
                let Ok(rows) = stmt.query_map([entity_id], |row| {
                    let value: Value = serde_json::from_str(
                        row.get::<_, String>(4).unwrap_or_default().as_str(),
                    )
                    .unwrap_or(Value::Null);
                    Ok(json!({
                        "id": row.get::<_, String>(0)?,
                        "ownerEntityId": row.get::<_, Option<String>>(1)?,
                        "kind": row.get::<_, String>(2)?,
                        "label": row.get::<_, String>(3)?,
                        "value": value,
                        "updatedEventId": row.get::<_, String>(5)?,
                    }))
                }) else {
                    return Vec::new();
                };
                rows.flatten().collect()
            }
        }
    }

    pub fn record_event(&mut self, event: &Value) -> Result<(), String> {
        let id = event.get("id").and_then(Value::as_str).unwrap_or_default().to_string();
        if id.is_empty() {
            return Ok(());
        }
        match &mut self.backend {
            Backend::File { data, .. } => {
                data.events.insert(id, event.clone());
                Ok(())
            }
            Backend::Sqlite { conn } => {
                let text = |name: &str| event.get(name).and_then(Value::as_str).unwrap_or_default().to_string();
                conn.execute(
                    "INSERT OR REPLACE INTO events (id, turn, action_kind, raw_input, outcome_summary, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    rusqlite::params![
                        text("id"),
                        event.get("turn").and_then(Value::as_i64).unwrap_or(0),
                        text("actionKind"),
                        text("rawInput"),
                        text("outcomeSummary"),
                        text("createdAt"),
                    ],
                )
                .map_err(|e| e.to_string())?;
                Ok(())
            }
        }
    }

    /// 事务（File：内存克隆回滚；Sqlite：BEGIN/COMMIT）。
    pub fn with_transaction<T>(
        &mut self,
        f: impl FnOnce(&mut Self) -> Result<T, String>,
    ) -> Result<T, String> {
        let is_file = matches!(self.backend, Backend::File { .. });
        if is_file {
            let backup = match &self.backend {
                Backend::File { data, .. } => data.clone(),
                Backend::Sqlite { .. } => unreachable!(),
            };
            match f(self) {
                Ok(value) => {
                    if let Backend::File { path, data } = &mut self.backend {
                        persist_file(path, data)?;
                    }
                    Ok(value)
                }
                Err(error) => {
                    if let Backend::File { data, .. } = &mut self.backend {
                        *data = backup;
                    }
                    Err(error)
                }
            }
        } else {
            let conn = match &mut self.backend {
                Backend::Sqlite { conn } => conn,
                Backend::File { .. } => unreachable!(),
            };
            conn.execute_batch("BEGIN IMMEDIATE").map_err(|e| e.to_string())?;
            // conn 的可变借用在此结束（NLL）；f 需要 &mut Self。
            match f(self) {
                Ok(value) => {
                    if let Backend::Sqlite { conn } = &mut self.backend {
                        conn.execute_batch("COMMIT").map_err(|e| e.to_string())?;
                    }
                    Ok(value)
                }
                Err(error) => {
                    if let Backend::Sqlite { conn } = &mut self.backend {
                        let _ = conn.execute_batch("ROLLBACK");
                    }
                    Err(error)
                }
            }
        }
    }

    /// 立即持久化（File 后端写回 json；Sqlite 即时生效）。
    pub fn flush(&mut self) -> Result<(), String> {
        if let Backend::File { path, data } = &mut self.backend {
            persist_file(path, data)?;
        }
        Ok(())
    }
}

fn persist_file(path: &Path, data: &FileData) -> Result<(), String> {
    let to_record = |map: &HashMap<String, Value>| -> Value {
        let mut object = serde_json::Map::new();
        for (key, value) in map {
            object.insert(key.clone(), value.clone());
        }
        Value::Object(object)
    };
    let payload = json!({
        "entities": to_record(&data.entities),
        "edges": to_record(&data.edges),
        "stateSlots": to_record(&data.state_slots),
        "events": to_record(&data.events),
    });
    std::fs::write(path, format!("{}\n", serde_json::to_string_pretty(&payload).unwrap_or_default()))
        .map_err(|e| e.to_string())
}

fn snapshot_from_sqlite(conn: &rusqlite::Connection) -> Value {
    // 复用 71 号读取面的 SELECT 形态。
    fn query_rows(
        conn: &rusqlite::Connection,
        sql: &str,
        map_row: fn(&rusqlite::Row<'_>) -> rusqlite::Result<Value>,
    ) -> Vec<Value> {
        let mut stmt = match conn.prepare(sql) {
            Ok(stmt) => stmt,
            Err(_) => return Vec::new(),
        };
        let collected: Vec<Value> = match stmt.query_map([], map_row) {
            Ok(rows) => rows.flatten().collect(),
            Err(_) => Vec::new(),
        };
        collected
    }

    let entities = query_rows(conn,
        "SELECT id, type, label, summary, status, created_event, updated_event FROM entities ORDER BY id",
        |row| {
            Ok(json!({
                "id": row.get::<_, String>(0)?,
                "type": row.get::<_, String>(1)?,
                "label": row.get::<_, String>(2)?,
                "summary": row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                "status": row.get::<_, Option<String>>(4)?.unwrap_or_default(),
                "createdEventId": row.get::<_, Option<String>>(5)?,
                "updatedEventId": row.get::<_, Option<String>>(6)?,
            }))
        },
    );
    let edges = query_rows(conn,
        "SELECT id, from_id, type, to_id, value_json, valid_from_event, valid_until_event, source_event_id, visibility_json, strength, confidence FROM edges ORDER BY id",
        |row| {
            let value: Value = serde_json::from_str(row.get::<_, String>(4).unwrap_or_default().as_str()).unwrap_or_else(|_| json!({}));
            let visibility: Value = serde_json::from_str(row.get::<_, String>(8).unwrap_or_default().as_str()).unwrap_or_else(|_| json!({}));
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
        },
    );
    let state_slots = query_rows(conn,
        "SELECT id, owner_entity_id, kind, label, value_json, updated_event FROM state_slots ORDER BY id",
        |row| {
            let value: Value = serde_json::from_str(row.get::<_, String>(4).unwrap_or_default().as_str()).unwrap_or(Value::Null);
            Ok(json!({
                "id": row.get::<_, String>(0)?,
                "ownerEntityId": row.get::<_, Option<String>>(1)?,
                "kind": row.get::<_, String>(2)?,
                "label": row.get::<_, String>(3)?,
                "value": value,
                "updatedEventId": row.get::<_, String>(5)?,
            }))
        },
    );
    let events = query_rows(conn,
        "SELECT id, turn, action_kind, raw_input, outcome_summary, created_at FROM events ORDER BY turn, id",
        |row| {
            Ok(json!({
                "id": row.get::<_, String>(0)?,
                "turn": row.get::<_, i64>(1)?,
                "actionKind": row.get::<_, String>(2)?,
                "rawInput": row.get::<_, String>(3)?,
                "outcomeSummary": row.get::<_, Option<String>>(4)?.unwrap_or_default(),
                "createdAt": row.get::<_, String>(5)?,
            }))
        },
    );
    json!({ "entities": entities, "edges": edges, "stateSlots": state_slots, "events": events })
}

fn id_of(value: &Value) -> String {
    value.get("id").and_then(Value::as_str).unwrap_or_default().to_string()
}

fn turn_of(value: &Value) -> i64 {
    value.get("turn").and_then(Value::as_i64).unwrap_or(0)
}

// ── reducer（play-reducer.ts） ───────────────────────────────────

const PLAYER_ENTITY_ID: &str = "actor_player";
const EVIDENCE_ORDER: &[&str] = &[
    "unknown", "hinted", "seen", "collected", "verified", "weaponized", "exposed", "exhausted",
];

#[derive(Debug)]
pub struct AppliedMutation {
    pub event: Value,
    pub blocked: bool,
}

/// `applyPlayMutation`：规范化 mutation → 事件构造 → 校验 → 事务内
/// （recordEvent + 非 blocked 时图变更）。
pub fn apply_play_mutation(
    db: &mut PlayGraphDb,
    mutation: &Value,
    raw_input: &str,
) -> Result<AppliedMutation, String> {
    let mutation = canonicalize_player_entity_ids(mutation);
    let mutation = resolve_edge_endpoint_labels(db, &mutation);
    let event = build_event(&mutation, raw_input);
    validate_mutation(db, &mutation)?;
    let blocked = mutation.get("blocked").and_then(Value::as_bool).unwrap_or(false);
    let event_clone = event.clone();
    db.with_transaction(|tx| {
        tx.record_event(&event_clone)?;
        if !blocked {
            apply_graph_changes(tx, &mutation)?;
        }
        Ok(())
    })?;
    Ok(AppliedMutation { event, blocked })
}

/// `seedPlayGraph`：仅图变更（不记事件）。
pub fn seed_play_graph(db: &mut PlayGraphDb, mutation: &Value) -> Result<(), String> {
    let mutation = canonicalize_player_entity_ids(mutation);
    let mutation = resolve_edge_endpoint_labels(db, &mutation);
    validate_mutation(db, &mutation)?;
    let blocked = mutation.get("blocked").and_then(Value::as_bool).unwrap_or(false);
    if blocked {
        return Ok(());
    }
    db.with_transaction(|tx| apply_graph_changes(tx, &mutation))
}

fn build_event(mutation: &Value, raw_input: &str) -> Value {
    json!({
        "id": mutation.get("eventId").and_then(Value::as_str).unwrap_or_default(),
        "turn": mutation.get("turn").and_then(Value::as_i64).unwrap_or(0),
        "actionKind": mutation.get("actionKind").and_then(Value::as_str).unwrap_or("do"),
        "rawInput": raw_input,
        "outcomeSummary": mutation
            .get("summary")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .or_else(|| mutation.get("blockedReason").and_then(Value::as_str))
            .unwrap_or_default(),
        "timeAdvance": mutation.get("timeAdvance").cloned().unwrap_or(Value::Null),
        "createdAt": crate::utils::utc_time::utc_now_iso(),
    })
}

fn canonicalize_player_entity_ids(mutation: &Value) -> Value {
    let mut next = mutation.clone();
    let canonicalize = |id: &Value| -> Value {
        let text = id.as_str().unwrap_or_default().trim();
        if text == "player" {
            json!(PLAYER_ENTITY_ID)
        } else {
            json!(text)
        }
    };
    if let Some(upserts) = next
        .pointer_mut("/entities/upsert")
        .and_then(Value::as_array_mut)
    {
        for entity in upserts {
            if let Some(id) = entity.get("id").cloned() {
                if let Some(obj) = entity.as_object_mut() {
                    obj.insert("id".into(), canonicalize(&id));
                }
            }
        }
    }
    if let Some(upserts) = next.pointer_mut("/edges/upsert").and_then(Value::as_array_mut) {
        for edge in upserts {
            for field in ["fromId", "toId"] {
                if let Some(value) = edge.get(field).cloned() {
                    if let Some(obj) = edge.as_object_mut() {
                        obj.insert(field.to_string(), canonicalize(&value));
                    }
                }
            }
        }
    }
    if let Some(upserts) = next.pointer_mut("/stateSlots/upsert").and_then(Value::as_array_mut) {
        for slot in upserts {
            if let Some(owner) = slot.get("ownerEntityId").cloned() {
                if !owner.is_null() {
                    if let Some(obj) = slot.as_object_mut() {
                        obj.insert("ownerEntityId".into(), canonicalize(&owner));
                    }
                }
            }
        }
    }
    next
}

fn resolve_edge_endpoint_labels(db: &PlayGraphDb, mutation: &Value) -> Value {
    let Some(upserts) = mutation.pointer("/edges/upsert").and_then(Value::as_array) else {
        return mutation.clone();
    };
    if upserts.is_empty() {
        return mutation.clone();
    }
    let turn_entities = mutation
        .pointer("/entities/upsert")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let alias_map = build_entity_alias_map(db, &turn_entities);
    if alias_map.is_empty() {
        return mutation.clone();
    }
    let mut next = mutation.clone();
    if let Some(edges) = next.pointer_mut("/edges/upsert").and_then(Value::as_array_mut) {
        for edge in edges {
            for field in ["fromId", "toId"] {
                let raw = edge.get(field).and_then(Value::as_str).unwrap_or_default().trim().to_string();
                if let Some(mapped) = alias_map.get(&raw) {
                    if let Some(obj) = edge.as_object_mut() {
                        obj.insert(field.to_string(), json!(mapped));
                    }
                }
            }
        }
    }
    next
}

fn build_entity_alias_map(
    db: &PlayGraphDb,
    turn_entities: &[Value],
) -> HashMap<String, String> {
    let mut aliases: HashMap<String, String> = HashMap::new();
    let mut ambiguous: std::collections::HashSet<String> = std::collections::HashSet::new();
    let add = |alias: Option<&str>, entity_id: Option<&str>,
                   aliases: &mut HashMap<String, String>,
                   ambiguous: &mut std::collections::HashSet<String>| {
        let alias = alias.map(str::trim).filter(|a| !a.is_empty());
        let id = entity_id.map(str::trim).filter(|i| !i.is_empty());
        let (Some(alias), Some(id)) = (alias, id) else {
            return;
        };
        if let Some(existing) = aliases.get(alias) {
            if existing != id {
                ambiguous.insert(alias.to_string());
                aliases.remove(alias);
                return;
            }
        }
        if !ambiguous.contains(alias) {
            aliases.insert(alias.to_string(), id.to_string());
        }
    };
    for entity in snapshot_entities(db) {
        let id = entity.get("id").and_then(Value::as_str).map(str::to_string);
        let label = entity.get("label").and_then(Value::as_str).map(str::to_string);
        add(id.as_deref(), id.as_deref(), &mut aliases, &mut ambiguous);
        add(label.as_deref(), id.as_deref(), &mut aliases, &mut ambiguous);
    }
    for entity in turn_entities {
        let id = entity.get("id").and_then(Value::as_str).map(str::to_string);
        let label = entity.get("label").and_then(Value::as_str).map(str::to_string);
        add(id.as_deref(), id.as_deref(), &mut aliases, &mut ambiguous);
        add(label.as_deref(), id.as_deref(), &mut aliases, &mut ambiguous);
    }
    aliases
}

fn snapshot_entities(db: &PlayGraphDb) -> Vec<Value> {
    db.snapshot()
        .get("entities")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn validate_mutation(db: &PlayGraphDb, mutation: &Value) -> Result<(), String> {
    let upserted: std::collections::HashSet<&str> = mutation
        .pointer("/entities/upsert")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|e| e.get("id").and_then(Value::as_str))
                .collect()
        })
        .unwrap_or_default();
    let entity_exists = |id: &str| upserted.contains(id) || db.get_entity(id).is_some();

    for slot in mutation
        .pointer("/stateSlots/upsert")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(owner) = slot.get("ownerEntityId").and_then(Value::as_str) else {
            continue;
        };
        if !owner.is_empty() && !entity_exists(owner) {
            return Err(format!(
                "Play mutation references missing entity in state slot {}: {}",
                slot.get("id").and_then(Value::as_str).unwrap_or_default(),
                owner
            ));
        }
    }

    for transition in mutation
        .pointer("/evidence/transitions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let entity_id = transition.get("entityId").and_then(Value::as_str).unwrap_or_default();
        let entity = if upserted.contains(entity_id) {
            mutation
                .pointer("/entities/upsert")
                .and_then(Value::as_array)
                .and_then(|items| {
                    items
                        .iter()
                        .find(|candidate| candidate.get("id").and_then(Value::as_str) == Some(entity_id))
                })
                .cloned()
        } else {
            db.get_entity(entity_id)
        };
        let Some(entity) = entity else {
            return Err(format!(
                "Play mutation references missing entity in evidence transition: {entity_id}"
            ));
        };
        let entity_type = entity.get("type").and_then(Value::as_str).unwrap_or_default();
        if entity_type != "evidence" && entity_type != "clue" {
            return Err(format!(
                "Play evidence transition requires evidence or clue entity: {entity_id}"
            ));
        }
        let current = current_evidence_status(db, entity_id);
        if let Some(from) = transition.get("from").and_then(Value::as_str) {
            if !from.is_empty() && from != current {
                return Err(format!(
                    "Play evidence transition expected {from} but current status is {current}"
                ));
            }
        }
        let to = transition.get("to").and_then(Value::as_str).unwrap_or_default();
        if evidence_rank(to) < evidence_rank(&current) {
            return Err(format!(
                "Play evidence transition cannot regress from {current} to {to}"
            ));
        }
    }
    Ok(())
}

fn apply_graph_changes(db: &mut PlayGraphDb, mutation: &Value) -> Result<(), String> {
    let upsert_entities: Vec<Value> = mutation
        .pointer("/entities/upsert")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let upserted: std::collections::HashSet<&str> = upsert_entities
        .iter()
        .filter_map(|e| e.get("id").and_then(Value::as_str))
        .collect();

    for entity in &upsert_entities {
        db.upsert_entity(entity)?;
    }
    for expire in mutation
        .pointer("/edges/expire")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let edge_id = expire.get("edgeId").and_then(Value::as_str).unwrap_or_default();
        let until = expire
            .get("validUntilEventId")
            .and_then(Value::as_str)
            .unwrap_or_default();
        db.expire_edge(edge_id, until)?;
    }
    // 关系边 fail-open：端点不存在的边跳过（一条坏引用不允许掀翻整回合）。
    for edge in mutation
        .pointer("/edges/upsert")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let from = edge.get("fromId").and_then(Value::as_str).unwrap_or_default();
        let to = edge.get("toId").and_then(Value::as_str).unwrap_or_default();
        let endpoint_exists =
            |id: &str| upserted.contains(id) || db.get_entity(id).is_some();
        if endpoint_exists(from) && endpoint_exists(to) {
            let target = if upserted.contains(to) {
                upsert_entities
                    .iter()
                    .find(|candidate| candidate.get("id").and_then(Value::as_str) == Some(to))
                    .cloned()
            } else {
                db.get_entity(to)
            };
            db.upsert_edge(&normalize_holding_edge(edge, target.as_ref()))?;
        }
    }
    for slot in mutation
        .pointer("/stateSlots/upsert")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        db.upsert_state_slot(&normalize_state_slot(slot))?;
    }
    let event_id = mutation.get("eventId").and_then(Value::as_str).unwrap_or_default();
    for transition in mutation
        .pointer("/evidence/transitions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let entity_id = transition.get("entityId").and_then(Value::as_str).unwrap_or_default();
        db.upsert_state_slot(&json!({
            "id": evidence_status_slot_id(entity_id),
            "ownerEntityId": entity_id,
            "kind": "evidence",
            "label": "证据状态",
            "value": {
                "previous": current_evidence_status(db, entity_id),
                "status": transition.get("to"),
                "reason": transition.get("reason").and_then(Value::as_str).unwrap_or_default(),
            },
            "updatedEventId": event_id,
        }))?;
    }
    Ok(())
}

/// `normalizeHoldingEdge`：holding 边目标非物理实体时降为 observed。
fn normalize_holding_edge(edge: &Value, target: Option<&Value>) -> Value {
    let Some(value) = edge.get("value") else {
        return edge.clone();
    };
    let is_record = value.is_object();
    let is_holding = is_record && value.get("role").and_then(Value::as_str) == Some("holding");
    if !is_holding {
        return edge.clone();
    }
    if is_physical_holding_target(target, value) {
        return edge.clone();
    }
    let mut next = edge.clone();
    if let Some(obj) = next.get_mut("value").and_then(Value::as_object_mut) {
        obj.insert("role".into(), json!("observed"));
    }
    next
}

fn is_physical_holding_target(target: Option<&Value>, value: &Value) -> bool {
    let Some(target) = target else {
        return false;
    };
    if target.get("type").and_then(Value::as_str) == Some("item") {
        return true;
    }
    let physical = value.get("physical").and_then(Value::as_bool) == Some(true)
        || value.get("portable").and_then(Value::as_bool) == Some(true);
    if physical {
        return matches!(
            target.get("type").and_then(Value::as_str),
            Some("evidence") | Some("clue") | Some("claim") | Some("proof_chain")
        );
    }
    false
}

/// `normalizeStateSlot`：数值 current 夹取到 [min, max]。
fn normalize_state_slot(slot: &Value) -> Value {
    let Some(value) = slot.get("value").and_then(Value::as_object) else {
        return slot.clone();
    };
    let Some(current) = value.get("current").and_then(Value::as_f64) else {
        return slot.clone();
    };
    let mut next = current;
    if let Some(min) = value.get("min").and_then(Value::as_f64) {
        next = next.max(min);
    }
    if let Some(max) = value.get("max").and_then(Value::as_f64) {
        next = next.min(max);
    }
    if (next - current).abs() < f64::EPSILON {
        return slot.clone();
    }
    let mut result = slot.clone();
    if let Some(obj) = result.get_mut("value").and_then(Value::as_object_mut) {
        obj.insert("current".into(), json!(next));
    }
    result
}

fn current_evidence_status(db: &PlayGraphDb, entity_id: &str) -> String {
    let slot = db
        .state_slots_for_entity(entity_id)
        .into_iter()
        .find(|slot| {
            slot.get("id").and_then(Value::as_str) == Some(&evidence_status_slot_id(entity_id))
                || slot.get("kind").and_then(Value::as_str) == Some("evidence")
        });
    let Some(status) = slot
        .as_ref()
        .and_then(|slot| slot.get("value"))
        .and_then(|value| value.get("status"))
        .and_then(Value::as_str)
    else {
        return "unknown".to_string();
    };
    if EVIDENCE_ORDER.contains(&status) {
        status.to_string()
    } else {
        "unknown".to_string()
    }
}

fn evidence_status_slot_id(entity_id: &str) -> String {
    format!("evidence:{entity_id}:status")
}

fn evidence_rank(status: &str) -> i32 {
    EVIDENCE_ORDER
        .iter()
        .position(|candidate| *candidate == status)
        .map(|index| index as i32)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mutation(event_id: &str, turn: i64, body: Value) -> Value {
        let mut base = json!({ "eventId": event_id, "turn": turn, "actionKind": "do", "blocked": false });
        if let (Some(obj), Some(body_obj)) = (base.as_object_mut(), body.as_object()) {
            for (key, value) in body_obj {
                obj.insert(key.clone(), value.clone());
            }
        }
        base
    }

    #[test]
    fn reducer_applies_entities_edges_and_holding_normalization() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = open_play_graph_db(dir.path()).unwrap();
        // legacy "player" 规范化 + holding 边目标为 actor（非物理）→ observed。
        let m = mutation("evt-1", 1, json!({
            "summary": "玩家捡起钥匙",
            "entities": { "upsert": [
                { "id": "player", "type": "actor", "label": "旅人", "summary": "" },
                { "id": "item_key", "type": "item", "label": "铜钥匙" },
                { "id": "actor_ghost", "type": "actor", "label": "看门人" }
            ]},
            "edges": { "upsert": [
                { "id": "e1", "fromId": "player", "type": "持有", "toId": "item_key",
                  "value": { "role": "holding" }, "validFromEventId": "evt-1", "sourceEventId": "evt-1" },
                { "id": "e2", "fromId": "player", "type": "信任", "toId": "看门人",
                  "value": { "role": "relation" }, "validFromEventId": "evt-1", "sourceEventId": "evt-1" }
            ]}
        }));
        let applied = apply_play_mutation(&mut db, &m, "捡起钥匙").unwrap();
        assert!(!applied.blocked);
        let snapshot = db.snapshot();
        let entities = snapshot["entities"].as_array().unwrap();
        assert!(entities.iter().any(|e| e["id"] == "actor_player"), "player → actor_player");
        // 别名解析：edges 的 toId "看门人" 应映射到实体 id actor_ghost。
        let edges = snapshot["edges"].as_array().unwrap();
        let trust = edges.iter().find(|e| e["id"] == "e2").unwrap();
        assert_eq!(trust["toId"], "actor_ghost");
        // item 目标保持 holding。
        let hold = edges.iter().find(|e| e["id"] == "e1").unwrap();
        assert_eq!(hold["value"]["role"], "holding");
        // 事件已记录。
        assert_eq!(snapshot["events"].as_array().unwrap().len(), 1);
        assert_eq!(snapshot["events"][0]["rawInput"], "捡起钥匙");
    }

    #[test]
    fn reducer_fail_open_edges_and_slot_clamp() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = open_play_graph_db(dir.path()).unwrap();
        let m = mutation("evt-1", 1, json!({
            "entities": { "upsert": [ { "id": "actor_player", "type": "actor", "label": "旅人" } ]},
            "edges": { "upsert": [
                { "id": "dangling", "fromId": "actor_player", "type": "知道", "toId": "ghost_entity",
                  "value": {}, "validFromEventId": "evt-1", "sourceEventId": "evt-1" }
            ]},
            "stateSlots": { "upsert": [
                { "id": "hp", "ownerEntityId": "actor_player", "kind": "resource", "label": "体力",
                  "value": { "current": 12, "min": 0, "max": 10 }, "updatedEventId": "evt-1" }
            ]}
        }));
        apply_play_mutation(&mut db, &m, "x").unwrap();
        let snapshot = db.snapshot();
        // 悬空边跳过（fail-open）。
        let edges = snapshot["edges"].as_array().unwrap();
        assert!(edges.iter().all(|e| e["id"] != "dangling"), "edges: {edges:?}");
        // 数值槽夹取到 max。
        let slot = snapshot["stateSlots"].as_array().unwrap().iter().find(|s| s["id"] == "hp").unwrap();
        assert_eq!(slot["value"]["current"].as_f64(), Some(10.0));
    }

    #[test]
    fn reducer_evidence_lifecycle_and_regression_guard() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = open_play_graph_db(dir.path()).unwrap();
        let seed = mutation("evt-0", 0, json!({
            "entities": { "upsert": [ { "id": "clue_1", "type": "clue", "label": "血迹" } ]}
        }));
        seed_play_graph(&mut db, &seed).unwrap();

        let advance = mutation("evt-1", 1, json!({
            "entities": { "upsert": [ { "id": "clue_1", "type": "clue", "label": "血迹" } ]},
            "evidence": { "transitions": [ { "entityId": "clue_1", "to": "seen", "reason": "注意到" } ]}
        }));
        apply_play_mutation(&mut db, &advance, "看").unwrap();
        let snapshot = db.snapshot();
        let slot = snapshot["stateSlots"].as_array().unwrap().iter().find(|s| s["id"] == "evidence:clue_1:status").unwrap();
        assert_eq!(slot["value"]["status"], "seen");
        assert_eq!(slot["value"]["previous"], "unknown");

        // 回退 → Err。
        let regress = mutation("evt-2", 2, json!({
            "entities": { "upsert": [ { "id": "clue_1", "type": "clue", "label": "血迹" } ]},
            "evidence": { "transitions": [ { "entityId": "clue_1", "to": "hinted" } ]}
        }));
        let err = apply_play_mutation(&mut db, &regress, "y").unwrap_err();
        assert!(err.contains("cannot regress"), "{err}");

        // 非证据实体 → Err。
        let bad = mutation("evt-3", 3, json!({
            "entities": { "upsert": [ { "id": "actor_x", "type": "actor", "label": "X" } ]},
            "evidence": { "transitions": [ { "entityId": "actor_x", "to": "seen" } ]}
        }));
        assert!(apply_play_mutation(&mut db, &bad, "z").is_err());
    }

    #[test]
    fn sqlite_backend_write_read_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("play.db");
        {
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            let _ = conn;
        }
        let mut db = open_play_graph_db(dir.path()).unwrap();
        assert!(matches!(db.backend, Backend::Sqlite { .. }));
        let m = mutation("evt-1", 1, json!({
            "entities": { "upsert": [ { "id": "actor_player", "type": "actor", "label": "旅人", "summary": "测试" } ]}
        }));
        apply_play_mutation(&mut db, &m, "q").unwrap();
        let snapshot = db.snapshot();
        assert_eq!(snapshot["entities"][0]["label"], "旅人");
        // 71 号读取面（play_graph_snapshot）读同一 sqlite 文件。
        let read_view = crate::play::play_graph_snapshot(dir.path());
        assert_eq!(read_view["entities"][0]["id"], "actor_player");
        assert_eq!(read_view["events"][0]["id"], "evt-1");
    }

    #[test]
    fn blocked_mutation_records_event_without_changes() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = open_play_graph_db(dir.path()).unwrap();
        let m = mutation("evt-9", 9, json!({
            "blocked": true,
            "blockedReason": "门锁着",
            "entities": { "upsert": [ { "id": "e", "type": "item", "label": "X" } ]}
        }));
        let applied = apply_play_mutation(&mut db, &m, "开门").unwrap();
        assert!(applied.blocked);
        let snapshot = db.snapshot();
        assert!(snapshot["entities"].as_array().unwrap().is_empty(), "blocked 不落图");
        assert_eq!(snapshot["events"][0]["outcomeSummary"], "门锁着");
    }
}
