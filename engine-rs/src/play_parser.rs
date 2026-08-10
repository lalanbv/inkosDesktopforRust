//! play 变更的 lenient 归一化（LLM 输出 → PlayMutation 可解析形状）。
//!
//! 移植自 `packages/core/src/models/play.ts` 的 normalizePlayMutation / backfill* /
//! buildLabelToId / normalizeTimeAdvance / edgeIdFromParts / slugifyId /
//! isLowInformationEdgeId + EDGE_KEY_ALIASES。
//!
//! 对自由形态 LLM JSON 做宽松归一化：强制类型、丢坏项、补 id、别名映射、低信息 edge id
//! 重生成。操作 serde_json::Value（输入形状不定），产物再反序列化为 PlayMutation。
//!
//! ## 移植要点
//! - 全部 on Value::Object/Array，无 IO，纯函数 → golden 差分可测
//! - 字符串 trim + `\s+→_` 替换、UTF-16 slice 截断（对齐 JS .slice(0,n)）

use serde_json::{Map, Value};
use std::collections::HashMap;

/// 端点/关系键别名 → 规范键（模型常用替代名）。
const EDGE_KEY_ALIASES: &[(&str, &str)] = &[
    ("from", "fromId"), ("source", "fromId"), ("subject", "fromId"),
    ("to", "toId"), ("target", "toId"), ("object", "toId"),
    ("relation", "type"), ("rel", "type"), ("kind", "type"), ("relationship", "type"),
];

/// `${prefix}_${base 或 x${index}}`，base = value.trim().replace(/\s+/g,"_").slice(0,30)。
fn slugify_id(prefix: &str, value: &Value, index: usize) -> String {
    let base = if let Value::String(s) = value {
        let cleaned: String = s.trim().replace(|c: char| c.is_whitespace(), "_");
        cleaned
    } else {
        String::new()
    };
    let core: String = base.chars().take(30).collect(); // UTF-16 对齐：base 全 ASCII/BMP，chars==码元
    if core.is_empty() {
        format!("{prefix}_x{index}")
    } else {
        format!("{prefix}_{core}")
    }
}

/// `edge_${from}_${type}_${to}`，每段 trim+ws→_+slice(0,48)，空用 fallback。
fn edge_id_from_parts(from_id: &Value, edge_type: &Value, to_id: &Value, index: usize) -> String {
    let clean = |value: &Value, fallback: String| -> String {
        if let Value::String(s) = value {
            let cleaned: String = s.trim().replace(|c: char| c.is_whitespace(), "_");
            let taken: String = cleaned.chars().take(48).collect();
            if !taken.is_empty() {
                return taken;
            }
        }
        fallback
    };
    format!(
        "edge_{}_{}_{}",
        clean(from_id, format!("from{index}")),
        clean(edge_type, "rel".into()),
        clean(to_id, format!("to{index}")),
    )
}

/// 给 upsert 数组中缺 id 的项补 id（从 labelKey 派生）。
fn backfill_upsert_ids(container: &Value, prefix: &str, label_key: &str) -> Value {
    let mut c = match container {
        Value::Object(m) => m.clone(),
        other => return other.clone(),
    };
    let upsert = match c.get("upsert") {
        Some(Value::Array(arr)) => arr.clone(),
        _ => return Value::Object(c),
    };
    let new_arr: Vec<Value> = upsert
        .into_iter()
        .enumerate()
        .map(|(i, item)| {
            let mut obj = match item {
                Value::Object(m) => m,
                other => return other,
            };
            let has_id = obj
                .get("id")
                .and_then(|v| v.as_str())
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false);
            if has_id {
                return Value::Object(obj);
            }
            let label = obj.get(label_key).cloned().unwrap_or(Value::Null);
            obj.insert("id".into(), Value::String(slugify_id(prefix, &label, i)));
            Value::Object(obj)
        })
        .collect();
    c.insert("upsert".into(), Value::Array(new_arr));
    Value::Object(c)
}

/// entities.upsert → label/id → id 映射（边可按 label 解析端点）。
fn build_label_to_id(entities: &Value) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let upsert = match entities.get("upsert") {
        Some(Value::Array(arr)) => arr,
        _ => return map,
    };
    for item in upsert {
        let o = match item {
            Value::Object(m) => m,
            _ => continue,
        };
        let id = match o.get("id").and_then(|v| v.as_str()) {
            Some(s) if !s.trim().is_empty() => s.trim().to_string(),
            _ => continue,
        };
        map.insert(id.clone(), id.clone());
        if let Some(label) = o.get("label").and_then(|v| v.as_str()) {
            let trimmed = label.trim();
            if !trimmed.is_empty() {
                map.insert(trimmed.to_string(), id.clone());
            }
        }
    }
    map
}

/// edge id 是否低信息（空 / `edge` / `edge_${type}`）——需重生成以避免同回合覆盖。
fn is_low_information_edge_id(id: &Value, edge_type: &Value) -> bool {
    let edge_id = match id.as_str() {
        Some(s) => s.trim(),
        None => return true,
    };
    if edge_id.is_empty() {
        return true;
    }
    let relation_type: String = match edge_type.as_str() {
        Some(s) => s.trim().replace(|c: char| c.is_whitespace(), "_"),
        None => String::new(),
    };
    if !relation_type.is_empty() {
        edge_id == format!("edge_{relation_type}")
    } else {
        edge_id == "edge"
    }
}

/// 补 edge 的 id/event 引用、按别名映射键、按 label 解析端点。
fn backfill_edges(container: &Value, event_id: &str, label_to_id: &HashMap<String, String>) -> Value {
    let mut c = match container {
        Value::Object(m) => m.clone(),
        other => return other.clone(),
    };
    let upsert = match c.get("upsert") {
        Some(Value::Array(arr)) => arr.clone(),
        _ => return Value::Object(c),
    };
    let has_text = |v: &Value| v.as_str().map(|s| !s.trim().is_empty()).unwrap_or(false);
    let resolve = |v: &Value| -> Value {
        if let Some(s) = v.as_str() {
            let trimmed = s.trim();
            if !trimmed.is_empty() {
                if let Some(id) = label_to_id.get(trimmed) {
                    return Value::String(id.clone());
                }
            }
        }
        v.clone()
    };
    let new_arr: Vec<Value> = upsert
        .into_iter()
        .enumerate()
        .map(|(i, item)| {
            let mut o: Map<String, Value> = match item {
                Value::Object(m) => m,
                _ => return item,
            };
            // 别名 → 规范键
            for (alias, canonical) in EDGE_KEY_ALIASES {
                if !has_text(o.get(*canonical).unwrap_or(&Value::Null)) && has_text(o.get(*alias).unwrap_or(&Value::Null)) {
                    o.insert((*canonical).into(), o.get(*alias).cloned().unwrap_or(Value::Null));
                }
            }
            // 按 label 解析端点
            let from = o.get("fromId").cloned().unwrap_or(Value::Null);
            o.insert("fromId".into(), resolve(&from));
            let to = o.get("toId").cloned().unwrap_or(Value::Null);
            o.insert("toId".into(), resolve(&to));
            if !has_text(o.get("validFromEventId").unwrap_or(&Value::Null)) {
                o.insert("validFromEventId".into(), Value::String(event_id.to_string()));
            }
            if !has_text(o.get("sourceEventId").unwrap_or(&Value::Null)) {
                o.insert("sourceEventId".into(), Value::String(event_id.to_string()));
            }
            if is_low_information_edge_id(o.get("id").unwrap_or(&Value::Null), o.get("type").unwrap_or(&Value::Null)) {
                let fid = o.get("fromId").cloned().unwrap_or(Value::Null);
                let ty = o.get("type").cloned().unwrap_or(Value::Null);
                let tid = o.get("toId").cloned().unwrap_or(Value::Null);
                o.insert("id".into(), Value::String(edge_id_from_parts(&fid, &ty, &tid, i)));
            }
            Value::Object(o)
        })
        .collect();
    c.insert("upsert".into(), Value::Array(new_arr));
    Value::Object(c)
}

/// 归一化 timeAdvance：标量→{elapsed:String}；对象补别名键。
pub fn normalize_time_advance(value: &Value) -> Value {
    match value {
        Value::Null => Value::Null,
        Value::String(s) if s.is_empty() => Value::Null,
        Value::String(s) => serde_json::json!({ "elapsed": s }),
        Value::Number(n) => serde_json::json!({ "elapsed": n.to_string() }),
        Value::Bool(b) => serde_json::json!({ "elapsed": b.to_string() }),
        Value::Array(_) => Value::Null,
        Value::Object(raw) => {
            let mut normalized = raw.clone();
            if !normalized.contains_key("elapsed") {
                for k in ["duration", "delta", "timePassed", "passed", "label"] {
                    if let Some(v) = raw.get(k) {
                        normalized.insert("elapsed".into(), v.clone());
                        break;
                    }
                }
            }
            if !normalized.contains_key("anchor") {
                for k in ["now", "current", "currentTime", "worldTime", "phase", "clock", "at"] {
                    if let Some(v) = raw.get(k) {
                        normalized.insert("anchor".into(), v.clone());
                        break;
                    }
                }
            }
            if !normalized.contains_key("rationale") {
                for k in ["reason", "why", "because"] {
                    if let Some(v) = raw.get(k) {
                        normalized.insert("rationale".into(), v.clone());
                        break;
                    }
                }
            }
            if !normalized.contains_key("synchronized") {
                for k in ["worldChanges", "concurrent", "simultaneous", "offscreen"] {
                    if let Some(v) = raw.get(k) {
                        normalized.insert("synchronized".into(), v.clone());
                        break;
                    }
                }
            }
            if let Some(Value::String(s)) = normalized.get("synchronized").cloned() {
                let arr = if s.trim().is_empty() {
                    Value::Array(vec![])
                } else {
                    Value::Array(vec![Value::String(s)])
                };
                normalized.insert("synchronized".into(), arr);
            }
            Value::Object(normalized)
        }
    }
}

/// 归一化 play 变更：容器形状、notes、timeAdvance、turn/eventId 推导、entity/edge id 补全。
pub fn normalize_play_mutation(value: &Value) -> Value {
    let mut v = match value {
        Value::Object(m) => m.clone(),
        other => return other.clone(),
    };

    // 裸数组 → {upsert}/{transitions}
    if let Some(Value::Array(_)) = v.get("entities") {
        let arr = v.get("entities").cloned().unwrap_or(Value::Array(vec![]));
        v.insert("entities".into(), serde_json::json!({ "upsert": arr }));
    }
    if let Some(Value::Array(_)) = v.get("edges") {
        let arr = v.get("edges").cloned().unwrap_or(Value::Array(vec![]));
        v.insert("edges".into(), serde_json::json!({ "upsert": arr }));
    }
    if let Some(Value::Array(_)) = v.get("stateSlots") {
        let arr = v.get("stateSlots").cloned().unwrap_or(Value::Array(vec![]));
        v.insert("stateSlots".into(), serde_json::json!({ "upsert": arr }));
    }
    if let Some(Value::Array(_)) = v.get("evidence") {
        let arr = v.get("evidence").cloned().unwrap_or(Value::Array(vec![]));
        v.insert("evidence".into(), serde_json::json!({ "transitions": arr }));
    }
    // notes: string → trim?[v]:[]
    if let Some(Value::String(s)) = v.get("notes").cloned() {
        let arr = if s.trim().is_empty() {
            Value::Array(vec![])
        } else {
            Value::Array(vec![Value::String(s)])
        };
        v.insert("notes".into(), arr);
    }
    // timeAdvance ← time（若 timeAdvance 缺）
    if v.get("timeAdvance").is_none() && v.contains_key("time") {
        let t = v.get("time").cloned().unwrap_or(Value::Null);
        v.insert("timeAdvance".into(), t);
    }
    if let Some(ta) = v.get("timeAdvance").cloned() {
        v.insert("timeAdvance".into(), normalize_time_advance(&ta));
    }

    // turn 推导
    let turn: Option<i64> = match v.get("turn") {
        Some(Value::Number(n)) if n.is_i64() && n.as_i64().unwrap_or(-1) >= 0 => n.as_i64(),
        Some(Value::String(s)) if s.trim().chars().all(|c| c.is_ascii_digit()) && !s.trim().is_empty() => {
            s.trim().parse::<i64>().ok().filter(|n| *n >= 0)
        }
        _ => None,
    };
    let event_id: String = match v.get("eventId").and_then(|x| x.as_str()) {
        Some(s) if !s.trim().is_empty() => s.to_string(),
        _ => match turn {
            Some(t) => format!("evt-{t}"),
            None => String::new(),
        },
    };
    v.insert("eventId".into(), Value::String(event_id.clone()));

    let entities = v.get("entities").cloned().unwrap_or(Value::Null);
    let entities = backfill_upsert_ids(&entities, "ent", "label");
    v.insert("entities".into(), entities.clone());

    let state_slots = v.get("stateSlots").cloned().unwrap_or(Value::Null);
    v.insert("stateSlots".into(), backfill_upsert_ids(&state_slots, "slot", "label"));

    let label_to_id = build_label_to_id(&entities);
    let edges = v.get("edges").cloned().unwrap_or(Value::Null);
    let edge_event = if event_id.is_empty() { "evt-0".to_string() } else { event_id };
    v.insert("edges".into(), backfill_edges(&edges, &edge_event, &label_to_id));

    Value::Object(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_and_edge_id() {
        assert_eq!(slugify_id("ent", &Value::String("林 动".into()), 0), "ent_林_动");
        assert_eq!(slugify_id("ent", &Value::Null, 5), "ent_x5");
        assert_eq!(
            edge_id_from_parts(&Value::String("a b".into()), &Value::String("holds".into()), &Value::String("c".into()), 1),
            "edge_a_b_holds_c"
        );
    }

    #[test]
    fn backfill_upsert_ids_fills_missing() {
        let input = serde_json::json!({ "upsert": [{ "label": "主角" }, { "id": "e2", "label": "x" }] });
        let out = backfill_upsert_ids(&input, "ent", "label");
        let arr = out.get("upsert").unwrap().as_array().unwrap();
        assert_eq!(arr[0].get("id").unwrap().as_str().unwrap(), "ent_主角");
        assert_eq!(arr[1].get("id").unwrap().as_str().unwrap(), "e2"); // 已有 id 保留
    }

    #[test]
    fn normalize_time_advance_scalar_and_object() {
        assert_eq!(normalize_time_advance(&Value::String("1小时".into())), serde_json::json!({ "elapsed": "1小时" }));
        let obj = serde_json::json!({ "duration": "2h", "reason": "休息" });
        let out = normalize_time_advance(&obj);
        assert_eq!(out.get("elapsed").unwrap().as_str().unwrap(), "2h");
        assert_eq!(out.get("rationale").unwrap().as_str().unwrap(), "休息");
    }

    #[test]
    fn normalize_play_mutation_bare_arrays_and_ids() {
        let input = serde_json::json!({
            "entities": [{ "label": "主角" }, { "label": "配角" }],
            "edges": [{ "from": "主角", "type": "认识", "to": "配角" }],
            "evidence": [{ "entityId": "e1", "to": "seen" }],
            "notes": "  ",
            "turn": 3
        });
        let out = normalize_play_mutation(&input);
        // 裸数组 → upsert/transitions 容器
        assert!(out["entities"]["upsert"].is_array());
        assert!(out["evidence"]["transitions"].is_array());
        // entity id 补全
        assert_eq!(out["entities"]["upsert"][0]["id"].as_str().unwrap(), "ent_主角");
        // notes 空 trim → []
        assert_eq!(out["notes"].as_array().unwrap().len(), 0);
        // eventId 从 turn 派生
        assert_eq!(out["eventId"].as_str().unwrap(), "evt-3");
        // edge 别名 from→fromId + 按 label 解析端点
        assert!(out["edges"]["upsert"][0]["fromId"].as_str().unwrap().contains("ent_主角"));
        assert!(out["edges"]["upsert"][0]["id"].as_str().unwrap().starts_with("edge_"));
    }

    #[test]
    fn low_information_edge_id_detection() {
        assert!(is_low_information_edge_id(&Value::Null, &Value::String("x".into())));
        assert!(is_low_information_edge_id(&Value::String("edge".into()), &Value::Null));
        assert!(is_low_information_edge_id(&Value::String("edge_holds".into()), &Value::String("holds".into())));
        assert!(!is_low_information_edge_id(&Value::String("edge_a_holds_b".into()), &Value::String("holds".into())));
    }
}
