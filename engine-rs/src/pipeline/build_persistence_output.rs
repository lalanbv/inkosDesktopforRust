//! buildPersistenceOutput —— writer 产物在正文被修订后的重分析装配。
//!
//! 移植自 `packages/core/src/pipeline/runner.ts` 的 `buildPersistenceOutput`
//! 私有方法（~40 行编排 + analyzer 调用面）。正文与草稿一致时原样返回；
//! 被审核环修订改变后经 chapter-analyzer 重跑结算，再回填 canonical 正文/
//! 字数并清空 post-write 校验结果（针对旧正文），保留 writer 的 hook 健康
//! 与 token 用量。

use std::path::Path;

use crate::agents::chapter_analyzer::{
    analyze_chapter, ChapterAnalyzerChat, ChapterAnalyzerCtx,
};
use crate::agents::writer::WriteChapterOutput;
use crate::models::book::BookConfig;
use crate::models::input_governance::{ContextPackage, RuleStack};
use crate::models::length_governance::LengthCountingMode;
use crate::utils::length_metrics::count_chapter_length;

/// 入参。
pub struct BuildPersistenceOutputParams<'a> {
    pub book: &'a BookConfig,
    pub book_dir: &'a Path,
    pub chapter_number: u32,
    pub output: &'a WriteChapterOutput,
    pub final_content: &'a str,
    pub counting_mode: LengthCountingMode,
    /// 治理三件（planner/composer 产物）。
    pub context_package: Option<&'a ContextPackage>,
    pub rule_stack: Option<&'a RuleStack>,
    pub chapter_intent: Option<&'a str>,
}

#[derive(Debug, thiserror::Error)]
pub enum BuildPersistenceOutputError {
    #[error("chapter analyzer failed: {0}")]
    Analyzer(String),
}

/// 重分析装配主入口。
pub async fn build_persistence_output(
    chat: &dyn ChapterAnalyzerChat,
    ctx: &ChapterAnalyzerCtx<'_>,
    params: &BuildPersistenceOutputParams<'_>,
) -> Result<WriteChapterOutput, BuildPersistenceOutputError> {
    // 正文与草稿一致 → 无需重分析。
    if params.final_content == params.output.content {
        return Ok(params.output.clone());
    }

    let analyzed = analyze_chapter(
        chat,
        ctx,
        &crate::agents::chapter_analyzer::AnalyzeChapterInput {
            book: params.book,
            book_dir: params.book_dir,
            chapter_number: params.chapter_number,
            chapter_content: params.final_content,
            chapter_title: Some(&params.output.title),
            chapter_intent: params.chapter_intent,
            context_package: params.context_package,
            rule_stack: params.rule_stack,
        },
    )
    .await
    .map_err(|e| BuildPersistenceOutputError::Analyzer(e.to_string()))?;

    Ok(WriteChapterOutput {
        chapter_number: analyzed.chapter_number,
        title: analyzed.title,
        content: params.final_content.to_string(),
        word_count: count_chapter_length(params.final_content, params.counting_mode),
        pre_write_check: analyzed.pre_write_check,
        post_settlement: analyzed.post_settlement,
        // 旧正文的 post-write 结果对新正文无意义。
        post_write_errors: Vec::new(),
        post_write_warnings: Vec::new(),
        // hook 健康与用量保留 writer 侧产物。
        hook_health_issues: params.output.hook_health_issues.clone(),
        // R2/359 号：张力评分同样保留 writer 侧产物（与 hook 健康同语义）。
        tension_metrics: params.output.tension_metrics.clone(),
        token_usage: params.output.token_usage,
        runtime_state_delta: None,
        runtime_state_snapshot: None,
        updated_state: analyzed.updated_state,
        updated_ledger: analyzed.updated_ledger,
        updated_hooks: analyzed.updated_hooks,
        chapter_summary: analyzed.chapter_summary,
        updated_chapter_summaries: None,
        updated_subplots: analyzed.updated_subplots,
        updated_emotional_arcs: analyzed.updated_emotional_arcs,
        updated_character_matrix: analyzed.updated_character_matrix,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::continuity::ChatOutcome;
    use crate::llm::provider::LLMMessage;
    use std::sync::Mutex;

    struct MockChat {
        response: String,
        user_prompts: Mutex<Vec<String>>,
    }

    #[async_trait::async_trait]
    impl ChapterAnalyzerChat for MockChat {
        async fn chat(
            &self,
            messages: Vec<LLMMessage>,
            _temperature: f64,
        ) -> Result<ChatOutcome, String> {
            self.user_prompts
                .lock()
                .unwrap()
                .push(messages[1].content.clone());
            Ok(ChatOutcome {
                content: self.response.clone(),
                usage: None,
            })
        }
    }

    fn book() -> BookConfig {
        BookConfig {
            id: "b".to_string(),
            title: "书".to_string(),
            platform: crate::models::book::Platform::Other,
            genre: "other".to_string(),
            status: crate::models::book::BookStatus::Active,
            target_chapters: 10,
            chapter_word_count: 3000,
            language: Some("zh".to_string()),
            created_at: String::new(),
            updated_at: String::new(),
            parent_book_id: None,
            fanfic_mode: None,
            series: None,
            writing: None,
        }
    }

    fn writer_output(content: &str) -> WriteChapterOutput {
        WriteChapterOutput {
            chapter_number: 3,
            title: "原标题".into(),
            content: content.into(),
            word_count: 10,
            pre_write_check: String::new(),
            post_settlement: String::new(),
            runtime_state_delta: None,
            runtime_state_snapshot: None,
            updated_state: "旧状态".into(),
            updated_ledger: String::new(),
            updated_hooks: "旧伏笔".into(),
            chapter_summary: String::new(),
            updated_chapter_summaries: None,
            updated_subplots: String::new(),
            updated_emotional_arcs: String::new(),
            updated_character_matrix: String::new(),
            post_write_errors: vec![],
            post_write_warnings: vec![],
            hook_health_issues: vec![crate::agents::writer::HookHealthIssue {
                severity: crate::agents::continuity::AuditSeverity::Warning,
                category: "hook-health".into(),
                description: "H01 偏老".into(),
                suggestion: "尽快推进".into(),
            }],
            tension_metrics: None,
            token_usage: crate::agents::writer::TokenUsage {
                prompt_tokens: 10,
                completion_tokens: 20,
                total_tokens: 30,
            },
        }
    }

    #[tokio::test]
    async fn unchanged_content_returns_writer_output_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let chat = MockChat { response: String::new(), user_prompts: Mutex::new(Vec::new()) };
        let ctx = ChapterAnalyzerCtx {
            project_root: dir.path(),
            builtin_genres_dir: dir.path(),
        };
        let output = writer_output("同一段正文");
        let book = book();
        let result = build_persistence_output(
            &chat,
            &ctx,
            &BuildPersistenceOutputParams {
                book: &book,
                book_dir: dir.path(),
                chapter_number: 3,
                output: &output,
                final_content: "同一段正文",
                counting_mode: LengthCountingMode::ZhChars,
                context_package: None,
                rule_stack: None,
                chapter_intent: None,
            },
        )
        .await
        .unwrap();
        // WriteChapterOutput 未派生 PartialEq——逐字段抽查关键位。
        assert_eq!(result.content, output.content);
        assert_eq!(result.title, output.title);
        assert_eq!(result.updated_state, output.updated_state);
        assert_eq!(result.word_count, output.word_count);
        assert!(chat.user_prompts.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn revised_content_reanalyzes_and_rebuilds_output() {
        let dir = tempfile::tempdir().unwrap();
        // genre 档案（builtin 目录）。
        tokio::fs::write(
            dir.path().join("other.md"),
            "---\nname: 都市\nid: other\nchapterTypes: [\"推进章\"]\nfatigueWords: []\nauditDimensions: [1]\n---\n题材正文\n",
        )
        .await
        .unwrap();
        let response = "=== CHAPTER_TITLE ===\n分析标题\n\n=== CHAPTER_CONTENT ===\n(忽略)\n\n=== UPDATED_STATE ===\n新状态\n\n=== UPDATED_HOOKS ===\n新伏笔\n\n=== CHAPTER_SUMMARY ===\n摘要行\n";
        let chat = MockChat {
            response: response.to_string(),
            user_prompts: Mutex::new(Vec::new()),
        };
        let ctx = ChapterAnalyzerCtx {
            project_root: dir.path(),
            builtin_genres_dir: dir.path(),
        };
        let output = writer_output("旧正文。");
        let book = book();
        let result = build_persistence_output(
            &chat,
            &ctx,
            &BuildPersistenceOutputParams {
                book: &book,
                book_dir: dir.path(),
                chapter_number: 3,
                output: &output,
                final_content: "修订后的新正文。",
                counting_mode: LengthCountingMode::ZhChars,
                context_package: None,
                rule_stack: None,
                chapter_intent: None,
            },
        )
        .await
        .unwrap();

        // canonical 正文与字数。
        assert_eq!(result.content, "修订后的新正文。");
        assert_eq!(
            result.word_count,
            count_chapter_length("修订后的新正文。", LengthCountingMode::ZhChars)
        );
        // 重分析产物。
        assert_eq!(result.updated_state, "新状态");
        assert_eq!(result.updated_hooks, "新伏笔");
        assert_eq!(result.chapter_summary, "摘要行");
        // 保留项。
        assert_eq!(result.hook_health_issues.len(), 1);
        assert_eq!(result.token_usage.total_tokens, 30);
        // user prompt 含修订后正文。
        assert!(chat.user_prompts.lock().unwrap()[0].contains("修订后的新正文。"));
    }

    #[tokio::test]
    async fn analyzer_failure_propagates_as_error() {
        struct FailingChat;
        #[async_trait::async_trait]
        impl ChapterAnalyzerChat for FailingChat {
            async fn chat(
                &self,
                _messages: Vec<LLMMessage>,
                _temperature: f64,
            ) -> Result<ChatOutcome, String> {
                Err("chat boom".to_string())
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let ctx = ChapterAnalyzerCtx {
            project_root: dir.path(),
            builtin_genres_dir: dir.path(),
        };
        let output = writer_output("旧正文。");
        let book = book();
        let error = build_persistence_output(
            &FailingChat,
            &ctx,
            &BuildPersistenceOutputParams {
                book: &book,
                book_dir: dir.path(),
                chapter_number: 3,
                output: &output,
                final_content: "新正文。",
                counting_mode: LengthCountingMode::ZhChars,
                context_package: None,
                rule_stack: None,
                chapter_intent: None,
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(
            error,
            BuildPersistenceOutputError::Analyzer(message) if message.contains("chat failed")
        ));
    }
}
