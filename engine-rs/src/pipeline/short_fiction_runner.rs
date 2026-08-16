//! 短篇 web 小说生产 runner（pipeline/short-fiction-runner.ts 主体，78 号）。
//!
//! `runShortFictionProduction`：可恢复三段链——outline（创建→审纲→一次修订，可从
//! v002.md 断点续跑）→ writer（整稿→缺章补写≤3 轮→审稿→一次修订（不完整降级
//! 保留 v1））→ final 落盘 → 包装（简介/卖点/封面提示词）→ 封面图（失败只降级
//! 不 fail）。稳定 storyId + status.json 让二次运行从磁盘续作；任一阶段失败写
//! failed 状态防半成品冒充成稿。
//!
//! cover 请求解析与三形态生图复用 [`crate::llm::cover`]（74/76 号）。

use std::path::Path;

use serde_json::{json, Value};

use crate::agents::short_fiction::{
    continue_draft, create_outline, find_empty_short_fiction_chapters, generate_package,
    revise_draft, revise_outline, review_draft, review_outline, validate_short_fiction_draft_for_final,
    write_draft, ShortFictionBatchDraft, ShortFictionReference, ShortFictionSalesPackage,
    SHORT_FICTION_DEFAULT_CHAPTERS, SHORT_FICTION_DEFAULT_CHARS_PER_CHAPTER,
    SHORT_FICTION_DRAFT_COMPLETION_ATTEMPTS, SHORT_FICTION_EN_DEFAULT_WORDS_PER_CHAPTER,
    SHORT_FICTION_EN_MAX_WORDS_PER_CHAPTER, SHORT_FICTION_EN_MIN_WORDS_PER_CHAPTER,
    SHORT_FICTION_MAX_CHAPTERS, SHORT_FICTION_MAX_CHARS_PER_CHAPTER, SHORT_FICTION_MIN_CHAPTERS,
    SHORT_FICTION_MIN_CHARS_PER_CHAPTER,
};
use crate::agents::short_fiction as agents;
use crate::llm::agent_router::AgentRouter;
use crate::llm::cover::{
    build_cover_image_prompt, generate_image_from_prompt, resolve_cover_generation_request,
    CoverSalesPackage,
};
use crate::utils::language::WritingLanguage;

pub struct ShortFictionRunOptions<'a> {
    pub project_root: &'a Path,
    pub router: &'a AgentRouter,
    pub direction: &'a str,
    pub reference: Option<&'a ShortFictionReference>,
    pub story_id: Option<&'a str>,
    pub out_dir: Option<&'a str>,
    pub chapter_count: Option<u32>,
    /// 语言原生单位：zh 汉字 / en 单词。
    pub chars_per_chapter: Option<u32>,
    pub language: WritingLanguage,
    pub cover: bool,
    pub on_progress: &'a mut (dyn FnMut(String) + Send),
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortFictionRunResult {
    pub story_id: String,
    pub outline_path: String,
    pub outline_review_path: String,
    pub draft_review_path: String,
    pub final_markdown_path: String,
    pub final_json_path: String,
    pub sales_package_path: String,
    pub cover_prompt_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover_image_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover_error: Option<String>,
}

// ── 路径辅助（short-fiction-runner.ts 逐字——与 script runner 同名但细节不同）──

fn safe_child_path(root: &Path, relative: &str) -> Result<std::path::PathBuf, String> {
    let parts: Vec<&str> = relative.split('/').collect();
    if parts.contains(&"..") || relative.starts_with('/') {
        return Err(format!("Path traversal blocked: {relative}"));
    }
    Ok(root.join(relative))
}

/// `normalizeOutputDir`：trim → 去首尾 `/` → 空回退 "shorts"。
fn normalize_output_dir(value: &str) -> String {
    let trimmed = value.trim();
    let normalized = trimmed
        .trim_matches('/')
        .split('/')
        .collect::<Vec<_>>()
        .join("/");
    if normalized.is_empty() {
        "shorts".to_string()
    } else {
        normalized
    }
}

/// `slugify`（短篇版）：lower → 引号删除 → 非 [\p{L}\p{N}] 折叠 '-' → 去首尾 →
/// 60 上限 → `short-{ms}` 回退。
fn slugify_short(value: &str) -> String {
    let lowered: String = value
        .to_lowercase()
        .chars()
        .filter(|c| *c != '\'' && *c != '"')
        .collect();
    let mut collapsed = String::with_capacity(lowered.len());
    let mut prev_dash = false;
    for ch in lowered.chars() {
        if ch.is_alphanumeric() {
            collapsed.push(ch);
            prev_dash = false;
        } else if !prev_dash {
            collapsed.push('-');
            prev_dash = true;
        }
    }
    let trimmed = collapsed.trim_matches('-').to_string();
    let mut out = String::new();
    let mut units = 0usize;
    for ch in trimmed.chars() {
        let ch_units = ch.len_utf16();
        if units + ch_units > 60 {
            break;
        }
        out.push(ch);
        units += ch_units;
    }
    if out.is_empty() {
        format!("short-{}", crate::interaction::session::utc_now_ms())
    } else {
        out
    }
}

/// `safeSegment`（短篇版）：危险字符逐个折叠 '-' + 空白段折叠 + 去首尾 + 80 上限；
/// **不 lower**（保留用户 storyId 大小写——与 script runner 版的差异点）。
fn safe_segment_short(value: &str) -> String {
    let mut cleaned = String::with_capacity(value.len());
    let mut in_ws_run = false;
    for ch in value.chars() {
        if matches!(ch, '\\' | '/' | ':' | '\0' | '*' | '?' | '"' | '<' | '>' | '|') {
            cleaned.push('-');
            in_ws_run = false;
        } else if ch.is_whitespace() {
            if !in_ws_run {
                cleaned.push('-');
                in_ws_run = true;
            }
        } else {
            cleaned.push(ch);
            in_ws_run = false;
        }
    }
    let trimmed = cleaned.trim_matches('-').to_string();
    let mut out = String::new();
    let mut units = 0usize;
    for ch in trimmed.chars() {
        let ch_units = ch.len_utf16();
        if units + ch_units > 80 {
            break;
        }
        out.push(ch);
        units += ch_units;
    }
    if out.is_empty() || out == "." || out == ".." {
        format!("short-{}", crate::interaction::session::utc_now_ms())
    } else {
        out
    }
}

/// `safeFileName`：危险字符折叠 '_' + 空白归一空格 + trim + 80 上限。
fn safe_file_name(value: &str) -> String {
    let cleaned: String = value
        .chars()
        .map(|c| {
            if matches!(c, '\\' | '/' | ':' | '\0' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else if c.is_whitespace() {
                ' '
            } else {
                c
            }
        })
        .collect();
    let trimmed = cleaned.trim();
    let mut out = String::new();
    let mut units = 0usize;
    for ch in trimmed.chars() {
        let ch_units = ch.len_utf16();
        if units + ch_units > 80 {
            break;
        }
        out.push(ch);
        units += ch_units;
    }
    if out.is_empty() {
        "short-fiction".to_string()
    } else {
        out
    }
}

/// `writeText`：trimEnd + 补单个换行（TS 语义）。
async fn write_text(root: &Path, relative_path: &str, content: &str) -> Result<(), String> {
    let full = safe_child_path(root, relative_path)?;
    if let Some(parent) = full.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|e| e.to_string())?;
    }
    let payload = format!("{}\n", content.trim_end());
    tokio::fs::write(&full, payload).await.map_err(|e| e.to_string())
}

/// `writeJson`：`JSON.stringify(value, null, 2)`（+ 尾换行）。
async fn write_json(root: &Path, relative_path: &str, value: &Value) -> Result<(), String> {
    write_text(root, relative_path, &serde_json::to_string_pretty(value).unwrap_or_default()).await
}

async fn write_binary(root: &Path, relative_path: &str, bytes: &[u8]) -> Result<(), String> {
    let full = safe_child_path(root, relative_path)?;
    if let Some(parent) = full.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|e| e.to_string())?;
    }
    tokio::fs::write(&full, bytes).await.map_err(|e| e.to_string())
}

async fn try_read_project_text(root: &Path, relative_path: &str) -> Option<String> {
    let full = safe_child_path(root, relative_path).ok()?;
    tokio::fs::read_to_string(&full).await.ok()
}

async fn project_file_exists(root: &Path, relative_path: &str) -> bool {
    match safe_child_path(root, relative_path) {
        Ok(full) => tokio::fs::metadata(&full).await.is_ok(),
        Err(_) => false,
    }
}

async fn is_failed_short_run(root: &Path, relative_path: &str) -> bool {
    let Some(raw) = try_read_project_text(root, relative_path).await else {
        return false;
    };
    serde_json::from_str::<Value>(&raw)
        .ok()
        .and_then(|parsed| parsed.get("status").and_then(Value::as_str).map(str::to_string))
        .map(|status| status == "failed")
        .unwrap_or(false)
}

/// `boundedInteger`：缺省回退 + 整数范围校验（文案逐字）。
fn bounded_integer(value: Option<u32>, fallback: u32, name: &str, min: u32, max: u32) -> Result<u32, String> {
    let parsed = value.unwrap_or(fallback);
    if parsed < min || parsed > max {
        return Err(format!("{name} must be an integer between {min} and {max}."));
    }
    Ok(parsed)
}

// ── 产物落盘 ────────────────────────────────────────────────────

fn chapter_file_name(number: u32) -> String {
    format!("{number:04}.md")
}

/// `writeDraftArtifacts`：drafts/{version}/full.md + draft.json + chapters/NNNN.md。
async fn write_draft_artifacts(
    root: &Path,
    base_dir: &str,
    version: &str,
    draft: &ShortFictionBatchDraft,
    language: WritingLanguage,
) -> Result<(), String> {
    let draft_dir = format!("{base_dir}/drafts/{version}");
    write_text(
        root,
        &format!("{draft_dir}/full.md"),
        &agents::render_draft_markdown(draft, language),
    )
    .await?;
    let draft_json = serde_json::to_value(draft).unwrap_or(Value::Null);
    write_json(root, &format!("{draft_dir}/draft.json"), &draft_json).await?;
    for chapter in &draft.chapters {
        write_text(
            root,
            &format!("{draft_dir}/chapters/{}", chapter_file_name(chapter.number)),
            &format!(
                "# {}\n\n{}",
                agents::format_chapter_heading(chapter.number, &chapter.title, language),
                chapter.content
            ),
        )
        .await?;
    }
    Ok(())
}

/// `writeFinalArtifacts`：final/full.md + {safeFileName(title)}.md + short-story.json
/// + chapters/NNNN.md。
async fn write_final_artifacts(
    root: &Path,
    base_dir: &str,
    draft: &ShortFictionBatchDraft,
    language: WritingLanguage,
) -> Result<(), String> {
    let final_dir = format!("{base_dir}/final");
    let markdown = agents::render_draft_markdown(draft, language);
    write_text(root, &format!("{final_dir}/full.md"), &markdown).await?;
    write_text(
        root,
        &format!("{final_dir}/{}.md", safe_file_name(&draft.story_title)),
        &markdown,
    )
    .await?;
    let draft_json = serde_json::to_value(draft).unwrap_or(Value::Null);
    write_json(root, &format!("{final_dir}/short-story.json"), &draft_json).await?;
    for chapter in &draft.chapters {
        write_text(
            root,
            &format!("{final_dir}/chapters/{}", chapter_file_name(chapter.number)),
            &format!(
                "# {}\n\n{}",
                agents::format_chapter_heading(chapter.number, &chapter.title, language),
                chapter.content
            ),
        )
        .await?;
    }
    Ok(())
}

/// sales-package.md 渲染（纯函数，双语标题）。
fn package_markdown(package: &ShortFictionSalesPackage, language: WritingLanguage) -> String {
    let (intro, selling, cover) = match language {
        WritingLanguage::En => ("## Synopsis", "## Selling Points", "## Cover Prompt"),
        WritingLanguage::Zh => ("## 简介", "## 卖点", "## 封面提示词"),
    };
    let mut lines = vec![
        format!("# {}", package.title),
        String::new(),
        intro.to_string(),
        String::new(),
        package.intro.clone(),
        String::new(),
        selling.to_string(),
        String::new(),
    ];
    lines.extend(package.selling_points.iter().map(|point| format!("- {point}")));
    lines.push(String::new());
    lines.push(cover.to_string());
    lines.push(String::new());
    lines.push(package.cover_prompt.clone());
    lines.join("\n")
}

/// `writePackageArtifacts`：sales-package.json + sales-package.md + cover-prompt.md。
async fn write_package_artifacts(
    root: &Path,
    base_dir: &str,
    package: &ShortFictionSalesPackage,
    language: WritingLanguage,
) -> Result<(), String> {
    let final_dir = format!("{base_dir}/final");
    let package_json = serde_json::to_value(package).unwrap_or(Value::Null);
    write_json(root, &format!("{final_dir}/sales-package.json"), &package_json).await?;
    write_text(
        root,
        &format!("{final_dir}/sales-package.md"),
        &package_markdown(package, language),
    )
    .await?;
    write_text(
        root,
        &format!("{final_dir}/cover-prompt.md"),
        if package.cover_prompt.is_empty() { "(empty)" } else { &package.cover_prompt },
    )
    .await?;
    Ok(())
}

/// 第二轮改稿未采用说明文件（双语逐字）。
fn revision_warning_markdown(warning: &str, language: WritingLanguage) -> String {
    match language {
        WritingLanguage::En => [
            "# Second revision not adopted",
            "",
            "The system refused to overwrite the complete first draft with an incomplete or unparsable revision.",
            "",
            "## Reason",
            "",
            warning,
        ]
        .join("\n"),
        WritingLanguage::Zh => [
            "# 第二轮改稿未采用",
            "",
            "系统没有用不完整和解析失败的改稿覆盖完整首稿。",
            "",
            "## 原因",
            "",
            warning,
        ]
        .join("\n"),
    }
}

/// `writeShortRunStatus`：状态 + updatedAt ISO。
async fn write_short_run_status(root: &Path, base_dir: &str, mut value: Value) -> Result<(), String> {
    if let Some(map) = value.as_object_mut() {
        map.insert(
            "updatedAt".to_string(),
            json!(crate::utils::utc_time::utc_now_iso()),
        );
    }
    write_json(root, &format!("{base_dir}/status.json"), &value).await
}

/// `generateCoverArtifact`：short 提示词生图 → final/cover.{png|jpg}。
async fn generate_cover_artifact(
    root: &Path,
    base_dir: &str,
    package: &ShortFictionSalesPackage,
    language: WritingLanguage,
) -> Result<String, String> {
    let request = resolve_cover_generation_request(root).await?;
    let size = std::env::var("INKOS_COVER_SIZE")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "1024x1360".to_string());
    let lang_str = match language {
        WritingLanguage::En => "en",
        WritingLanguage::Zh => "zh",
    };
    let prompt_package = CoverSalesPackage {
        title: &package.title,
        intro: &package.intro,
        selling_points: &package.selling_points,
        cover_prompt: &package.cover_prompt,
    };
    let prompt = build_cover_image_prompt(&prompt_package, "short", Some(lang_str));
    let image = generate_image_from_prompt(&request, &prompt, &size).await?;
    let image_name = if image.extension == "jpg" { "cover.jpg" } else { "cover.png" };
    let relative = format!("{base_dir}/final/{image_name}");
    write_binary(root, &relative, &image.bytes).await?;
    Ok(relative)
}

fn build_short_run_result(
    story_id: &str,
    base_dir: &str,
    cover_image_path: Option<String>,
    cover_error: Option<String>,
) -> ShortFictionRunResult {
    ShortFictionRunResult {
        story_id: story_id.to_string(),
        outline_path: format!("{base_dir}/outline/v002.md"),
        outline_review_path: format!("{base_dir}/reviews/outline-v001.md"),
        draft_review_path: format!("{base_dir}/reviews/draft-v001.md"),
        final_markdown_path: format!("{base_dir}/final/full.md"),
        final_json_path: format!("{base_dir}/final/short-story.json"),
        sales_package_path: format!("{base_dir}/final/sales-package.md"),
        cover_prompt_path: format!("{base_dir}/final/cover-prompt.md"),
        cover_image_path,
        cover_error,
    }
}

// ── 主链 ────────────────────────────────────────────────────────

/// `runShortFictionProduction`：入口（resume 探测 + 失败状态落盘 + produceShort）。
pub async fn run_short_fiction_production(
    options: ShortFictionRunOptions<'_>,
) -> Result<ShortFictionRunResult, String> {
    let root = options.project_root;
    let out_dir = normalize_output_dir(options.out_dir.unwrap_or("shorts"));
    let provided_story_id = options.story_id.map(safe_segment_short);

    // 稳定 storyId 让二次运行从磁盘续作；已完成（且非 failed）直接按原样返回。
    if let Some(story_id) = &provided_story_id {
        let base_dir = format!("{out_dir}/{story_id}");
        if project_file_exists(root, &format!("{base_dir}/final/full.md")).await
            && !is_failed_short_run(root, &format!("{base_dir}/status.json")).await
        {
            return Ok(build_short_run_result(
                story_id,
                &base_dir,
                None,
                Some("already-complete".to_string()),
            ));
        }
    }

    let result = produce_short(options, root, &out_dir, provided_story_id.as_deref()).await;
    if let Err(error) = &result {
        // 半成品标 failed——防 drafts 冒充成稿（无 updatedAt，TS 外层 writeJson 逐字）。
        if let Some(story_id) = &provided_story_id {
            let _ = write_json(
                root,
                &format!("{out_dir}/{story_id}/status.json"),
                &json!({ "status": "failed", "error": error }),
            )
            .await;
        }
    }
    result
}

#[allow(clippy::too_many_lines)]
async fn produce_short(
    options: ShortFictionRunOptions<'_>,
    root: &Path,
    out_dir: &str,
    provided_story_id: Option<&str>,
) -> Result<ShortFictionRunResult, String> {
    let language = options.language;
    let chapter_count = bounded_integer(
        options.chapter_count,
        SHORT_FICTION_DEFAULT_CHAPTERS,
        "chapterCount",
        SHORT_FICTION_MIN_CHAPTERS,
        SHORT_FICTION_MAX_CHAPTERS,
    )?;
    // charsPerChapter 是语言原生单位：zh 汉字（900-1200）/ en 单词（600-800）。
    let chars_per_chapter = match language {
        WritingLanguage::En => bounded_integer(
            options.chars_per_chapter,
            SHORT_FICTION_EN_DEFAULT_WORDS_PER_CHAPTER,
            "charsPerChapter",
            SHORT_FICTION_EN_MIN_WORDS_PER_CHAPTER,
            SHORT_FICTION_EN_MAX_WORDS_PER_CHAPTER,
        )?,
        WritingLanguage::Zh => bounded_integer(
            options.chars_per_chapter,
            SHORT_FICTION_DEFAULT_CHARS_PER_CHAPTER,
            "charsPerChapter",
            SHORT_FICTION_MIN_CHARS_PER_CHAPTER,
            SHORT_FICTION_MAX_CHARS_PER_CHAPTER,
        )?,
    };

    // v002.md 已在 → writer 及之后只依赖大纲 Markdown，断点续跑。
    let resumed_content = match provided_story_id {
        Some(story_id) => {
            try_read_project_text(root, &format!("{out_dir}/{story_id}/outline/v002.md")).await
        }
        None => None,
    };

    let outline_markdown: String;
    let story_id: String;
    let base_dir: String;
    if provided_story_id.is_some() && resumed_content.as_deref().is_some_and(|c| !c.trim().is_empty()) {
        story_id = provided_story_id.unwrap_or_default().to_string();
        base_dir = format!("{out_dir}/{story_id}");
        outline_markdown = resumed_content.unwrap_or_default();
        (options.on_progress)("Resuming from existing outline (skipping outline stages)...".to_string());
    } else {
        (options.on_progress)("Creating short fiction outline...".to_string());
        let outline_v1 = create_outline(
            options.router,
            options.direction,
            chapter_count,
            chars_per_chapter,
            options.reference,
            language,
        )
        .await?;

        story_id = provided_story_id
            .map(str::to_string)
            .unwrap_or_else(|| {
                let source = if outline_v1.story_title.is_empty() {
                    options.direction
                } else {
                    &outline_v1.story_title
                };
                safe_segment_short(&slugify_short(source))
            });
        base_dir = format!("{out_dir}/{story_id}");
        write_text(root, &format!("{base_dir}/outline/v001.md"), &outline_v1.raw_content).await?;

        (options.on_progress)("Reviewing outline...".to_string());
        let outline_review = review_outline(
            options.router,
            options.direction,
            &outline_v1.raw_content,
            options.reference,
            language,
        )
        .await?;
        write_text(root, &format!("{base_dir}/reviews/outline-v001.md"), &outline_review).await?;

        (options.on_progress)("Revising outline once...".to_string());
        let outline_v2 = revise_outline(
            options.router,
            options.direction,
            &outline_v1,
            &outline_review,
            options.reference,
            chapter_count,
            chars_per_chapter,
            language,
        )
        .await?;
        write_text(root, &format!("{base_dir}/outline/v002.md"), &outline_v2.raw_content).await?;
        outline_markdown = outline_v2.raw_content;
    }

    let mut revision_warning: Option<String> = None;
    let result_body = async {
        let mut final_draft: ShortFictionBatchDraft;
        (options.on_progress)("Writing full short fiction draft...".to_string());
        let mut draft_v1 = write_draft(
            options.router,
            options.direction,
            &outline_markdown,
            chapter_count,
            chars_per_chapter,
            language,
        )
        .await?;
        let mut missing_from_draft = find_empty_short_fiction_chapters(&draft_v1);
        if !missing_from_draft.is_empty() {
            write_draft_artifacts(root, &base_dir, "v001-partial", &draft_v1, language).await?;
            let mut attempt = 1;
            while !missing_from_draft.is_empty() && attempt <= SHORT_FICTION_DRAFT_COMPLETION_ATTEMPTS {
                let joined = missing_from_draft
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                (options.on_progress)(format!(
                    "Completing missing short fiction chapters: {joined}..."
                ));
                draft_v1 = continue_draft(
                    options.router,
                    options.direction,
                    &outline_markdown,
                    chapter_count,
                    chars_per_chapter,
                    language,
                    &draft_v1,
                )
                .await?;
                missing_from_draft = find_empty_short_fiction_chapters(&draft_v1);
                if !missing_from_draft.is_empty() {
                    write_draft_artifacts(root, &base_dir, "v001-partial", &draft_v1, language).await?;
                }
                attempt += 1;
            }
        }
        validate_short_fiction_draft_for_final(&draft_v1, Some(chapter_count))?;
        write_draft_artifacts(root, &base_dir, "v001", &draft_v1, language).await?;

        (options.on_progress)("Reviewing full draft...".to_string());
        let draft_review = review_draft(
            options.router,
            options.direction,
            &outline_markdown,
            chapter_count,
            chars_per_chapter,
            language,
            &draft_v1,
        )
        .await?;
        write_text(root, &format!("{base_dir}/reviews/draft-v001.md"), &draft_review).await?;

        final_draft = draft_v1.clone();
        (options.on_progress)("Revising full draft once...".to_string());
        let revision: Result<ShortFictionBatchDraft, String> = async {
            let draft_v2 = revise_draft(
                options.router,
                options.direction,
                &outline_markdown,
                chapter_count,
                chars_per_chapter,
                language,
                &draft_v1,
                &draft_review,
            )
            .await?;
            validate_short_fiction_draft_for_final(&draft_v2, Some(chapter_count))?;
            write_draft_artifacts(root, &base_dir, "v002", &draft_v2, language).await?;
            Ok(draft_v2)
        }
        .await;
        match revision {
            Ok(draft_v2) => final_draft = draft_v2,
            Err(error) => {
                write_text(
                    root,
                    &format!("{base_dir}/reviews/draft-v002-warning.md"),
                    &revision_warning_markdown(&error, language),
                )
                .await?;
                revision_warning = Some(error);
            }
        }

        write_final_artifacts(root, &base_dir, &final_draft, language).await?;

        (options.on_progress)("Generating synopsis and cover prompt...".to_string());
        let package = generate_package(
            options.router,
            options.direction,
            &outline_markdown,
            &final_draft,
            language,
        )
        .await?;
        write_package_artifacts(root, &base_dir, &package, language).await?;
        Ok(package)
    };

    let sales_package = match result_body.await {
        Ok(package) => package,
        Err(error) => {
            // 生产段失败：status failed（带 updatedAt）+ 原样上抛。
            let _ = write_short_run_status(
                root,
                &base_dir,
                json!({ "status": "failed", "error": error }),
            )
            .await;
            return Err(error);
        }
    };

    // 封面：false → disabled；失败 → coverError 降级（不 fail 任务）。
    let (cover_image_path, cover_error) = if !options.cover {
        (None, Some("disabled".to_string()))
    } else {
        match generate_cover_artifact(root, &base_dir, &sales_package, language).await {
            Ok(path) => (Some(path), None),
            Err(error) => (None, Some(error)),
        }
    };

    if let Some(warning) = &revision_warning {
        let _ = write_short_run_status(
            root,
            &base_dir,
            json!({ "status": "complete", "warning": format!("revision skipped: {warning}") }),
        )
        .await;
    }

    Ok(build_short_run_result(&story_id, &base_dir, cover_image_path, cover_error))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_short_collapses_and_falls_back() {
        assert_eq!(slugify_short("Night's Rain!"), "nights-rain");
        assert_eq!(slugify_short("  --- 标题 ---  "), "标题");
        assert!(slugify_short("!!!").starts_with("short-"));
        assert!(slugify_short("").starts_with("short-"));
    }

    #[test]
    fn safe_segment_short_preserves_case() {
        // 空白段折叠为单个 '-'（TS replace(/\s+/g, "-")）。
        assert_eq!(safe_segment_short("My Story"), "My-Story");
        assert_eq!(safe_segment_short("a/b\\c:d*e?f\"g<h>i|j"), "a-b-c-d-e-f-g-h-i-j");
        // 空白段折叠为单个 '-'。
        assert_eq!(safe_segment_short("a   b"), "a-b");
        // 空段回退。
        assert!(safe_segment_short("---").starts_with("short-"));
        assert_eq!(safe_segment_short("..."), "...");
        assert!(safe_segment_short(".").starts_with("short-"));
    }

    #[test]
    fn safe_file_name_sanitizes() {
        // 全角冒号不在 TS 折叠集（仅 ASCII :）；'/' 折叠 '_'。
        assert_eq!(safe_file_name("夜雨：追凶/第一章"), "夜雨：追凶_第一章");
        assert_eq!(safe_file_name("a:b|c"), "a_b_c");
        assert_eq!(safe_file_name("  多 空白  "), "多 空白");
        assert_eq!(safe_file_name(""), "short-fiction");
    }

    #[test]
    fn bounded_integer_range_messages() {
        assert_eq!(bounded_integer(None, 12, "chapterCount", 12, 18).unwrap(), 12);
        assert_eq!(bounded_integer(Some(15), 12, "chapterCount", 12, 18).unwrap(), 15);
        let err = bounded_integer(Some(20), 12, "chapterCount", 12, 18).unwrap_err();
        assert_eq!(err, "chapterCount must be an integer between 12 and 18.");
        let err = bounded_integer(Some(850), 1000, "charsPerChapter", 900, 1200).unwrap_err();
        assert_eq!(err, "charsPerChapter must be an integer between 900 and 1200.");
    }

    #[test]
    fn package_markdown_shapes() {
        let package = ShortFictionSalesPackage {
            title: "夜雨追凶".into(),
            intro: "雨夜追凶简介。".into(),
            selling_points: vec!["节奏快".into(), "反转强".into()],
            cover_prompt: "3:4 竖图".into(),
            raw_content: String::new(),
        };
        let zh = package_markdown(&package, WritingLanguage::Zh);
        assert!(zh.starts_with("# 夜雨追凶\n\n## 简介\n\n雨夜追凶简介。\n\n## 卖点\n\n- 节奏快\n- 反转强\n\n## 封面提示词\n\n3:4 竖图"), "{zh}");
        let en = package_markdown(&package, WritingLanguage::En);
        assert!(en.contains("## Synopsis"));
        assert!(en.contains("- 节奏快"));
    }

    #[test]
    fn revision_warning_markdown_shapes() {
        let zh = revision_warning_markdown("漏了两章", WritingLanguage::Zh);
        assert!(zh.starts_with("# 第二轮改稿未采用\n\n系统没有用不完整和解析失败的改稿覆盖完整首稿。\n\n## 原因\n\n漏了两章"), "{zh}");
        let en = revision_warning_markdown("two chapters missing", WritingLanguage::En);
        assert!(en.starts_with("# Second revision not adopted"), "{en}");
        assert!(en.contains("## Reason\n\ntwo chapters missing"));
    }

    #[test]
    fn result_paths_and_chapter_file_names() {
        assert_eq!(chapter_file_name(1), "0001.md");
        assert_eq!(chapter_file_name(12), "0012.md");
        let result = build_short_run_result("sid", "shorts/sid", Some("shorts/sid/final/cover.png".into()), None);
        assert_eq!(result.outline_path, "shorts/sid/outline/v002.md");
        assert_eq!(result.outline_review_path, "shorts/sid/reviews/outline-v001.md");
        assert_eq!(result.draft_review_path, "shorts/sid/reviews/draft-v001.md");
        assert_eq!(result.final_markdown_path, "shorts/sid/final/full.md");
        assert_eq!(result.final_json_path, "shorts/sid/final/short-story.json");
        assert_eq!(result.sales_package_path, "shorts/sid/final/sales-package.md");
        assert_eq!(result.cover_prompt_path, "shorts/sid/final/cover-prompt.md");
        // serde camelCase + Option none 省略。
        let value = serde_json::to_value(&result).unwrap();
        assert_eq!(value["storyId"], "sid");
        assert_eq!(value["finalMarkdownPath"], "shorts/sid/final/full.md");
        assert!(value.get("coverError").is_none());
        assert_eq!(value["coverImagePath"], "shorts/sid/final/cover.png");
    }
}
