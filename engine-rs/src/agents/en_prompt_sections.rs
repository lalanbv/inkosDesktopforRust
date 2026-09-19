//! 英文 prompt 段（en-prompt-sections）。
//!
//! 移植自 `packages/core/src/agents/en-prompt-sections.ts`（纯逻辑）。
//! [`build_english_genre_intro`] 是 [`super::writer_prompts`] 英文分支的段构造依赖。
//! 529 号清偿：TS 侧现仅存 `buildEnglishGenreIntro` 一个导出（c56586ec 后续演进），
//! core_rules / anti_ai_rules / character_method / pre_write_checklist 四函数在 TS
//! 已不存在且 Rust 无引用，随 529 writer 对齐一并删除。

use crate::models::book::BookConfig;
use crate::models::genre_profile::GenreProfile;

/// 题材介绍（英文版）。对齐 TS `buildEnglishGenreIntro`。
pub fn build_english_genre_intro(book: &BookConfig, gp: &GenreProfile) -> String {
    format!(
        "You are a professional {} web fiction author writing for English-speaking platforms (Royal Road, Kindle Unlimited, Scribble Hub).\n\nTarget: {} words per chapter, {} total chapters.\n\nWrite in English. Vary sentence length. Mix short punchy sentences with longer flowing ones. Maintain consistent narrative voice throughout.",
        gp.name, book.chapter_word_count, book.target_chapters
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::book::{BookStatus, Platform};

    fn test_book() -> BookConfig {
        BookConfig {
            series_id: None,
            id: "b1".to_string(),
            title: "t".to_string(),
            platform: Platform::Other,
            genre: "xianxia".to_string(),
            status: BookStatus::Active,
            target_chapters: 300,
            chapter_word_count: 2500,
            language: None,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
            parent_book_id: None,
            fanfic_mode: None,
            series: None,
            writing: None,
            governance: None,
        }
    }

    fn test_gp() -> GenreProfile {
        GenreProfile {
            name: "Xianxia".to_string(),
            pacing_rule: "Fast-paced with breathing room".to_string(),
            power_scaling: true,
            numerical_system: true,
            ..GenreProfile::default()
        }
    }

    #[test]
    fn genre_intro_embeds_counts() {
        let s = build_english_genre_intro(&test_book(), &test_gp());
        assert!(s.starts_with(
            "You are a professional Xianxia web fiction author writing for English-speaking platforms"
        ));
        assert!(s.contains("Target: 2500 words per chapter, 300 total chapters."));
    }
}
