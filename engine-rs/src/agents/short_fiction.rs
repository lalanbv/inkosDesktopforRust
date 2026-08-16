//! 短篇 web 小说 agent 面（agents/short-fiction.ts，78 号）。
//!
//! 六代理 LLM 调用（outline / outline-review / outline-revise / writer /
//! draft-review / draft-revise / package——writer 复用续写）+ `=== TAG ===`
//! 块解析（tagged 优先、Markdown 章节标题/正文回退、重复标题块、fence 清洗）
//! + 完整性校验与 Markdown 渲染。提示词构造复用 [`crate::prompts::short_fiction`]。

use crate::llm::agent_router::AgentRouter;
use crate::llm::provider::{LLMMessage, LLMRole};
use crate::prompts::short_fiction::{
    build_draft_continuation_user_prompt, build_draft_review_system_prompt,
    build_draft_review_user_prompt, build_draft_revision_followup, build_outline_review_system_prompt,
    build_outline_review_user_prompt, build_outline_revision_followup, build_outline_system_prompt,
    build_outline_user_prompt, build_package_system_prompt, build_package_user_prompt,
    build_writer_system_prompt, build_writer_user_prompt, ShortFictionDraftContinuationPromptInput,
    ShortFictionDraftPromptInput, ShortFictionDraftReviewPromptInput,
    ShortFictionDraftRevisionPromptInput, ShortFictionOutlinePromptInput,
    ShortFictionOutlineReviewPromptInput, ShortFictionOutlineRevisionPromptInput,
    ShortFictionPackagePromptInput, ShortFictionReferencePromptInput,
};
use crate::utils::language::WritingLanguage;
use crate::utils::length_metrics::{count_chapter_length, resolve_length_counting_mode};

pub const SHORT_FICTION_DEFAULT_CHAPTERS: u32 = 12;
pub const SHORT_FICTION_MIN_CHAPTERS: u32 = 12;
pub const SHORT_FICTION_MAX_CHAPTERS: u32 = 18;
pub const SHORT_FICTION_DEFAULT_CHARS_PER_CHAPTER: u32 = 1000;
pub const SHORT_FICTION_MIN_CHARS_PER_CHAPTER: u32 = 900;
pub const SHORT_FICTION_MAX_CHARS_PER_CHAPTER: u32 = 1200;
pub const SHORT_FICTION_EN_DEFAULT_WORDS_PER_CHAPTER: u32 = 650;
pub const SHORT_FICTION_EN_MIN_WORDS_PER_CHAPTER: u32 = 600;
pub const SHORT_FICTION_EN_MAX_WORDS_PER_CHAPTER: u32 = 800;

/// 单章缺失补写轮数上限（SHORT_FICTION_DRAFT_COMPLETION_ATTEMPTS）。
pub const SHORT_FICTION_DRAFT_COMPLETION_ATTEMPTS: u32 = 3;

// ── 类型 ────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ShortFictionReference {
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct ShortFictionOutline {
    pub story_title: String,
    pub raw_content: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortFictionChapter {
    pub number: u32,
    pub title: String,
    pub content: String,
    /// 语言原生计量：zh 汉字数 / en 单词数。
    pub char_count: u32,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortFictionBatchDraft {
    pub story_title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opening_hook: Option<String>,
    pub chapters: Vec<ShortFictionChapter>,
    pub raw_content: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortFictionSalesPackage {
    pub title: String,
    pub intro: String,
    pub selling_points: Vec<String>,
    pub cover_prompt: String,
    pub raw_content: String,
}

// ── LLM 调用面（router 键 = pipeline.createAgentContext 的 agent 名） ──

fn message(role: LLMRole, content: String) -> LLMMessage {
    LLMMessage { role, content, tool_calls: None, tool_call_id: None }
}

fn reference_prompt_input(reference: Option<&ShortFictionReference>) -> Option<ShortFictionReferencePromptInput> {
    reference.map(|r| ShortFictionReferencePromptInput { text: Some(r.text.clone()) })
}

/// `ShortFictionOutlineAgent.createOutline`（short-outline，0.55 / 8192）。
pub async fn create_outline(
    router: &AgentRouter,
    direction: &str,
    chapter_count: u32,
    chars_per_chapter: u32,
    reference: Option<&ShortFictionReference>,
    language: WritingLanguage,
) -> Result<ShortFictionOutline, String> {
    let input = ShortFictionOutlinePromptInput {
        direction: direction.to_string(),
        chapter_count,
        chars_per_chapter,
        reference: reference_prompt_input(reference),
    };
    let content = retry_short_fiction_call(
        || {
            let messages = vec![
                message(LLMRole::System, build_outline_system_prompt(language)),
                message(LLMRole::User, build_outline_user_prompt(&input, language)),
            ];
            router.chat("short-outline", messages, 0.55, Some(8192))
        },
        "short-fiction-outline",
    )
    .await?;
    Ok(parse_outline(&content.content, language))
}

/// `ShortFictionOutlineReviewerAgent.reviewOutline`（short-outline-review，0.3 / 4096）。
pub async fn review_outline(
    router: &AgentRouter,
    direction: &str,
    outline_raw_content: &str,
    reference: Option<&ShortFictionReference>,
    language: WritingLanguage,
) -> Result<String, String> {
    let input = ShortFictionOutlineReviewPromptInput {
        direction: direction.to_string(),
        outline_raw_content: outline_raw_content.to_string(),
        reference: reference_prompt_input(reference),
    };
    let content = retry_short_fiction_call(
        || {
            let messages = vec![
                message(LLMRole::System, build_outline_review_system_prompt(language)),
                message(LLMRole::User, build_outline_review_user_prompt(&input, language)),
            ];
            router.chat("short-outline-review", messages, 0.3, Some(4096))
        },
        "short-fiction-outline-reviewer",
    )
    .await?;
    Ok(content.content.trim().to_string())
}

/// `ShortFictionOutlineReviserAgent.reviseOutline`（short-outline，0.45 / 8192，
/// 四消息轮：system/user/assistant(原稿)/user(修订 followup)）。
#[allow(clippy::too_many_arguments)]
pub async fn revise_outline(
    router: &AgentRouter,
    direction: &str,
    outline: &ShortFictionOutline,
    review: &str,
    reference: Option<&ShortFictionReference>,
    chapter_count: u32,
    chars_per_chapter: u32,
    language: WritingLanguage,
) -> Result<ShortFictionOutline, String> {
    let base = ShortFictionOutlinePromptInput {
        direction: direction.to_string(),
        chapter_count,
        chars_per_chapter,
        reference: reference_prompt_input(reference),
    };
    let revision = ShortFictionOutlineRevisionPromptInput {
        direction: direction.to_string(),
        outline_raw_content: outline.raw_content.clone(),
        reference: reference_prompt_input(reference),
        review: review.to_string(),
        chapter_count,
        chars_per_chapter,
    };
    let content = retry_short_fiction_call(
        || {
            let messages = vec![
                message(LLMRole::System, build_outline_system_prompt(language)),
                message(LLMRole::User, build_outline_user_prompt(&base, language)),
                message(LLMRole::Assistant, outline.raw_content.trim().to_string()),
                message(LLMRole::User, build_outline_revision_followup(&revision, language)),
            ];
            router.chat("short-outline", messages, 0.45, Some(8192))
        },
        "short-fiction-outline-reviser",
    )
    .await?;
    Ok(parse_outline(&content.content, language))
}

/// charsPerChapter 是语言原生单位；2.2 倍系数按 zh 汉字校准（en 单词留余量）。
pub fn estimate_short_fiction_max_tokens(chapter_count: u32, chars_per_chapter: u32) -> u32 {
    let estimated = (f64::from(chapter_count) * f64::from(chars_per_chapter) * 2.2).ceil() as u32 + 4096;
    estimated.max(12_288)
}

/// `ShortFictionWriterAgent.writeDraft`（short-writer，0.58）。
pub async fn write_draft(
    router: &AgentRouter,
    direction: &str,
    outline_markdown: &str,
    chapter_count: u32,
    chars_per_chapter: u32,
    language: WritingLanguage,
) -> Result<ShortFictionBatchDraft, String> {
    let input = ShortFictionDraftPromptInput {
        direction: direction.to_string(),
        outline_markdown: outline_markdown.to_string(),
        chapter_count,
        chars_per_chapter,
    };
    let max_tokens = estimate_short_fiction_max_tokens(chapter_count, chars_per_chapter);
    let content = retry_short_fiction_call(
        || {
            let messages = vec![
                message(LLMRole::System, build_writer_system_prompt(language)),
                message(LLMRole::User, build_writer_user_prompt(&input, language)),
            ];
            router.chat("short-writer", messages, 0.58, Some(max_tokens))
        },
        "short-fiction-writer",
    )
    .await?;
    Ok(parse_batch_draft(
        &content.content,
        Some(chapter_count),
        language,
    ))
}

/// `ShortFictionWriterAgent.continueDraft`（short-writer，0.68；无缺章直接返回）。
pub async fn continue_draft(
    router: &AgentRouter,
    direction: &str,
    outline_markdown: &str,
    chapter_count: u32,
    chars_per_chapter: u32,
    language: WritingLanguage,
    draft: &ShortFictionBatchDraft,
) -> Result<ShortFictionBatchDraft, String> {
    let missing_chapters = find_empty_short_fiction_chapters(draft);
    if missing_chapters.is_empty() {
        return Ok(draft.clone());
    }
    let input = ShortFictionDraftContinuationPromptInput {
        direction: direction.to_string(),
        outline_markdown: outline_markdown.to_string(),
        chapter_count,
        chars_per_chapter,
        existing_draft_markdown: render_draft_markdown(draft, language),
        missing_chapters: missing_chapters.clone(),
    };
    let max_tokens = estimate_short_fiction_max_tokens(missing_chapters.len() as u32, chars_per_chapter);
    let content = retry_short_fiction_call(
        || {
            let messages = vec![
                message(LLMRole::System, build_writer_system_prompt(language)),
                message(LLMRole::User, build_draft_continuation_user_prompt(&input, language)),
            ];
            router.chat("short-writer", messages, 0.68, Some(max_tokens))
        },
        "short-fiction-writer",
    )
    .await?;
    let merged = format!("{}\n\n{}", draft.raw_content.trim(), content.content.trim());
    Ok(parse_batch_draft(&merged, Some(chapter_count), language))
}

/// `ShortFictionDraftReviewerAgent.reviewDraft`（short-draft-review，0.3 / 8192）。
pub async fn review_draft(
    router: &AgentRouter,
    direction: &str,
    outline_markdown: &str,
    chapter_count: u32,
    chars_per_chapter: u32,
    language: WritingLanguage,
    draft: &ShortFictionBatchDraft,
) -> Result<String, String> {
    let input = ShortFictionDraftReviewPromptInput {
        direction: direction.to_string(),
        outline_markdown: outline_markdown.to_string(),
        chapter_count,
        chars_per_chapter,
        draft_markdown: render_draft_markdown(draft, language),
    };
    let content = retry_short_fiction_call(
        || {
            let messages = vec![
                message(LLMRole::System, build_draft_review_system_prompt(language)),
                message(LLMRole::User, build_draft_review_user_prompt(&input, language)),
            ];
            router.chat("short-draft-review", messages, 0.3, Some(8192))
        },
        "short-fiction-draft-reviewer",
    )
    .await?;
    Ok(content.content.trim().to_string())
}

/// `ShortFictionDraftReviserAgent.reviseDraft`（short-revise，0.45；四消息轮）。
#[allow(clippy::too_many_arguments)]
pub async fn revise_draft(
    router: &AgentRouter,
    direction: &str,
    outline_markdown: &str,
    chapter_count: u32,
    chars_per_chapter: u32,
    language: WritingLanguage,
    draft: &ShortFictionBatchDraft,
    review: &str,
) -> Result<ShortFictionBatchDraft, String> {
    let base = ShortFictionDraftPromptInput {
        direction: direction.to_string(),
        outline_markdown: outline_markdown.to_string(),
        chapter_count,
        chars_per_chapter,
    };
    let revision = ShortFictionDraftRevisionPromptInput {
        direction: direction.to_string(),
        outline_markdown: outline_markdown.to_string(),
        chapter_count,
        chars_per_chapter,
        review: review.to_string(),
    };
    let assistant_content = {
        let trimmed = draft.raw_content.trim();
        if trimmed.is_empty() {
            render_draft_markdown(draft, language)
        } else {
            trimmed.to_string()
        }
    };
    let max_tokens = estimate_short_fiction_max_tokens(chapter_count, chars_per_chapter);
    let content = retry_short_fiction_call(
        || {
            let messages = vec![
                message(LLMRole::System, build_writer_system_prompt(language)),
                message(LLMRole::User, build_writer_user_prompt(&base, language)),
                message(LLMRole::Assistant, assistant_content.clone()),
                message(LLMRole::User, build_draft_revision_followup(&revision, language)),
            ];
            router.chat("short-revise", messages, 0.45, Some(max_tokens))
        },
        "short-fiction-draft-reviser",
    )
    .await?;
    Ok(parse_batch_draft(&content.content, Some(chapter_count), language))
}

/// `ShortFictionPackagingAgent.generatePackage`（short-package，0.45 / 4096）。
pub async fn generate_package(
    router: &AgentRouter,
    direction: &str,
    outline_markdown: &str,
    draft: &ShortFictionBatchDraft,
    language: WritingLanguage,
) -> Result<ShortFictionSalesPackage, String> {
    let input = ShortFictionPackagePromptInput {
        direction: direction.to_string(),
        outline_markdown: outline_markdown.to_string(),
        draft_markdown: render_draft_markdown(draft, language),
        draft_title: draft.story_title.clone(),
    };
    let content = retry_short_fiction_call(
        || {
            let messages = vec![
                message(LLMRole::System, build_package_system_prompt(language)),
                message(LLMRole::User, build_package_user_prompt(&input, language)),
            ];
            router.chat("short-package", messages, 0.45, Some(4096))
        },
        "short-fiction-packaging",
    )
    .await?;
    Ok(parse_sales_package(&content.content, &draft.story_title))
}

// ── 瞬态错误重试（retryShortFictionCall：2 次尝试） ─────────────

async fn retry_short_fiction_call<F, Fut>(mut operation: F, label: &str) -> Result<crate::agents::continuity::ChatOutcome, String>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<crate::agents::continuity::ChatOutcome, String>>,
{
    let mut last_error = String::new();
    for attempt in 1..=2 {
        match operation().await {
            Ok(outcome) => return Ok(outcome),
            Err(e) => {
                last_error = e;
                if attempt >= 2 || !is_transient_short_fiction_error(&last_error) {
                    return Err(last_error);
                }
            }
        }
    }
    let _ = label;
    Err(last_error)
}

fn is_transient_short_fiction_error(message: &str) -> bool {
    let lowered = message.to_lowercase();
    lowered.contains("unexpected eof")
        || lowered.contains("econnreset")
        || lowered.contains("socket hang up")
        || lowered.contains("terminated")
        || lowered.contains("fetch failed")
}

// ── tagged 块提取（extractTaggedBlocks 族——逐行扫描对齐 JS 正则语义）──

/// 行是否 `=== TAG ===` 形态（大小写不敏感、允许任意空白）。
fn line_matches_tag(line: &str, tag: &str) -> bool {
    let inner = strip_tag_fences(line);
    inner.is_some_and(|inner| inner.eq_ignore_ascii_case(tag))
}

/// 剥 `=== ... ===` 壳；非 tag 行 → None（内部须剥离首尾空白）。
fn strip_tag_fences(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    let rest = trimmed.strip_prefix("===")?;
    let inner = rest.strip_suffix("===")?;
    Some(inner.trim())
}

/// 通用 tag 行判定（`^\s*===\s*[A-Z0-9_ ]+\s*===\s*$`，i 标志 → 大小写均可）。
fn is_general_tag_line(line: &str) -> bool {
    let Some(inner) = strip_tag_fences(line) else {
        return false;
    };
    !inner.is_empty()
        && inner
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ' ')
}

/// `extractTaggedBlocks`：所有 `=== tag ===` 块（到下一个通用 tag 行或结尾，trim）。
fn extract_tagged_blocks(raw: &str, tag: &str) -> Vec<String> {
    let lines: Vec<&str> = raw.split('\n').collect();
    let mut blocks = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if line_matches_tag(lines[i], tag) {
            let mut j = i + 1;
            while j < lines.len() && !is_general_tag_line(lines[j]) {
                j += 1;
            }
            blocks.push(lines[i + 1..j].join("\n").trim().to_string());
            i = j;
        } else {
            i += 1;
        }
    }
    blocks
}

fn extract_tagged_block(raw: &str, tag: &str) -> String {
    extract_tagged_blocks(raw, tag).into_iter().next().unwrap_or_default()
}

fn extract_last_non_empty_tagged_block(raw: &str, tag: &str) -> String {
    let blocks = extract_tagged_blocks(raw, tag);
    blocks
        .iter()
        .rev()
        .find(|block| !block.trim().is_empty())
        .cloned()
        .unwrap_or_default()
}

fn extract_first_heading_re() -> &'static regex::Regex {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r"(?m)^#\s+(.+)$").unwrap())
}

fn extract_first_heading(raw: &str) -> String {
    extract_first_heading_re()
        .captures(raw)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str().trim().to_string())
        .unwrap_or_default()
}

/// zh `第N章` / en `Chapter N` 前缀模式（Markdown 回退提取用）。
fn markdown_chapter_prefix_regex(number: u32) -> regex::Regex {
    regex::Regex::new(&format!(
        r"第\s*{number}\s*章\s*|Chapter\s*{number}\s*[:：.\-–—]?\s*"
    ))
    .unwrap()
}

fn extract_markdown_chapter_title(raw: &str, number: u32) -> String {
    let prefix = markdown_chapter_prefix_regex(number);
    for line in raw.split('\n') {
        let Some(rest) = line.trim_start().strip_prefix("##") else {
            continue;
        };
        // `^##\s*(?:prefix)?(.+)$`：剥 ## 与空白后按前缀再剥。
        let after_ws = rest.trim_start();
        let stripped = prefix.find(after_ws).map(|m| &after_ws[m.end()..]).unwrap_or(after_ws);
        if !stripped.trim().is_empty() {
            return stripped.trim().to_string();
        }
    }
    String::new()
}

/// `extractMarkdownChapterContent`：首个 `##` 标题行到下一个 `##` 行（或结尾）。
fn extract_markdown_chapter_content(raw: &str, number: u32) -> String {
    let _ = number; // 前缀在 TS 中可选——任意 ## 标题行都终止/起始
    let lines: Vec<&str> = raw.split('\n').collect();
    let Some(start) = lines.iter().position(|line| line.trim_start().starts_with("##")) else {
        return String::new();
    };
    let end = lines[start + 1..]
        .iter()
        .position(|line| line.trim_start().starts_with("##"))
        .map(|offset| start + 1 + offset)
        .unwrap_or(lines.len());
    lines[start + 1..end].join("\n").trim().to_string()
}

/// `extractDuplicateTitleTaggedChapterContent`：`CHAPTER N TITLE` 出现 ≥2 次时，
/// 第二次出现后的内容（到下一个 `CHAPTER d+ (TITLE|CONTENT)` / `SHORT_FICTION_*`
/// tag 行或结尾）。
fn extract_duplicate_title_tagged_chapter_content(raw: &str, number: u32) -> String {
    let tag = format!("CHAPTER {number} TITLE");
    let lines: Vec<&str> = raw.split('\n').collect();
    let occurrences: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line_matches_tag(line, &tag))
        .map(|(index, _)| index)
        .collect();
    if occurrences.len() < 2 {
        return String::new();
    }
    let start = occurrences[1] + 1;
    let end = lines[start..]
        .iter()
        .position(|line| terminates_duplicate_title_block(line))
        .map(|offset| start + offset)
        .unwrap_or(lines.len());
    lines[start..end].join("\n").trim().to_string()
}

fn terminates_duplicate_title_block(line: &str) -> bool {
    let Some(inner) = strip_tag_fences(line) else {
        return false;
    };
    let is_chapter = inner.len() >= 8
        && inner[..8].eq_ignore_ascii_case("CHAPTER ")
        && inner[8..].chars().next().is_some_and(|c| c.is_ascii_digit())
        && (inner.ends_with(" TITLE") || inner.ends_with(" CONTENT"));
    is_chapter || inner.to_uppercase().starts_with("SHORT_FICTION_")
}

/// `sanitizeChapterContent`：首尾 fence 剥离 + 残留 tag 行移除 + trim。
fn sanitize_chapter_content(raw: &str) -> String {
    let mut value = raw.to_string();
    // ^```(?:md|markdown)?\s*（一次）
    let leading_fence = regex::Regex::new(r"(?i)^```(?:md|markdown)?\s*").unwrap();
    value = leading_fence.replacen(&value, 1, "").into_owned();
    // ```\s*$（一次，非多行——仅串尾）
    let trailing_fence = regex::Regex::new(r"(?i)```\s*$").unwrap();
    value = trailing_fence.replacen(&value, 1, "").into_owned();
    // ^===\s*[A-Z0-9_ ]+\s*===\s*$（gim——所有独立 tag 行）
    let without_tags: Vec<&str> = value
        .split('\n')
        .filter(|line| !is_general_tag_line(line))
        .collect();
    without_tags.join("\n").trim().to_string()
}

// ── 标题归一（normalizeTitle / normalizeChapterTitle / heading） ──

fn heading_marker_re() -> &'static regex::Regex {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r"^#+\s*").unwrap())
}

fn book_title_wrapper_re() -> &'static regex::Regex {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r"^《(.+)》$").unwrap())
}

fn normalize_title(raw: &str) -> String {
    for line in raw.split('\n') {
        let stripped = heading_marker_re().replace(line.trim(), "");
        let trimmed = stripped.trim().to_string();
        if trimmed.is_empty() {
            continue;
        }
        let unwrapped = book_title_wrapper_re().replace(&trimmed, "$1");
        return unwrapped.trim().to_string();
    }
    String::new()
}

fn chapter_prefix_re(number: u32, language: WritingLanguage) -> regex::Regex {
    match language {
        WritingLanguage::En => regex::Regex::new(&format!(
            r"(?i)^Chapter\s*{number}\s*[:：.\-–—]?\s*"
        ))
        .unwrap(),
        WritingLanguage::Zh => {
            regex::Regex::new(&format!(r"^第\s*{number}\s*章\s*")).unwrap()
        }
    }
}

fn normalize_chapter_title(raw: &str, number: u32, language: WritingLanguage) -> String {
    let prefix = chapter_prefix_re(number, language);
    let normalized = normalize_title(raw);
    let stripped = prefix.replace(&normalized, "").trim().to_string();
    if stripped.is_empty() {
        fallback_chapter_title(number, language)
    } else {
        stripped
    }
}

fn untitled_short_title(language: WritingLanguage) -> &'static str {
    match language {
        WritingLanguage::En => "Untitled Short Story",
        WritingLanguage::Zh => "未命名短篇",
    }
}

fn fallback_chapter_title(number: u32, language: WritingLanguage) -> String {
    match language {
        WritingLanguage::En => format!("Chapter {number}"),
        WritingLanguage::Zh => format!("第{number}章"),
    }
}

/// `formatShortFictionChapterHeading`：已有前缀原样；无前缀补 `第N章 ` / `Chapter N: `。
pub fn format_chapter_heading(number: u32, title: &str, language: WritingLanguage) -> String {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return fallback_chapter_title(number, language);
    }
    match language {
        WritingLanguage::En => {
            let re = regex::Regex::new(&format!(r"(?i)^Chapter\s*{number}\b")).unwrap();
            if re.is_match(trimmed) {
                trimmed.to_string()
            } else {
                format!("Chapter {number}: {trimmed}")
            }
        }
        WritingLanguage::Zh => {
            let re = regex::Regex::new(&format!(r"^第\s*{number}\s*章")).unwrap();
            if re.is_match(trimmed) {
                trimmed.to_string()
            } else {
                format!("第{number}章 {trimmed}")
            }
        }
    }
}

// ── 结构解析（parseShortFiction* 族） ────────────────────────────

pub fn parse_outline(raw_content: &str, language: WritingLanguage) -> ShortFictionOutline {
    let fallback_title = untitled_short_title(language);
    let candidate = extract_tagged_block(raw_content, "SHORT_FICTION_PLAN_TITLE")
        .if_not_empty()
        .or_else(|| extract_tagged_block(raw_content, "SHORT_FICTION_TITLE").if_not_empty())
        .or_else(|| extract_first_heading(raw_content).if_not_empty())
        .map(|value| {
            let normalized = normalize_title(&value);
            if normalized.is_empty() {
                fallback_title.to_string()
            } else {
                normalized
            }
        })
        .unwrap_or_else(|| fallback_title.to_string());
    ShortFictionOutline {
        story_title: candidate,
        raw_content: raw_content.trim().to_string(),
    }
}

pub fn parse_batch_draft(
    raw_content: &str,
    expected_chapters: Option<u32>,
    language: WritingLanguage,
) -> ShortFictionBatchDraft {
    let expected = expected_chapters.unwrap_or(SHORT_FICTION_DEFAULT_CHAPTERS);
    let counting_mode = resolve_length_counting_mode(language);
    let fallback_title = untitled_short_title(language);
    let story_title = {
        let candidate = extract_tagged_block(raw_content, "SHORT_FICTION_TITLE")
            .if_not_empty()
            .or_else(|| extract_first_heading(raw_content).if_not_empty());
        match candidate {
            Some(value) => {
                let normalized = normalize_title(&value);
                if normalized.is_empty() {
                    fallback_title.to_string()
                } else {
                    normalized
                }
            }
            None => fallback_title.to_string(),
        }
    };
    let opening_hook = extract_tagged_block(raw_content, "SHORT_FICTION_OPENING_HOOK")
        .if_not_empty()
        .or_else(|| extract_tagged_block(raw_content, "OPENING_HOOK").if_not_empty());

    let mut chapters = Vec::with_capacity(expected as usize);
    for number in 1..=expected {
        let title_source = extract_tagged_block(raw_content, &format!("CHAPTER {number} TITLE"))
            .if_not_empty()
            .or_else(|| extract_markdown_chapter_title(raw_content, number).if_not_empty());
        let title = match title_source {
            Some(value) => normalize_chapter_title(&value, number, language),
            None => fallback_chapter_title(number, language),
        };
        let content = sanitize_chapter_content(
            &extract_last_non_empty_tagged_block(raw_content, &format!("CHAPTER {number} CONTENT"))
                .if_not_empty()
                .or_else(|| extract_duplicate_title_tagged_chapter_content(raw_content, number).if_not_empty())
                .or_else(|| extract_markdown_chapter_content(raw_content, number).if_not_empty())
                .unwrap_or_default(),
        );
        chapters.push(ShortFictionChapter {
            number,
            title,
            char_count: count_chapter_length(&content, counting_mode),
            content,
        });
    }

    ShortFictionBatchDraft {
        story_title,
        opening_hook: opening_hook.map(|hook| hook.trim().to_string()).filter(|hook| !hook.is_empty()),
        chapters,
        raw_content: raw_content.to_string(),
    }
}

/// `validateShortFictionDraftForFinal`：章数 + 空章校验（错误文案逐字）。
pub fn validate_short_fiction_draft_for_final(
    draft: &ShortFictionBatchDraft,
    expected_chapters: Option<u32>,
) -> Result<(), String> {
    if let Some(expected) = expected_chapters {
        if draft.chapters.len() as u32 != expected {
            return Err(format!(
                "Short-hit draft is incomplete; expected {expected} chapters, got {}.",
                draft.chapters.len()
            ));
        }
    }
    let empty = find_empty_short_fiction_chapters(draft);
    if !empty.is_empty() {
        let joined = empty
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        return Err(format!("Short-hit draft is incomplete; empty chapters: {joined}."));
    }
    Ok(())
}

pub fn find_empty_short_fiction_chapters(draft: &ShortFictionBatchDraft) -> Vec<u32> {
    draft
        .chapters
        .iter()
        .filter(|chapter| chapter.content.trim().is_empty())
        .map(|chapter| chapter.number)
        .collect()
}

pub fn render_draft_markdown(draft: &ShortFictionBatchDraft, language: WritingLanguage) -> String {
    let hook_heading = match language {
        WritingLanguage::En => "## Opening Hook",
        WritingLanguage::Zh => "## 开篇钩子",
    };
    let mut parts: Vec<String> = Vec::with_capacity(2 + draft.chapters.len());
    parts.push(format!("# {}", draft.story_title));
    if let Some(hook) = draft.opening_hook.as_deref().filter(|hook| !hook.trim().is_empty()) {
        parts.push(format!("{hook_heading}\n\n{hook}"));
    }
    for chapter in &draft.chapters {
        parts.push(format!(
            "## {}\n\n{}",
            format_chapter_heading(chapter.number, &chapter.title, language),
            chapter.content
        ));
    }
    parts
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

pub fn parse_sales_package(raw_content: &str, fallback_title: &str) -> ShortFictionSalesPackage {
    let fallback = if fallback_title.is_empty() { "未命名短篇" } else { fallback_title };
    let title = {
        let candidate = extract_tagged_block(raw_content, "SHORT_FICTION_PACKAGE_TITLE")
            .if_not_empty()
            .or_else(|| extract_tagged_block(raw_content, "SHORT_FICTION_TITLE").if_not_empty());
        match candidate {
            Some(value) => {
                let normalized = normalize_title(&value);
                if normalized.is_empty() {
                    fallback.to_string()
                } else {
                    normalized
                }
            }
            None => fallback.to_string(),
        }
    };
    let intro = extract_tagged_block(raw_content, "SHORT_FICTION_INTRO")
        .if_not_empty()
        .or_else(|| extract_tagged_block(raw_content, "INTRO").if_not_empty())
        .unwrap_or_default();
    let selling_raw = extract_tagged_block(raw_content, "SHORT_FICTION_SELLING_POINTS")
        .if_not_empty()
        .or_else(|| extract_tagged_block(raw_content, "SELLING_POINTS").if_not_empty())
        .unwrap_or_default();
    let bullet_re = regex::Regex::new(r"^\s*[-*]\s*").unwrap();
    let selling_points: Vec<String> = selling_raw
        .split('\n')
        .map(|line| bullet_re.replace(line, "").trim().to_string())
        .filter(|line| !line.is_empty())
        .collect();
    let cover_prompt = extract_tagged_block(raw_content, "SHORT_FICTION_COVER_PROMPT")
        .if_not_empty()
        .or_else(|| extract_tagged_block(raw_content, "COVER_PROMPT").if_not_empty())
        .unwrap_or_default();
    ShortFictionSalesPackage {
        title,
        intro: intro.trim().to_string(),
        selling_points,
        cover_prompt: cover_prompt.trim().to_string(),
        raw_content: raw_content.trim().to_string(),
    }
}

/// String 空串 → None（TS `||` 短路的链式辅助）。
trait IfNotEmpty {
    fn if_not_empty(self) -> Option<String>;
}

impl IfNotEmpty for String {
    fn if_not_empty(self) -> Option<String> {
        if self.is_empty() {
            None
        } else {
            Some(self)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft_raw(chapters: u32) -> String {
        let mut out = String::from(
            "=== SHORT_FICTION_TITLE ===\n《夜雨追凶》\n\n=== SHORT_FICTION_OPENING_HOOK ===\n雨夜钩子\n\n",
        );
        for n in 1..=chapters {
            out.push_str(&format!(
                "=== CHAPTER {n} TITLE ===\n标题{n}\n=== CHAPTER {n} CONTENT ===\n第{n}章正文内容。\n\n"
            ));
        }
        out
    }

    #[test]
    fn outline_parsing_prefers_tagged_title() {
        let raw = "=== SHORT_FICTION_PLAN_TITLE ===\n《迷雾》\n\n=== SHORT_FICTION_PLAN ===\n方案正文";
        let outline = parse_outline(raw, WritingLanguage::Zh);
        assert_eq!(outline.story_title, "迷雾");
        assert_eq!(outline.raw_content, raw.trim());

        // 回退链：first heading。
        let heading_only = parse_outline("# 白夜行\n\n正文", WritingLanguage::Zh);
        assert_eq!(heading_only.story_title, "白夜行");

        // 全空 → fallback。
        let empty = parse_outline("没有任何标题", WritingLanguage::Zh);
        assert_eq!(empty.story_title, "未命名短篇");
        let empty_en = parse_outline("nothing", WritingLanguage::En);
        assert_eq!(empty_en.story_title, "Untitled Short Story");
    }

    #[test]
    fn batch_draft_parses_tagged_chapters() {
        let draft = parse_batch_draft(&draft_raw(2), Some(2), WritingLanguage::Zh);
        assert_eq!(draft.story_title, "夜雨追凶");
        assert_eq!(draft.opening_hook.as_deref(), Some("雨夜钩子"));
        assert_eq!(draft.chapters.len(), 2);
        assert_eq!(draft.chapters[0].title, "标题1");
        assert_eq!(draft.chapters[0].content, "第1章正文内容。");
        // charCount：zh 汉字计量（剥空白后）。
        assert!(draft.chapters[0].char_count > 0);
        // JSON 形态 camelCase + openingHook 保留。
        let json = serde_json::to_value(&draft).unwrap();
        assert_eq!(json["storyTitle"], "夜雨追凶");
        assert_eq!(json["chapters"][0]["charCount"], json["chapters"][0]["charCount"]);
    }

    #[test]
    fn batch_draft_last_non_empty_content_wins() {
        // 同一 CONTENT tag 出现两次（续写合并形态）：取最后一个非空块。
        let raw = "=== SHORT_FICTION_TITLE ===\nT\n=== CHAPTER 1 CONTENT ===\n旧内容\n=== CHAPTER 1 CONTENT ===\n新内容\n";
        let draft = parse_batch_draft(raw, Some(1), WritingLanguage::Zh);
        assert_eq!(draft.chapters[0].content, "新内容");
    }

    #[test]
    fn batch_draft_markdown_fallback_and_duplicate_title() {
        // 无 tagged 块：## 标题回退（前缀需数字形态"第1章"，中文数字不剥——TS 同款）。
        let raw = "# 故事\n\n## 第1章 开端\n\n第一章正文。\n\n## 第2章 转折\n\n第二章正文。\n";
        let draft = parse_batch_draft(raw, Some(2), WritingLanguage::Zh);
        assert_eq!(draft.story_title, "故事");
        assert_eq!(draft.chapters[0].title, "开端");
        assert_eq!(draft.chapters[0].content, "第一章正文。");
        // TS 正则前缀可选：markdown 回退内容任何 N 都取第一个 ## 段（quirk 逐字对齐）。
        assert_eq!(draft.chapters[1].content, "第一章正文。");
        // 中文数字前缀不剥离（`第\s*1\s*章` 要求数字 1）。
        let zh_num = parse_batch_draft("# T\n\n## 第一章 开端\n\n正文。\n", Some(1), WritingLanguage::Zh);
        assert_eq!(zh_num.chapters[0].title, "第一章 开端");

        // 重复 CHAPTER N TITLE（标题块出现两次）：第二个 TITLE tag 后全部内容
        // （含重复标题行自身）到下一个 CHAPTER/SHORT_FICTION tag —— TS 逐字语义。
        let dup = "=== CHAPTER 1 TITLE ===\n一\n=== CHAPTER 1 TITLE ===\n一\n重复标题下的正文。\n=== CHAPTER 1 CONTENT ===\n";
        let draft = parse_batch_draft(dup, Some(1), WritingLanguage::Zh);
        assert_eq!(draft.chapters[0].content, "一\n重复标题下的正文。");
    }

    #[test]
    fn batch_draft_sanitizes_fences_and_tag_lines() {
        let raw = "=== SHORT_FICTION_TITLE ===\nT\n=== CHAPTER 1 CONTENT ===\n```md\n正文第一段。\n=== CHAPTER 1 TITLE ===\n```\n";
        let draft = parse_batch_draft(raw, Some(1), WritingLanguage::Zh);
        assert!(draft.chapters[0].content.contains("正文第一段。"), "{}", draft.chapters[0].content);
        assert!(!draft.chapters[0].content.contains("```"));
        assert!(!draft.chapters[0].content.contains("=== CHAPTER"));
    }

    #[test]
    fn empty_chapter_validation_and_render() {
        let raw = "=== SHORT_FICTION_TITLE ===\nT\n=== CHAPTER 1 CONTENT ===\n有内容\n=== CHAPTER 2 CONTENT ===\n\n";
        let draft = parse_batch_draft(raw, Some(2), WritingLanguage::Zh);
        assert_eq!(find_empty_short_fiction_chapters(&draft), vec![2]);
        assert!(validate_short_fiction_draft_for_final(&draft, Some(2)).is_err());
        let err = validate_short_fiction_draft_for_final(&draft, Some(2)).unwrap_err();
        assert!(err.contains("empty chapters: 2"), "{err}");

        let complete = parse_batch_draft(&draft_raw(2), Some(2), WritingLanguage::Zh);
        assert!(validate_short_fiction_draft_for_final(&complete, Some(2)).is_ok());
        // 章数不匹配错误文案。
        let wrong = validate_short_fiction_draft_for_final(&complete, Some(3)).unwrap_err();
        assert!(wrong.contains("expected 3 chapters, got 2"), "{wrong}");

        let markdown = render_draft_markdown(&complete, WritingLanguage::Zh);
        assert!(markdown.starts_with("# 夜雨追凶"));
        assert!(markdown.contains("## 开篇钩子\n\n雨夜钩子"));
        assert!(markdown.contains("## 第1章 标题1\n\n第1章正文内容。"));
    }

    #[test]
    fn heading_format_normalization() {
        assert_eq!(format_chapter_heading(3, "暴雨", WritingLanguage::Zh), "第3章 暴雨");
        assert_eq!(format_chapter_heading(3, "第3章 暴雨", WritingLanguage::Zh), "第3章 暴雨");
        assert_eq!(format_chapter_heading(3, "", WritingLanguage::Zh), "第3章");
        assert_eq!(format_chapter_heading(3, "Storm", WritingLanguage::En), "Chapter 3: Storm");
        assert_eq!(format_chapter_heading(3, "chapter 3 Storm", WritingLanguage::En), "chapter 3 Storm");
        // normalizeChapterTitle 前缀剥离。
        assert_eq!(normalize_chapter_title("第2章 夜行", 2, WritingLanguage::Zh), "夜行");
        assert_eq!(normalize_chapter_title("Chapter 2: Night", 2, WritingLanguage::En), "Night");
    }

    #[test]
    fn sales_package_parsing() {
        let raw = "=== SHORT_FICTION_PACKAGE_TITLE ===\n《追凶》\n=== SHORT_FICTION_INTRO ===\n一个雨夜的追凶。\n=== SHORT_FICTION_SELLING_POINTS ===\n- 节奏快\n* 反转强\n=== SHORT_FICTION_COVER_PROMPT ===\n3:4 竖图封面\n";
        let package = parse_sales_package(raw, "回退标题");
        assert_eq!(package.title, "追凶");
        assert_eq!(package.intro, "一个雨夜的追凶。");
        assert_eq!(package.selling_points, vec!["节奏快", "反转强"]);
        assert_eq!(package.cover_prompt, "3:4 竖图封面");
        // 空输出 → fallback 标题。
        let empty = parse_sales_package("无任何块", "回退标题");
        assert_eq!(empty.title, "回退标题");
        assert!(empty.selling_points.is_empty());
        // 默认 fallback。
        let default_fallback = parse_sales_package("", "");
        assert_eq!(default_fallback.title, "未命名短篇");
    }

    #[test]
    fn token_estimate_and_transient_errors() {
        assert_eq!(estimate_short_fiction_max_tokens(1, 100), 12_288);
        // 12 章 × 1000 字：f64 里 12000*2.2=26400.000000000004 → ceil 26401 + 4096（JS 同值）。
        assert_eq!(estimate_short_fiction_max_tokens(12, 1000), 30_497);
        assert!(is_transient_short_fiction_error("Unexpected EOF in stream"));
        assert!(is_transient_short_fiction_error("socket hang up"));
        assert!(is_transient_short_fiction_error("fetch failed"));
        assert!(!is_transient_short_fiction_error("HTTP 502 bad gateway"));
    }
}
