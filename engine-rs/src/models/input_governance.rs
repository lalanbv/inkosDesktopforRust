//! 输入治理类型。
//!
//! 移植自 `packages/core/src/models/input-governance.ts`。
//! 本模块先移植 [`ChapterMemo`]（chapter-memo-parser 的产物）；其余类型
//! （ChapterIntent / ContextSource / ContextPackage / RuleLayerScope）按下游域移植进度补全。

use serde::{Deserialize, Serialize};
#[cfg(feature = "export-bindings")]
use ts_rs::TS;

/// 章节规划备忘录（LLM planner 输出经 [`crate::utils::chapter_memo_parser::parse_memo`] 解析后产物）。
///
/// 字段契约对齐 TS `ChapterMemoSchema`：
/// - `chapter`：`z.number().int().min(1)` → u32（≥1 由解析器入参保证）
/// - `goal`：`z.string().min(1).max(50)` → 显示用短目标（≤50 UTF-16 码元）
/// - `is_golden_opening`：`z.boolean().default(false)`
/// - `body`：`z.string().min(1)` → 完整 memo 正文（含完整目标）
/// - `thread_refs`：`z.array(z.string()).default([])` → 关联线索 ID（去重保序）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ChapterMemo {
    pub chapter: u32,
    pub goal: String,
    pub is_golden_opening: bool,
    pub body: String,
    pub thread_refs: Vec<String>,
}
