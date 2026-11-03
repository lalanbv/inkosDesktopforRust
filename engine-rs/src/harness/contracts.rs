//! harness 契约（882 号 M1a）：`packages/core/src/harness/contracts.ts` 的 serde
//! 移植——zod strict ↔ `deny_unknown_fields` 互逆（876 号硬门禁：写出字段集必须
//! 与 TS 可读集精确互逆）。非 strict 的上游子对象（production/contextRecipe）按
//! TS 语义「未知键剥离」= 不加 deny。

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const HARNESS_VERSION: u32 = 2;

/// TS `UNSAFE_RESOURCE_ID_RE`：控制字符/DEL/路径分隔/Windows 保留字符/引号/花括号尖括号竖线。
const UNSAFE_RESOURCE_ID_CHARS: &[char] = &[
    '\u{0}', '\u{1}', '\u{2}', '\u{3}', '\u{4}', '\u{5}', '\u{6}', '\u{7}', '\u{8}', '\t', '\n',
    '\u{b}', '\u{c}', '\r', '\u{e}', '\u{f}', '\u{10}', '\u{11}', '\u{12}', '\u{13}', '\u{14}',
    '\u{15}', '\u{16}', '\u{17}', '\u{18}', '\u{19}', '\u{1a}', '\u{1b}', '\u{1c}', '\u{1d}',
    '\u{1e}', '\u{1f}', '\u{7f}', '/', '\\', ':', '*', '?', '"', '`', '{', '}', '<', '>', '|',
];

pub fn validate_harness_id(value: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err("ID must not be empty".into());
    }
    if value.chars().count() > 128 {
        return Err("ID must not exceed 128 characters".into());
    }
    if value.trim() != value {
        return Err("ID must not have surrounding whitespace".into());
    }
    if value == "." || value == ".." || value.contains("..") {
        return Err("ID cannot traverse directories".into());
    }
    if value.chars().any(|c| UNSAFE_RESOURCE_ID_CHARS.contains(&c)) {
        return Err("ID contains unsafe path characters".into());
    }
    Ok(())
}

pub fn validate_work_resource_id(value: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err("Resource ID must not be empty".into());
    }
    if value.chars().count() > 120 {
        return Err("Resource ID must not exceed 120 characters".into());
    }
    if value.trim() != value {
        return Err("Resource ID must not have surrounding whitespace".into());
    }
    if value == "." || value == ".." || value.contains("..") {
        return Err("Resource ID cannot traverse directories".into());
    }
    if value.chars().any(|c| UNSAFE_RESOURCE_ID_CHARS.contains(&c)) {
        return Err("Resource ID contains unsafe path characters".into());
    }
    Ok(())
}

fn validate_relative_artifact_path(value: &str) -> Result<(), String> {
    // TS：非 / 开头、非 \ 开头、非 `[a-z]:[\\/]` 盘符、按 [\\/]+ 切段无 ".."。
    let lower = value.to_ascii_lowercase();
    let drive_letter = lower.len() >= 2
        && lower.as_bytes()[0].is_ascii_alphabetic()
        && (lower.as_bytes()[1] == b':' )
        && lower[2..].starts_with(['\\', '/']);
    let ok = !value.is_empty()
        && !value.starts_with('/')
        && !value.starts_with('\\')
        && !drive_letter
        && !value.split(['\\', '/']).any(|segment| segment == "..");
    if ok {
        Ok(())
    } else {
        Err("Artifact paths must stay inside the work directory".into())
    }
}

// ── 枚举（值与 TS z.enum 逐字对齐，serde 小写直出） ──────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EpisodeStatus {
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArtifactRevisionStatus {
    Candidate,
    Current,
    Superseded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkStatus {
    Draft,
    Active,
    Archived,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConfirmationValue {
    Execute,
    Confirm,
}

// ── WorkLineage ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct WorkLineage {
    pub relation: String,
    #[serde(rename = "sourceWorkId")]
    pub source_work_id: String,
    #[serde(rename = "sourceArtifactId", skip_serializing_if = "Option::is_none")]
    pub source_artifact_id: Option<String>,
    #[serde(rename = "sourceRevisionId", skip_serializing_if = "Option::is_none")]
    pub source_revision_id: Option<String>,
}

// ── ArtifactRevision / ArtifactManifest ────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ArtifactRevision {
    pub id: String,
    /// z.nullable（非 optional）——缺省与 null 均序列化为 null。
    #[serde(rename = "parentRevisionId")]
    pub parent_revision_id: Option<String>,
    pub path: String,
    #[serde(rename = "snapshotPath", skip_serializing_if = "Option::is_none")]
    pub snapshot_path: Option<String>,
    #[serde(rename = "contentType")]
    pub content_type: String,
    pub status: ArtifactRevisionStatus,
    pub checksum: String,
    #[serde(rename = "byteLength")]
    pub byte_length: u64,
    #[serde(rename = "episodeId", skip_serializing_if = "Option::is_none")]
    pub episode_id: Option<String>,
    #[serde(rename = "createdAt")]
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactManifest {
    pub id: String,
    pub kind: String,
    #[serde(rename = "currentRevisionId")]
    pub current_revision_id: Option<String>,
    pub revisions: Vec<ArtifactRevision>,
    pub metadata: Map<String, Value>,
}

impl ArtifactManifest {
    /// 三条 superRefine 不变量（contracts.ts L68-99）：
    /// ① currentRevisionId 必须指向已知 revision；② current 至多一个；
    /// ③ currentRevisionId 非 null 时必须恰为 status=current 的那条。
    pub fn validate(&self) -> Result<(), String> {
        let mut current: Option<&ArtifactRevision> = None;
        for revision in &self.revisions {
            if Some(&revision.id) == self.current_revision_id.as_ref() && current.is_some() {
                return Err("An artifact can have only one current revision".into());
            }
            if revision.status == ArtifactRevisionStatus::Current {
                if current.is_some() {
                    return Err("An artifact can have only one current revision".into());
                }
                current = Some(revision);
            }
        }
        if let Some(expected) = self.current_revision_id.as_ref() {
            match current {
                Some(revision) if revision.id == *expected => {}
                Some(_) => {
                    return Err(
                        "currentRevisionId must identify the revision whose status is current".into(),
                    );
                }
                None => {
                    return Err(
                        "currentRevisionId must reference a known revision".into()
                    );
                }
            }
        }
        Ok(())
    }
}

// ── WorkManifest ───────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct WorkManifest {
    pub version: u32,
    pub id: String,
    pub title: String,
    #[serde(rename = "profileId")]
    pub profile_id: String,
    pub language: String,
    pub status: WorkStatus,
    pub lineage: Vec<WorkLineage>,
    pub artifacts: Vec<ArtifactManifest>,
    pub metadata: Map<String, Value>,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    #[serde(rename = "updatedAt")]
    pub updated_at: String,
}

impl WorkManifest {
    /// zod parse 的等价校验：id/profileId 域校验、title 非空、每个 artifact
    /// 不变量、lineage 条目域校验。
    pub fn validate(&self) -> Result<(), String> {
        validate_work_resource_id(&self.id)?;
        validate_harness_id(&self.profile_id)?;
        if self.title.is_empty() {
            return Err("title must not be empty".into());
        }
        for lineage in &self.lineage {
            validate_harness_id(&lineage.relation)?;
            validate_work_resource_id(&lineage.source_work_id)?;
            if let Some(source_artifact_id) = lineage.source_artifact_id.as_deref() {
                validate_work_resource_id(source_artifact_id)?;
            }
            if let Some(source_revision_id) = lineage.source_revision_id.as_deref() {
                validate_harness_id(source_revision_id)?;
            }
        }
        for artifact in &self.artifacts {
            validate_work_resource_id(&artifact.id)?;
            validate_harness_id(&artifact.kind)?;
            artifact.validate()?;
            for revision in &artifact.revisions {
                validate_harness_id(&revision.id)?;
                validate_relative_artifact_path(&revision.path)?;
                if let Some(snapshot_path) = revision.snapshot_path.as_deref() {
                    validate_relative_artifact_path(snapshot_path)?;
                }
                if let Some(episode_id) = revision.episode_id.as_deref() {
                    validate_harness_id(episode_id)?;
                }
            }
        }
        Ok(())
    }
}

// ── WorkProfile ────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfirmationPolicy {
    #[serde(rename = "inferredMutation")]
    pub inferred_mutation: ConfirmationValue,
    #[serde(rename = "explicitRecoverableMutation")]
    pub explicit_recoverable_mutation: ConfirmationValue,
    #[serde(rename = "destructiveMutation")]
    pub destructive_mutation: ConfirmationValue,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextRecipe {
    pub id: String,
    #[serde(rename = "sourceIds")]
    pub source_ids: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ProfileProduction {
    #[serde(rename = "maxChaptersPerCall", skip_serializing_if = "Option::is_none")]
    pub max_chapters_per_call: Option<u32>,
    #[serde(
        rename = "minChapterLengthRatio",
        skip_serializing_if = "Option::is_none"
    )]
    pub min_chapter_length_ratio: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct WorkProfile {
    pub version: u32,
    pub id: String,
    pub title: String,
    pub description: String,
    #[serde(rename = "capabilityIds")]
    pub capability_ids: Vec<String>,
    #[serde(rename = "requiredSkillIds")]
    pub required_skill_ids: Vec<String>,
    #[serde(rename = "recommendedSkillIds")]
    pub recommended_skill_ids: Vec<String>,
    #[serde(rename = "artifactKinds")]
    pub artifact_kinds: Vec<String>,
    pub confirmation: ConfirmationPolicy,
    #[serde(rename = "contextRecipe", skip_serializing_if = "Option::is_none")]
    pub context_recipe: Option<ContextRecipe>,
    #[serde(rename = "artifactSchemas", default)]
    pub artifact_schemas: Map<String, Value>,
    #[serde(rename = "qualityCriteria", default)]
    pub quality_criteria: Vec<String>,
    #[serde(default)]
    pub production: ProfileProduction,
}

impl WorkProfile {
    /// zod parse 等价校验：id/title 域、capabilityIds ≥1、production 比例上限。
    pub fn validate(&self) -> Result<(), String> {
        validate_harness_id(&self.id)?;
        if self.title.is_empty() {
            return Err("title must not be empty".into());
        }
        if self.capability_ids.is_empty() {
            return Err("capabilityIds must not be empty".into());
        }
        for capability_id in &self.capability_ids {
            validate_harness_id(capability_id)?;
        }
        for skill_id in self.required_skill_ids.iter().chain(self.recommended_skill_ids.iter()) {
            validate_harness_id(skill_id)?;
        }
        for kind in &self.artifact_kinds {
            validate_harness_id(kind)?;
        }
        if let Some(ratio) = self.production.min_chapter_length_ratio {
            if ratio <= 0.0 || ratio > 1.0 {
                return Err("minChapterLengthRatio must be within (0, 1]".into());
            }
        }
        if let Some(max_chapters) = self.production.max_chapters_per_call {
            if max_chapters == 0 {
                return Err("maxChaptersPerCall must be positive".into());
            }
        }
        Ok(())
    }
}

// ── CreativeEpisode / EpisodeEvent ─────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct CreativeEpisode {
    pub version: u32,
    pub id: String,
    #[serde(rename = "workId")]
    pub work_id: Option<String>,
    #[serde(rename = "profileId")]
    pub profile_id: Option<String>,
    pub status: EpisodeStatus,
    #[serde(rename = "startedAt")]
    pub started_at: String,
    #[serde(rename = "completedAt")]
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct CreativeEpisodeEvent {
    pub version: u32,
    #[serde(rename = "episodeId")]
    pub episode_id: String,
    pub seq: u64,
    pub timestamp: String,
    #[serde(rename = "type")]
    pub event_type: String,
    #[serde(rename = "workId")]
    pub work_id: Option<String>,
    #[serde(rename = "capabilityId", skip_serializing_if = "Option::is_none")]
    pub capability_id: Option<String>,
    #[serde(rename = "actionId", skip_serializing_if = "Option::is_none")]
    pub action_id: Option<String>,
    pub payload: Map<String, Value>,
}
