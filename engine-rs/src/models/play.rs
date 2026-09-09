//! play（互动电影/游玩）模型 —— 类型层。
//!
//! 移植自 `packages/core/src/models/play.ts`（340 行）。
//!
//! ## 范围说明
//! play.ts 的 zod schema 大量使用 `.catch/.transform/.preprocess` 做 **lenient 归一化**
//! （normalizePlayMutation / backfillUpsertIds / backfillEdges / buildLabelToId /
//! normalizeTimeAdvance / edgeIdFromParts / slugifyId 等）——这是 LLM 输出的宽松解析器逻辑，
//! 非纯类型。本文件移植 **z.infer 类型**（解析后的稳定形状）；归一化函数已随
//! `play_parser` 模块移植（含单测，见 play_parser.rs）。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 动作种类。对齐 TS `z.enum(["look","say","move","do","wait"])`。Default = Do（对齐 .catch("do")）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PlayActionKind {
    #[serde(rename = "look")] Look,
    #[serde(rename = "say")] Say,
    #[serde(rename = "move")] Move,
    #[default]
    #[serde(rename = "do")] Do,
    #[serde(rename = "wait")] Wait,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct PlayActionIntent {
    pub action_kind: PlayActionKindDefault, // catch("do")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_entity_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_location_label: Option<String>,
    pub intent: String,
    pub manner: String,
    pub risk: String,
    pub ambiguity: String,
    pub secondary_actions: Vec<String>,
}

/// 带默认 Do 的动作种类（.catch("do") 后必存在）。复用 PlayActionKind + 默认。
pub type PlayActionKindDefault = PlayActionKind;

/// 实体类型。11 值枚举。Default = Actor（仅 deserialize 兜底用；实际校验在 play_parser）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PlayEntityType {
    #[default]
    #[serde(rename = "actor")] Actor,
    #[serde(rename = "location")] Location,
    #[serde(rename = "item")] Item,
    #[serde(rename = "evidence")] Evidence,
    #[serde(rename = "clue")] Clue,
    #[serde(rename = "claim")] Claim,
    #[serde(rename = "proof_chain")] ProofChain,
    #[serde(rename = "organization")] Organization,
    #[serde(rename = "rule")] Rule,
    #[serde(rename = "scene")] Scene,
    #[serde(rename = "event")] Event,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct PlayEntity {
    pub id: String,
    #[serde(rename = "type")]
    pub entity_type: PlayEntityType,
    pub label: String,
    pub summary: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_event_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_event_id: Option<String>,
}

pub type PlayVisibility = HashMap<String, String>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PlayEdge {
    pub id: String,
    #[serde(rename = "fromId")]
    pub from_id: String,
    #[serde(rename = "type")]
    pub edge_type: String,
    #[serde(rename = "toId")]
    pub to_id: String,
    #[serde(default)]
    pub value: HashMap<String, serde_json::Value>,
    #[serde(rename = "validFromEventId")]
    pub valid_from_event_id: String,
    #[serde(rename = "validUntilEventId", default)]
    pub valid_until_event_id: Option<String>, // nullable default null
    #[serde(rename = "sourceEventId")]
    pub source_event_id: String,
    #[serde(default)]
    pub visibility: PlayVisibility,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strength: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
}

/// 状态槽种类。7 值枚举。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlayStateSlotKind {
    #[serde(rename = "resource")] Resource,
    #[serde(rename = "relation")] Relation,
    #[serde(rename = "pressure")] Pressure,
    #[serde(rename = "clue")] Clue,
    #[serde(rename = "evidence")] Evidence,
    #[serde(rename = "flag")] Flag,
    #[serde(rename = "timer")] Timer,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PlayStateSlot {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none", rename = "ownerEntityId")]
    pub owner_entity_id: Option<String>, // nullable optional
    pub kind: PlayStateSlotKind,
    pub label: String,
    pub value: serde_json::Value, // z.unknown()
    #[serde(rename = "updatedEventId")]
    pub updated_event_id: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct PlayTimeAdvance {
    pub elapsed: String,
    pub anchor: String,
    pub rationale: String,
    pub synchronized: Vec<String>,
}

/// 证据状态。8 值枚举。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlayEvidenceStatus {
    #[serde(rename = "unknown")] Unknown,
    #[serde(rename = "hinted")] Hinted,
    #[serde(rename = "seen")] Seen,
    #[serde(rename = "collected")] Collected,
    #[serde(rename = "verified")] Verified,
    #[serde(rename = "weaponized")] Weaponized,
    #[serde(rename = "exposed")] Exposed,
    #[serde(rename = "exhausted")] Exhausted,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PlayEvidenceTransition {
    #[serde(rename = "entityId")]
    pub entity_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<PlayEvidenceStatus>,
    #[serde(rename = "to")]
    pub to: PlayEvidenceStatus,
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PlayEvent {
    pub id: String,
    pub turn: u32,
    pub action_kind: PlayActionKind,
    #[serde(rename = "rawInput")]
    pub raw_input: String,
    #[serde(default, rename = "outcomeSummary")]
    pub outcome_summary: String,
    #[serde(skip_serializing_if = "Option::is_none", rename = "timeAdvance")]
    pub time_advance: Option<PlayTimeAdvance>,
    #[serde(rename = "createdAt")]
    pub created_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct PlayEdgeExpire {
    #[serde(rename = "edgeId")]
    pub edge_id: String,
    #[serde(rename = "validUntilEventId")]
    pub valid_until_event_id: String,
    #[serde(default)]
    pub reason: String,
}

// ── Mutation（解析后形状）────────────────────────────────────────
// 注：normalizePlayMutation / backfill* / buildLabelToId / normalizeTimeAdvance 等
// lenient 归一化逻辑见 play_parser 模块（已移植，LLM 输出先归一化再反序列化到本文件类型）。

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct PlayEntityMutation {
    #[serde(default)]
    pub upsert: Vec<PlayEntity>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct PlayEdgeMutation {
    #[serde(default)]
    pub upsert: Vec<PlayEdge>,
    #[serde(default)]
    pub expire: Vec<PlayEdgeExpire>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct PlayStateSlotMutation {
    #[serde(default)]
    pub upsert: Vec<PlayStateSlot>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct PlayEvidenceMutation {
    #[serde(default)]
    pub transitions: Vec<PlayEvidenceTransition>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct PlayMutation {
    #[serde(rename = "eventId")]
    pub event_id: String,
    pub turn: u32,
    pub action_kind: PlayActionKind,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none", rename = "timeAdvance")]
    pub time_advance: Option<PlayTimeAdvance>,
    pub entities: PlayEntityMutation,
    pub edges: PlayEdgeMutation,
    #[serde(rename = "stateSlots")]
    pub state_slots: PlayStateSlotMutation,
    pub evidence: PlayEvidenceMutation,
    pub blocked: bool,
    #[serde(rename = "blockedReason")]
    pub blocked_reason: String,
    pub notes: Vec<String>,
}
