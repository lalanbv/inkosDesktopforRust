//! 运行时状态模型（增量 delta + hook/summary/fact 持久化结构）。
//!
//! 移植自 `packages/core/src/models/runtime-state.ts`（144 行，纯类型）。

use serde::{Deserialize, Serialize};
#[cfg(feature = "export-bindings")]
use ts_rs::TS;
use std::collections::HashMap;

/// 状态清单（schemaVersion 固定 2）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct StateManifest {
    pub schema_version: u32, // z.literal(2)
    pub language: String,    // zh | en
    pub last_applied_chapter: u32,
    pub projection_version: u32,
    #[serde(default)]
    pub migration_warnings: Vec<String>,
}

/// 伏笔状态。对齐 TS `"open" | "progressing" | "deferred" | "resolved"`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(
    feature = "export-bindings",
    ts(export, type = "\"open\" | \"progressing\" | \"deferred\" | \"resolved\"")
)]
pub enum HookStatus {
    #[serde(rename = "open")]
    Open,
    #[serde(rename = "progressing")]
    Progressing,
    #[serde(rename = "deferred")]
    Deferred,
    #[serde(rename = "resolved")]
    Resolved,
}

/// 伏笔回收节奏。对齐 TS 5 值枚举。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(
    feature = "export-bindings",
    ts(export, type = "\"immediate\" | \"near-term\" | \"mid-arc\" | \"slow-burn\" | \"endgame\"")
)]
pub enum HookPayoffTiming {
    #[serde(rename = "immediate")]
    Immediate,
    #[serde(rename = "near-term")]
    NearTerm,
    #[serde(rename = "mid-arc")]
    MidArc,
    #[serde(rename = "slow-burn")]
    SlowBurn,
    #[serde(rename = "endgame")]
    Endgame,
}

/// 伏笔记录（持久化行）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct HookRecord {
    pub hook_id: String,
    pub start_chapter: u32,
    #[serde(rename = "type")]
    pub hook_type: String,
    pub status: HookStatus,
    pub last_advanced_chapter: u32,
    #[serde(default)]
    pub expected_payoff: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payoff_timing: Option<HookPayoffTiming>,
    #[serde(default)]
    pub notes: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub depends_on: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pays_off_in_arc: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub core_hook: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub half_life_chapters: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub advanced_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub promoted: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
pub struct HooksState {
    #[serde(default)]
    pub hooks: Vec<HookRecord>,
}

/// 章节摘要行（持久化；比 chapter_cadence 的输入行字段更全）。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase", default)]
pub struct ChapterSummaryRow {
    pub chapter: u32,
    pub title: String,
    pub characters: String,
    pub events: String,
    pub state_changes: String,
    pub hook_activity: String,
    pub mood: String,
    pub chapter_type: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
pub struct ChapterSummariesState {
    #[serde(default)]
    pub rows: Vec<ChapterSummaryRow>,
}

/// 当前状态事实（SPO 三元组 + 章节有效性）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct CurrentStateFact {
    pub subject: String,
    pub predicate: String,
    pub object: String,
    pub valid_from_chapter: u32,
    pub valid_until_chapter: Option<u32>, // nullable
    pub source_chapter: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct CurrentStateState {
    pub chapter: u32,
    #[serde(default)]
    pub facts: Vec<CurrentStateFact>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct CurrentStatePatch {
    #[serde(skip_serializing_if = "Option::is_none", rename = "currentLocation")]
    pub current_location: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "protagonistState")]
    pub protagonist_state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "currentGoal")]
    pub current_goal: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "currentConstraint")]
    pub current_constraint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "currentAlliances")]
    pub current_alliances: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "currentConflict")]
    pub current_conflict: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
pub struct HookOps {
    #[serde(default)]
    pub upsert: Vec<HookRecord>,
    #[serde(default)]
    pub mention: Vec<String>,
    #[serde(default)]
    pub resolve: Vec<String>,
    #[serde(default)]
    pub defer: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct NewHookCandidate {
    #[serde(rename = "type")]
    pub hook_type: String,
    #[serde(default)]
    pub expected_payoff: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payoff_timing: Option<HookPayoffTiming>,
    #[serde(default)]
    pub notes: String,
}

/// 章节状态增量（architect/consolidator 产出，应用到持久化状态）。
/// 不 derive ts-rs（含 serde_json::Value 的 *Ops 字段，serde 契约优先）。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeStateDelta {
    pub chapter: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_state_patch: Option<CurrentStatePatch>,
    #[serde(default)]
    pub hook_ops: HookOps,
    #[serde(default)]
    pub new_hook_candidates: Vec<NewHookCandidate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chapter_summary: Option<ChapterSummaryRow>,
    #[serde(default)]
    pub subplot_ops: Vec<HashMap<String, serde_json::Value>>,
    #[serde(default)]
    pub emotional_arc_ops: Vec<HashMap<String, serde_json::Value>>,
    #[serde(default)]
    pub character_matrix_ops: Vec<HashMap<String, serde_json::Value>>,
    #[serde(default)]
    pub notes: Vec<String>,
}
