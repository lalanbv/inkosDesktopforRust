//! 创建源解析（971/972 号 P5 余项）：TS `packages/core/src/agent/creation-source.ts`
//! `loadCreationSource` 的翻译域切片——registered 引用（works 工件）/ inline 文本 /
//! 项目内文件三路 + 歧义守卫 + lineage 绑定校验。
//!
//! TS 全量面差异备案：①`currentExecutionAuthorRequest` 交叉校验（inline 文本须作者
//! 逐字给出）依赖聊天执行 ALS——HTTP 直连 create 无执行作用域（TS 直连面同样仅在
//! 会话内生效），不移植；②`sources`/`source`（单引用）两形态都实现（TS sources 数组
//! 逐条 registered）。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::harness::artifact_reader::read_artifact_revision;
use crate::harness::contracts::{WorkLineage, WorkManifest};
use crate::harness::work_store::load_work_manifest;
use crate::materials::ingest_material;
use crate::utils::path::safe_child_path;

/// TS `CreationSourceReference`（strict 三键）。
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct CreationArtifactReference {
    pub work_id: String,
    pub artifact_id: String,
    #[serde(rename = "revisionId", default, skip_serializing_if = "Option::is_none")]
    pub revision_id: Option<String>,
}

/// 解析产物：正文 + 名称 + lineage（derived-from）。
pub struct CreationSourceResolved {
    pub text: String,
    pub name: String,
    pub lineage: Vec<WorkLineage>,
}

fn coded(code: &str, message: impl std::fmt::Display) -> String {
    format!("{code}: {message}")
}

/// TS `loadCreationSource`（翻译域切片）。入参为 create 端点已剥出的可选四元组
/// （sources / sourceText / filePath——TS source 单引用为代理域形态，HTTP 面不收）。
#[allow(clippy::too_many_arguments)]
pub async fn load_creation_source(
    project_root: &Path,
    target_work_id: Option<&str>,
    sources: Option<&[Value]>,
    source_text: Option<&str>,
    source_path: Option<&str>,
    source_name: Option<&str>,
) -> Result<CreationSourceResolved, String> {
    let previous: Option<WorkManifest> = match target_work_id {
        Some(work_id) => match load_work_manifest(project_root, work_id).await {
            Ok(manifest) => Some(manifest),
            Err(error) if error.is_not_found() => None,
            Err(error) => return Err(error.to_string()),
        },
        None => None,
    };
    // TS 歧义守卫：四形态至多一（`CREATION_SOURCE_CONFLICT`）。
    let present = [
        sources.map(|_| "sources").filter(|_| sources.is_some_and(|list| !list.is_empty())),
        source_text.map(|_| "sourceText").filter(|_| source_text.is_some_and(|t| !t.trim().is_empty())),
        source_path.map(|_| "sourcePath").filter(|_| source_path.is_some_and(|p| !p.trim().is_empty())),
    ];
    if present.iter().filter(|item| item.is_some()).count() > 1 {
        return Err(coded(
            "CREATION_SOURCE_CONFLICT",
            "Select one source input: a reference, ordered references, a file, or supplied text.",
        ));
    }

    // TS `readRegistered`：artifact 修订读取 + lineage 钉定 + 文本类型守卫。
    // （异步 helper：借用 project_root/previous，await read_artifact_revision。）
    async fn read_registered(
        project_root: &Path,
        previous: Option<&WorkManifest>,
        source_name: Option<&str>,
        work_id: &str,
        artifact_id: &str,
        revision_id: Option<&str>,
    ) -> Result<CreationSourceResolved, String> {
        // TS：previous.lineage 钉定旧修订（同名工件 resume 用）。
        let pinned = previous.and_then(|work| {
            work.lineage
                .iter()
                .find(|item| {
                    item.source_work_id == work_id
                        && item.source_artifact_id.as_deref() == Some(artifact_id)
                })
                .and_then(|item| item.source_revision_id.clone())
        });
        let resolved = read_artifact_revision(project_root, work_id, artifact_id, revision_id.or(pinned.as_deref()))
            .await
            .map_err(|error| coded(error.code, &error.message))?;
        // 975 号：xhtml 工件（章节文件，translation-create registered 源主形态）
        // 属合法文本面——content-type 集与 JSON 一并放行。
        if !resolved.revision.content_type.starts_with("text/")
            && resolved.revision.content_type != "application/json"
            && resolved.revision.content_type != "application/xhtml+xml"
        {
            return Err(coded(
                "SOURCE_NOT_TEXT",
                "Select a registered text manuscript or structured text artifact.",
            ));
        }
        let text = String::from_utf8(resolved.bytes)
            .map_err(|error| coded("SOURCE_NOT_TEXT", error.to_string()))?;
        Ok(CreationSourceResolved {
            text,
            name: source_name
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(String::from)
                .unwrap_or_else(|| resolved.work.title.clone()),
            lineage: vec![WorkLineage {
                relation: "derived-from".into(),
                source_work_id: resolved.work.id,
                source_artifact_id: Some(resolved.artifact.id),
                source_revision_id: Some(resolved.revision.id),
            }],
        })
    }

    let result = if let Some(sources) = sources.filter(|list| !list.is_empty()) {
        let references: Vec<CreationArtifactReference> = sources
            .iter()
            .map(|item| {
                serde_json::from_value(item.clone()).map_err(|error| {
                    coded("WORK_RESOURCE_ID_INVALID", format!("invalid source reference: {error}"))
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        if references.is_empty() {
            return Err(coded("CREATION_SOURCE_REQUIRED", "Select at least one source artifact."));
        }
        let identities: BTreeSet<String> = references
            .iter()
            .map(|reference| json!([reference.work_id, reference.artifact_id]).to_string())
            .collect();
        if identities.len() != references.len() {
            return Err(coded("CREATION_SOURCE_CONFLICT", "Select each source artifact once."));
        }
        let mut texts = Vec::new();
        let mut names: Vec<String> = Vec::new();
        let mut lineage = Vec::new();
        for reference in &references {
            let resolved = read_registered(
                project_root,
                previous.as_ref(),
                source_name,
                &reference.work_id,
                &reference.artifact_id,
                reference.revision_id.as_deref(),
            )
            .await?;
            texts.push(resolved.text);
            if !names.contains(&resolved.name) {
                names.push(resolved.name);
            }
            lineage.extend(resolved.lineage);
        }
        CreationSourceResolved {
            text: texts.join("\n\n"),
            name: source_name
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(String::from)
                .unwrap_or_else(|| names.join(" / ")),
            lineage,
        }
    } else if let Some(text) = source_text.map(str::trim).filter(|t| !t.is_empty()) {
        CreationSourceResolved {
            text: text.to_string(),
            name: source_name
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(String::from)
                .unwrap_or_else(|| "source".into()),
            lineage: Vec::new(),
        }
    } else {
        let Some(path_text) = source_path.map(str::trim).filter(|p| !p.is_empty()) else {
            return Err(coded(
                "CREATION_SOURCE_REQUIRED",
                "Select a source artifact, file path, or author-supplied text.",
            ));
        };
        if Path::new(path_text).is_absolute() {
            return Err(coded(
                "CREATION_SOURCE_PATH_ABSOLUTE",
                "Creation sourcePath must be project-relative. Upload or ingest the file first.",
            ));
        }
        let path: PathBuf = safe_child_path(
            &project_root.to_string_lossy(),
            path_text,
        )
        .map_err(|error| coded("CREATION_SOURCE_PATH_INVALID", error))?;
        let parts: Vec<String> = path_text.split('/').map(str::to_string).collect();
        let mut reference: Option<(String, String)> = None;
        if parts.first().map(String::as_str) == Some("works") && parts.get(1).is_some() {
            let work = load_work_manifest(project_root, &parts[1]).await.map_err(|error| error.to_string())?;
            let work_path = parts[2..].join("/");
            let artifact = work.artifacts.iter().find(|artifact| {
                artifact.revisions.iter().any(|revision| {
                    revision.path == work_path
                        && (Some(&revision.id) == artifact.current_revision_id.as_ref()
                            || previous.as_ref().is_some_and(|previous| {
                                previous.lineage.iter().any(|item| {
                                    item.source_artifact_id.as_deref() == Some(artifact.id.as_str())
                                        && item.source_revision_id.as_deref() == Some(revision.id.as_str())
                                })
                            }))
                })
            });
            match artifact {
                Some(artifact) => reference = Some((work.id.clone(), artifact.id.clone())),
                None => {
                    return Err(coded(
                        "SOURCE_ARTIFACT_REQUIRED",
                        "Work source must be a registered artifact; inspect the source Work for its accepted versions.",
                    ))
                }
            }
        }
        if let Some((work_id, artifact_id)) = reference {
            read_registered(project_root, previous.as_ref(), source_name, &work_id, &artifact_id, None).await?
        } else {
            let filename_owned = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| path_text.to_string());
            let title_owned = source_name
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(String::from)
                .or_else(|| path.file_stem().map(|stem| stem.to_string_lossy().into_owned()));
            // TS：readMaterialSource 传原始相对 sourcePath（非 safeChildPath 解析后的
            // 绝对路径——绝对路径二次 safe_child_path 在 Rust 逻辑归一下会 root+abs 拼接）。
            let material = ingest_material(
                project_root,
                &crate::materials::IngestMaterialInput {
                    source_kind: "file",
                    allow_private_egress: false,
                    url: None,
                    file_path: Some(path_text),
                    filename: Some(&filename_owned),
                    mime_type: None,
                    title: title_owned.as_deref(),
                    purpose: Some("reference"),
                },
            )
            .await?;
            let text = tokio::fs::read_to_string(project_root.join(&material.markdown_path))
                .await
                .map_err(|e| e.to_string())?;
            CreationSourceResolved { text, name: material.title, lineage: Vec::new() }
        }
    };

    // TS 绑定校验：draft 已绑定 source-material.md 时，正文与 lineage 必须一致。
    if let Some(previous) = &previous {
        let Some(bound) = previous.artifacts.iter().find(|artifact| {
            artifact
                .revisions
                .iter()
                .any(|revision| revision.id == artifact.current_revision_id.as_deref().unwrap_or("") && revision.path == "source/source-material.md")
        }) else {
            return Ok(result);
        };
        let original = read_artifact_revision(project_root, &previous.id, &bound.id, None)
            .await
            .map_err(|error| coded(error.code, &error.message))?;
        let pinned: Vec<String> = previous
            .lineage
            .iter()
            .filter_map(|item| item.source_revision_id.as_ref().map(|_| format!("{}/{}/{}", item.source_work_id, item.source_artifact_id.clone().unwrap_or_default(), item.source_revision_id.clone().unwrap_or_default())))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let resolved_lineage: Vec<String> = result
            .lineage
            .iter()
            .map(|item| format!("{}/{}/{}", item.source_work_id, item.source_artifact_id.clone().unwrap_or_default(), item.source_revision_id.clone().unwrap_or_default()))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        if original.bytes != result.text.as_bytes() || pinned != resolved_lineage {
            return Err(coded(
                "CREATION_SOURCE_CONFLICT",
                "This draft already has a different source. Resume with its pinned source or create another Work.",
            ));
        }
    }
    Ok(result)
}
