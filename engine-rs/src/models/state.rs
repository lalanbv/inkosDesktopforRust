//! 规范状态文件（每本书的三真相源）。
//!
//! 移植自 `packages/core/src/models/state.ts`（52 行，纯类型）。
//! 这些以 markdown 持久化，但在此定义类型契约。

use serde::{Deserialize, Serialize};
#[cfg(feature = "export-bindings")]
use ts_rs::TS;

/// 当前状态（章节/地点/主角/敌人/已知真相/当前冲突/锚点）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct CurrentState {
    pub chapter: u32,
    pub location: String,
    pub protagonist: ProtagonistState,
    pub enemies: Vec<EnemyState>,
    pub known_truths: Vec<String>,
    pub current_conflict: String,
    pub anchor: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ProtagonistState {
    pub status: String,
    pub current_goal: String,
    pub constraints: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct EnemyState {
    pub name: String,
    pub relationship: String,
    pub threat: String,
}

/// 粒子账本条目（资源收支）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct LedgerEntry {
    pub chapter: u32,
    pub opening_value: f64,
    pub source: String,
    pub resource_completeness: String,
    pub delta: f64,
    pub closing_value: f64,
    pub basis: String,
}

/// 粒子账本（硬上限 + 当前总量 + 条目）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ParticleLedger {
    pub hard_cap: f64,
    pub current_total: f64,
    pub entries: Vec<LedgerEntry>,
}

/// 待回收伏笔。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct PendingHook {
    pub id: String,
    pub origin_chapter: u32,
    #[serde(rename = "type")]
    pub hook_type: String,
    pub status: PendingHookStatus,
    pub last_progress: String,
    pub expected_resolution: String,
    pub note: String,
}

/// 伏笔状态。对齐 TS `"open" | "progressing" | "resolved"`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export, type = "\"open\" | \"progressing\" | \"resolved\""))]
pub enum PendingHookStatus {
    #[serde(rename = "open")]
    Open,
    #[serde(rename = "progressing")]
    Progressing,
    #[serde(rename = "resolved")]
    Resolved,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
pub struct PendingHooks {
    pub hooks: Vec<PendingHook>,
}
