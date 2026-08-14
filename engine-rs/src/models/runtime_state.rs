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
    #[serde(rename = "progressing")]
    Progressing,
    #[serde(rename = "deferred")]
    Deferred,
    #[serde(rename = "resolved")]
    Resolved,
    /// 默认变体；反序列化未知值（如 "pressured"、"near_payoff"）落入此处，
    /// 原文由 [`HookRecord::status_raw`] 保留。
    #[serde(rename = "open", other)]
    Open,
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
///
/// `Deserialize` 为手写实现：status 原文（"pressured" 等非枚举值）归一化进枚举的
/// 同时保留到 `status_raw`，判定链仍可读原文（见字段注释）。
#[derive(Debug, Clone, Serialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct HookRecord {
    pub hook_id: String,
    pub start_chapter: u32,
    #[serde(rename = "type")]
    pub hook_type: String,
    pub status: HookStatus,
    /// 原始状态串（markdown/DB 单元格里的原文，如 "pressured"、"near_payoff"）。
    ///
    /// TS `StoredHook.status` 是 string，`recycleThreshold` / `isRecycleTerminalStatus`
    /// 等回收判定直接吃原文；Rust 侧 `status` 归一化为 4 值枚举会丢掉这些非枚举值，
    /// 因此用本字段保留原文，判定链经 [`crate::utils::hook_lifecycle::hook_status_text`]
    /// 读取（空串时回退枚举规范名）。序列化跳过空值，规范记录 JSON 形状不变。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub status_raw: String,
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

impl<'de> Deserialize<'de> for HookRecord {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // TS zod `HookStatusSchema` 是严格枚举——非规范 status（"pressured" 等）会让
        // 整个 HooksState 解析失败、回退 markdown。Rust 侧选择更宽容的超集：归一化进
        // 枚举 + 原文存 status_raw，判定链（回收阈值/终态）仍能吃到原文语义。
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Raw {
            hook_id: String,
            start_chapter: u32,
            #[serde(rename = "type")]
            hook_type: String,
            status: String,
            #[serde(default)]
            status_raw: Option<String>,
            last_advanced_chapter: u32,
            #[serde(default)]
            expected_payoff: String,
            #[serde(default)]
            payoff_timing: Option<HookPayoffTiming>,
            #[serde(default)]
            notes: String,
            #[serde(default)]
            depends_on: Option<Vec<String>>,
            #[serde(default)]
            pays_off_in_arc: Option<String>,
            #[serde(default)]
            core_hook: Option<bool>,
            #[serde(default)]
            half_life_chapters: Option<u32>,
            #[serde(default)]
            advanced_count: Option<u32>,
            #[serde(default)]
            promoted: Option<bool>,
        }

        let raw = Raw::deserialize(deserializer)?;
        let status = crate::utils::hook_lifecycle::normalize_stored_hook_status(&raw.status);
        let canonical = crate::utils::hook_lifecycle::hook_status_canonical(status);
        // 显式 statusRaw 优先（自产输出回读）；status 为规范名时留空保持序列化形状稳定；
        // 非规范原文（别名/未知值）保留进 status_raw。
        let status_raw = match raw.status_raw {
            Some(explicit) => explicit,
            None if raw.status == canonical => String::new(),
            None => raw.status,
        };

        Ok(HookRecord {
            hook_id: raw.hook_id,
            start_chapter: raw.start_chapter,
            hook_type: raw.hook_type,
            status,
            status_raw,
            last_advanced_chapter: raw.last_advanced_chapter,
            expected_payoff: raw.expected_payoff,
            payoff_timing: raw.payoff_timing,
            notes: raw.notes,
            depends_on: raw.depends_on,
            pays_off_in_arc: raw.pays_off_in_arc,
            core_hook: raw.core_hook,
            half_life_chapters: raw.half_life_chapters,
            advanced_count: raw.advanced_count,
            promoted: raw.promoted,
        })
    }
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
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
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
