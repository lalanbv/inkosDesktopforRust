//! 数据模型域（Phase 1 叶子，canonical 类型真源）。
//!
//! 自 `packages/core/src/models/*.ts` 移植。所有跨域共享的数据结构定义在此，
//! 经 ts-rs 生成 `.ts` 供前端与 Node sidecar 消费（迁移规划 v1 §5.1 单一真源）。
//!
//! ## 已移植
//! - [`length_governance`]：长度计量/归一化/规格类型（zh_chars / en_words 等）
//! - PoC 占位：`ChapterStatus` / `BookMeta`（将在 Phase 1 后续按 core 实际模型补全）
//!
//! ## 待移植
//! book / chapter / state / runtime-state / genre-profile / style-profile / detection /
//! input-governance / context-compression / play / project / book-rules

pub mod length_governance;

// PoC 占位类型（后续迁到各自文件）——保留以维持 ts-rs 导出测试不破坏。
pub use self::placeholders::{BookMeta, ChapterStatus};

/// PoC 占位类型集合（将在 Phase 1 后续拆分到 book.rs / chapter.rs）。
pub mod placeholders {
    use serde::{Deserialize, Serialize};
    #[cfg(feature = "export-bindings")]
    use ts_rs::TS;

    /// 章节状态机（对齐 packages/core 的 chapter 状态语义）。
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
}
