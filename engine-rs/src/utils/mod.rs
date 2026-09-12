//! 工具函数域（Phase 1 叶子，纯函数，无内部依赖）。
//!
//! 自 `packages/core/src/utils/*.ts` 自下而上移植。每个子模块对应一个 TS 源文件，
//! 移植纪律：行为 1:1 复刻 + golden 差分测试守门（见 `tests/golden/utils/`）。
//!
//! ## 已移植
//! - [`book_id`] / [`cadence_policy`] / [`chapter_cadence`] / [`chapter_memo_parser`]
//! - [`chapter_splitter`] / [`language`] / [`length_metrics`] / [`path`] / [`pov_filter`]
//! - [`outline_paths`]：Phase 5 散文大纲路径解析（story_frame/volume_map/roles/
//!   current_state 派生回退）——ContinuityAuditor 等编排 agent 的真相文件入口
//!
//! ## 待移植
//! writing-methodology / ...

pub mod analytics;
pub mod asset_library;
pub mod atomic_file_set;
pub mod book_eval;
pub mod book_id;
pub mod cadence_policy;
pub mod chapter_cadence;
pub mod chapter_memo_parser;
pub mod chapter_splitter;
pub mod context_assembly;
pub mod context_source_tier;
pub mod deconstruction;
pub mod detection_insights;
pub mod entity_roster;
pub mod context_filter;
pub mod governed_context;
pub mod governed_working_set;
pub mod hook_arbiter;
pub mod hook_governance;
pub mod hook_health;
pub mod hook_ledger_validator;
pub mod hook_lifecycle;
pub mod hook_policy;
pub mod hook_promotion;
pub mod hook_stale_detection;
pub mod language;
pub mod length_metrics;
pub mod local_search;
pub mod log_file;
pub mod llm_endpoint_auth;
pub mod llm_env;
pub mod long_span_fatigue;
pub mod memory_retrieval;
pub mod narrative_control;
pub mod outline_paths;
pub mod path;
pub mod planning_materials;
pub mod spot_fix_patches;
pub mod pov_filter;
pub mod info_gap_ledger;
pub mod promise_ledger;
pub mod quality_trend;
pub mod style_feature_engine;
pub mod resume_advice;
pub mod runtime_writer;
pub mod semantic_retrieval;
pub mod story_markdown;
pub mod tension_curve;
pub mod truth_dialect;
pub mod utc_time;
pub mod writing_methodology;

pub use book_id::{assert_safe_book_id, derive_book_id_from_title, is_safe_book_id};
pub use cadence_policy::{resolve_cadence_pressure, CadencePressure, CadencePressureParams};
pub use chapter_cadence::{analyze_chapter_cadence, is_high_tension_mood, ChapterCadenceAnalysis, CadenceSummaryRow};
pub use chapter_memo_parser::{parse_memo, PlannerParseError};
pub use chapter_splitter::{split_chapters, SplitChapter};
pub use pov_filter::{extract_pov_from_outline, filter_hooks_by_pov, filter_matrix_by_pov};
pub use language::{infer_language, utf16_len, WritingLanguage};
pub use length_metrics::{
    build_length_spec, count_chapter_length, default_chapter_length,
    format_length_count, is_outside_hard_range, is_outside_soft_range,
    resolve_length_counting_mode,
};
pub use path::to_posix_path;
pub mod web_search;
