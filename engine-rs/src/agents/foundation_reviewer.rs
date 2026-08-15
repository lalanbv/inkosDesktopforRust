//! 基础设定审核器（foundation-reviewer，58 号）。
//!
//! 移植自 `packages/core/src/agents/foundation-reviewer.ts`（210 行）+
//! `runner.ts` 的 `generateAndReviewFoundation` 审核环（L535-L633）：
//! 五维打分（0-100，总分 ≥80 且无单维 <60 通过）→ 未过带反馈重生成
//! （maxRetries 默认 2）→ 终审兜底接受。

use regex::Regex;
use std::sync::OnceLock;

use crate::agents::architect::ArchitectOutput;
use crate::agents::continuity::ChatOutcome;
use crate::llm::provider::{LLMMessage, LLMRole};
use crate::utils::language::WritingLanguage;

/// 审核 chat 端口。
#[async_trait::async_trait]
pub trait FoundationReviewerChat: Send + Sync {
    async fn chat(&self, messages: Vec<LLMMessage>, temperature: f64) -> Result<ChatOutcome, String>;
}

const PASS_THRESHOLD: i64 = 80;
const DIMENSION_FLOOR: i64 = 60;

/// 审核结果。对齐 TS `FoundationReviewResult`。
#[derive(Debug, Clone, PartialEq)]
pub struct FoundationReviewResult {
    pub passed: bool,
    pub total_score: i64,
    pub dimensions: Vec<FoundationReviewDimension>,
    pub overall_feedback: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FoundationReviewDimension {
    pub name: String,
    pub score: i64,
    pub feedback: String,
}

/// 审核模式（原创 / 同人 / 系列续写）。对齐 TS `mode`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FoundationReviewMode {
    Original,
    Fanfic,
    Series,
}

/// 审核参数。
pub struct ReviewParams<'a> {
    pub foundation: &'a ArchitectOutput,
    pub mode: FoundationReviewMode,
    pub source_canon: Option<&'a str>,
    pub style_guide: Option<&'a str>,
    pub language: WritingLanguage,
    pub target_chapters: Option<u32>,
}

/// 审核主入口（temp 0.3）。对齐 TS `FoundationReviewerAgent.review`。
pub async fn review_foundation(
    chat: &dyn FoundationReviewerChat,
    params: &ReviewParams<'_>,
) -> Result<FoundationReviewResult, String> {
    let canon_block = params
        .source_canon
        .filter(|c| !c.is_empty())
        .map(|c| format!("\n## 原作正典参照\n{c}\n"))
        .unwrap_or_default();
    let style_block = params
        .style_guide
        .filter(|s| !s.is_empty())
        .map(|s| format!("\n## 原作风格参照\n{s}\n"))
        .unwrap_or_default();

    let dimensions: Vec<String> = match params.mode {
        FoundationReviewMode::Original => original_dimensions(params.language, params.target_chapters),
        FoundationReviewMode::Fanfic | FoundationReviewMode::Series => {
            derivative_dimensions(params.language, params.mode)
        }
    };

    let dimension_list = dimensions
        .iter()
        .enumerate()
        .map(|(i, dim)| format!("{}. {dim}", i + 1))
        .collect::<Vec<_>>()
        .join("\n");
    let system_prompt = if params.language == WritingLanguage::En {
        format!(
            "You are a senior fiction editor reviewing a new book's foundation (worldbuilding + outline + rules).\n\nScore each dimension (0-100) with specific feedback:\n\n{dimension_list}\n\n## Scoring\n- 80+ Pass — ready to write\n- 60-79 Needs revision\n- <60 Fundamental direction problem\n\n## Output format (strict)\n=== DIMENSION: 1 ===\nScore: {{0-100}}\nFeedback: {{specific feedback}}\n\n=== DIMENSION: 2 ===\nScore: {{0-100}}\nFeedback: {{specific feedback}}\n\n...\n\n=== OVERALL ===\nTotal: {{weighted average}}\nPassed: {{yes/no}}\nSummary: {{1-2 paragraphs — biggest problem and best quality}}\n{canon_block}{style_block}\nBe strict. 80 means \"ready to write without changes.\""
        )
    } else {
        format!(
            "你是一位资深小说编辑，正在审核一本新书的基础设定（世界观 + 大纲 + 规则）。\n\n你需要从以下维度逐项打分（0-100），并给出具体意见：\n\n{dimension_list}\n\n## 评分标准\n- 80+ 通过，可以开始写作\n- 60-79 有明显问题，需要修改\n- <60 方向性错误，需要重新设计\n\n## 输出格式（严格遵守）\n=== DIMENSION: 1 ===\n分数：{{0-100}}\n意见：{{具体反馈}}\n\n=== DIMENSION: 2 ===\n分数：{{0-100}}\n意见：{{具体反馈}}\n\n...（每个维度一个 block）\n\n=== OVERALL ===\n总分：{{加权平均}}\n通过：{{是/否}}\n总评：{{1-2段总结，指出最大的问题和最值得保留的优点}}\n{canon_block}{style_block}\n审核时要严格。不要因为\"还行\"就给高分。80分意味着\"可以直接开写，不需要改\"。"
        )
    };
    let f = params.foundation;
    let user_prompt = if params.language == WritingLanguage::En {
        format!(
            "## Story Bible\n{}\n\n## Volume Outline\n{}\n\n## Book Rules\n{}\n\n## Initial State\n{}\n\n## Initial Hooks\n{}",
            f.story_bible, f.volume_outline, f.book_rules, f.current_state, f.pending_hooks
        )
    } else {
        format!(
            "## 世界设定\n{}\n\n## 卷纲\n{}\n\n## 规则\n{}\n\n## 初始状态\n{}\n\n## 初始伏笔\n{}",
            f.story_bible, f.volume_outline, f.book_rules, f.current_state, f.pending_hooks
        )
    };

    let response = chat
        .chat(
            vec![
                LLMMessage { role: LLMRole::System, content: system_prompt },
                LLMMessage { role: LLMRole::User, content: user_prompt },
            ],
            0.3,
        )
        .await?;
    Ok(parse_review_result(&response.content, &dimensions))
}

/// 原创模式五维（目标章数自适应窗口）。对齐 TS `originalDimensions`。
fn original_dimensions(language: WritingLanguage, target_chapters: Option<u32>) -> Vec<String> {
    let target = match target_chapters {
        Some(t) if t > 0 => t.min(1_000_000),
        _ => 40,
    };
    let opening_window = target.min(5);
    let repeat_window = target.clamp(3, 10);
    if language == WritingLanguage::En {
        vec![
            format!("Core Conflict (Is there a clear, compelling central conflict that can sustain the requested {target} chapters?)"),
            format!("Opening Momentum (Can the first {opening_window} chapters create a page-turning hook?)"),
            "World Coherence (Is the worldbuilding internally consistent and specific?)".to_string(),
            "Character Differentiation (Are the main characters distinct in voice and motivation?)".to_string(),
            format!("Pacing Feasibility (Does the outline fit the requested {target} chapters and avoid repeating the same beat for {repeat_window} chapters?)"),
        ]
    } else {
        vec![
            format!("核心冲突（是否有清晰且有足够张力的核心冲突支撑用户要求的{target}章？）"),
            format!("开篇节奏（前{opening_window}章能否形成翻页驱动力？）"),
            "世界一致性（世界观是否内洽且具体？）".to_string(),
            "角色区分度（主要角色的声音和动机是否各不相同？）".to_string(),
            format!("节奏可行性（大纲是否适配用户要求的{target}章，并避免连续{repeat_window}章同一种节拍？）"),
        ]
    }
}

/// 衍生模式（同人/系列）五维。对齐 TS `derivativeDimensions`。
fn derivative_dimensions(language: WritingLanguage, mode: FoundationReviewMode) -> Vec<String> {
    let mode_label = match mode {
        FoundationReviewMode::Fanfic => {
            if language == WritingLanguage::En { "Fan Fiction" } else { "同人" }
        }
        _ => {
            if language == WritingLanguage::En { "Series" } else { "系列" }
        }
    };
    if language == WritingLanguage::En {
        vec![
            format!("Source DNA Preservation (Does the {mode_label} respect the original's world rules, character personalities, and established facts?)"),
            "New Narrative Space (Is there a clear divergence point or new territory that gives the story room to be ORIGINAL, not a retelling?)".to_string(),
            "Core Conflict (Is the new story's central conflict compelling and distinct from the original?)".to_string(),
            "Opening Momentum (Can the first 5 chapters create a page-turning hook without requiring 3 chapters of setup?)".to_string(),
            "Pacing Feasibility (Does the outline avoid the trap of re-walking the original's plot beats?)".to_string(),
        ]
    } else {
        vec![
            format!("原作DNA保留（{mode_label}是否尊重原作的世界规则、角色性格、已确立事实？）"),
            "新叙事空间（是否有明确的分岔点或新领域，让故事有原创空间，而非复述原作？）".to_string(),
            "核心冲突（新故事的核心冲突是否有足够张力且区别于原作？）".to_string(),
            "开篇节奏（前5章能否形成翻页驱动力，不需要3章铺垫？）".to_string(),
            "节奏可行性（卷纲是否避免了重走原作剧情节拍的陷阱？）".to_string(),
        ]
    }
}

/// 解析审核输出（维度缺失 → 50 分 + "(parse failed)"）。对齐 TS `parseReviewResult`。
fn parse_review_result(content: &str, dimensions: &[String]) -> FoundationReviewResult {
    let mut parsed: Vec<FoundationReviewDimension> = Vec::with_capacity(dimensions.len());
    for (i, name) in dimensions.iter().enumerate() {
        // TS `([\s\S]*?)(?==== |$)` 的 lookahead 在 Rust regex 不可用——
        // 各维独立全文搜索，消费终止符不影响后续维度匹配。
        let pattern = format!(
            r"=== DIMENSION: {} ===\s*[\s\S]*?(?:分数|Score)[：:]\s*(\d+)[\s\S]*?(?:意见|Feedback)[：:]\s*([\s\S]*?)\s*(?:===|$)",
            i + 1
        );
        let regex = Regex::new(&pattern).expect("dimension regex");
        match regex.captures(content) {
            Some(caps) => {
                let score: i64 = caps[1].parse().unwrap_or(50);
                parsed.push(FoundationReviewDimension {
                    name: name.clone(),
                    score,
                    feedback: caps[2].trim().to_string(),
                });
            }
            None => parsed.push(FoundationReviewDimension {
                name: name.clone(),
                score: 50,
                feedback: "(parse failed)".to_string(),
            }),
        }
    }

    let total_score = if parsed.is_empty() {
        0
    } else {
        (parsed.iter().map(|d| d.score).sum::<i64>() as f64 / parsed.len() as f64).round() as i64
    };
    let any_below_floor = parsed.iter().any(|d| d.score < DIMENSION_FLOOR);
    let passed = total_score >= PASS_THRESHOLD && !any_below_floor;

    let overall_re = overall_re();
    let overall_feedback = overall_re
        .captures(content)
        .map(|c| c[1].trim().to_string())
        .unwrap_or_else(|| "(parse failed)".to_string());

    FoundationReviewResult { passed, total_score, dimensions: parsed, overall_feedback }
}

fn overall_re() -> &'static Regex {
    // TS: /=== OVERALL ===[\s\S]*?(?:总评|Summary)[：:]\s*([\s\S]*?)$/
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"=== OVERALL ===[\s\S]*?(?:总评|Summary)[：:]\s*([\s\S]*?)$").expect("overall regex")
    })
}

/// 审核反馈拼接（重生成时的 reviewFeedback）。对齐 TS `buildFoundationReviewFeedback`。
pub fn build_foundation_review_feedback(review: &FoundationReviewResult, language: WritingLanguage) -> String {
    let dimension_lines = review
        .dimensions
        .iter()
        .map(|dimension| {
            if language == WritingLanguage::En {
                format!("- {} [{}]: {}", dimension.name, dimension.score, dimension.feedback)
            } else {
                format!("- {}（{}分）：{}", dimension.name, dimension.score, dimension.feedback)
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    if language == WritingLanguage::En {
        format!(
            "## Overall Feedback\n{}\n\n## Dimension Notes\n{}",
            review.overall_feedback,
            if dimension_lines.is_empty() { "- none".to_string() } else { dimension_lines }
        )
    } else {
        format!(
            "## 总评\n{}\n\n## 分维度意见\n{}",
            review.overall_feedback,
            if dimension_lines.is_empty() { "- 无".to_string() } else { dimension_lines }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_review_result_full_shape() {
        let dimensions = vec!["维度甲".to_string(), "维度乙".to_string()];
        let content = "\
=== DIMENSION: 1 ===
分数：85
意见：冲突清晰。

=== DIMENSION: 2 ===
分数：90
意见：节奏合理。

=== OVERALL ===
总分：88
通过：是
总评：整体扎实，可以开写。";
        let result = parse_review_result(content, &dimensions);
        assert!(result.passed);
        assert_eq!(result.total_score, 88);
        assert_eq!(result.dimensions[0].score, 85);
        assert_eq!(result.overall_feedback, "整体扎实，可以开写。");
    }

    #[test]
    fn parse_review_floor_and_missing_dimensions() {
        let dimensions = vec!["维度甲".to_string(), "维度乙".to_string()];
        // 维度 2 缺失 → 50 分；总分 68 但无地板跌破？85+50=135/2≈68 <80 → 不过。
        let content = "\
=== DIMENSION: 1 ===
分数：85
意见：好。

=== OVERALL ===
总评：尚可。";
        let result = parse_review_result(content, &dimensions);
        assert!(!result.passed);
        assert_eq!(result.dimensions[1].score, 50);
        assert_eq!(result.dimensions[1].feedback, "(parse failed)");
        // 单维 <60 → 地板拒绝。
        let content2 = "\
=== DIMENSION: 1 ===
分数：90
意见：优。

=== DIMENSION: 2 ===
分数：55
意见：弱。

=== OVERALL ===
总评：一强一弱。";
        let result2 = parse_review_result(content2, &dimensions);
        assert!(!result2.passed, "地板 60 未过应拒绝");
    }

    #[test]
    fn original_dimensions_windows() {
        let zh = original_dimensions(WritingLanguage::Zh, Some(3));
        assert!(zh[1].contains("前3章"));
        assert!(zh[4].contains("连续3章"));
        let default = original_dimensions(WritingLanguage::En, None);
        assert!(default[0].contains("requested 40 chapters"));
    }

    #[test]
    fn review_feedback_bilingual() {
        let review = FoundationReviewResult {
            passed: false,
            total_score: 70,
            dimensions: vec![FoundationReviewDimension {
                name: "核心冲突".to_string(),
                score: 70,
                feedback: "张力不足".to_string(),
            }],
            overall_feedback: "总评文本".to_string(),
        };
        let zh = build_foundation_review_feedback(&review, WritingLanguage::Zh);
        assert!(zh.contains("## 总评"));
        assert!(zh.contains("核心冲突（70分）：张力不足"));
    }
}
