//! 数据模型域（Phase 1 叶子，无内部依赖，最先移植）。
//!
//! 作为整个引擎的 **canonical 类型真源**：所有跨域共享的数据结构定义在此，
//! 经 ts-rs 生成 `.ts` 供前端与 Node sidecar 消费（见迁移规划 v1 §5.1）。
//!
//! ## PoC：ts-rs 类型生成
//! `cargo test --features export-bindings` 会把带 `#[derive(TS)]` 的类型
//! 导出到 `../bindings/`（由 ts-rs 默认行为决定，可配置）。前端 import 后，
//! Rust 改类型 → 重新生成 → tsc 立即报错，单一真源不漂移。

use serde::{Deserialize, Serialize};

#[cfg(feature = "export-bindings")]
use ts_rs::TS;

/// 章节状态机（对齐 packages/core 的 chapter 状态语义）。
/// 移植时须与 TS 版逐字段比对，golden 差分测试守门。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
pub enum ChapterStatus {
    Draft,
    Planned,
    Written,
    Reviewed,
    Approved,
    Rejected,
}

/// 书籍元数据（骨架字段，后续按 core 实际模型补全）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
pub struct BookMeta {
    pub id: String,
    pub title: String,
    pub genre: String,
    pub language: String,
    pub target_words: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 序列化往返：确保 serde 行为确定（与 TS JSON 契约对齐的基础）。
    #[test]
    fn chapter_status_roundtrips_through_json() {
        for s in [
            ChapterStatus::Draft,
            ChapterStatus::Written,
            ChapterStatus::Approved,
        ] {
            let json = serde_json::to_string(&s).unwrap();
            let back: ChapterStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(s, back, "往返失真: {json}");
        }
    }

    #[test]
    fn book_meta_serializes_with_expected_fields() {
        let m = BookMeta {
            id: "b1".into(),
            title: "示例".into(),
            genre: "litrpg".into(),
            language: "zh".into(),
            target_words: 80000,
        };
        let v: serde_json::Value = serde_json::to_value(&m).unwrap();
        assert_eq!(v["id"], "b1");
        assert_eq!(v["target_words"], 80000);
    }
}
