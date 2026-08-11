//! 翻译域（文本处理）。
//!
//! 移植自 `packages/core/src/translation/text.ts`（89 行，纯函数）。
//! 依赖已移植的 [`crate::utils::split_chapters`]。
//!
//! ## 待移植（需 reqwest / llm）
//! translation/runner（翻译执行流）/ source（爬取源）/ epub/export（文件 IO）。

pub mod text;

pub use text::{
    decode_html, normalize_translation_text, segment_translation_text_vec,
    split_translation_chapters, strip_html, TranslationTextChapter,
};
