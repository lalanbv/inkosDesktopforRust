//! length-normalizer —— 章节长度单次修正（压缩/扩写）。
//!
//! 移植自 `packages/core/src/agents/length-normalizer.ts`（236 行）。write-next
//! 链的专用长度步骤：只在硬区间漂移时触发，与 reviser 的问题清单**互不混合**。
//!
//! 安全护栏（golden 守门）：
//! - 输出疑似截断（未收尾标点 + 字数低于硬下限）→ 保留原文
//! - 输出越过对侧硬边界（超上限压到下限以下 / 反之）→ 保留原文
//! - 包装行剥离超过 50% 内容 → 疑似正则误伤，退回 trim 原文
//!
//! ## 移植纪律
//! - prompt（系统/用户）逐字移植；`stripped.length < trimmed.length * 0.5`
//!   按 UTF-16 码元比较
//! - wrapper 行识别的 5 组正则逐字移植（含 `\b` 的 ASCII 语义——模式仅涉
//!   ASCII 词首，`(?i)` + 默认词边界等价）

use async_trait::async_trait;
use regex::Regex;
use std::sync::OnceLock;

use crate::agents::continuity::{AuditTokenUsage, ChatOutcome};
use crate::llm::provider::{LLMMessage, LLMRole};
use crate::models::length_governance::{LengthCountingMode, LengthNormalizeMode, LengthSpec};
use crate::utils::language::utf16_len;
use crate::utils::length_metrics::{
    choose_normalize_mode, count_chapter_length, is_outside_hard_range, is_outside_soft_range,
};

/// agent 名。对齐 TS `LengthNormalizerAgent.name`。
pub const LENGTH_NORMALIZER_NAME: &str = "length-normalizer";

/// LLM 聊天端口（temperature 0.2）。
#[async_trait]
pub trait LengthNormalizerChat: Send + Sync {
    async fn chat(
        &self,
        messages: Vec<LLMMessage>,
        temperature: f64,
    ) -> Result<ChatOutcome, String>;
}

/// normalize 入参。对齐 TS `NormalizeLengthInput`。
pub struct NormalizeLengthInput<'a> {
    pub chapter_content: &'a str,
    pub length_spec: &'a LengthSpec,
    pub chapter_intent: Option<&'a str>,
    pub reduced_control_block: Option<&'a str>,
}

/// normalize 出参。对齐 TS `NormalizeLengthOutput`。
#[derive(Debug, Clone, PartialEq)]
pub struct NormalizeLengthOutput {
    pub normalized_content: String,
    pub final_count: u32,
    pub applied: bool,
    pub mode: LengthNormalizeMode,
    pub warning: Option<String>,
    pub token_usage: Option<AuditTokenUsage>,
}

#[derive(Debug, thiserror::Error)]
pub enum NormalizeChapterError {
    #[error("LLM chat failed: {0}")]
    Chat(String),
}

fn counting(spec: &LengthSpec) -> LengthCountingMode {
    spec.counting_mode
}

/// 章节长度单次修正主入口。
pub async fn normalize_chapter(
    chat: &dyn LengthNormalizerChat,
    input: &NormalizeLengthInput<'_>,
) -> Result<NormalizeLengthOutput, NormalizeChapterError> {
    let spec = input.length_spec;
    let original_count = count_chapter_length(input.chapter_content, counting(spec));
    let mode = if spec.normalize_mode == LengthNormalizeMode::None {
        choose_normalize_mode(original_count, spec.soft_min, spec.soft_max)
    } else {
        spec.normalize_mode
    };

    if mode == LengthNormalizeMode::None {
        return Ok(NormalizeLengthOutput {
            normalized_content: input.chapter_content.to_string(),
            final_count: original_count,
            applied: false,
            mode,
            warning: None,
            token_usage: None,
        });
    }

    let system_prompt = build_system_prompt(mode);
    let user_prompt = build_user_prompt(input, original_count, mode);
    let response = chat
        .chat(
            vec![
                LLMMessage { role: LLMRole::System, content: system_prompt },
                LLMMessage { role: LLMRole::User, content: user_prompt },
            ],
            0.2,
        )
        .await
        .map_err(NormalizeChapterError::Chat)?;

    let sanitized_content = sanitize_normalized_content(&response.content, input.chapter_content);
    let sanitized_count = count_chapter_length(&sanitized_content, counting(spec));
    let was_truncated = sanitized_content != input.chapter_content
        && sanitized_count < spec.hard_min
        && looks_truncated(&sanitized_content);
    let crossed_hard_range = sanitized_content != input.chapter_content
        && crosses_opposite_hard_bound(original_count, sanitized_count, spec);
    let normalized_content = if was_truncated || crossed_hard_range {
        input.chapter_content.to_string()
    } else {
        sanitized_content
    };
    let final_count = count_chapter_length(&normalized_content, counting(spec));
    let warning = if was_truncated {
        Some("Length normalizer output appeared truncated; kept original chapter.".to_string())
    } else if crossed_hard_range {
        Some("Length normalizer output crossed the hard range; kept original chapter.".to_string())
    } else {
        build_warning(final_count, spec)
    };

    Ok(NormalizeLengthOutput {
        applied: normalized_content != input.chapter_content,
        normalized_content,
        final_count,
        mode,
        warning,
        token_usage: response.usage,
    })
}

/// 系统提示词。逐字移植 TS `buildSystemPrompt`。
pub fn build_system_prompt(mode: LengthNormalizeMode) -> String {
    let action = if mode == LengthNormalizeMode::Compress { "compress" } else { "expand" };
    format!(
        "你是一位章节长度修正器。你的任务是对章节正文做一次单次修正，只能执行一次，不得递归重写。\n\n修正目标：\n- {action} 章节长度到给定目标区间\n- 保留章节原有事实、关键钩子、角色名和必须保留的标记\n- 不要引入新的支线、未来揭示或额外总结\n- 不要在正文外输出任何解释"
    )
}

/// 用户提示词。逐字移植 TS `buildUserPrompt`。
pub fn build_user_prompt(input: &NormalizeLengthInput<'_>, original_count: u32, mode: LengthNormalizeMode) -> String {
    let intent_block = input
        .chapter_intent
        .map(|intent| format!("\n## Chapter Intent\n{intent}\n"))
        .unwrap_or_default();
    let control_block = input
        .reduced_control_block
        .map(|block| format!("\n## Reduced Control Block\n{block}\n"))
        .unwrap_or_default();
    let action = if mode == LengthNormalizeMode::Compress { "压缩" } else { "扩写" };

    format!(
        "请对下面正文做一次{action}修正。\n\n## Length Spec\n- Target: {target}\n- Soft Range: {soft_min}-{soft_max}\n- Hard Range: {hard_min}-{hard_max}\n- Counting Mode: {counting}\n\n## Current Count\n{original_count}\n\n## Correction Rules\n- 只修正一次，不要递归\n- 保留正文中的关键标记、人物名、地点名和已有事实\n- 不要凭空新增子情节\n- 不要插入解释性总结或分析\n- 输出修正后的完整正文，不要加标签\n\n{intent_block}{control_block}\n## Chapter Content\n{content}",
        target = input.length_spec.target,
        soft_min = input.length_spec.soft_min,
        soft_max = input.length_spec.soft_max,
        hard_min = input.length_spec.hard_min,
        hard_max = input.length_spec.hard_max,
        counting = counting_mode_label(input.length_spec.counting_mode),
        content = input.chapter_content,
    )
}

fn counting_mode_label(mode: LengthCountingMode) -> &'static str {
    match mode {
        LengthCountingMode::ZhChars => "zh_chars",
        LengthCountingMode::EnWords => "en_words",
    }
}

pub fn build_warning(final_count: u32, spec: &LengthSpec) -> Option<String> {
    if !is_outside_soft_range(final_count, spec.soft_min, spec.soft_max) {
        return None;
    }
    if is_outside_hard_range(final_count, spec.hard_min, spec.hard_max) {
        return Some(format!(
            "Final count {final_count} is outside the hard range {}-{} after one normalization pass.",
            spec.hard_min, spec.hard_max
        ));
    }
    Some(format!(
        "Final count {final_count} is outside the soft range {}-{} after one normalization pass.",
        spec.soft_min, spec.soft_max
    ))
}

pub fn crosses_opposite_hard_bound(original_count: u32, candidate_count: u32, spec: &LengthSpec) -> bool {
    (original_count > spec.hard_max && candidate_count < spec.hard_min)
        || (original_count < spec.hard_min && candidate_count > spec.hard_max)
}

/// 净化 LLM 输出：fence 提取 → 包装行剥离（>50% 剥离疑似误伤退回 trim）→ trim。
pub fn sanitize_normalized_content(raw_content: &str, fallback_content: &str) -> String {
    let trimmed = raw_content.trim();
    if trimmed.is_empty() {
        return fallback_content.to_string();
    }

    if let Some(fenced) = extract_first_fenced_block(trimmed) {
        return fenced;
    }

    if let Some(stripped) = strip_common_wrappers(trimmed) {
        // 剥离后为空 = 响应只有包装文本，用原文。
        if stripped.is_empty() {
            return fallback_content.to_string();
        }
        // 护栏：剥离超 50% 内容 → 正则疑似过激。
        if utf16_len(&stripped) < (utf16_len(trimmed) as f64 * 0.5) as usize {
            return trimmed.to_string();
        }
        return stripped;
    }

    trimmed.to_string()
}

/// 截断检测：正常收尾（```、收尾标点）→ false；中英文流中截 → true。
pub fn looks_truncated(content: &str) -> bool {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.ends_with("```") {
        return false;
    }
    if ending_punct_re().is_match(trimmed) {
        return false;
    }
    if trailing_ws_re().is_match(content) && mid_punct_re().is_match(trimmed) {
        return true;
    }
    comma_punct_re().is_match(trimmed) || word_char_re().is_match(trimmed)
}

fn extract_first_fenced_block(content: &str) -> Option<String> {
    let captures = fence_re().captures(content)?;
    let body = captures.get(1)?.as_str().trim();
    if body.is_empty() {
        None
    } else {
        Some(body.to_string())
    }
}

fn strip_common_wrappers(content: &str) -> Option<String> {
    let mut removed_any = false;
    let mut kept_lines: Vec<&str> = Vec::new();

    for raw_line in content.split('\n') {
        let trimmed = raw_line.trim();
        if is_wrapper_line(trimmed) {
            removed_any = true;
            continue;
        }
        kept_lines.push(raw_line);
    }

    if !removed_any {
        return None;
    }
    Some(kept_lines.join("\n").trim().to_string())
}

fn is_wrapper_line(line: &str) -> bool {
    if line.is_empty() {
        return false;
    }
    if fence_open_re().is_match(line) {
        return true;
    }
    if heading_wrapper_re().is_match(line) {
        return true;
    }
    if zh_intro_re().is_match(line) {
        return true;
    }
    if zh_first_person_re().is_match(line) {
        return true;
    }
    if en_intro_re().is_match(line) {
        return true;
    }
    if en_first_person_re().is_match(line) {
        return true;
    }
    false
}

// ---- 静态正则（逐字移植 TS） ----

fn ending_punct_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r#"[。！？!?」』"’）)\]】》…]$"#).unwrap())
}

fn trailing_ws_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\n\s*$").unwrap())
}

fn mid_punct_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"[，,；;：:]$").unwrap())
}

fn comma_punct_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"[，,；;：、]$").unwrap())
}

fn word_char_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"[\x{4e00}-\x{9fff}A-Za-z0-9]$").unwrap())
}

fn fence_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"```(?:[a-zA-Z-]+)?\s*\n([\s\S]*?)\n```").unwrap())
}

fn fence_open_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^```").unwrap())
}

fn heading_wrapper_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)^#+\s*(说明|解释|注释|analysis|analysis note)\b").unwrap())
}

fn zh_intro_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"(?i)^(下面是|以下是).*(正文|章节|压缩|扩写|修正|修改|调整|改写|润色|结果|内容|输出|版本)").unwrap()
    })
}

fn zh_first_person_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)^我先.*(压缩|扩写|修正|修改|调整|改写|润色|处理).*(正文|章节)?").unwrap())
}

fn en_intro_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"(?i)^(here(?:'s| is)|below is).*(chapter|draft|content|rewrite|revised|compressed|expanded|normalized|adjusted|output|version|result)").unwrap()
    })
}

fn en_first_person_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"(?i)^i(?:'ll| will)\s+(rewrite|revise|reword|compress|expand|normalize|adjust|shorten|lengthen|trim|fix)\b").unwrap()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(mode: LengthNormalizeMode) -> LengthSpec {
        LengthSpec {
            target: 3000,
            soft_min: 2250,
            soft_max: 3750,
            hard_min: 1500,
            hard_max: 4500,
            counting_mode: LengthCountingMode::ZhChars,
            normalize_mode: mode,
        }
    }

    struct MockChat {
        content: String,
    }

    #[async_trait]
    impl LengthNormalizerChat for MockChat {
        async fn chat(&self, _messages: Vec<LLMMessage>, _temperature: f64) -> Result<ChatOutcome, String> {
            Ok(ChatOutcome { content: self.content.clone(), usage: None })
        }
    }

    #[tokio::test]
    async fn mode_none_returns_original_untouched() {
        // 软区间包住 2 字内容 → chooseNormalizeMode 也判 none（不触 LLM）。
        let spec = LengthSpec {
            target: 2,
            soft_min: 1,
            soft_max: 3,
            hard_min: 1,
            hard_max: 100,
            counting_mode: LengthCountingMode::ZhChars,
            normalize_mode: LengthNormalizeMode::None,
        };
        let input = NormalizeLengthInput {
            chapter_content: "正文",
            length_spec: &spec,
            chapter_intent: None,
            reduced_control_block: None,
        };
        // choose_normalize_mode 对软区间内字数 → none。
        let out = normalize_chapter(&MockChat { content: "不该被调用".into() }, &input)
            .await
            .unwrap();
        assert!(!out.applied);
        assert_eq!(out.normalized_content, "正文");
        assert_eq!(out.mode, LengthNormalizeMode::None);
    }

    #[tokio::test]
    async fn compress_applies_and_reports_warning() {
        let spec = spec(LengthNormalizeMode::Compress);
        let input = NormalizeLengthInput {
            chapter_content: &"长".repeat(5000),
            length_spec: &spec,
            chapter_intent: Some("## Goal\n目标"),
            reduced_control_block: None,
        };
        let chat = MockChat { content: "短".repeat(3000) };
        let out = normalize_chapter(&chat, &input).await.unwrap();
        assert!(out.applied);
        assert_eq!(out.final_count, 3000);
        assert!(out.warning.is_none()); // 软区间内
        assert_eq!(out.mode, LengthNormalizeMode::Compress);
    }

    #[tokio::test]
    async fn truncated_output_keeps_original() {
        let spec = spec(LengthNormalizeMode::Expand);
        let input = NormalizeLengthInput {
            chapter_content: &"短".repeat(1400),
            length_spec: &spec,
            chapter_intent: None,
            reduced_control_block: None,
        };
        // 输出仍低于硬下限且无收尾标点 → 疑似截断，保留原文。
        let chat = MockChat { content: "他继续走了很".to_string() };
        let out = normalize_chapter(&chat, &input).await.unwrap();
        assert!(!out.applied);
        assert_eq!(out.normalized_content, "短".repeat(1400));
        assert_eq!(
            out.warning.as_deref(),
            Some("Length normalizer output appeared truncated; kept original chapter.")
        );
    }

    #[tokio::test]
    async fn crossing_opposite_bound_keeps_original() {
        let spec = spec(LengthNormalizeMode::Compress);
        let input = NormalizeLengthInput {
            chapter_content: &"长".repeat(5000),
            length_spec: &spec,
            chapter_intent: None,
            reduced_control_block: None,
        };
        // 超上限(5000)压到下限以下(1000，带句号非截断)→ 越对侧边界，保留原文。
        let chat = MockChat { content: "句。".repeat(500) };
        let out = normalize_chapter(&chat, &input).await.unwrap();
        assert!(!out.applied);
        assert_eq!(
            out.warning.as_deref(),
            Some("Length normalizer output crossed the hard range; kept original chapter.")
        );
    }

    #[test]
    fn sanitize_extracts_fence_prefers_clean_body() {
        let fenced = "下面是压缩后的版本：\n```\n正文甲\n```\n完毕";
        assert_eq!(sanitize_normalized_content(fenced, "fallback"), "正文甲");
        assert_eq!(sanitize_normalized_content("", "fallback"), "fallback");
        assert_eq!(sanitize_normalized_content("  \n", "fallback"), "fallback");
    }

    #[test]
    fn sanitize_strips_wrapper_lines_with_guard() {
        let wrapped = "下面是修正后的正文：\n真正的正文内容在这里。";
        assert_eq!(sanitize_normalized_content(wrapped, "fb"), "真正的正文内容在这里。");

        // 全部行都是包装 → 回退原文。
        let all_wrapper = "我先压缩一下正文";
        assert_eq!(sanitize_normalized_content(all_wrapper, "fb"), "fb");

        // 剥离超 50%（UTF-16）→ 退回 trim。
        let mostly_wrapper = "以下是压缩后的完整版本输出\n短";
        assert_eq!(sanitize_normalized_content(mostly_wrapper, "fb"), mostly_wrapper);
    }

    #[test]
    fn looks_truncated_matrix() {
        assert!(!looks_truncated("正常收尾。"));
        assert!(!looks_truncated("code```"));
        assert!(looks_truncated("他继续走了很"));
        assert!(looks_truncated("话说到一半，"));
        assert!(!looks_truncated(""));
    }

    #[test]
    fn prompts_are_verbatim() {
        let zh = build_system_prompt(LengthNormalizeMode::Compress);
        assert!(zh.contains("- compress 章节长度到给定目标区间"));
        let sp = spec(LengthNormalizeMode::Expand);
        let input = NormalizeLengthInput {
            chapter_content: "正文",
            length_spec: &sp,
            chapter_intent: Some("意图"),
            reduced_control_block: Some("控制"),
        };
        let user = build_user_prompt(&input, 1234, LengthNormalizeMode::Expand);
        assert!(user.contains("请对下面正文做一次扩写修正。"));
        assert!(user.contains("- Target: 3000"));
        assert!(user.contains("## Current Count\n1234"));
        assert!(user.contains("## Chapter Intent\n意图\n"));
        assert!(user.contains("## Reduced Control Block\n控制\n"));
        assert!(user.ends_with("## Chapter Content\n正文"));
    }
}
