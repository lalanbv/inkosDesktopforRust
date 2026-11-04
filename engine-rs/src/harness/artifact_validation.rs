//! 工件写入校验（884 号 M1b 移植；887 号 M3a 兑换短篇 fail-closed 分支）：
//! `packages/core/src/harness/artifact-validation.ts` `validatedArtifactWrites` 移植。
//!
//! ## 域分支状态（884 fail-closed 边界 → 887 兑换）
//! - `short-manuscript`（authority 文档投影再生）与 `short-package`（销售包三件
//!   投影）：**887 号已兑换为真实实现**（纯函数层 `production::short_fiction`）。
//! - `translation-*` / `story-graph`：仍 fail-closed（各自属翻译/影游管线，M3
//!   后续轮兑换）——比 884 号范围收窄。
//!
//! TS 侧**失败**分支（exports/drafts/派生 .md/packaging 对）逐字对齐错误码。

use serde_json::{json, Value};

use super::action_error::ActionError;
use super::artifact_edit_policy::assert_generic_artifact_editable;
use super::contracts::WorkManifest;
use crate::production::short_fiction::{
    self as short_fiction, ShortDraftBounds, ShortFictionBatchDraft,
};
use crate::utils::atomic_file_set::AtomicFileWrite;
use crate::utils::language::WritingLanguage;
use crate::utils::length_metrics::{count_chapter_length, resolve_length_counting_mode};

/// 短篇 final 的派生 .md 豁免三件（cover-request 是可编辑正典件）。
const SHORT_FICTION_EXEMPT_FINAL_MD: &[&str] = &[
    "source/final/sales-package.md",
    "source/final/cover-prompt.md",
    "source/final/cover-request.md",
];

#[derive(Debug, Clone, thiserror::Error)]
#[error("{message}")]
pub struct ArtifactValidationError {
    pub code: &'static str,
    pub message: String,
    pub authority_path: Option<String>,
}

impl ArtifactValidationError {
    fn into_action_error(self) -> ActionError {
        let mut error =
            ActionError::coded(self.message, self.code);
        if let Some(authority) = self.authority_path {
            // TS ArtifactValidationError 以 authorityPath 属性挂载——failure facts
            // 不提取它（actionFailureFacts 白名单外），仅 Display 层可见。
            error = error.with_recovery(json!({ "authorityPath": authority }));
        }
        error
    }
}

fn write_for(work: &WorkManifest, path: &str, content: String) -> AtomicFileWrite {
    AtomicFileWrite {
        relative_path: format!("works/{}/{}", work.id, path),
        content: crate::utils::atomic_file_set::FileContent::Text(content),
    }
}

fn profile_schema<'a>(profile: Option<&'a super::contracts::WorkProfile>, path: &str) -> Option<&'a str> {
    profile
        .and_then(|p| p.artifact_schemas.get(path))
        .and_then(|value| value.as_str())
}

fn has_short_manuscript(work: &WorkManifest, profile: Option<&super::contracts::WorkProfile>) -> bool {
    work.profile_id == "short-fiction"
        || profile
            .map(|p| p.artifact_schemas.values().any(|v| v.as_str() == Some("short-manuscript")))
            .unwrap_or(false)
}

/// fail-closed 迁移边界（884 号）：L2 域投影分支。
fn domain_projection_unavailable(schema: &str) -> ActionError {
    ActionError::coded(
        format!(
            "Artifact schema \"{schema}\" requires domain production capabilities that are not yet available on this engine (work domain M3 migration boundary). Adopt this revision from the Node engine, or port the domain pipeline (round M3) first."
        ),
        "ARTIFACT_DOMAIN_PROJECTION_UNAVAILABLE",
    )
}

/// TS `validatedArtifactWrites`：校验 authority 文档并在同一提交内再生确定性投影
///（投影再生面在 Rust 侧 fail-closed，见模块头注释）。
pub fn validated_artifact_writes(
    work: &WorkManifest,
    path: &str,
    content: &str,
    profile: Option<&super::contracts::WorkProfile>,
    // TS：previousContent 仅用于 translation /translated/ 结构保持校验——该分支
    // 已 fail-closed（884 号 M3 边界），参数保留以对齐 TS 签名（M3 接通时复用）。
    _previous_content: Option<&str>,
) -> Result<Vec<AtomicFileWrite>, ActionError> {
    if let Some(profile) = profile {
        assert_generic_artifact_editable(work, path, profile)?;
    }
    if path.starts_with("source/exports/") {
        // 导出件是投递副本：指回注册源工件让其修订后再导出。
        let original = work.artifacts.iter().find(|artifact| {
            format!("source/exports/{}.md", encode_uri_component(&artifact.id)) == path
        });
        let authority = original.and_then(|artifact| {
            artifact
                .revisions
                .iter()
                .find(|revision| Some(revision.id.as_str()) == artifact.current_revision_id.as_deref())
                .map(|revision| revision.path.clone())
        });
        let recovery = match original.and_then(|artifact| artifact.current_revision_id.clone()) {
            Some(revision_id) => json!({
                "action": "workspace__read",
                "parameters": {
                    "workId": work.id,
                    "artifactId": original.map(|artifact| artifact.id.clone()),
                    "revisionId": revision_id,
                },
                "reason": "Read and revise the source, then export its updated revision.",
            }),
            None => json!({
                "action": "workspace__inspect_work",
                "parameters": { "workId": work.id },
                "reason": "Select the registered source artifact rather than a delivery copy.",
            }),
        };
        return Err(ActionError {
            message: "Exported artifacts are delivery copies. Revise the registered source artifact, then export it again.".into(),
            code: Some("ARTIFACT_DERIVED".into()),
            recovery: Some(Box::new(recovery)),
            path: authority,
            capability_id: None,
        });
    }

    let schema = profile_schema(profile, path).filter(|schema| *schema != "text");
    let mut document: Option<Value> = None;
    if path.ends_with(".json") || schema.is_some() {
        match serde_json::from_str::<Value>(content) {
            Ok(parsed) => document = Some(parsed),
            Err(_) => {
                return Err(ActionError::coded(
                    "Artifact is not valid JSON",
                    "ARTIFACT_INVALID",
                ));
            }
        }
    }

    // TS 顶部 try 的 schema 快验（artifact-validation.ts L39-40）：parse 失败 =
    // ARTIFACT_INVALID（887 号 M3a 兑换 884 号 fail-closed：短篇两支真实校验；
    // translation-*/story-graph 仍 fail-closed——各自属翻译/影游管线 M3 后续轮）。
    if let Some(schema) = schema {
        match schema {
            "short-manuscript" => {
                if let Some(document) = &document {
                    if let Err(error) =
                        serde_json::from_value::<crate::production::short_fiction::ShortFictionBatchDraft>(
                            document.clone(),
                        )
                    {
                        return Err(ActionError::coded(
                            format!("Error: {error}"),
                            "ARTIFACT_INVALID",
                        ));
                    }
                }
            }
            "short-package" => {
                if let Some(document) = &document {
                    if crate::production::short_fiction::check_short_package_tool(document).is_err() {
                        return Err(ActionError::coded(
                            "Error: Invalid short package",
                            "ARTIFACT_INVALID",
                        ));
                    }
                }
            }
            "translation-manifest" | "translation-glossary" | "translation-chapter" | "story-graph" => {
                // 884 号 fail-closed 边界残余（范围已收窄：短篇两支 887 号兑换）。
                return Err(domain_projection_unavailable(schema));
            }
            _ => {}
        }
    }

    let short_manuscript = has_short_manuscript(work, profile);
    if short_manuscript && path.starts_with("source/drafts/") {
        let has_manuscript = super::artifact_edit_policy::has_current_revision_at(
            &work.artifacts,
            "source/final/short-story.json",
        );
        let authority = has_manuscript.then_some("source/final/short-story.json");
        // TS：`/\/chapters\/(\d+)\.md$/` 匹配时 recovery 携带 chapterNumbers。
        let chapter = path
            .split_once("/chapters/")
            .and_then(|(_, file)| file.strip_suffix(".md"))
            .filter(|number| !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|number| number.parse::<u32>().ok());
        let recovery = if has_manuscript {
            json!({
                "action": "short-fiction__revise_short_fiction",
                "parameters": chapter.map(|n| json!({ "chapterNumbers": [n] })).unwrap_or_else(|| json!({})),
                "reason": "Apply the user request to the accepted manuscript and regenerate its projections.",
            })
        } else {
            json!({
                "action": "short-fiction__draft_short_fiction",
                "parameters": {
                    "workId": work.id,
                    "minChapterLength": null,
                    "maxChapterLength": null,
                    "openingHookChars": null,
                    "coverRequired": null,
                },
                "reason": "Complete and repair the saved draft; no accepted manuscript exists yet.",
            })
        };
        return Err(ActionError {
            message: "Draft checkpoints are maintained by short-fiction production. Use the available draft or manuscript operation.".into(),
            code: Some("ARTIFACT_DERIVED".into()),
            recovery: Some(Box::new(recovery)),
            path: authority.map(str::to_string),
            capability_id: None,
        });
    }
    if short_manuscript && path.starts_with("source/final/") {
        let authority = "source/final/short-story.json";
        // TS L62-68：sales-package.json → 校验 + rawContent 渲染 + 三件投影
        //（887 号 M3a 兑换）。document 额外键保留（sales = {...document, rawContent}）。
        if path == "source/final/sales-package.json" {
            let document = document.as_ref().ok_or_else(|| {
                ActionError::coded("Invalid short-fiction sales package", "ARTIFACT_INVALID")
            })?;
            if crate::production::short_fiction::check_short_package_tool(document).is_err() {
                return Err(artifact_validation_error(
                    "ARTIFACT_INVALID",
                    "Invalid short-fiction sales package",
                    Some(path),
                ));
            }
            let language =
                if work.language == "en" { WritingLanguage::En } else { WritingLanguage::Zh };
            let mut sales = document.clone();
            let rendered = crate::production::short_fiction::render_sales_package_from_value(
                &sales,
                language,
            )
            .map_err(|error| {
                artifact_validation_error("ARTIFACT_INVALID", error, Some(path))
            })?;
            if let Value::Object(map) = &mut sales {
                map.insert("rawContent".into(), Value::String(rendered));
            }
            let raw_content = sales
                .get("rawContent")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let cover_prompt = sales
                .get("coverPrompt")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            return Ok(vec![
                write_for(
                    work,
                    path,
                    serde_json::to_string_pretty(&sales).unwrap_or_default(),
                ),
                write_for(work, "source/final/sales-package.md", raw_content),
                write_for(work, "source/final/cover-prompt.md", cover_prompt),
            ]);
        }
        if path.ends_with(".md") && !SHORT_FICTION_EXEMPT_FINAL_MD.contains(&path) {
            return Err(ActionError {
                message: "Edit the manuscript authority document or use the short-fiction revision action".into(),
                code: Some("ARTIFACT_DERIVED".into()),
                recovery: None,
                path: Some(authority.to_string()),
                capability_id: None,
            });
        }
        // TS L72-94：authority 文档 → parse→charCount 重算→终稿校验→连续性检查
        //→markdown 再渲染→drafts/v001+full.md+chapters+已注册 final .md 投影族
        // 多写（887 号 M3a 兑换）。
        if path == authority {
            let document = document.clone().ok_or_else(|| {
                ActionError::coded("Artifact is not valid JSON", "ARTIFACT_INVALID")
            })?;
            let result = regenerate_short_manuscript_projections(work, &document);
            match result {
                Ok(writes) => return Ok(writes),
                Err(message) => {
                    // TS catch → ArtifactValidationError("ARTIFACT_INVALID", error.message, authority)。
                    return Err(artifact_validation_error(
                        "ARTIFACT_INVALID",
                        message,
                        Some(authority),
                    ));
                }
            }
        }
        if path == "source/final/sales-package.md" || path == "source/final/cover-prompt.md" {
            return Err(ActionError {
                message: "Regenerate packaging from the current manuscript".into(),
                code: Some("ARTIFACT_DERIVED".into()),
                recovery: None,
                path: Some("source/final/sales-package.json".to_string()),
                capability_id: None,
            });
        }
    }
    // TS：translation 域分支（manifest 所有权/章节结构保持校验 + manifest 再格式化）
    // ——同样属 M3 边界。document 已解析（.json 或带 schema）即进入；.md 纯文本
    //（document 无）走默认单写。
    if (work.profile_id == "translation" || schema.is_some_and(|s| s.starts_with("translation-")))
        && document.is_some()
    {
        return Err(domain_projection_unavailable("translation"));
    }
    // TS 默认分支：translation manifest 的 JSON 再格式化属上面 fail-closed 域；
    // 其余（含 translation profile 下的普通文本）= 单写原文。
    Ok(vec![write_for(work, path, content.to_string())])
}

/// TS `ArtifactValidationError` 的错误码面（adopt 工具体引用）。
pub fn artifact_validation_error(
    code: &'static str,
    message: impl Into<String>,
    authority_path: Option<&str>,
) -> ActionError {
    ArtifactValidationError {
        code,
        message: message.into(),
        authority_path: authority_path.map(str::to_string),
    }
    .into_action_error()
}

/// `encodeURIComponent` 等价（TS `source/exports/${encodeURIComponent(id)}.md`）。
pub fn encode_uri_component(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// TS `validatedArtifactWrites` authority 分支（artifact-validation.ts L72-94，
/// 887 号 M3a 兑换）：authority 文档 → parse → charCount 重算 → 终稿校验 →
/// 连续性检查 → markdown 再渲染 → drafts/v001 + full.md + chapters + 已注册
/// final .md 投影族多写。错误统一上抛为 message（catch → ARTIFACT_INVALID）。
fn regenerate_short_manuscript_projections(
    work: &WorkManifest,
    document: &Value,
) -> Result<Vec<AtomicFileWrite>, String> {
    let authority = "source/final/short-story.json";
    let language = if work.language == "en" { WritingLanguage::En } else { WritingLanguage::Zh };

    let run = || -> Result<Vec<AtomicFileWrite>, String> {
        let mut draft: ShortFictionBatchDraft = serde_json::from_value(document.clone())
            .map_err(|error| error.to_string())?;
        let mode = resolve_length_counting_mode(language);
        for chapter in &mut draft.chapters {
            chapter.char_count = count_chapter_length(&chapter.content, mode);
        }
        short_fiction::validate_short_fiction_draft_for_final(
            &draft,
            &ShortDraftBounds {
                expected_chapters: Some(draft.chapters.len() as u32),
                language: Some(language),
                ..ShortDraftBounds::default()
            },
        )
        .map_err(|error| error.to_string())?;
        if draft.chapters.is_empty()
            || draft
                .chapters
                .iter()
                .enumerate()
                .any(|(index, chapter)| chapter.number != index as u32 + 1)
        {
            return Err("Chapters must be contiguous".into());
        }
        let markdown = short_fiction::render_short_fiction_draft_markdown(&draft, language)
            .map_err(|error| error.to_string())?;
        draft.raw_content = markdown.clone();

        // 投影族：current 修订中以 source/final/*.md 注册、排除 chapters/ 与三
        // 豁免、排除 full.md 本身（TS 同款 filter）。
        let projections: Vec<String> = work
            .artifacts
            .iter()
            .flat_map(|artifact| {
                artifact
                    .revisions
                    .iter()
                    .filter(|revision| Some(&revision.id) == artifact.current_revision_id.as_ref())
            })
            .map(|revision| &revision.path)
            .filter(|path| {
                path.starts_with("source/final/")
                    && path.ends_with(".md")
                    && !path.contains("/chapters/")
                    && !SHORT_FICTION_EXEMPT_FINAL_MD.contains(&path.as_str())
                    && path.as_str() != "source/final/full.md"
            })
            .cloned()
            .collect();

        let draft_json = serde_json::to_string_pretty(&draft).map_err(|error| error.to_string())?;
        let mut writes = vec![
            write_for(work, authority, draft_json.clone()),
            write_for(work, "source/drafts/v001/draft.json", draft_json),
            write_for(work, "source/drafts/v001/full.md", markdown.clone()),
            write_for(work, "source/final/full.md", markdown.clone()),
        ];
        for projection in &projections {
            writes.push(write_for(work, projection, markdown.clone()));
        }
        for chapter in &draft.chapters {
            let heading = short_fiction::format_short_fiction_chapter_heading(
                chapter.number,
                &chapter.title,
                language,
            )
            .map_err(|error| error.to_string())?;
            for base in ["source/final", "source/drafts/v001"] {
                writes.push(write_for(
                    work,
                    &format!("{base}/chapters/{:04}.md", chapter.number),
                    format!("# {heading}\n\n{}", chapter.content),
                ));
            }
        }
        Ok(writes)
    };
    run()
}
