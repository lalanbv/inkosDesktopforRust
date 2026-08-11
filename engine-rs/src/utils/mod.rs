//! 工具函数域（Phase 1 叶子，纯函数，无内部依赖）。
//!
//! 自 `packages/core/src/utils/*.ts` 自下而上移植。每个子模块对应一个 TS 源文件，
//! 移植纪律：行为 1:1 复刻 + golden 差分测试守门（见 `tests/golden/utils/`）。
//!
//! ## 已移植
//! - [`book_id`] / [`cadence_policy`] / [`chapter_cadence`] / [`chapter_memo_parser`]
//! - [`chapter_splitter`] / [`language`] / [`length_metrics`] / [`path`] / [`pov_filter`]
//!
//! ## 待移植
//! context-filter（依赖本模块 DEFAULT_CHAPTER_CADENCE_WINDOW）/ writing-methodology / ...

pub mod analytics;
pub mod book_id;
pub mod cadence_policy;
pub mod chapter_cadence;
pub mod chapter_memo_parser;
pub mod chapter_splitter;
pub mod context_filter;
pub mod hook_governance;
pub mod hook_lifecycle;
pub mod hook_policy;
pub mod language;
pub mod length_metrics;
pub mod llm_endpoint_auth;
pub mod llm_env;
pub mod long_span_fatigue;
pub mod narrative_control;
pub mod path;
pub mod spot_fix_patches;
pub mod pov_filter;

pub use book_id::{assert_safe_book_id, derive_book_id_from_title, is_safe_book_id};
pub use cadence_policy::{resolve_cadence_pressure, CadencePressure, CadencePressureParams};
pub use chapter_cadence::{analyze_chapter_cadence, is_high_tension_mood, ChapterCadenceAnalysis, CadenceSummaryRow};
pub use chapter_memo_parser::{parse_memo, PlannerParseError};
pub use chapter_splitter::{split_chapters, SplitChapter};
pub use pov_filter::{extract_pov_from_outline, filter_hooks_by_pov, filter_matrix_by_pov};
pub use language::{infer_language, WritingLanguage};
pub use length_metrics::{
    build_length_spec, choose_normalize_mode, count_chapter_length, default_chapter_length,
    format_length_count, is_outside_hard_range, is_outside_soft_range,
    resolve_length_counting_mode,
};
pub use path::to_posix_path;
