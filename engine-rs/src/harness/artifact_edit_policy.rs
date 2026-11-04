//! 工件域归属策略（884 号 M1b）：`packages/core/src/harness/artifact-edit-policy.ts`
//! 忠实移植。域维护的存储命名空间拒绝通用替换面（adopt/replace），改走域操作。

use serde_json::json;

use super::action_error::ActionError;
use super::contracts::{ArtifactManifest, WorkManifest, WorkProfile};

const SHORT_FICTION_EXEMPT_FINAL_MD: &[&str] = &[
    "source/final/sales-package.md",
    "source/final/cover-prompt.md",
    "source/final/cover-request.md",
];

const INTERACTIVE_FILM_OWNED_PATHS: &[&str] = &[
    "source/brief.md",
    "source/story-graph.json",
    "source/authoring-state.json",
    "source/delivery-requirements.json",
    "source/interactive-spec.md",
    "source/story-tree.md",
    "source/flags.json",
    "source/flags.md",
];

const FILM_BUNDLE_PROJECTIONS: &[&str] = &[
    "source/script.md",
    "source/storyboard.md",
    "source/image-prompts.md",
    "source/assets.json",
];

/// 「当前修订指向该路径」判定（validatedArtifactWrites 短篇分支复用，884 号）。
pub fn has_current_revision_at(artifacts: &[ArtifactManifest], path: &str) -> bool {
    artifacts.iter().any(|artifact| {
        artifact.revisions.iter().any(|revision| {
            Some(revision.id.as_str()) == artifact.current_revision_id.as_deref()
                && revision.path == path
        })
    })
}

/// TS `artifactDomainOwner`。
pub fn artifact_domain_owner(
    work: &WorkManifest,
    path: &str,
    profile: &WorkProfile,
) -> Option<&'static str> {
    let has = |capability: &str| profile.capability_ids.iter().any(|id| id == capability);
    if has("short-fiction")
        && (path == "source/production-state.json"
            || path.starts_with("source/drafts/")
            || path == "source/final/short-story.json"
            || path.starts_with("source/final/chapters/")
            || (path.starts_with("source/final/")
                && path.ends_with(".md")
                && !SHORT_FICTION_EXEMPT_FINAL_MD.contains(&path)))
    {
        return Some("short-fiction");
    }
    if has("longform")
        && (path == "source/book.json"
            || path.starts_with("source/story/")
            || path.starts_with("source/chapters/"))
    {
        return Some("longform");
    }
    if has("interactive-world") && (path == "source/world.json" || path.starts_with("source/runs/")) {
        return Some("interactive-world");
    }
    if profile.artifact_schemas.get(path).and_then(|v| v.as_str()) == Some("story-graph") {
        return Some("interactive-film");
    }
    if has("interactive-film") {
        if INTERACTIVE_FILM_OWNED_PATHS.contains(&path) || path.starts_with("source/snapshots/") {
            return Some("interactive-film");
        }
        // 影游 bundle 的 script/images 是 interactive-spec 的投影；组合 profile 中
        // 独立 script 没有这样的源 spec，不归域。
        let film_bundle = has_current_revision_at(&work.artifacts, "source/interactive-spec.md");
        if film_bundle && FILM_BUNDLE_PROJECTIONS.contains(&path) {
            return Some("interactive-film");
        }
    }
    None
}

fn chapter_number(path: &str) -> Option<u32> {
    // TS `/^source\/(?:final|drafts\/[^/]+)\/chapters\/(\d+)\.md$/`——drafts 版本段
    // 不含 `/`（`[^/]+`）。
    let rest = path.strip_prefix("source/")?;
    let tail = if let Some(after) = rest.strip_prefix("final/") {
        after
    } else {
        let after = rest.strip_prefix("drafts/")?;
        let (segment, remainder) = after.split_once('/')?;
        if segment.is_empty() {
            return None;
        }
        remainder
    };
    let file = tail.strip_prefix("chapters/")?;
    let number = file.strip_suffix(".md")?;
    if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    number.parse::<u32>().ok()
}

/// TS `assertGenericArtifactEditable`（method 默认 "replacement"）。
pub fn assert_generic_artifact_editable(
    work: &WorkManifest,
    path: &str,
    profile: &WorkProfile,
) -> Result<(), ActionError> {
    assert_generic_artifact_editable_with_method(work, path, profile, "replacement")
}

pub fn assert_generic_artifact_editable_with_method(
    work: &WorkManifest,
    path: &str,
    profile: &WorkProfile,
    method: &str,
) -> Result<(), ActionError> {
    let owner = artifact_domain_owner(work, path, profile);
    // 显式宿主替换/采用保留其经校验的投影提交；生成式手稿编辑必须走域操作。
    if owner == Some("short-fiction") && method == "replacement" {
        return Ok(());
    }
    let manuscript = owner == Some("short-fiction")
        && has_current_revision_at(&work.artifacts, "source/final/short-story.json");
    let chapter = chapter_number(path);
    let Some(owner) = owner else { return Ok(()) };
    let domain = owner.to_string();
    let recovery = if owner == "short-fiction" {
        if manuscript {
            json!({
                "action": "short-fiction__revise_short_fiction",
                "parameters": chapter.map(|n| json!({ "chapterNumbers": [n] })).unwrap_or_else(|| json!({})),
                "reason": "Apply the original author request through the manuscript revision action. It can change an authorized book title while preserving prose and updating registered projections and the production contract.",
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
                "reason": "Resume draft production; no accepted manuscript exists yet.",
            })
        }
    } else {
        json!({
            "action": "workspace__inspect_work",
            "parameters": { "workId": work.id },
            "reason": format!(
                "Select the {owner} operation for this artifact. Other independent text artifacts remain editable."
            ),
        })
    };
    Err(ActionError {
        message: format!(
            "This artifact is maintained by {owner} operations; use its domain edit action to preserve associated state."
        ),
        code: Some("ARTIFACT_DOMAIN_ACTION_REQUIRED".into()),
        recovery: Some(Box::new(recovery)),
        path: Some(path.to_string()),
        capability_id: Some(domain),
    })
}
