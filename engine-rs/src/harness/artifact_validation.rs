//! 工件写入校验（884 号 M1b）：`packages/core/src/harness/artifact-validation.ts`
//! `validatedArtifactWrites` 移植。
//!
//! ## 884 号迁移边界（fail-closed 原则）
//! L2 生产管线（短篇 authority 文档投影再生 / 销售包渲染 / 翻译所有权校验 /
//! story-graph 变量类型投影）属 M3 域，未随本轮迁移。这些 schema 分支在 TS adopt
//! 会**成功**并再生确定性投影；Rust 侧 fail-closed（`ARTIFACT_DOMAIN_PROJECTION_UNAVAILABLE`
//! 显式报错）而非静默裸写——裸写会写出「Node 预期投影已再生而实际没有」的腐化
//! 清单态，违反双引擎交替读写硬门禁。TS 侧**失败**分支（exports/drafts/派生 .md）
//! 则逐字对齐错误码。M3 移植对应 runner 时移除 fail-closed 分支。

use serde_json::{json, Value};

use super::action_error::ActionError;
use super::artifact_edit_policy::assert_generic_artifact_editable;
use super::contracts::WorkManifest;
use crate::utils::atomic_file_set::AtomicFileWrite;

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

    // 884 号 fail-closed 分支：带域 schema 的文档在 TS 会做域校验/投影再生。
    // 注意 story-graph 通常已在 assertGenericArtifactEditable 被域归属拒绝
    //（owner=interactive-film）；此处兜底自定义 profile 的 schema 直配路径。
    if let Some(schema) = schema {
        if matches!(
            schema,
            "short-manuscript"
                | "short-package"
                | "translation-manifest"
                | "translation-glossary"
                | "translation-chapter"
                | "story-graph"
        ) {
            return Err(domain_projection_unavailable(schema));
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
        // TS：sales-package.json 在此再生三件投影——M3 边界，fail-closed。
        if path == "source/final/sales-package.json" {
            return Err(domain_projection_unavailable("short-package"));
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
        // TS：authority 文档在此再生 drafts/v001 + full.md + chapters + 投影——M3 边界。
        if path == authority {
            return Err(domain_projection_unavailable("short-manuscript"));
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
