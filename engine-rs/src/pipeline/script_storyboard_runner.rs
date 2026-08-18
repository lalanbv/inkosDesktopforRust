//! 剧本/分镜创作 runner（pipeline/script-storyboard-runner.ts 消费面，76 号）。
//!
//! `runScriptCreation` / `runStoryboardCreation` / `runInteractiveFilmCreation`：
//! 项目目录落盘（spec → LLM 正文 → status.json；storyboard/interactive-film 另提取
//! image-prompts + assets 三目录 + assets.json manifest；interactive-film 追加五节
//! Markdown + story-graph.json 生成与 fallback，77 号）。

use std::path::Path;

use serde_json::{json, Value};

use crate::agents::script_storyboard as agents;
use crate::agents::script_storyboard::{
    InteractiveFilmCreationInput, ScriptCreationInput, StoryboardCreationInput,
};
use crate::interactive_film::StoryGraph;
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

pub struct InteractiveFilmCreationRunOptions<'a> {
    pub project_root: &'a Path,
    pub router: &'a AgentRouter,
    pub title: &'a str,
    pub instruction: &'a str,
    pub source_kind: Option<&'a str>,
    pub source_text: Option<&'a str>,
    pub source_path: Option<&'a str>,
    pub requirements: Option<&'a str>,
    pub target_audience: Option<&'a str>,
    pub episode_count: Option<u32>,
    pub episode_duration: Option<&'a str>,
    pub budget: Option<&'a str>,
    pub reference_mode: Option<&'a str>,
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

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractiveFilmCreationRunResult {
    pub project_id: String,
    pub base_dir: String,
    pub story_graph_path: String,
    pub spec_path: String,
    pub story_tree_path: String,
    pub flags_path: String,
    pub script_path: String,
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


/// TS `textArtifact`（141 号）：内容保证尾换行的产物条目。
fn text_artifact(relative_path: String, content: &str) -> crate::utils::atomic_file_set::AtomicFileWrite {
    use crate::utils::atomic_file_set::{AtomicFileWrite, FileContent};
    let content = if content.ends_with('\n') { content.to_string() } else { format!("{content}\n") };
    AtomicFileWrite { relative_path, content: FileContent::Text(content) }
}

/// TS `assertNonEmptyArtifacts`：任一产物空 → 拒交。
fn assert_non_empty_artifacts(
    artifacts: &[crate::utils::atomic_file_set::AtomicFileWrite],
) -> Result<(), String> {
    for artifact in artifacts {
        let empty = match &artifact.content {
            crate::utils::atomic_file_set::FileContent::Text(text) => text.trim().is_empty(),
            crate::utils::atomic_file_set::FileContent::Bytes(bytes) => bytes.is_empty(),
        };
        if empty {
            return Err(format!("Production artifact is empty: {}", artifact.relative_path));
        }
    }
    Ok(())
}

/// 三面共用的 commit 尾（141 号：产物 + 完成快照同事务；快照形态对齐 TS
/// 各 commitProductionArtifacts 块——kind 分面、status complete、stage commit）。
async fn commit_production_complete(
    project_root: &std::path::Path,
    artifacts: Vec<crate::utils::atomic_file_set::AtomicFileWrite>,
    run_path: String,
    kind: crate::production::ProductionKind,
    project_id: &str,
) -> Result<(), String> {
    let artifact_paths: Vec<String> = artifacts.iter().map(|a| a.relative_path.clone()).collect();
    crate::production::commit_production_artifacts(
        project_root,
        artifacts,
        &run_path,
        &crate::production::ProductionRunSnapshot::create(crate::production::CreateRunInput {
            kind,
            id: project_id.to_string(),
            status: crate::production::ProductionRunStatus::Complete,
            stage: "commit".to_string(),
            artifacts: artifact_paths,
            observations: Vec::new(),
            model: None,
            skill_ids: None,
            resume_cursor: None,
            error: None,
        }),
        Vec::new(),
        None,
    )
    .await
    .map_err(|e| e.to_string())
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

    (options.on_progress)("Writing script draft...".to_string());
    let script = agents::normalize_script_episode_end_labels(&agents::write_script(options.router, &input).await?);
    // 141 号：交付校验（恰好一份人物节 + 非空剧本正文节）先行——拒则零提交。
    agents::assert_script_deliverable(&script, options.language)?;
    let artifacts = vec![
        text_artifact(rel_path(&[&base_dir, "script-spec.md"]), &spec),
        text_artifact(rel_path(&[&base_dir, "script.md"]), &script),
    ];
    assert_non_empty_artifacts(&artifacts)?;
    commit_production_complete(
        options.project_root,
        artifacts,
        rel_path(&[&base_dir, "status.json"]),
        crate::production::ProductionKind::Script,
        &project_id,
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

    (options.on_progress)("Writing storyboard and image prompts...".to_string());
    let storyboard = agents::write_storyboard(options.router, &input).await?;
    let image_prompts = agents::extract_storyboard_image_prompts(&storyboard);
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
    let artifacts = vec![
        text_artifact(rel_path(&[&base_dir, "storyboard-spec.md"]), &spec),
        text_artifact(rel_path(&[&base_dir, "storyboard.md"]), &storyboard),
        text_artifact(rel_path(&[&base_dir, "image-prompts.md"]), &image_prompts),
        text_artifact(
            rel_path(&[&base_dir, "assets.json"]),
            &serde_json::to_string_pretty(&manifest).unwrap_or_default(),
        ),
    ];
    assert_non_empty_artifacts(&artifacts)?;
    commit_production_complete(
        options.project_root,
        artifacts,
        rel_path(&[&base_dir, "status.json"]),
        crate::production::ProductionKind::Storyboard,
        &project_id,
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

// ── interactive-film 全链（77 号） ────────────────────────────────

/// `runInteractiveFilmCreation`：spec → 五节 LLM 交付稿 → 五文件 + assets
/// manifest → story graph（LLM 生成失败回退最小可玩图）→ status。
pub async fn run_interactive_film_creation(
    options: InteractiveFilmCreationRunOptions<'_>,
) -> Result<InteractiveFilmCreationRunResult, String> {
    let project_id = safe_segment(options.project_id.unwrap_or(&slugify(options.title)));
    let base_dir = resolve_project_base_dir(options.out_dir.unwrap_or("interactive-films"), &project_id)?;
    let source_text = resolve_source_text(options.project_root, options.source_text, options.source_path).await?;
    let input = InteractiveFilmCreationInput {
        title: options.title.to_string(),
        source_kind: options.source_kind.map(String::from),
        source_text,
        requirements: Some(merge_requirements(options.instruction, options.requirements, options.language)),
        target_audience: options.target_audience.map(String::from),
        episode_count: options.episode_count,
        episode_duration: options.episode_duration.map(String::from),
        budget: options.budget.map(String::from),
        reference_mode: options.reference_mode.map(String::from),
        language: options.language.map(String::from),
    };

    (options.on_progress)("Writing interactive-film creation spec...".to_string());
    let spec = agents::render_interactive_film_spec(&input);

    (options.on_progress)("Writing story tree, flags, script, storyboard, and image prompts...".to_string());
    let package_markdown = agents::write_interactive_film(options.router, &input).await?;
    let story_tree = agents::required_section(&package_markdown, &["剧情树", "Story Tree", "Branching Story Tree"]);
    let flags = agents::required_section(&package_markdown, &[
        "旗标与变量系统说明",
        "变量与旗标表",
        "变量和旗标表",
        "变量表",
        "旗标表",
        "Variables and Flags",
        "Flag Table",
    ]);
    let script = agents::required_section(&package_markdown, &["互动剧本", "Interactive Script", "Script"]);
    let storyboard = agents::required_section(&package_markdown, &[
        "分镜与图像提示词",
        "分镜表",
        "Storyboard and Image Prompts",
        "Storyboard",
    ]);
    let image_prompts = agents::extract_storyboard_image_prompts(&storyboard);
    let story_graph_path = rel_path(&["interactive-films", &project_id, "story-graph.json"]);

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

    (options.on_progress)("Writing interactive-film story graph...".to_string());
    let graph = create_interactive_film_story_graph(
        options.router,
        &project_id,
        options.title,
        &input,
        &story_tree,
        &flags,
        &script,
        &image_prompts,
        options.on_progress,
    )
    .await;
    graph.validate_schema_version().map_err(|e| e.to_string())?;

    // 141 号：TS 已把 graph JSON 并入同事务（story-graph.json 为产物之一，
    // saveStoryGraph 独立写移除）；schema 校验保留在提交前。
    let artifacts = vec![
        text_artifact(rel_path(&[&base_dir, "interactive-spec.md"]), &spec),
        text_artifact(rel_path(&[&base_dir, "story-tree.md"]), &story_tree),
        text_artifact(rel_path(&[&base_dir, "flags.md"]), &flags),
        text_artifact(
            rel_path(&[&base_dir, "script.md"]),
            &agents::normalize_script_episode_end_labels(&script),
        ),
        text_artifact(rel_path(&[&base_dir, "storyboard.md"]), &storyboard),
        text_artifact(rel_path(&[&base_dir, "image-prompts.md"]), &image_prompts),
        text_artifact(
            rel_path(&[&base_dir, "assets.json"]),
            &serde_json::to_string_pretty(&manifest).unwrap_or_default(),
        ),
        text_artifact(
            story_graph_path.clone(),
            &serde_json::to_string_pretty(&graph).unwrap_or_default(),
        ),
    ];
    assert_non_empty_artifacts(&artifacts)?;
    commit_production_complete(
        options.project_root,
        artifacts,
        rel_path(&[&base_dir, "status.json"]),
        crate::production::ProductionKind::InteractiveFilm,
        &project_id,
    )
    .await?;

    Ok(InteractiveFilmCreationRunResult {
        project_id,
        story_graph_path,
        spec_path: rel_path(&[&base_dir, "interactive-spec.md"]),
        story_tree_path: rel_path(&[&base_dir, "story-tree.md"]),
        flags_path: rel_path(&[&base_dir, "flags.md"]),
        script_path: rel_path(&[&base_dir, "script.md"]),
        storyboard_path: rel_path(&[&base_dir, "storyboard.md"]),
        image_prompts_path: rel_path(&[&base_dir, "image-prompts.md"]),
        assets_manifest_path: rel_path(&[&base_dir, "assets.json"]),
        assets_dir: rel_path(&[&base_dir, "assets"]),
        base_dir,
    })
}

/// `createInteractiveFilmStoryGraph`：LLM 生成 → 失败回退最小可玩图。
#[allow(clippy::too_many_arguments)]
async fn create_interactive_film_story_graph(
    router: &AgentRouter,
    project_id: &str,
    title: &str,
    input: &InteractiveFilmCreationInput,
    story_tree: &str,
    flags: &str,
    script: &str,
    image_prompts: &str,
    on_progress: &mut (dyn FnMut(String) + Send),
) -> StoryGraph {
    let premise = build_interactive_film_graph_premise(input, story_tree, flags, script, image_prompts);
    match agents::generate_story_graph_from_premise(router, project_id, title, &premise, input.language.as_deref()).await {
        Ok(graph) => graph,
        Err(error) => {
            on_progress(format!(
                "Story graph JSON generation failed; writing a minimal playable graph. {error}"
            ));
            build_fallback_story_graph(project_id, title, input, image_prompts)
        }
    }
}

/// `buildInteractiveFilmGraphPremise`：创作需求 + 五段交付稿拼接前提。
fn build_interactive_film_graph_premise(
    input: &InteractiveFilmCreationInput,
    story_tree: &str,
    flags: &str,
    script: &str,
    image_prompts: &str,
) -> String {
    if input.language.as_deref() == Some("en") {
        [
            Some(format!("Creation brief: {}", input.requirements.as_deref().unwrap_or_default())),
            input.target_audience.as_deref().map(|a| format!("Target audience: {a}")),
            input.episode_count.map(|c| format!("Segments/episodes: {c}")),
            input.episode_duration.as_deref().map(|d| format!("Per-segment duration: {d}")),
            input.budget.as_deref().map(|b| format!("Budget: {b}")),
            input.reference_mode.as_deref().map(|r| format!("Reference mode: {r}")),
            Some(format!("Story tree:\n{story_tree}")),
            Some(format!("Variables and flags:\n{flags}")),
            Some(format!("Interactive script:\n{script}")),
            Some(format!("Image prompts:\n{image_prompts}")),
        ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join("\n\n")
    } else {
        [
            Some(format!("创作需求：{}", input.requirements.as_deref().unwrap_or_default())),
            input.target_audience.as_deref().map(|a| format!("目标受众：{a}")),
            input.episode_count.map(|c| format!("段落/集数：{c}")),
            input.episode_duration.as_deref().map(|d| format!("单段时长：{d}")),
            input.budget.as_deref().map(|b| format!("预算：{b}")),
            input.reference_mode.as_deref().map(|r| format!("参考模式：{r}")),
            Some(format!("剧情树：\n{story_tree}")),
            Some(format!("变量旗标：\n{flags}")),
            Some(format!("互动剧本：\n{script}")),
            Some(format!("图像提示词：\n{image_prompts}")),
        ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}

/// `buildFallbackStoryGraph`：start + act 链（末幕 branch 双选择）+ 双结局 +
/// story_progress counter——LLM 失败时保证 graph 可玩。
fn build_fallback_story_graph(
    project_id: &str,
    title: &str,
    input: &InteractiveFilmCreationInput,
    image_prompts: &str,
) -> StoryGraph {
    use crate::interactive_film::{
        Choice, Effect, EffectOp, Ending, EndingType, ImageSlot, NodeType, Position, StoryNode,
        Variable, VariableType, WorldAnchor,
    };
    let en = input.language.as_deref() == Some("en");
    let prompts = agents::parse_storyboard_prompt_lines(image_prompts);
    // (episodeCount ?? prompts.length) || 3，再 clamp [2, 8]。
    let act_base = input.episode_count.map(|c| c as usize).unwrap_or(prompts.len());
    let act_count = if act_base == 0 { 3 } else { act_base }.clamp(2, 8);
    let requirements_or_title = || {
        input
            .requirements
            .as_deref()
            .map(str::trim)
            .filter(|r| !r.is_empty())
            .unwrap_or(title)
    };
    let prompt_at = |index: usize| -> String {
        prompts
            .get(index)
            .cloned()
            .or_else(|| prompts.first().cloned())
            .unwrap_or_else(|| requirements_or_title().to_string())
    };
    let story_step = || Effect {
        var: "story_progress".to_string(),
        op: EffectOp::Add,
        value: json!(1),
    };

    let mut nodes = vec![StoryNode {
        id: "start".to_string(),
        title: if en { "Opening".into() } else { "开场".to_string() },
        node_type: NodeType::Start,
        scene_desc: requirements_or_title().to_string(),
        dialogue: Vec::new(),
        choices: vec![Choice {
            id: "start-act-1".to_string(),
            text: if en { "Enter Act 1".into() } else { "进入第一幕".to_string() },
            target_node_id: "act-1".to_string(),
            condition: None,
            effects: Vec::new(),
            weight: None,
        }],
        image_slot: Some(ImageSlot { prompt: prompt_at(0), asset_ref: None }),
        act: "start".to_string(),
        position: Some(Position { x: 0.0, y: 0.0 }),
    }];

    for index in 1..=act_count {
        let is_last = index == act_count;
        nodes.push(StoryNode {
            id: format!("act-{index}"),
            title: if en { format!("Act {index}") } else { format!("第 {index} 幕") },
            node_type: if is_last { NodeType::Branch } else { NodeType::Normal },
            scene_desc: if en {
                format!("Act {index} of the interactive film \"{title}\".")
            } else {
                format!("互动影游《{title}》第 {index} 幕。")
            },
            dialogue: Vec::new(),
            choices: if is_last {
                vec![
                    Choice {
                        id: "to-ending-a".to_string(),
                        text: if en { "Complete the main objective".into() } else { "完成主线目标".to_string() },
                        target_node_id: "ending-a".to_string(),
                        condition: None,
                        effects: vec![story_step()],
                        weight: None,
                    },
                    Choice {
                        id: "to-ending-b".to_string(),
                        text: if en { "Take the other aftermath".into() } else { "进入另一条余波".to_string() },
                        target_node_id: "ending-b".to_string(),
                        condition: None,
                        effects: vec![story_step()],
                        weight: None,
                    },
                ]
            } else {
                vec![Choice {
                    id: format!("act-{index}-next"),
                    text: if en { "Keep going".into() } else { "继续推进".to_string() },
                    target_node_id: format!("act-{}", index + 1),
                    condition: None,
                    effects: vec![story_step()],
                    weight: None,
                }]
            },
            image_slot: Some(ImageSlot { prompt: prompt_at(index - 1), asset_ref: None }),
            act: format!("act-{index}"),
            position: Some(Position {
                x: (index * 260) as f64,
                y: if index % 2 == 0 { 120.0 } else { 0.0 },
            }),
        });
    }

    nodes.push(
        StoryNode {
            id: "ending-a".to_string(),
            title: if en { "Ending One".into() } else { "结局一".to_string() },
            node_type: NodeType::Ending,
            scene_desc: if en {
                "The main objective is completed and the story converges.".to_string()
            } else {
                "主线目标被完成，故事进入收束。".to_string()
            },
            dialogue: Vec::new(),
            choices: Vec::new(),
            image_slot: None,
            act: "ending".to_string(),
            position: Some(Position { x: ((act_count + 1) * 260) as f64, y: -80.0 }),
        },
    );
    nodes.push(StoryNode {
        id: "ending-b".to_string(),
        title: if en { "Ending Two".into() } else { "结局二".to_string() },
        node_type: NodeType::Ending,
        scene_desc: if en {
            "The player keeps the other aftermath and the story closes on the forked path.".to_string()
        } else {
            "玩家选择保留另一条余波，故事进入分岔收束。".to_string()
        },
        dialogue: Vec::new(),
        choices: Vec::new(),
        image_slot: None,
        act: "ending".to_string(),
        position: Some(Position { x: ((act_count + 1) * 260) as f64, y: 120.0 }),
    });

    StoryGraph {
        schema_version: 1,
        project_id: project_id.to_string(),
        title: title.to_string(),
        world_anchor: Some(WorldAnchor {
            story_core: requirements_or_title().to_string(),
            theme: String::new(),
            genre: "interactive-film".to_string(),
            world_rules: input.reference_mode.clone().unwrap_or_default(),
            duration_minutes: 0.0,
        }),
        characters: Vec::new(),
        variables: vec![Variable {
            name: "story_progress".to_string(),
            variable_type: VariableType::Counter,
            default: json!(0),
            desc: if en { "Story progression".to_string() } else { "剧情推进进度".to_string() },
        }],
        nodes,
        endings: vec![
            Ending {
                id: "ending-a".to_string(),
                node_id: "ending-a".to_string(),
                title: if en { "Ending One".into() } else { "结局一".to_string() },
                ending_type: EndingType::Neutral,
                description: if en {
                    "The main objective is completed.".to_string()
                } else {
                    "主线目标被完成。".to_string()
                },
            },
            Ending {
                id: "ending-b".to_string(),
                node_id: "ending-b".to_string(),
                title: if en { "Ending Two".into() } else { "结局二".to_string() },
                ending_type: EndingType::Secret,
                description: if en {
                    "The player keeps the other aftermath.".to_string()
                } else {
                    "玩家选择保留另一条余波。".to_string()
                },
            },
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn film_input(episode_count: Option<u32>, requirements: &str) -> InteractiveFilmCreationInput {
        InteractiveFilmCreationInput {
            title: "迷雾宅邸".to_string(),
            source_kind: None,
            source_text: None,
            requirements: Some(requirements.to_string()),
            target_audience: Some("青年玩家".to_string()),
            episode_count,
            episode_duration: Some("10 分钟".to_string()),
            budget: None,
            reference_mode: None,
            language: None,
        }
    }

    #[test]
    fn fallback_graph_shapes() {
        let input = film_input(Some(3), "雨夜旧宅的探索");
        let prompts = "1. 雨夜街口\n2. 灯下人影";
        let graph = build_fallback_story_graph("mist-manor", "迷雾宅邸", &input, prompts);

        assert_eq!(graph.schema_version, 1);
        assert_eq!(graph.project_id, "mist-manor");
        // start + act-1..3 + 双结局 = 6 节点。
        assert_eq!(graph.nodes.len(), 6);
        assert_eq!(graph.nodes[0].id, "start");
        assert_eq!(graph.nodes[0].node_type, crate::interactive_film::NodeType::Start);
        // 末幕 branch 双选择（ending-a/b，各带 story_progress add 1）。
        let last = &graph.nodes[3];
        assert_eq!(last.id, "act-3");
        assert_eq!(last.node_type, crate::interactive_film::NodeType::Branch);
        assert_eq!(last.choices.len(), 2);
        assert_eq!(last.choices[0].target_node_id, "ending-a");
        assert_eq!(last.choices[0].effects[0].var, "story_progress");
        // 中间幕单选择链（start 选择 effects 空；幕间选择带 story_progress）。
        assert_eq!(graph.nodes[1].choices[0].target_node_id, "act-2");
        assert_eq!(graph.nodes[1].choices[0].effects[0].var, "story_progress");
        assert!(graph.nodes[0].choices[0].effects.is_empty());
        // 结局节点类型与坐标布局。
        assert_eq!(graph.nodes[4].node_type, crate::interactive_film::NodeType::Ending);
        assert_eq!(graph.nodes[4].position.as_ref().unwrap().x, 1040.0);
        assert_eq!(graph.nodes[4].position.as_ref().unwrap().y, -80.0);
        assert_eq!(graph.nodes[5].position.as_ref().unwrap().y, 120.0);
        assert_eq!(graph.nodes[1].position.as_ref().unwrap().y, 0.0);
        assert_eq!(graph.nodes[2].position.as_ref().unwrap().y, 120.0);
        // counter 变量 + 双结局类型。
        assert_eq!(graph.variables.len(), 1);
        assert_eq!(graph.variables[0].name, "story_progress");
        assert_eq!(graph.variables[0].default, serde_json::json!(0));
        assert_eq!(graph.endings[0].ending_type, crate::interactive_film::EndingType::Neutral);
        assert_eq!(graph.endings[1].ending_type, crate::interactive_film::EndingType::Secret);
        // imageSlot：prompts 轮换（act-2 用第 2 条，act-3 越界回第 1 条）。
        assert_eq!(graph.nodes[2].image_slot.as_ref().unwrap().prompt, "灯下人影");
        assert_eq!(graph.nodes[3].image_slot.as_ref().unwrap().prompt, "雨夜街口");
        let anchor = graph.world_anchor.as_ref().unwrap();
        assert_eq!(anchor.story_core, "雨夜旧宅的探索");
        assert_eq!(anchor.genre, "interactive-film");
    }

    #[test]
    fn fallback_graph_act_count_clamp() {
        // 无集数无提示词 → 3 幕回退。
        let input = film_input(None, "需求");
        let graph = build_fallback_story_graph("p1", "t", &input, "");
        assert_eq!(graph.nodes.len(), 6);
        // 超上限 → 8 幕。
        let input = film_input(Some(9), "需求");
        let graph = build_fallback_story_graph("p1", "t", &input, "");
        assert_eq!(graph.nodes.len(), 11);
    }

    #[test]
    fn graph_premise_zh_sections() {
        let input = film_input(Some(5), "悬疑向多结局");
        let premise = build_interactive_film_graph_premise(&input, "主线树", "旗标表", "剧本正文", "提示词集");
        for needle in [
            "创作需求：悬疑向多结局",
            "目标受众：青年玩家",
            "段落/集数：5",
            "单段时长：10 分钟",
            "剧情树：\n主线树",
            "变量旗标：\n旗标表",
            "互动剧本：\n剧本正文",
            "图像提示词：\n提示词集",
        ] {
            assert!(premise.contains(needle), "premise 缺 {needle}：\n{premise}");
        }
        assert!(!premise.contains("预算"), "premise 不应含空缺段：\n{premise}");
        assert!(!premise.contains("参考模式"), "premise 不应含空缺段：\n{premise}");
    }

    #[test]
    fn graph_premise_en_sections() {
        let input = InteractiveFilmCreationInput {
            language: Some("en".to_string()),
            target_audience: None,
            budget: Some("low".to_string()),
            ..film_input(Some(2), "brief")
        };
        let premise = build_interactive_film_graph_premise(&input, "tree", "flags", "script", "prompts");
        assert!(premise.contains("Creation brief: brief"));
        assert!(premise.contains("Budget: low"));
        assert!(premise.contains("Segments/episodes: 2"));
        assert!(premise.contains("Story tree:\ntree"));
        assert!(!premise.contains("Target audience"), "premise：{premise}");
    }
}
