//! 剧本/分镜创作 runner（pipeline/script-storyboard-runner.ts 消费面，76 号）。
//!
//! `runScriptCreation` / `runStoryboardCreation`：项目目录落盘（spec → LLM 正文 →
//! status.json；storyboard 另提取 image-prompts + assets 三目录 + assets.json
//! manifest）。interactive-film 全链（剧情树/旗标/story graph 生成）随 77 号。

use std::path::Path;

use serde_json::{json, Value};

use crate::agents::script_storyboard as agents;
use crate::agents::script_storyboard::{ScriptCreationInput, StoryboardCreationInput};
use crate::llm::agent_router::AgentRouter;

pub struct ScriptCreationRunOptions<'a> {
    pub project_root: &'a Path,
    pub router: &'a AgentRouter,
    pub title: &'a str,
    pub instruction: &'a str,
    pub source_kind: Option<&'a str>,
    pub target_format: Option<&'a str>,
    pub source_text: Option<&'a str>,
    pub source_path: Option<&'a str>,
    pub requirements: Option<&'a str>,
    pub episode_count: Option<u32>,
    pub episode_duration: Option<&'a str>,
    pub language: Option<&'a str>,
    pub project_id: Option<&'a str>,
    pub out_dir: Option<&'a str>,
    pub on_progress: &'a mut (dyn FnMut(String) + Send),
}

pub struct StoryboardCreationRunOptions<'a> {
    pub project_root: &'a Path,
    pub router: &'a AgentRouter,
    pub title: &'a str,
    pub instruction: &'a str,
    pub source_kind: Option<&'a str>,
    pub source_text: Option<&'a str>,
    pub source_path: Option<&'a str>,
    pub requirements: Option<&'a str>,
    pub visual_style: Option<&'a str>,
    pub aspect_ratio: Option<&'a str>,
    pub granularity: Option<&'a str>,
    pub max_shots: Option<u32>,
    pub language: Option<&'a str>,
    pub project_id: Option<&'a str>,
    pub out_dir: Option<&'a str>,
    pub on_progress: &'a mut (dyn FnMut(String) + Send),
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptCreationRunResult {
    pub project_id: String,
    pub base_dir: String,
    pub spec_path: String,
    pub script_path: String,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoryboardCreationRunResult {
    pub project_id: String,
    pub base_dir: String,
    pub spec_path: String,
    pub storyboard_path: String,
    pub image_prompts_path: String,
    pub assets_manifest_path: String,
    pub assets_dir: String,
}

// ── 路径辅助（runner.ts 逐字） ───────────────────────────────────

/// `slugify`：lower + 非 [\p{L}\p{N}] 折叠 '-' + 去首尾 + 60 上限。
fn slugify(value: &str) -> String {
    let lowered = value.trim().to_lowercase();
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
        "project".to_string()
    } else {
        out
    }
}

/// `safeSegment`（runner 版）：危险字符折叠 '-' + 去首尾 + 80 上限 + "short-{ms}" 回退。
fn safe_segment(value: &str) -> String {
    let cleaned: String = value
        .trim()
        .chars()
        .flat_map(char::to_lowercase)
        .map(|c| {
            if matches!(c, '\\' | '/' | ':' | '\0' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_whitespace() {
                '-'
            } else {
                c
            }
        })
        .collect();
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
        return format!("short-{}", crate::interaction::session::utc_now_ms());
    }
    out
}

fn normalize_output_dir(value: &str) -> Result<String, String> {
    let text: String = value
        .trim()
        .trim_matches('/')
        .split('/')
        .collect::<Vec<_>>()
        .join("/");
    if text.is_empty() || text.contains("..") || text.contains('\0') {
        return Err(format!("Invalid output directory: {value:?}"));
    }
    Ok(text)
}

fn resolve_project_base_dir(out_dir: &str, project_id: &str) -> Result<String, String> {
    let output_dir = normalize_output_dir(out_dir)?;
    let basename = output_dir.rsplit('/').next().unwrap_or_default();
    if basename == project_id {
        Ok(output_dir)
    } else {
        Ok(format!("{output_dir}/{project_id}"))
    }
}

fn rel_path(segments: &[&str]) -> String {
    segments.join("/")
}

fn safe_child_path(root: &Path, relative: &str) -> Result<std::path::PathBuf, String> {
    // 词法防逃逸（runner 的 safeChildPath 语义——resolve 后 relative 校验）。
    let parts: Vec<&str> = relative.split('/').collect();
    if parts.contains(&"..") || relative.starts_with('/') {
        return Err(format!("Path traversal blocked: {relative}"));
    }
    Ok(root.join(relative))
}

async fn resolve_source_text(
    project_root: &Path,
    source_text: Option<&str>,
    source_path: Option<&str>,
) -> Result<Option<String>, String> {
    if let Some(direct) = source_text.map(str::trim).filter(|t| !t.is_empty()) {
        return Ok(Some(direct.to_string()));
    }
    let Some(path) = source_path.map(str::trim).filter(|p| !p.is_empty()) else {
        return Ok(None);
    };
    let full = safe_child_path(project_root, path)?;
    Ok(Some(
        tokio::fs::read_to_string(&full).await.map_err(|e| e.to_string())?,
    ))
}

async fn write_project_text(project_root: &Path, relative_path: &str, content: &str) -> Result<(), String> {
    let full = safe_child_path(project_root, relative_path)?;
    if let Some(parent) = full.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|e| e.to_string())?;
    }
    let payload = if content.ends_with('\n') {
        content.to_string()
    } else {
        format!("{content}\n")
    };
    tokio::fs::write(&full, payload).await.map_err(|e| e.to_string())
}

async fn ensure_project_dir(project_root: &Path, relative_path: &str) -> Result<(), String> {
    let full = safe_child_path(project_root, relative_path)?;
    tokio::fs::create_dir_all(&full).await.map_err(|e| e.to_string())
}

fn merge_requirements(instruction: &str, requirements: Option<&str>, language: Option<&str>) -> String {
    let extra_label = if language == Some("en") {
        "Additional requirements:"
    } else {
        "补充要求："
    };
    let mut parts = vec![instruction.trim().to_string()];
    if let Some(extra) = requirements.map(str::trim).filter(|r| !r.is_empty()) {
        parts.push(format!("\n{extra_label}\n{extra}"));
    }
    parts.into_iter().filter(|part| !part.is_empty()).collect::<Vec<_>>().join("\n")
}

// ── assets manifest ─────────────────────────────────────────────

/// `createStoryboardAssetsManifest`：image-prompts → shot-NNN 资产条目。
fn create_storyboard_assets_manifest(
    title: &str,
    project_id: &str,
    base_dir: &str,
    storyboard_path: &str,
    image_prompts_path: &str,
    image_prompts: &str,
    created_at: &str,
) -> Value {
    let assets_dir = rel_path(&[base_dir, "assets"]);
    let prompts = agents::parse_storyboard_prompt_lines(image_prompts);
    json!({
        "version": 1,
        "kind": "storyboard_assets",
        "title": title,
        "projectId": project_id,
        "baseDir": base_dir,
        "storyboardPath": storyboard_path,
        "imagePromptsPath": image_prompts_path,
        "assetsDir": assets_dir,
        "sourceDir": rel_path(&[&assets_dir, "source"]),
        "generatedDir": rel_path(&[&assets_dir, "generated"]),
        "selectedDir": rel_path(&[&assets_dir, "selected"]),
        "createdAt": created_at,
        "assets": prompts.iter().enumerate().map(|(index, prompt)| {
            json!({
                "shotId": format!("shot-{:03}", index + 1),
                "prompt": prompt,
                "sourceRefs": [],
                "variants": [],
                "status": "prompt_ready",
            })
        }).collect::<Vec<_>>(),
    })
}

// ── run 函数 ────────────────────────────────────────────────────

pub async fn run_script_creation(options: ScriptCreationRunOptions<'_>) -> Result<ScriptCreationRunResult, String> {
    let project_id = safe_segment(options.project_id.unwrap_or(&slugify(options.title)));
    let base_dir = resolve_project_base_dir(options.out_dir.unwrap_or("dramas"), &project_id)?;
    let source_text = resolve_source_text(options.project_root, options.source_text, options.source_path).await?;
    let input = ScriptCreationInput {
        title: options.title.to_string(),
        source_kind: options.source_kind.map(String::from),
        target_format: options.target_format.map(String::from),
        source_text: source_text.clone(),
        requirements: Some(merge_requirements(options.instruction, options.requirements, options.language)),
        episode_count: options.episode_count,
        episode_duration: options.episode_duration.map(String::from),
        language: options.language.map(String::from),
    };

    (options.on_progress)("Writing script creation spec...".to_string());
    let spec = agents::render_script_spec(&input);
    write_project_text(options.project_root, &rel_path(&[&base_dir, "script-spec.md"]), &spec).await?;

    (options.on_progress)("Writing script draft...".to_string());
    let script = agents::normalize_script_episode_end_labels(&agents::write_script(options.router, &input).await?);
    write_project_text(options.project_root, &rel_path(&[&base_dir, "script.md"]), &script).await?;
    let status = json!({
        "status": "completed",
        "kind": "script",
        "title": options.title,
        "completedAt": crate::utils::utc_time::utc_now_iso(),
    });
    write_project_text(
        options.project_root,
        &rel_path(&[&base_dir, "status.json"]),
        &serde_json::to_string_pretty(&status).unwrap_or_default(),
    )
    .await?;

    Ok(ScriptCreationRunResult {
        spec_path: rel_path(&[&base_dir, "script-spec.md"]),
        script_path: rel_path(&[&base_dir, "script.md"]),
        project_id,
        base_dir,
    })
}

pub async fn run_storyboard_creation(options: StoryboardCreationRunOptions<'_>) -> Result<StoryboardCreationRunResult, String> {
    let project_id = safe_segment(options.project_id.unwrap_or(&slugify(options.title)));
    let base_dir = resolve_project_base_dir(options.out_dir.unwrap_or("storyboards"), &project_id)?;
    let source_text = resolve_source_text(options.project_root, options.source_text, options.source_path).await?;
    let input = StoryboardCreationInput {
        title: options.title.to_string(),
        source_kind: options.source_kind.map(String::from),
        source_text,
        requirements: Some(merge_requirements(options.instruction, options.requirements, options.language)),
        visual_style: options.visual_style.map(String::from),
        aspect_ratio: options.aspect_ratio.map(String::from),
        granularity: options.granularity.map(String::from),
        max_shots: options.max_shots,
        language: options.language.map(String::from),
    };

    (options.on_progress)("Writing storyboard creation spec...".to_string());
    let spec = agents::render_storyboard_spec(&input);
    write_project_text(options.project_root, &rel_path(&[&base_dir, "storyboard-spec.md"]), &spec).await?;

    (options.on_progress)("Writing storyboard and image prompts...".to_string());
    let storyboard = agents::write_storyboard(options.router, &input).await?;
    write_project_text(options.project_root, &rel_path(&[&base_dir, "storyboard.md"]), &storyboard).await?;
    let image_prompts = agents::extract_storyboard_image_prompts(&storyboard);
    write_project_text(options.project_root, &rel_path(&[&base_dir, "image-prompts.md"]), &image_prompts).await?;
    for sub in ["source", "generated", "selected"] {
        ensure_project_dir(options.project_root, &rel_path(&[&base_dir, "assets", sub])).await?;
    }
    let manifest = create_storyboard_assets_manifest(
        options.title,
        &project_id,
        &base_dir,
        &rel_path(&[&base_dir, "storyboard.md"]),
        &rel_path(&[&base_dir, "image-prompts.md"]),
        &image_prompts,
        &crate::utils::utc_time::utc_now_iso(),
    );
    write_project_text(
        options.project_root,
        &rel_path(&[&base_dir, "assets.json"]),
        &serde_json::to_string_pretty(&manifest).unwrap_or_default(),
    )
    .await?;
    let status = json!({
        "status": "completed",
        "kind": "storyboard",
        "title": options.title,
        "completedAt": crate::utils::utc_time::utc_now_iso(),
    });
    write_project_text(
        options.project_root,
        &rel_path(&[&base_dir, "status.json"]),
        &serde_json::to_string_pretty(&status).unwrap_or_default(),
    )
    .await?;

    Ok(StoryboardCreationRunResult {
        spec_path: rel_path(&[&base_dir, "storyboard-spec.md"]),
        storyboard_path: rel_path(&[&base_dir, "storyboard.md"]),
        image_prompts_path: rel_path(&[&base_dir, "image-prompts.md"]),
        assets_manifest_path: rel_path(&[&base_dir, "assets.json"]),
        assets_dir: rel_path(&[&base_dir, "assets"]),
        project_id,
        base_dir,
    })
}
