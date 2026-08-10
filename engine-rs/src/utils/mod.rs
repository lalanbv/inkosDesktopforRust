//! 工具函数域（Phase 1 叶子，纯函数，无内部依赖）。
//!
//! 自 `packages/core/src/utils/*.ts` 自下而上移植。每个子模块对应一个 TS 源文件，
//! 移植纪律：行为 1:1 复刻 + golden 差分测试守门（见 `tests/golden/utils/`）。
//!
//! ## 已移植
//! - [`book_id`]：书 ID 派生 + 安全校验（路径遍历/控制字符/shell 元字符防护）
//! - [`cadence_policy`]：节奏压力阈值与判定（medium/high/none）
//! - [`chapter_memo_parser`]：LLM planner 备忘录解析（栅栏/散文剥离 + 严格小节校验）
//! - [`language`]：写作语言推断（CJK vs Latin 占比）
//! - [`length_metrics`]：章节长度度量（zh_chars / en_words + 软硬区间规格）
//! - [`path`]：项目相对路径归一化（Windows `\` → POSIX `/`）
//! - [`pov_filter`]：POV 感知的上下文过滤（信息边界 / 伏笔可见性）
//!
//! ## 待移植（按依赖序）
//! context-filter / chapter-splitter / writing-methodology / story-markdown / ...

pub mod book_id;
pub mod cadence_policy;
pub mod chapter_memo_parser;
pub mod language;
pub mod length_metrics;
pub mod path;
pub mod pov_filter;

pub use book_id::{assert_safe_book_id, derive_book_id_from_title, is_safe_book_id};
pub use cadence_policy::{resolve_cadence_pressure, CadencePressure, CadencePressureParams};
pub use chapter_memo_parser::{parse_memo, PlannerParseError};
pub use pov_filter::{extract_pov_from_outline, filter_hooks_by_pov, filter_matrix_by_pov};
pub use language::{infer_language, WritingLanguage};
pub use length_metrics::{
    build_length_spec, choose_normalize_mode, count_chapter_length, default_chapter_length,
    format_length_count, is_outside_hard_range, is_outside_soft_range,
    resolve_length_counting_mode,
};
pub use path::to_posix_path;
